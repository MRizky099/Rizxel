//! The Plane tool's work plane (Goxel `goxel.plane`, Qubicle slice mode).
//!
//! The Plane tool does not write voxels. Clicking a face places an
//! axis-aligned plane on it; while the plane is visible, drawing tools snap
//! onto it, so voxels can be placed in empty space. With Slice enabled the
//! plane works like Qubicle's 2D slice mode: it selects one *slice layer*
//! that drawing tools reach from either side and are confined to. The view
//! keeps the slice layer, everything behind it and one more layer beyond it
//! for context; the rest is hidden (view only). Edits write straight into
//! the layer, so unticking Slice shows them merged (latest edit wins). The
//! destructive "Cut" removes the hidden part through
//! [`super::slice_layers`].

use glam::IVec3;
use rizxel_math::{BoundSize, VoxelBox};
use serde::{Deserialize, Serialize};

use super::{KeepSide, SliceRequest};
use crate::raycast::{raycast_face, Ray, RayHit};

/// Axis-aligned work plane. Not part of the undo history.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkPlane {
    /// `0 = X`, `1 = Y`, `2 = Z`: the plane's normal axis.
    pub axis: usize,
    /// Boundary coordinate along `axis`, in `0..=bounds[axis]`.
    pub coord: i32,
    /// Drawn and used as a drawing surface.
    pub visible: bool,
    /// Hide one side of the model (view clip) and confine edits to the other.
    pub slice: bool,
    /// Hide the negative side instead of the positive one.
    pub flip: bool,
}

impl Default for WorkPlane {
    fn default() -> Self {
        Self {
            axis: 1,
            coord: 0,
            visible: false,
            slice: false,
            flip: false,
        }
    }
}

impl WorkPlane {
    /// True while the plane takes part in picking and drawing.
    pub fn active(&self) -> bool {
        self.visible || self.slice
    }

    /// Copy with `axis` and `coord` forced into valid ranges for `bounds`.
    /// While slicing, at least the slice layer stays inside the bounds.
    pub fn clamped(mut self, bounds: BoundSize) -> Self {
        self.axis = self.axis.min(2);
        let size = bounds.0[self.axis] as i32;
        let (lo, hi) = match (self.slice, self.keep_side()) {
            (false, _) => (0, size),
            (true, KeepSide::Negative) => (1, size),
            (true, KeepSide::Positive) => (0, size - 1),
        };
        self.coord = self.coord.clamp(lo, hi.max(lo));
        self
    }

    /// Index of the slice layer: the visible layer touching the plane.
    pub fn slice_layer(&self, bounds: BoundSize) -> i32 {
        let p = self.clamped(bounds);
        match p.keep_side() {
            KeepSide::Negative => p.coord - 1,
            KeepSide::Positive => p.coord,
        }
        .clamp(0, bounds.0[p.axis] as i32 - 1)
    }

    /// Moves the plane so that `layer` becomes the slice layer.
    pub fn set_slice_layer(&mut self, layer: i32, bounds: BoundSize) {
        self.coord = match self.keep_side() {
            KeepSide::Negative => layer + 1,
            KeepSide::Positive => layer,
        };
        *self = self.clamped(bounds);
    }

    /// Hides the other side, keeping the same slice layer.
    pub fn flip_side(&mut self, bounds: BoundSize) {
        let layer = self.slice_layer(bounds);
        self.flip = !self.flip;
        self.set_slice_layer(layer, bounds);
    }

    /// Region drawing tools may write while slicing: the slice layer only.
    pub fn edit_box(&self, bounds: BoundSize) -> Option<VoxelBox> {
        if !self.slice {
            return None;
        }
        let layer = self.slice_layer(bounds);
        let mut b = VoxelBox::from_bounds(bounds);
        b.min[self.axis.min(2)] = layer;
        b.max[self.axis.min(2)] = layer + 1;
        Some(b)
    }

    /// Places the plane on the hit face (the face's own plane).
    pub fn set_from_hit(&mut self, hit: &RayHit) {
        self.axis = hit.axis();
        self.coord = hit.face_coord();
    }

    /// Side that stays visible while slicing.
    pub fn keep_side(&self) -> KeepSide {
        if self.flip {
            KeepSide::Positive
        } else {
            KeepSide::Negative
        }
    }

    /// `+1` when the hidden side is toward `+axis`, `-1` otherwise.
    pub fn hidden_sign(&self) -> i32 {
        if self.flip {
            -1
        } else {
            1
        }
    }

    /// Boundary of the hidden part while slicing: one layer past the
    /// slice layer, so the voxels just beyond the plane stay visible.
    fn view_cut(&self, bounds: BoundSize) -> i32 {
        let p = self.clamped(bounds);
        let size = bounds.0[p.axis] as i32;
        match p.keep_side() {
            KeepSide::Negative => (p.coord + 1).min(size),
            KeepSide::Positive => (p.coord - 1).max(0),
        }
    }

    /// Visible region while slicing (slice layer, the side behind it and
    /// one layer beyond it); `None` when not slicing.
    pub fn clip_box(&self, bounds: BoundSize) -> Option<VoxelBox> {
        if !self.slice {
            return None;
        }
        let p = self.clamped(bounds);
        let cut = self.view_cut(bounds);
        let mut b = VoxelBox::from_bounds(bounds);
        match p.keep_side() {
            KeepSide::Negative => b.max[p.axis] = cut,
            KeepSide::Positive => b.min[p.axis] = cut,
        }
        Some(b)
    }

    /// Destructive cut that removes the hidden part.
    pub fn cut_request(&self, bounds: BoundSize, cap: bool) -> SliceRequest {
        let p = self.clamped(bounds);
        SliceRequest {
            axis: p.axis,
            cut: self.view_cut(bounds),
            keep: p.keep_side(),
            cap,
        }
    }

    /// Pick on the plane inside the bounds' cross-section. The target cell
    /// is reported so that additive tools write `hit.adjacent()` and the
    /// others act on `hit.voxel`, both being the same cell:
    ///
    /// - without Slice, the cell on the camera's side of the plane (a
    ///   drawing surface);
    /// - with Slice, the slice layer cell under the cursor, from whichever
    ///   side the camera looks at it (Qubicle's 2D slice mode).
    pub fn pick(&self, ray: &Ray, bounds: BoundSize, additive: bool) -> Option<RayHit> {
        if !self.active() {
            return None;
        }
        let p = self.clamped(bounds);
        let axis = p.axis;
        let (plane_at, layer) = if p.slice {
            let layer = p.slice_layer(bounds);
            (layer as f32 + 0.5, Some(layer))
        } else {
            (p.coord as f32, None)
        };
        let point = raycast_face(ray, axis, plane_at)?;
        let mut cell = point.floor().as_ivec3();
        let size = bounds.0.as_ivec3();
        for a in (0..3).filter(|&a| a != axis) {
            if cell[a] < 0 || cell[a] >= size[a] {
                return None;
            }
        }
        // Unit normal pointing back toward the camera.
        let mut toward = IVec3::ZERO;
        toward[axis] = if ray.dir[axis] > 0.0 { -1 } else { 1 };
        cell[axis] = match layer {
            Some(l) => l,
            None if ray.dir[axis] > 0.0 => p.coord - 1,
            None => p.coord,
        };
        Some(RayHit {
            voxel: if additive { cell - toward } else { cell },
            normal: toward,
            t: (point - ray.origin).length(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;

    fn b() -> BoundSize {
        BoundSize::new(8, 8, 8)
    }

    #[test]
    fn placed_on_the_clicked_face() {
        let mut p = WorkPlane::default();
        p.set_from_hit(&RayHit {
            voxel: IVec3::new(2, 3, 4),
            normal: IVec3::Y,
            t: 0.0,
        });
        assert_eq!((p.axis, p.coord), (1, 4));
        p.set_from_hit(&RayHit {
            voxel: IVec3::new(2, 3, 4),
            normal: IVec3::NEG_X,
            t: 0.0,
        });
        assert_eq!((p.axis, p.coord), (0, 2));
        p.coord = 99;
        assert_eq!(p.clamped(b()).coord, 8);
    }

    #[test]
    fn clip_box_keeps_one_side() {
        let mut p = WorkPlane {
            axis: 0,
            coord: 3,
            ..WorkPlane::default()
        };
        assert_eq!(p.clip_box(b()), None);
        p.slice = true;
        // Plane at x = 3: slice layer 2 and layer 3 beyond it stay visible.
        let c = p.clip_box(b()).unwrap();
        assert_eq!((c.min.x, c.max.x), (0, 4));
        assert_eq!(p.cut_request(b(), false).cut, 4);
        p.flip = true;
        let c = p.clip_box(b()).unwrap();
        assert_eq!((c.min.x, c.max.x), (2, 8));
        assert_eq!(p.cut_request(b(), false).keep, KeepSide::Positive);
    }

    #[test]
    fn surface_pick_faces_the_camera() {
        let p = WorkPlane {
            axis: 1,
            coord: 2,
            visible: true,
            ..WorkPlane::default()
        };
        let down = Ray::new(Vec3::new(3.5, 10.0, 3.5), Vec3::NEG_Y);
        let hit = p.pick(&down, b(), true).unwrap();
        assert_eq!(hit.adjacent(), IVec3::new(3, 2, 3));
        assert_eq!(
            p.pick(&down, b(), false).unwrap().voxel,
            IVec3::new(3, 2, 3)
        );
        let up = Ray::new(Vec3::new(3.5, -10.0, 3.5), Vec3::Y);
        assert_eq!(
            p.pick(&up, b(), true).unwrap().adjacent(),
            IVec3::new(3, 1, 3)
        );
        let outside = Ray::new(Vec3::new(30.5, 10.0, 3.5), Vec3::NEG_Y);
        assert_eq!(p.pick(&outside, b(), true), None);
        let hidden = WorkPlane {
            visible: false,
            ..p
        };
        assert_eq!(hidden.pick(&down, b(), true), None);
    }

    #[test]
    fn slice_pick_targets_the_visible_layer_from_both_sides() {
        let p = WorkPlane {
            axis: 1,
            coord: 4,
            slice: true,
            ..WorkPlane::default()
        };
        for ray in [
            Ray::new(Vec3::new(1.5, 20.0, 1.5), Vec3::NEG_Y),
            Ray::new(Vec3::new(1.5, -20.0, 1.5), Vec3::Y),
        ] {
            let add = p.pick(&ray, b(), true).unwrap();
            assert_eq!(add.adjacent(), IVec3::new(1, 3, 1));
            let erase = p.pick(&ray, b(), false).unwrap();
            assert_eq!(erase.voxel, IVec3::new(1, 3, 1));
        }
    }

    #[test]
    fn slice_layer_stays_visible_and_is_the_only_editable_one() {
        // Qubicle: slicing X at layer 3 keeps layers 0..=3 visible.
        let mut p = WorkPlane {
            axis: 0,
            slice: true,
            ..WorkPlane::default()
        };
        p.set_slice_layer(3, b());
        assert_eq!(p.coord, 4);
        assert_eq!(p.slice_layer(b()), 3);
        let view = p.clip_box(b()).unwrap();
        assert_eq!((view.min.x, view.max.x), (0, 5), "layer 4 stays visible");
        let edit = p.edit_box(b()).unwrap();
        assert_eq!((edit.min.x, edit.max.x), (3, 4));
        assert_eq!(edit.size(), IVec3::new(1, 8, 8));

        // Flipping keeps the same slice layer and shows the other side.
        p.flip_side(b());
        assert_eq!(p.slice_layer(b()), 3);
        let view = p.clip_box(b()).unwrap();
        assert_eq!((view.min.x, view.max.x), (2, 8));

        // The slice layer never leaves the bounds.
        p.set_slice_layer(99, b());
        assert_eq!(p.slice_layer(b()), 7);
        p.flip_side(b());
        p.set_slice_layer(-5, b());
        assert_eq!(p.slice_layer(b()), 0);
        assert!(p.clip_box(b()).unwrap().is_valid());
        assert_eq!(WorkPlane::default().edit_box(b()), None);
    }
}

//! The Plane tool's work plane (Goxel `goxel.plane`) and Qubicle-style
//! Slice Mode.
//!
//! Without Slice the plane is a drawing surface: clicking a face with the
//! Plane tool places it there, and while it is visible drawing tools snap
//! onto it, so voxels can be placed in empty space.
//!
//! With Slice on (Qubicle 3.1.2 Slice Mode, a global toggle) the plane
//! selects one *slice layer*. The side between the slice and the camera is
//! hidden (view only; [`WorkPlane::face_camera`] flips it with the camera),
//! the far side stays visible behind the slice sheet, and every tool edits
//! the slice layer only ([`WorkPlane::slice_box`]). Edits write straight
//! into the model, so turning Slice off shows them merged. The destructive
//! "Cut" removes the hidden part through [`super::slice_layers`].

use glam::{IVec3, Vec3};
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
    /// Slice Mode: hide the camera's side and edit the slice layer only.
    pub slice: bool,
    /// Hide the negative side instead of the positive one. While slicing it
    /// follows the camera ([`WorkPlane::face_camera`]).
    pub flip: bool,
    /// `(axis, layer)` of the last slice, restored when Slice Mode is
    /// turned on away from a voxel.
    #[serde(default)]
    pub last_slice: Option<(usize, i32)>,
}

impl Default for WorkPlane {
    fn default() -> Self {
        Self {
            axis: 1,
            coord: 0,
            visible: false,
            slice: false,
            flip: false,
            last_slice: None,
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

    /// Number of slices along the plane's axis.
    pub fn slice_count(&self, bounds: BoundSize) -> i32 {
        bounds.0[self.axis.min(2)] as i32
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

    /// While slicing, hides the side the camera is on (`eye` in voxel
    /// space), keeping the slice layer. Returns whether the side changed.
    pub fn face_camera(&mut self, eye: Vec3, bounds: BoundSize) -> bool {
        if !self.slice {
            return false;
        }
        let center = self.slice_layer(bounds) as f32 + 0.5;
        let flip = eye[self.axis.min(2)] < center;
        if flip == self.flip {
            return false;
        }
        self.flip_side(bounds);
        true
    }

    /// Places the plane on the hit face (the face's own plane).
    pub fn set_from_hit(&mut self, hit: &RayHit) {
        self.axis = hit.axis();
        self.coord = hit.face_coord();
    }

    /// Slices at the hit voxel: the axis of the hit face, the voxel's layer.
    pub fn slice_at(&mut self, hit: &RayHit, bounds: BoundSize) {
        self.axis = hit.axis();
        self.set_slice_layer(hit.voxel[self.axis], bounds);
    }

    /// Slice Mode toggle (Qubicle's `2D` button and `Space`). Turning it on
    /// over a voxel (`hover`) slices at that voxel ([`WorkPlane::slice_at`]);
    /// otherwise the last slice comes back (Y layer 0 the first time).
    /// Turning it off remembers the slice. Returns the new state.
    pub fn toggle_slice(&mut self, hover: Option<&RayHit>, bounds: BoundSize) -> bool {
        if self.slice {
            self.last_slice = Some((self.axis, self.slice_layer(bounds)));
            self.slice = false;
            return false;
        }
        self.slice = true;
        match hover {
            Some(hit) => self.slice_at(hit, bounds),
            None => {
                let (axis, layer) = self.last_slice.unwrap_or((1, 0));
                self.axis = axis.min(2);
                self.set_slice_layer(layer, bounds);
            }
        }
        true
    }

    /// Moves the slice by `steps` layers away from the camera (Qubicle's
    /// *Next*; negative steps are *Previous*).
    pub fn step_slice(&mut self, steps: i32, bounds: BoundSize) {
        let layer = self.slice_layer(bounds) - self.hidden_sign() * steps;
        self.set_slice_layer(layer, bounds);
    }

    /// Moves the slice onto the axis and layer of `other` (the slice an
    /// undone edit was made on), keeping the hidden side. Outside Slice
    /// Mode it only becomes the slice to restore.
    pub fn restore_slice(&mut self, other: &WorkPlane, bounds: BoundSize) {
        let target = (other.axis.min(2), other.slice_layer(bounds));
        if self.slice {
            self.axis = target.0;
            self.set_slice_layer(target.1, bounds);
        } else {
            self.last_slice = Some(target);
        }
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

    /// Coordinate of the translucent slice sheet: the slice layer's face
    /// toward the visible far side, so the slice voxels are drawn in front
    /// of it and the far side behind it. Without Slice, the plane itself.
    pub fn sheet_coord(&self, bounds: BoundSize) -> i32 {
        let p = self.clamped(bounds);
        if !p.slice {
            return p.coord;
        }
        let layer = p.slice_layer(bounds);
        match p.keep_side() {
            KeepSide::Negative => layer,
            KeepSide::Positive => layer + 1,
        }
    }

    /// Visible region while slicing (the slice layer and the far side);
    /// `None` when not slicing.
    pub fn clip_box(&self, bounds: BoundSize) -> Option<VoxelBox> {
        if !self.slice {
            return None;
        }
        let p = self.clamped(bounds);
        let mut b = VoxelBox::from_bounds(bounds);
        match p.keep_side() {
            KeepSide::Negative => b.max[p.axis] = p.coord,
            KeepSide::Positive => b.min[p.axis] = p.coord,
        }
        Some(b)
    }

    /// The slice layer: the only editable and pickable region while
    /// slicing; `None` when not slicing.
    pub fn slice_box(&self, bounds: BoundSize) -> Option<VoxelBox> {
        if !self.slice {
            return None;
        }
        let p = self.clamped(bounds);
        let layer = p.slice_layer(bounds);
        let mut b = VoxelBox::from_bounds(bounds);
        b.min[p.axis] = layer;
        b.max[p.axis] = layer + 1;
        Some(b)
    }

    /// Destructive cut that removes the hidden part.
    pub fn cut_request(&self, bounds: BoundSize, cap: bool) -> SliceRequest {
        let p = self.clamped(bounds);
        SliceRequest {
            axis: p.axis,
            cut: p.coord,
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

    fn b() -> BoundSize {
        BoundSize::new(8, 8, 8)
    }

    fn hit(voxel: IVec3, normal: IVec3) -> RayHit {
        RayHit {
            voxel,
            normal,
            t: 0.0,
        }
    }

    #[test]
    fn placed_on_the_clicked_face() {
        let mut p = WorkPlane::default();
        p.set_from_hit(&hit(IVec3::new(2, 3, 4), IVec3::Y));
        assert_eq!((p.axis, p.coord), (1, 4));
        p.set_from_hit(&hit(IVec3::new(2, 3, 4), IVec3::NEG_X));
        assert_eq!((p.axis, p.coord), (0, 2));
        p.coord = 99;
        assert_eq!(p.clamped(b()).coord, 8);
    }

    #[test]
    fn clip_box_keeps_the_slice_and_the_far_side() {
        let mut p = WorkPlane {
            axis: 0,
            coord: 3,
            ..WorkPlane::default()
        };
        assert_eq!(p.clip_box(b()), None);
        assert_eq!(p.slice_box(b()), None);
        p.slice = true;
        // Plane at x = 3, +X hidden: slice layer 2 and everything below it.
        let c = p.clip_box(b()).unwrap();
        assert_eq!((c.min.x, c.max.x), (0, 3));
        let s = p.slice_box(b()).unwrap();
        assert_eq!((s.min.x, s.max.x), (2, 3));
        assert_eq!(p.cut_request(b(), false).cut, 3);
        p.flip_side(b());
        let c = p.clip_box(b()).unwrap();
        assert_eq!((c.min.x, c.max.x), (2, 8));
        assert_eq!(p.slice_box(b()), Some(s), "same slice layer");
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
    fn slice_layer_survives_flips_and_stays_in_bounds() {
        let mut p = WorkPlane {
            axis: 0,
            slice: true,
            ..WorkPlane::default()
        };
        p.set_slice_layer(3, b());
        assert_eq!(p.coord, 4);
        assert_eq!(p.slice_layer(b()), 3);
        let view = p.clip_box(b()).unwrap();
        assert_eq!((view.min.x, view.max.x), (0, 4));
        assert_eq!(p.sheet_coord(b()), 3, "sheet on the far face");

        p.flip_side(b());
        assert_eq!(p.slice_layer(b()), 3);
        let view = p.clip_box(b()).unwrap();
        assert_eq!((view.min.x, view.max.x), (3, 8));
        assert_eq!(p.sheet_coord(b()), 4);

        p.set_slice_layer(99, b());
        assert_eq!(p.slice_layer(b()), 7);
        p.flip_side(b());
        p.set_slice_layer(-5, b());
        assert_eq!(p.slice_layer(b()), 0);
        assert!(p.clip_box(b()).unwrap().is_valid());
        assert_eq!(WorkPlane::default().clip_box(b()), None);
    }

    #[test]
    fn the_camera_side_is_hidden() {
        let mut p = WorkPlane {
            axis: 2,
            slice: true,
            ..WorkPlane::default()
        };
        p.set_slice_layer(5, b());
        // Camera in front (+Z): the +Z side is hidden.
        p.face_camera(Vec3::new(4.0, 4.0, 30.0), b());
        assert_eq!(p.hidden_sign(), 1);
        assert_eq!(p.clip_box(b()).unwrap().max.z, 6);
        // Camera behind: the -Z side is hidden, same slice.
        assert!(p.face_camera(Vec3::new(4.0, 4.0, -30.0), b()));
        assert_eq!(p.hidden_sign(), -1);
        assert_eq!(p.slice_layer(b()), 5);
        assert_eq!(p.clip_box(b()).unwrap().min.z, 5);
        assert!(!p.face_camera(Vec3::new(4.0, 4.0, -30.0), b()));
        // Never while not slicing.
        p.slice = false;
        assert!(!p.face_camera(Vec3::new(4.0, 4.0, 30.0), b()));
    }

    #[test]
    fn toggle_slices_at_the_hovered_voxel_or_restores_the_last_slice() {
        let mut p = WorkPlane::default();
        // First time over empty space: Y, layer 0.
        assert!(p.toggle_slice(None, b()));
        assert_eq!((p.axis, p.slice_layer(b())), (1, 0));
        assert!(!p.toggle_slice(None, b()));
        // Over the front face of voxel (5, 4, 3): Z slice at layer 3.
        assert!(p.toggle_slice(Some(&hit(IVec3::new(5, 4, 3), IVec3::Z)), b()));
        assert_eq!((p.axis, p.slice_layer(b())), (2, 3));
        p.flip_side(b());
        assert!(!p.toggle_slice(None, b()));
        assert_eq!(p.last_slice, Some((2, 3)));
        // Over a side face of (2, 4, 3): X slice at layer 2.
        p.toggle_slice(Some(&hit(IVec3::new(2, 4, 3), IVec3::NEG_X)), b());
        assert_eq!((p.axis, p.slice_layer(b())), (0, 2));
        p.toggle_slice(None, b());
        // Empty space restores X layer 2.
        p.axis = 1;
        p.toggle_slice(None, b());
        assert_eq!((p.axis, p.slice_layer(b())), (0, 2));
    }

    #[test]
    fn next_moves_away_from_the_camera_and_restore_moves_back() {
        let mut p = WorkPlane {
            axis: 2,
            slice: true,
            ..WorkPlane::default()
        };
        p.set_slice_layer(5, b());
        p.face_camera(Vec3::new(4.0, 4.0, 30.0), b());
        p.step_slice(1, b());
        assert_eq!(p.slice_layer(b()), 4, "Next goes away from the camera");
        p.flip_side(b());
        p.step_slice(1, b());
        assert_eq!(p.slice_layer(b()), 5);
        p.step_slice(-99, b());
        assert_eq!(p.slice_layer(b()), 0);

        let edit = p;
        p.axis = 1;
        p.set_slice_layer(7, b());
        p.restore_slice(&edit, b());
        assert_eq!((p.axis, p.slice_layer(b()), p.flip), (2, 0, edit.flip));
        p.slice = false;
        p.restore_slice(&WorkPlane { coord: 4, ..edit }, b());
        assert_eq!(p.last_slice, Some((2, 4)));
    }
}

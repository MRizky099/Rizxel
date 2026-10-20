//! Shape tool: a cube, sphere or cylinder dragged out from an anchor cell.
//!
//! The drag spans a rectangle on the plane of the pressed face (anchor to
//! the hovered cell, projected onto that plane). The shape then grows away
//! from the face by the rectangle's smaller side, so a square drag gives a
//! cube, a ball or a cylinder as tall as it is wide; with
//! [`ShapeOptions::planar`] it stays one voxel thick.

use glam::IVec3;
use rizxel_math::{BoundSize, VoxelBox};

use super::brush::soft_box_shape;
use super::{apply_symmetry, ShapeOptions};

/// Box of a shape drag. `anchor` and `current` are cells; `grow` is the
/// unit direction the shape extends in from the anchor's plane (`+normal`
/// for additive operations, `-normal` to carve into the model).
pub fn shape_box(anchor: IVec3, current: IVec3, grow: IVec3, planar: bool) -> VoxelBox {
    let axis = (0..3).find(|&a| grow[a] != 0).unwrap_or(1);
    let mut end = current;
    end[axis] = anchor[axis];
    let extent = (end - anchor).abs() + IVec3::ONE;
    let depth = if planar {
        1
    } else {
        (0..3)
            .filter(|&a| a != axis)
            .map(|a| extent[a])
            .min()
            .unwrap_or(1)
    };
    end[axis] = anchor[axis] + grow[axis].signum() * (depth - 1);
    VoxelBox::from_corners(anchor, end)
}

/// Cells of the shape inscribed in `area` (cylinders stand along Y), after
/// antialiasing and symmetry.
pub fn shape_cells(area: &VoxelBox, opts: &ShapeOptions, bounds: BoundSize) -> Vec<IVec3> {
    if !area.is_valid() {
        return Vec::new();
    }
    let cells = soft_box_shape(area.min, area.max, opts.shape, opts.antialiasing);
    apply_symmetry(cells, bounds, opts.symmetry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BrushShape;

    fn count(n: i32, shape: BrushShape) -> usize {
        let area = shape_box(IVec3::ZERO, IVec3::new(n - 1, 0, n - 1), IVec3::Y, false);
        let opts = ShapeOptions {
            shape,
            ..ShapeOptions::default()
        };
        shape_cells(&area, &opts, BoundSize::new(16, 16, 16)).len()
    }

    #[test]
    fn square_drags_give_solid_shapes() {
        for n in 1..=3 {
            assert_eq!(count(n, BrushShape::Cube), (n * n * n) as usize);
        }
        assert_eq!(count(1, BrushShape::Sphere), 1);
        assert_eq!(count(2, BrushShape::Sphere), 8);
        // 3³ ball: center, 6 faces and 12 edges; corners are outside.
        assert_eq!(count(3, BrushShape::Sphere), 19);
        assert_eq!(count(1, BrushShape::Cylinder), 1);
        assert_eq!(count(2, BrushShape::Cylinder), 8);
        assert_eq!(count(3, BrushShape::Cylinder), 27);
        // Larger shapes approach their analytic volume.
        let n = 12;
        let r = n as f32 / 2.0;
        let sphere = count(n, BrushShape::Sphere) as f32;
        let expected = 4.0 / 3.0 * std::f32::consts::PI * r * r * r;
        assert!((sphere - expected).abs() < expected * 0.08, "{sphere}");
        let cylinder = count(n, BrushShape::Cylinder) as f32;
        let expected = std::f32::consts::PI * r * r * n as f32;
        assert!((cylinder - expected).abs() < expected * 0.08, "{cylinder}");
    }

    #[test]
    fn box_grows_away_from_the_face() {
        // Drag on top of the model (+Y face) from (1, 4, 1) to (4, 9, 2):
        // the hovered height is ignored, depth is the smaller side (2).
        let b = shape_box(IVec3::new(1, 4, 1), IVec3::new(4, 9, 2), IVec3::Y, false);
        assert_eq!((b.min, b.max), (IVec3::new(1, 4, 1), IVec3::new(5, 6, 3)));
        // Carving goes into the model.
        let b = shape_box(
            IVec3::new(1, 4, 1),
            IVec3::new(4, 4, 2),
            IVec3::NEG_Y,
            false,
        );
        assert_eq!((b.min, b.max), (IVec3::new(1, 3, 1), IVec3::new(5, 5, 3)));
        let flat = shape_box(IVec3::new(1, 4, 1), IVec3::new(4, 4, 2), IVec3::Y, true);
        assert_eq!(flat.size(), IVec3::new(4, 1, 2));
    }
}

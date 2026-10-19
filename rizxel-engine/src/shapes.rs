//! Rasterization of shape layers (cube, sphere, cylinder) into voxels.

use glam::{IVec3, Vec3};
use rizxel_math::{BoundSize, ShapeKind, Voxel, VoxelBox, VoxelVolume, EMPTY_VOXEL};

/// Fills the part of `area` inside `bounds` with `color` following `shape`.
/// Spheres and cylinders are inscribed in `area`: a cell is filled when its
/// center lies inside the primitive.
pub fn rasterize_shape(
    shape: ShapeKind,
    area: &VoxelBox,
    color: Voxel,
    bounds: BoundSize,
) -> VoxelVolume {
    let mut out = VoxelVolume::new(bounds);
    let clip = area.intersect(&VoxelBox::from_bounds(bounds));
    if color == EMPTY_VOXEL || !area.is_valid() || !clip.is_valid() {
        return out;
    }
    let center = (area.min + area.max).as_vec3() * 0.5;
    let half = area.size().as_vec3() * 0.5;
    let inside = |p: IVec3| -> bool {
        // Normalized offset of the cell center from the shape center.
        let d = (p.as_vec3() + Vec3::splat(0.5) - center) / half;
        match shape {
            ShapeKind::Cube => true,
            ShapeKind::Sphere => d.length_squared() <= 1.0,
            ShapeKind::Cylinder => d.x * d.x + d.z * d.z <= 1.0,
        }
    };
    for z in clip.min.z..clip.max.z {
        for y in clip.min.y..clip.max.y {
            for x in clip.min.x..clip.max.x {
                let p = IVec3::new(x, y, z);
                if inside(p) {
                    out.set(p, color);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(n: i32) -> VoxelBox {
        VoxelBox::from_corners(IVec3::ZERO, IVec3::splat(n - 1))
    }

    #[test]
    fn cube_fills_box_clipped_to_bounds() {
        let b = BoundSize::new(4, 4, 4);
        let v = rasterize_shape(ShapeKind::Cube, &area(6), 3, b);
        assert_eq!(v.voxel_count(), 64);
        assert_eq!(v.get(IVec3::splat(3)), 3);
    }

    #[test]
    fn sphere_and_cylinder_are_inscribed() {
        let b = BoundSize::new(16, 16, 16);
        let sphere = rasterize_shape(ShapeKind::Sphere, &area(10), 2, b);
        let cylinder = rasterize_shape(ShapeKind::Cylinder, &area(10), 2, b);
        let cube = rasterize_shape(ShapeKind::Cube, &area(10), 2, b);
        assert!(sphere.voxel_count() < cylinder.voxel_count());
        assert!(cylinder.voxel_count() < cube.voxel_count());
        // Center filled, corners empty.
        assert_eq!(sphere.get(IVec3::splat(5)), 2);
        assert_eq!(sphere.get(IVec3::ZERO), 0);
        // Cylinder spans the full height along its axis.
        assert_eq!(cylinder.get(IVec3::new(5, 0, 5)), 2);
        assert_eq!(cylinder.get(IVec3::new(5, 9, 5)), 2);
        // Symmetric: volume close to (4/3)πr³ for r = 5.
        let expected = 4.0 / 3.0 * std::f32::consts::PI * 125.0;
        assert!((sphere.voxel_count() as f32 - expected).abs() < expected * 0.1);
    }

    #[test]
    fn empty_color_or_box_gives_nothing() {
        let b = BoundSize::new(8, 8, 8);
        assert!(rasterize_shape(ShapeKind::Cube, &area(4), 0, b).is_empty());
        let outside = VoxelBox::from_corners(IVec3::splat(20), IVec3::splat(22));
        assert!(rasterize_shape(ShapeKind::Cube, &outside, 1, b).is_empty());
    }
}

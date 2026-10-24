//! Selection tool: box selection between two corner cells.
//!
//! The tool picks the voxel under the press as one corner and the voxel
//! (or the press face's plane) under the pointer as the other; every solid
//! cell inside the box joins the selection, combined with the existing one
//! by the [`super::MaskMode`]. The selection type itself lives in the
//! application layer; this module only computes cells.
//!
//! Box mode (Phase 10c) keeps the box open after the drag: its faces are
//! moved by handles or typed values ([`resize_face`], [`move_box`],
//! [`fit_box`]) and the cells inside are re-selected live.

use std::collections::HashSet;

use glam::{IVec3, Vec3};
use rizxel_math::{Aabb, VoxelBox, VoxelVolume, EMPTY_VOXEL, TILE_SIZE};

/// Largest box (in cells) whose selection is recomputed on every change;
/// bigger boxes update when the handle is released.
pub const MAX_LIVE_BOX: i64 = 512 * 1024;

/// Box spanning the cells `a` and `b` (both included).
pub fn selection_box(a: IVec3, b: IVec3) -> VoxelBox {
    VoxelBox::from_corners(a, b)
}

/// Solid cells of any of `volumes` inside `area`, sorted (z, y, x) so the
/// result is deterministic.
pub fn select_box(volumes: &[&VoxelVolume], area: &VoxelBox) -> Vec<IVec3> {
    if !area.is_valid() {
        return Vec::new();
    }
    let region = Aabb::new(area.min.as_vec3(), area.max.as_vec3());
    let mut seen = HashSet::new();
    for v in volumes {
        // Only the tiles overlapping the box are visited.
        for (origin, tile) in v.tiles_in(Some(&region)) {
            let lo = (area.min - *origin).max(IVec3::ZERO);
            let hi = (area.max - *origin).min(IVec3::splat(TILE_SIZE));
            for z in lo.z..hi.z {
                for y in lo.y..hi.y {
                    for x in lo.x..hi.x {
                        let l = IVec3::new(x, y, z);
                        if tile.get_local(l) != EMPTY_VOXEL {
                            seen.insert(*origin + l);
                        }
                    }
                }
            }
        }
    }
    let mut out: Vec<IVec3> = seen.into_iter().collect();
    out.sort_by_key(|p| (p.z, p.y, p.x));
    out
}

/// Number of cells in `area`.
pub fn box_cells(area: &VoxelBox) -> i64 {
    area.size().as_i64vec3().element_product()
}

/// `area` moved into `limit` and shrunk to fit it, at least one cell per
/// axis (typed position and size values).
pub fn fit_box(area: VoxelBox, limit: &VoxelBox) -> VoxelBox {
    let room = limit.size().max(IVec3::ONE);
    let size = area.size().clamp(IVec3::ONE, room);
    let min = area.min.clamp(limit.min, limit.min + room - size);
    VoxelBox {
        min,
        max: min + size,
    }
}

/// `area` with its `+axis` (`positive`) or `-axis` face moved `amount`
/// cells outward (negative: inward). The box keeps at least one cell and
/// stays inside `limit`.
pub fn resize_face(
    area: VoxelBox,
    axis: usize,
    positive: bool,
    amount: i32,
    limit: &VoxelBox,
) -> VoxelBox {
    let mut out = area;
    if positive {
        out.max[axis] = (area.max[axis] + amount).clamp(area.min[axis] + 1, limit.max[axis]);
    } else {
        out.min[axis] = (area.min[axis] - amount).clamp(limit.min[axis], area.max[axis] - 1);
    }
    out
}

/// `area` moved by `offset`, stopped at the sides of `limit`.
pub fn move_box(area: VoxelBox, offset: IVec3, limit: &VoxelBox) -> VoxelBox {
    let moved = VoxelBox {
        min: area.min + offset,
        max: area.max + offset,
    };
    fit_box(moved, limit)
}

/// Face of `area` the ray `origin + t * dir` enters first: `(axis,
/// positive side, t)`, or `None` when the ray misses the box (or starts
/// inside it).
pub fn box_face_hit(area: &VoxelBox, origin: Vec3, dir: Vec3) -> Option<(usize, bool, f32)> {
    let (lo, hi) = (area.min.as_vec3(), area.max.as_vec3());
    let mut t_near = f32::NEG_INFINITY;
    let mut t_far = f32::INFINITY;
    let mut face = None;
    for axis in 0..3 {
        if dir[axis].abs() < 1e-9 {
            if origin[axis] < lo[axis] || origin[axis] > hi[axis] {
                return None;
            }
            continue;
        }
        let a = (lo[axis] - origin[axis]) / dir[axis];
        let b = (hi[axis] - origin[axis]) / dir[axis];
        // Moving toward +axis the ray enters through the min face.
        let (t0, t1, positive) = if a < b { (a, b, false) } else { (b, a, true) };
        if t0 > t_near {
            t_near = t0;
            face = Some((axis, positive));
        }
        t_far = t_far.min(t1);
    }
    let (axis, positive) = face?;
    (t_near <= t_far && t_near >= 0.0).then_some((axis, positive, t_near))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rizxel_math::BoundSize;

    fn bx(min: [i32; 3], max: [i32; 3]) -> VoxelBox {
        VoxelBox {
            min: IVec3::from_array(min),
            max: IVec3::from_array(max),
        }
    }

    #[test]
    fn resize_face_keeps_one_cell_and_the_bounds() {
        let limit = VoxelBox::from_bounds(BoundSize::new(10, 10, 10));
        let area = bx([2, 2, 2], [5, 5, 5]);
        assert_eq!(
            resize_face(area, 0, true, 2, &limit),
            bx([2, 2, 2], [7, 5, 5])
        );
        assert_eq!(
            resize_face(area, 1, false, 1, &limit),
            bx([2, 1, 2], [5, 5, 5])
        );
        assert_eq!(
            resize_face(area, 2, true, -1, &limit),
            bx([2, 2, 2], [5, 5, 4])
        );
        // Collapsing stops at one cell, from either side.
        for amount in [-3, -10] {
            assert_eq!(resize_face(area, 0, true, amount, &limit).size().x, 1);
            let b = resize_face(area, 0, false, amount, &limit);
            assert_eq!((b.min.x, b.size().x), (4, 1));
        }
        // Growing stops at the bounds.
        assert_eq!(resize_face(area, 0, true, 50, &limit).max.x, 10);
        assert_eq!(resize_face(area, 0, false, 50, &limit).min.x, 0);
        for k in -20..20 {
            for axis in 0..3 {
                for positive in [false, true] {
                    let r = resize_face(area, axis, positive, k, &limit);
                    assert!(r.size().min_element() >= 1);
                    assert_eq!(r.intersect(&limit), r, "inside the bounds");
                }
            }
        }
    }

    #[test]
    fn move_and_fit_clamp_to_the_bounds() {
        let limit = VoxelBox::from_bounds(BoundSize::new(8, 4, 8));
        let area = bx([1, 1, 1], [4, 3, 3]);
        assert_eq!(
            move_box(area, IVec3::new(2, 0, -1), &limit),
            bx([3, 1, 0], [6, 3, 2])
        );
        // Stops at the far side with its size kept.
        assert_eq!(
            move_box(area, IVec3::new(20, 20, -20), &limit),
            bx([5, 2, 0], [8, 4, 2])
        );
        // Typed sizes: at least 1, at most the bounds.
        let typed = VoxelBox {
            min: IVec3::new(-3, 2, 6),
            max: IVec3::new(-3, 12, 7),
        };
        assert_eq!(fit_box(typed, &limit), bx([0, 0, 6], [1, 4, 7]));
        // A slice slab locks one axis to one cell.
        let slab = bx([0, 2, 0], [8, 3, 8]);
        assert_eq!(fit_box(area, &slab), bx([1, 2, 1], [4, 3, 3]));
    }

    #[test]
    fn box_over_a_hollow_cube_selects_its_shell() {
        let mut v = VoxelVolume::new(BoundSize::new(40, 40, 40));
        // 5×5×5 hollow cube at 14..19 (across a tile border at 16).
        let inner = |c: i32| (15..18).contains(&c);
        for z in 14..19 {
            for y in 14..19 {
                for x in 14..19 {
                    if !(inner(x) && inner(y) && inner(z)) {
                        v.set(IVec3::new(x, y, z), 1);
                    }
                }
            }
        }
        v.set(IVec3::new(30, 30, 30), 2);
        let cells = select_box(&[&v], &bx([13, 13, 13], [20, 20, 20]));
        assert_eq!(cells.len(), 125 - 27);
        assert!(!cells.contains(&IVec3::splat(16)));
        // Same answer as filtering every voxel.
        let area = bx([15, 0, 0], [40, 17, 40]);
        let mut all: Vec<IVec3> = v
            .iter()
            .map(|(p, _)| p)
            .filter(|p| area.contains(*p))
            .collect();
        all.sort_by_key(|p| (p.z, p.y, p.x));
        assert_eq!(select_box(&[&v], &area), all);
        assert_eq!(box_cells(&bx([0, 0, 0], [2, 3, 4])), 24);
    }

    #[test]
    fn ray_finds_the_entered_face() {
        let area = bx([2, 2, 2], [4, 4, 4]);
        let hit = box_face_hit(&area, Vec3::new(3.0, 10.0, 3.0), Vec3::NEG_Y);
        assert_eq!(hit, Some((1, true, 6.0)));
        let hit = box_face_hit(&area, Vec3::new(-5.0, 3.0, 3.5), Vec3::X);
        assert_eq!(hit.map(|h| (h.0, h.1)), Some((0, false)));
        assert_eq!(box_face_hit(&area, Vec3::new(9.0, 9.0, 9.0), Vec3::X), None);
    }

    #[test]
    fn box_selects_solid_cells_of_every_volume() {
        let b = BoundSize::new(8, 8, 8);
        let mut low = VoxelVolume::new(b);
        let mut high = VoxelVolume::new(b);
        for x in 0..4 {
            low.set(IVec3::new(x, 0, 0), 1);
        }
        high.set(IVec3::new(1, 0, 0), 2);
        high.set(IVec3::new(1, 1, 0), 2);
        let area = selection_box(IVec3::new(2, 1, 0), IVec3::ZERO);
        assert_eq!((area.min, area.max), (IVec3::ZERO, IVec3::new(3, 2, 1)));
        assert_eq!(select_box(&[&low], &area).len(), 3);
        // Overlapping voxels count once.
        assert_eq!(
            select_box(&[&low, &high], &area),
            vec![
                IVec3::new(0, 0, 0),
                IVec3::new(1, 0, 0),
                IVec3::new(2, 0, 0),
                IVec3::new(1, 1, 0),
            ]
        );
        // A single-cell box on an empty cell selects nothing.
        let empty = selection_box(IVec3::splat(5), IVec3::splat(5));
        assert!(select_box(&[&low, &high], &empty).is_empty());
    }
}

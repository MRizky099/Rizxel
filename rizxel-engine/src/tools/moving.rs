//! Move tool: translates the active layer (or only its selected voxels) by
//! whole voxels, or resizes the bounds in Resize drag mode.

use std::collections::{HashMap, HashSet};

use glam::{IVec3, Vec2};
use rizxel_math::{BoundSize, Voxel, VoxelVolume, EMPTY_VOXEL};

use super::{filter_placeable, VoxelChange};

/// Screen drag to a voxel offset along one world axis: picks the axis whose
/// on-screen direction best matches `delta` and measures how many voxels it
/// covers. `axis_screen[i]` is the screen motion (pixels) of one voxel along
/// `+axis i`. Returns `None` while no axis is visible on screen.
pub fn drag_axis(delta: Vec2, axis_screen: [Vec2; 3]) -> Option<(usize, i32)> {
    let dir = delta.normalize_or_zero();
    let (axis, s) = axis_screen
        .iter()
        .enumerate()
        .filter(|(_, s)| s.length() > 0.5)
        .max_by(|(_, a), (_, b)| {
            let score = |s: &Vec2| dir.dot(s.normalize()).abs();
            score(a).total_cmp(&score(b))
        })?;
    let amount = (delta.dot(*s) / s.length_squared()).round() as i32;
    Some((axis, amount))
}

/// Bounds grown (or shrunk) by `delta` voxels along `axis`, clamped to the
/// valid bound sizes.
pub fn resize_bounds(bounds: BoundSize, axis: usize, delta: i32) -> BoundSize {
    let mut size = bounds.0.as_ivec3();
    size[axis] = (size[axis] + delta).max(BoundSize::MIN as i32);
    BoundSize::new(size.x as u32, size.y as u32, size.z as u32)
}

/// Changes that move every voxel of `volume` by `offset`. Voxels pushed
/// outside the bounds are dropped (the yellow bounds are a hard limit);
/// undo brings them back.
pub fn move_changes(volume: &VoxelVolume, offset: IVec3) -> Vec<VoxelChange> {
    if offset == IVec3::ZERO {
        return Vec::new();
    }
    let moved: HashMap<IVec3, Voxel> = volume.iter().map(|(p, v)| (p + offset, v)).collect();
    let mut changes: Vec<VoxelChange> = volume
        .iter()
        .map(|(pos, old)| VoxelChange {
            pos,
            old,
            new: moved.get(&pos).copied().unwrap_or(EMPTY_VOXEL),
        })
        .collect();
    changes.extend(
        moved
            .iter()
            .filter(|(p, _)| volume.get(**p) == EMPTY_VOXEL)
            .map(|(&pos, &new)| VoxelChange {
                pos,
                old: EMPTY_VOXEL,
                new,
            }),
    );
    filter_placeable(changes, volume.bounds())
}

/// Changes that move the selected solid voxels of `volume` by `offset`;
/// unselected voxels stay. Where a moved voxel lands on an unselected one
/// it overwrites it, or, without `overwrite`, the whole move is refused
/// (`None`). Voxels pushed outside the bounds are dropped.
pub fn move_selection_changes(
    volume: &VoxelVolume,
    selection: &HashSet<IVec3>,
    offset: IVec3,
    overwrite: bool,
) -> Option<Vec<VoxelChange>> {
    if offset == IVec3::ZERO {
        return Some(Vec::new());
    }
    let sources: HashMap<IVec3, Voxel> = selection
        .iter()
        .map(|&p| (p, volume.get(p)))
        .filter(|&(_, v)| v != EMPTY_VOXEL)
        .collect();
    let moved: HashMap<IVec3, Voxel> = sources.iter().map(|(&p, &v)| (p + offset, v)).collect();
    let collides = moved
        .keys()
        .any(|d| !sources.contains_key(d) && volume.get(*d) != EMPTY_VOXEL);
    if collides && !overwrite {
        return None;
    }
    let mut changes: Vec<VoxelChange> = sources
        .iter()
        .map(|(&pos, &old)| VoxelChange {
            pos,
            old,
            new: moved.get(&pos).copied().unwrap_or(EMPTY_VOXEL),
        })
        .collect();
    changes.extend(
        moved
            .iter()
            .filter(|(p, _)| !sources.contains_key(*p))
            .map(|(&pos, &new)| VoxelChange {
                pos,
                old: volume.get(pos),
                new,
            }),
    );
    Some(filter_placeable(changes, volume.bounds()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_move_leaves_the_rest_and_handles_collisions() {
        let mut v = VoxelVolume::new(BoundSize::new(8, 8, 8));
        for x in 0..4 {
            v.set(IVec3::new(x, 0, 0), 1 + x as Voxel);
        }
        let sel: HashSet<IVec3> = [IVec3::new(0, 0, 0), IVec3::new(1, 0, 0), IVec3::Y].into();
        let apply = |changes: &[VoxelChange]| {
            let mut out = v.clone();
            for c in changes {
                assert_eq!(v.get(c.pos), c.old);
                out.set(c.pos, c.new);
            }
            out
        };
        // Up by one: no collision, x = 2..3 stay.
        let up = move_selection_changes(&v, &sel, IVec3::Y, false).unwrap();
        let out = apply(&up);
        assert_eq!(out.get(IVec3::new(0, 1, 0)), 1);
        assert_eq!(out.get(IVec3::new(1, 1, 0)), 2);
        assert_eq!(out.get(IVec3::new(2, 0, 0)), 3);
        assert_eq!(out.voxel_count(), 4);
        // Right by two lands on x = 2..3: refused, or overwritten.
        assert_eq!(move_selection_changes(&v, &sel, IVec3::X * 2, false), None);
        let out = apply(&move_selection_changes(&v, &sel, IVec3::X * 2, true).unwrap());
        assert_eq!(out.get(IVec3::new(2, 0, 0)), 1);
        assert_eq!(out.get(IVec3::new(3, 0, 0)), 2);
        assert_eq!(out.voxel_count(), 2);
        // Left by one overlaps only itself: allowed without overwrite; the
        // voxel pushed past x = 0 leaves the bounds.
        let out = apply(&move_selection_changes(&v, &sel, IVec3::NEG_X, false).unwrap());
        assert_eq!(out.get(IVec3::ZERO), 2);
        assert_eq!(out.voxel_count(), 3);
    }

    #[test]
    fn move_offsets_every_voxel() {
        let mut v = VoxelVolume::new(BoundSize::new(8, 8, 8));
        v.set(IVec3::new(0, 0, 0), 1);
        v.set(IVec3::new(1, 0, 0), 2);
        v.set(IVec3::new(7, 0, 0), 3);
        let changes = move_changes(&v, IVec3::new(3, 0, 0));
        let mut out = v.clone();
        for c in &changes {
            assert_eq!(v.get(c.pos), c.old);
            out.set(c.pos, c.new);
        }
        assert_eq!(out.get(IVec3::new(3, 0, 0)), 1);
        assert_eq!(out.get(IVec3::new(4, 0, 0)), 2);
        assert_eq!(out.voxel_count(), 2, "x = 10 leaves the bounds");
        assert!(move_changes(&v, IVec3::ZERO).is_empty());
    }

    #[test]
    fn drag_picks_the_best_aligned_axis() {
        let axes = [
            Vec2::new(10.0, 0.0),
            Vec2::new(0.0, -10.0),
            Vec2::new(-6.0, 6.0),
        ];
        assert_eq!(drag_axis(Vec2::new(31.0, 2.0), axes), Some((0, 3)));
        assert_eq!(drag_axis(Vec2::new(0.0, 20.0), axes), Some((1, -2)));
        assert_eq!(drag_axis(Vec2::new(-12.0, 12.0), axes), Some((2, 2)));
        assert_eq!(drag_axis(Vec2::ONE, [Vec2::ZERO; 3]), None);
    }

    #[test]
    fn resize_clamps() {
        let b = BoundSize::new(8, 8, 8);
        assert_eq!(resize_bounds(b, 1, 4), BoundSize::new(8, 12, 8));
        assert_eq!(resize_bounds(b, 0, -20), BoundSize::new(1, 8, 8));
        assert_eq!(resize_bounds(b, 2, 1000), BoundSize::new(8, 8, 256));
    }
}

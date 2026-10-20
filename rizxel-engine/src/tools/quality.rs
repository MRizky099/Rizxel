//! Shared placement filter: every tool's output goes through here so the
//! yellow model bounds are a hard limit and no-op writes never reach the
//! undo history.

use std::collections::{HashMap, HashSet};

use glam::IVec3;
use rizxel_math::{BoundSize, Voxel, VoxelVolume, EMPTY_VOXEL};

use super::{VoxelChange, VoxelOp};

fn in_bounds(p: IVec3, bounds: BoundSize) -> bool {
    p.cmpge(IVec3::ZERO).all() && p.as_uvec3().cmplt(bounds.0).all()
}

/// Drops positions outside `0..bounds` and changes with `old == new`. A
/// position listed twice keeps its first `old` and its last `new`.
pub fn filter_placeable(changes: Vec<VoxelChange>, bounds: BoundSize) -> Vec<VoxelChange> {
    let mut index: HashMap<IVec3, usize> = HashMap::with_capacity(changes.len());
    let mut out: Vec<VoxelChange> = Vec::with_capacity(changes.len());
    for c in changes {
        if !in_bounds(c.pos, bounds) {
            continue;
        }
        match index.get(&c.pos) {
            Some(&i) => out[i].new = c.new,
            None => {
                index.insert(c.pos, out.len());
                out.push(c);
            }
        }
    }
    out.retain(|c| c.old != c.new);
    out
}

/// Applies `op` with `color` to the given cells of `volume`:
///
/// - `Add` writes `color` everywhere (overwriting existing voxels),
/// - `Join` writes `color` into empty cells only (existing colors win),
/// - `Subtract` clears solid cells,
/// - `Paint` recolors solid cells.
///
/// Duplicate positions are ignored; the result is bounds-filtered.
pub fn apply_op(
    volume: &VoxelVolume,
    positions: impl IntoIterator<Item = IVec3>,
    op: VoxelOp,
    color: Voxel,
) -> Vec<VoxelChange> {
    let mut seen = HashSet::new();
    let changes = positions
        .into_iter()
        .filter(|p| seen.insert(*p))
        .filter_map(|pos| {
            let old = volume.get(pos);
            let solid = old != EMPTY_VOXEL;
            let new = match op {
                VoxelOp::Add => color,
                VoxelOp::Join if !solid => color,
                VoxelOp::Subtract if solid => EMPTY_VOXEL,
                VoxelOp::Paint if solid => color,
                _ => return None,
            };
            Some(VoxelChange { pos, old, new })
        })
        .collect();
    filter_placeable(changes, volume.bounds())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(x: i32, old: Voxel, new: Voxel) -> VoxelChange {
        VoxelChange {
            pos: IVec3::new(x, 0, 0),
            old,
            new,
        }
    }

    #[test]
    fn filter_drops_out_of_bounds_and_noops() {
        let b = BoundSize::new(4, 4, 4);
        let out = filter_placeable(
            vec![c(0, 0, 1), c(-1, 0, 1), c(4, 0, 1), c(1, 2, 2), c(0, 1, 3)],
            b,
        );
        assert_eq!(out, vec![c(0, 0, 3)]);
    }

    #[test]
    fn ops_follow_their_semantics() {
        let mut v = VoxelVolume::new(BoundSize::new(4, 4, 4));
        v.set(IVec3::ZERO, 2);
        let cells = [IVec3::ZERO, IVec3::X, IVec3::X];
        let add = apply_op(&v, cells, VoxelOp::Add, 5);
        assert_eq!(add, vec![c(0, 2, 5), c(1, 0, 5)]);
        assert_eq!(apply_op(&v, cells, VoxelOp::Join, 5), vec![c(1, 0, 5)]);
        assert_eq!(apply_op(&v, cells, VoxelOp::Subtract, 5), vec![c(0, 2, 0)]);
        assert_eq!(apply_op(&v, cells, VoxelOp::Paint, 5), vec![c(0, 2, 5)]);
        assert!(apply_op(&v, cells, VoxelOp::Paint, 2).is_empty(), "no-op");
    }
}

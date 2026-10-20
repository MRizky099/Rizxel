//! Shared placement filter: every tool's output goes through here so the
//! yellow model bounds are a hard limit and no-op writes never reach the
//! undo history. Also the guarded flood fill and color distance shared by
//! Fill and Fuzzy Select.

use std::collections::{HashMap, HashSet, VecDeque};

use glam::IVec3;
use rizxel_math::{BoundSize, Palette, Rgba8, Voxel, VoxelVolume, EMPTY_VOXEL};

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

/// Hard cap on the cells one flood may visit (Fill, Fuzzy Select). Larger
/// regions abort instead of stalling the editor.
pub const MAX_FLOOD_CELLS: usize = 2_000_000;

/// The six face neighbours of a cell.
pub const NEIGHBORS_6: [IVec3; 6] = [
    IVec3::X,
    IVec3::NEG_X,
    IVec3::Y,
    IVec3::NEG_Y,
    IVec3::Z,
    IVec3::NEG_Z,
];

/// A flood stopped at [`MAX_FLOOD_CELLS`] (or the cap passed to [`flood`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FloodCapped {
    pub limit: usize,
}

impl FloodCapped {
    pub fn message(&self) -> String {
        format!(
            "Region too large (over {} cells): nothing changed",
            self.limit
        )
    }
}

/// Breadth-first flood over 6-connected cells from `seed`, visiting the
/// cells for which `accept` holds (the seed included). Returns the cells in
/// visit order, or [`FloodCapped`] once more than `limit` cells match.
pub fn flood(
    seed: IVec3,
    limit: usize,
    mut accept: impl FnMut(IVec3) -> bool,
) -> Result<Vec<IVec3>, FloodCapped> {
    if !accept(seed) {
        return Ok(Vec::new());
    }
    let mut seen = HashSet::from([seed]);
    let mut queue = VecDeque::from([seed]);
    let mut out = Vec::new();
    while let Some(p) = queue.pop_front() {
        out.push(p);
        if out.len() > limit {
            return Err(FloodCapped { limit });
        }
        for n in NEIGHBORS_6 {
            let q = p + n;
            if !seen.contains(&q) && accept(q) {
                seen.insert(q);
                queue.push_back(q);
            }
        }
    }
    Ok(out)
}

/// Largest per-channel (RGBA) difference between two colors.
pub fn color_distance(a: Rgba8, b: Rgba8) -> u8 {
    [
        a.r.abs_diff(b.r),
        a.g.abs_diff(b.g),
        a.b.abs_diff(b.b),
        a.a.abs_diff(b.a),
    ]
    .into_iter()
    .max()
    .unwrap_or(0)
}

/// True when palette entries `a` and `b` differ by at most `threshold`
/// (equal indices always match, missing entries read as transparent black).
pub fn colors_match(palette: &Palette, a: Voxel, b: Voxel, threshold: u8) -> bool {
    if a == b {
        return true;
    }
    let rgba = |v: Voxel| palette.get(v as usize).unwrap_or_default();
    color_distance(rgba(a), rgba(b)) <= threshold
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flood_visits_connected_cells_and_caps() {
        let inside = |p: IVec3| p.cmpge(IVec3::ZERO).all() && p.cmplt(IVec3::splat(3)).all();
        assert_eq!(flood(IVec3::ONE, 100, inside).unwrap().len(), 27);
        assert_eq!(flood(IVec3::splat(9), 100, inside).unwrap(), Vec::new());
        assert_eq!(
            flood(IVec3::ONE, 10, inside),
            Err(FloodCapped { limit: 10 })
        );
    }

    #[test]
    fn color_distance_is_the_largest_channel_difference() {
        let a = Rgba8::new(10, 20, 30, 255);
        assert_eq!(color_distance(a, a), 0);
        assert_eq!(color_distance(a, Rgba8::new(15, 20, 22, 255)), 8);
        assert_eq!(color_distance(a, Rgba8::new(10, 20, 30, 0)), 255);
    }

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

//! One-shot transforms of a selection or layer: 90° rotation, flip and
//! mirror. Pure functions over voxel cells; the app turns their result into
//! one undo step.

use std::collections::{HashMap, HashSet};

use glam::IVec3;
use rizxel_math::{BoundSize, Voxel, VoxelVolume, EMPTY_VOXEL};

use super::symmetry::mirror as mirror_cell;
use super::VoxelChange;

/// A rotation or flip of the cells inside a box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transform {
    /// Quarter turns about `axis`, counterclockwise seen from `+axis`
    /// (right-handed). Negative turns go clockwise.
    Rotate { axis: usize, turns: i32 },
    /// Reverses the cells along `axis`.
    Flip { axis: usize },
}

impl Transform {
    /// Size of a `size` box after the transform.
    pub fn size(self, size: IVec3) -> IVec3 {
        match self {
            Transform::Rotate { axis, turns } if turns.rem_euclid(2) == 1 => {
                let (a, b) = other_axes(axis);
                let mut out = size;
                out[a] = size[b];
                out[b] = size[a];
                out
            }
            _ => size,
        }
    }

    /// Box `(min, size)` after the transform: same center, snapped to whole
    /// cells. Truncating division keeps a turn and its inverse exact, so
    /// four turns (or two flips) are the identity.
    pub fn bounds(self, min: IVec3, size: IVec3) -> (IVec3, IVec3) {
        let new = self.size(size);
        (min + (size - new) / 2, new)
    }

    /// Where cell `p` of the box `(min, size)` lands.
    pub fn apply(self, p: IVec3, min: IVec3, size: IVec3) -> IVec3 {
        let (new_min, _) = self.bounds(min, size);
        let l = p - min;
        let local = match self {
            Transform::Flip { axis } => {
                let mut out = l;
                out[axis] = size[axis] - 1 - l[axis];
                out
            }
            Transform::Rotate { axis, turns } => {
                let (a, b) = other_axes(axis);
                let mut out = l;
                let mut s = size;
                for _ in 0..turns.rem_euclid(4) {
                    // (a, b) -> (-b, a), shifted back into the box.
                    let (la, lb) = (out[a], out[b]);
                    out[a] = s[b] - 1 - lb;
                    out[b] = la;
                    (s[a], s[b]) = (s[b], s[a]);
                }
                out
            }
        };
        new_min + local
    }
}

/// The two axes a rotation about `axis` turns, in right-handed order.
fn other_axes(axis: usize) -> (usize, usize) {
    match axis {
        0 => (1, 2),
        1 => (2, 0),
        _ => (0, 1),
    }
}

/// Box `(min, size)` around `cells`.
pub fn cells_box(cells: impl IntoIterator<Item = IVec3>) -> Option<(IVec3, IVec3)> {
    cells
        .into_iter()
        .map(|p| (p, p + IVec3::ONE))
        .reduce(|(a0, a1), (b0, b1)| (a0.min(b0), a1.max(b1)))
        .map(|(lo, hi)| (lo, hi - lo))
}

/// `cells` rotated by quarter turns about `axis` around the center of their
/// box `(min, size)`.
pub fn rotate90(
    cells: &[(IVec3, Voxel)],
    axis: usize,
    turns: i32,
    min: IVec3,
    size: IVec3,
) -> Vec<(IVec3, Voxel)> {
    let t = Transform::Rotate { axis, turns };
    cells
        .iter()
        .map(|&(p, v)| (t.apply(p, min, size), v))
        .collect()
}

/// `cells` reversed along `axis` inside their box `(min, size)`.
pub fn flip(cells: &[(IVec3, Voxel)], axis: usize, min: IVec3, size: IVec3) -> Vec<(IVec3, Voxel)> {
    let t = Transform::Flip { axis };
    cells
        .iter()
        .map(|&(p, v)| (t.apply(p, min, size), v))
        .collect()
}

/// Changes that replace the voxels `from` (cleared) with `to` (written,
/// overwriting what is there). `None` when a target leaves `bounds`: a
/// transform is refused as a whole, never clipped.
pub fn relocate_changes(
    volume: &VoxelVolume,
    from: &[(IVec3, Voxel)],
    to: &[(IVec3, Voxel)],
) -> Option<Vec<VoxelChange>> {
    if to.iter().any(|&(p, _)| !volume.in_bounds(p)) {
        return None;
    }
    let mut new: HashMap<IVec3, Voxel> = from.iter().map(|&(p, _)| (p, EMPTY_VOXEL)).collect();
    new.extend(to.iter().copied());
    let mut changes: Vec<VoxelChange> = new
        .into_iter()
        .map(|(pos, new)| VoxelChange {
            pos,
            old: volume.get(pos),
            new,
        })
        .filter(|c| c.old != c.new)
        .collect();
    // Deterministic order (undo records and tests).
    changes.sort_by_key(|c| (c.pos.z, c.pos.y, c.pos.x));
    Some(changes)
}

/// One-shot mirror across the center plane of `bounds` on `axis` (the live
/// Symmetry plane): the cells of `region` on the `-axis` side are copied onto
/// the `+axis` side, so that side becomes their mirror image. Cells on an odd
/// axis' center plane stay. `region` is the selection, or `None` for every
/// cell of the layer.
pub fn mirror(
    volume: &VoxelVolume,
    region: Option<&HashSet<IVec3>>,
    axis: usize,
    bounds: BoundSize,
) -> Vec<VoxelChange> {
    let size = bounds.0[axis] as i32;
    let negative = |p: IVec3| 2 * p[axis] + 1 < size;
    let positive = |p: IVec3| 2 * p[axis] + 1 > size;
    let in_region = |p: IVec3| region.is_none_or(|r| r.contains(&p));
    let mut writes: HashMap<IVec3, Voxel> = HashMap::new();
    // Solid sources are copied across.
    for (p, v) in volume.iter() {
        if negative(p) && in_region(p) {
            let q = mirror_cell(p, bounds, axis);
            if region.is_none() || in_region(q) {
                writes.insert(q, v);
            }
        }
    }
    // A positive-side voxel whose mirror cell is empty is cleared.
    for (q, _) in volume.iter() {
        if positive(q) && in_region(q) && !writes.contains_key(&q) {
            let p = mirror_cell(q, bounds, axis);
            if in_region(p) && volume.get(p) == EMPTY_VOXEL {
                writes.insert(q, EMPTY_VOXEL);
            }
        }
    }
    let mut changes: Vec<VoxelChange> = writes
        .into_iter()
        .map(|(pos, new)| VoxelChange {
            pos,
            old: volume.get(pos),
            new,
        })
        .filter(|c| c.old != c.new)
        .collect();
    changes.sort_by_key(|c| (c.pos.z, c.pos.y, c.pos.x));
    changes
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An L of three colored cells (asymmetric on every axis).
    fn shape() -> Vec<(IVec3, Voxel)> {
        vec![
            (IVec3::new(2, 3, 4), 1),
            (IVec3::new(3, 3, 4), 2),
            (IVec3::new(2, 4, 4), 3),
            (IVec3::new(2, 3, 5), 4),
        ]
    }

    fn boxed(cells: &[(IVec3, Voxel)]) -> (IVec3, IVec3) {
        cells_box(cells.iter().map(|c| c.0)).unwrap()
    }

    fn sorted(mut v: Vec<(IVec3, Voxel)>) -> Vec<(IVec3, Voxel)> {
        v.sort_by_key(|(p, _)| (p.z, p.y, p.x));
        v
    }

    #[test]
    fn four_turns_and_two_flips_are_the_identity() {
        // Uneven boxes too: 4×1×2 rotates into 1×4×2 and back.
        let long = vec![(IVec3::new(5, 5, 5), 1), (IVec3::new(8, 5, 6), 2)];
        for cells in [shape(), long] {
            for axis in 0..3 {
                let mut c = cells.clone();
                for _ in 0..4 {
                    let (min, size) = boxed(&c);
                    c = rotate90(&c, axis, 1, min, size);
                }
                assert_eq!(sorted(c), sorted(cells.clone()), "rotate axis {axis}");
                // A turn and its inverse (on the turned box) cancel out.
                let (min, size) = boxed(&cells);
                let t = Transform::Rotate { axis, turns: 1 };
                let (tmin, tsize) = t.bounds(min, size);
                let turned = rotate90(&cells, axis, 1, min, size);
                let back = rotate90(&turned, axis, -1, tmin, tsize);
                assert_eq!(sorted(back), sorted(cells.clone()), "inverse axis {axis}");
                let (min, size) = boxed(&cells);
                let twice = flip(&flip(&cells, axis, min, size), axis, min, size);
                assert_eq!(sorted(twice), sorted(cells.clone()), "flip axis {axis}");
            }
        }
    }

    #[test]
    fn rotation_about_y_matches_the_expected_cells() {
        // Box min (2,3,4), size (2,2,2). +90° about Y maps (x, z) -> (z, -x):
        // +X goes to -Z and +Z goes to +X.
        let cells = shape();
        let (min, size) = boxed(&cells);
        let out = sorted(rotate90(&cells, 1, 1, min, size));
        let expected = sorted(vec![
            (IVec3::new(2, 3, 5), 1),
            (IVec3::new(2, 3, 4), 2),
            (IVec3::new(2, 4, 5), 3),
            (IVec3::new(3, 3, 5), 4),
        ]);
        assert_eq!(out, expected);
    }

    #[test]
    fn rotation_keeps_the_center() {
        // A 4×1×1 bar along X turns into a 1×1×4 bar along Z around the same
        // center, snapped by half a cell (center x 12 -> 11.5, z 10.5 -> 11).
        let bar: Vec<_> = (0..4).map(|x| (IVec3::new(10 + x, 0, 10), 1)).collect();
        let (min, size) = boxed(&bar);
        let out = rotate90(&bar, 1, 1, min, size);
        let (nmin, nsize) = boxed(&out);
        assert_eq!(nsize, IVec3::new(1, 1, 4));
        assert_eq!(nmin, IVec3::new(11, 0, 9));
    }

    #[test]
    fn leaving_the_bounds_is_refused() {
        let mut v = VoxelVolume::new(BoundSize::new(8, 8, 8));
        let bar: Vec<_> = (0..4).map(|y| (IVec3::new(0, y, 0), 1)).collect();
        for &(p, c) in &bar {
            v.set(p, c);
        }
        let (min, size) = boxed(&bar);
        // About Z the 1×4 bar becomes 4×1 centered at x = 0: out of bounds.
        let turned = rotate90(&bar, 2, 1, min, size);
        assert!(relocate_changes(&v, &bar, &turned).is_none());
        let flipped = flip(&bar, 1, min, size);
        let changes = relocate_changes(&v, &bar, &flipped).unwrap();
        assert!(changes.is_empty(), "a uniform bar flips onto itself");
    }

    #[test]
    fn mirror_copies_the_negative_half() {
        let b = BoundSize::new(8, 4, 4);
        let mut v = VoxelVolume::new(b);
        v.set(IVec3::new(1, 0, 0), 5);
        v.set(IVec3::new(5, 1, 0), 6); // + side, no counterpart: cleared
        let changes = mirror(&v, None, 0, b);
        let mut out = v.clone();
        for c in &changes {
            out.set(c.pos, c.new);
        }
        assert_eq!(out.get(IVec3::new(6, 0, 0)), 5);
        assert_eq!(out.get(IVec3::new(5, 1, 0)), EMPTY_VOXEL);
        assert_eq!(out.voxel_count(), 2);

        // Within a selection only selected cells take part.
        let sel: HashSet<IVec3> = [IVec3::new(1, 0, 0), IVec3::new(6, 0, 0)].into();
        let c = mirror(&v, Some(&sel), 0, b);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].pos, IVec3::new(6, 0, 0));
    }
}

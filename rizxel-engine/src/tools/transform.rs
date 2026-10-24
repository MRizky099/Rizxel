//! One-shot transforms of a selection or layer: 90° rotation, flip and
//! mirror, and the Transform tool's free rotation and scaling (Phase 10c).
//! Pure functions over voxel cells; the app turns their result into one
//! undo step.

use std::collections::{HashMap, HashSet};

use glam::{Affine3A, IVec3, Mat3, Vec3};
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

/// Free transform of the Transform tool: scale, then rotate (about X, then
/// Y, then Z), about a pivot, then move.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransformParams {
    /// Rotation in degrees about X, Y and Z (right-handed).
    pub rotate: Vec3,
    /// Scale factor per axis (1 = unchanged).
    pub scale: Vec3,
    /// Move in voxels, applied last.
    pub offset: IVec3,
}

impl Default for TransformParams {
    fn default() -> Self {
        Self {
            rotate: Vec3::ZERO,
            scale: Vec3::ONE,
            offset: IVec3::ZERO,
        }
    }
}

/// Smallest scale factor the Transform tool accepts.
pub const MIN_SCALE: f32 = 1.0 / 64.0;
/// Largest scale factor the Transform tool accepts.
pub const MAX_SCALE: f32 = 64.0;

impl TransformParams {
    pub fn is_identity(&self) -> bool {
        *self == Self::default()
    }

    /// Rotation and scale as one matrix (scale first).
    pub fn matrix(&self) -> Mat3 {
        let r = self.rotate * (std::f32::consts::PI / 180.0);
        Mat3::from_rotation_z(r.z)
            * Mat3::from_rotation_y(r.y)
            * Mat3::from_rotation_x(r.x)
            * Mat3::from_diagonal(self.scale)
    }

    /// Quarter turns per axis and integer up/down scale factors when the
    /// transform is exact on the grid: every angle a multiple of 90° and
    /// every scale `n` or `1/n`.
    pub fn exact(&self) -> Option<([i32; 3], IVec3, IVec3)> {
        let mut turns = [0; 3];
        let (mut up, mut down) = (IVec3::ONE, IVec3::ONE);
        for a in 0..3 {
            let q = self.rotate[a] / 90.0;
            if (q - q.round()).abs() > 1e-4 {
                return None;
            }
            turns[a] = (q.round() as i32).rem_euclid(4);
            let s = self.scale[a];
            if s >= 1.0 && (s - s.round()).abs() < 1e-4 {
                up[a] = s.round() as i32;
            } else if s > 0.0 && s < 1.0 && ((1.0 / s) - (1.0 / s).round()).abs() < 1e-3 {
                down[a] = (1.0 / s).round() as i32;
            } else {
                return None;
            }
        }
        Some((turns, up, down))
    }
}

/// `cells` mapped through the affine `m` about `pivot` (voxel space:
/// `p' = m(p - pivot) + pivot`), resampled by inverse nearest-neighbour
/// mapping: every destination cell inside the transformed box takes the
/// source voxel under its center mapped back. Unlike moving each source
/// voxel forward, this never leaves holes.
pub fn transform_cells(cells: &[(IVec3, Voxel)], m: Affine3A, pivot: Vec3) -> Vec<(IVec3, Voxel)> {
    let Some((min, size)) = cells_box(cells.iter().map(|c| c.0)) else {
        return Vec::new();
    };
    let det = m.matrix3.determinant();
    if !det.is_finite() || det.abs() < 1e-9 {
        return Vec::new();
    }
    let source: HashMap<IVec3, Voxel> = cells.iter().copied().collect();
    let forward = |p: Vec3| m.transform_point3(p - pivot) + pivot;
    let (lo, hi) = (min.as_vec3(), (min + size).as_vec3());
    let mut dmin = Vec3::splat(f32::INFINITY);
    let mut dmax = Vec3::splat(f32::NEG_INFINITY);
    for i in 0..8 {
        let corner = Vec3::new(
            if i & 1 == 0 { lo.x } else { hi.x },
            if i & 2 == 0 { lo.y } else { hi.y },
            if i & 4 == 0 { lo.z } else { hi.z },
        );
        let q = forward(corner);
        dmin = dmin.min(q);
        dmax = dmax.max(q);
    }
    // Small tolerance so exact boxes do not grow by float noise.
    let (dlo, dhi) = (
        (dmin + Vec3::splat(1e-3)).floor().as_ivec3(),
        (dmax - Vec3::splat(1e-3)).ceil().as_ivec3(),
    );
    let inverse = m.inverse();
    let mut out = Vec::new();
    for z in dlo.z..dhi.z {
        for y in dlo.y..dhi.y {
            for x in dlo.x..dhi.x {
                let q = IVec3::new(x, y, z);
                let back = inverse.transform_point3(q.as_vec3() + Vec3::splat(0.5) - pivot) + pivot;
                if let Some(&v) = source.get(&back.floor().as_ivec3()) {
                    out.push((q, v));
                }
            }
        }
    }
    out
}

fn floor_div(a: IVec3, b: IVec3) -> IVec3 {
    IVec3::new(
        a.x.div_euclid(b.x),
        a.y.div_euclid(b.y),
        a.z.div_euclid(b.z),
    )
}

/// Exact integer scaling about the cell corner `pivot`: each voxel becomes
/// an `up` block per axis; `down` merges blocks (aligned to `pivot`) into
/// one voxel of their most common color, kept when at least half the
/// block is solid. Down-scaling is done first.
pub fn scale_cells(
    cells: &[(IVec3, Voxel)],
    up: IVec3,
    down: IVec3,
    pivot: IVec3,
) -> Vec<(IVec3, Voxel)> {
    let (up, down) = (up.max(IVec3::ONE), down.max(IVec3::ONE));
    let mut merged: Vec<(IVec3, Voxel)> = if down == IVec3::ONE {
        cells.to_vec()
    } else {
        let mut blocks: HashMap<IVec3, HashMap<Voxel, u32>> = HashMap::new();
        for &(p, v) in cells {
            let b = floor_div(p - pivot, down);
            *blocks.entry(b).or_default().entry(v).or_default() += 1;
        }
        let volume = down.x * down.y * down.z;
        blocks
            .into_iter()
            .filter_map(|(b, colors)| {
                let solid: u32 = colors.values().sum();
                if (solid as i32) * 2 < volume {
                    return None;
                }
                let (&v, _) = colors
                    .iter()
                    .max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0)))?;
                Some((pivot + b, v))
            })
            .collect()
    };
    if up != IVec3::ONE {
        merged = merged
            .into_iter()
            .flat_map(|(p, v)| {
                let base = pivot + (p - pivot) * up;
                (0..up.z).flat_map(move |z| {
                    (0..up.y)
                        .flat_map(move |y| (0..up.x).map(move |x| (base + IVec3::new(x, y, z), v)))
                })
            })
            .collect();
    }
    merged.sort_by_key(|(p, _)| (p.z, p.y, p.x));
    merged
}

/// `cells` transformed by `params` about `pivot` (voxel space). Exact on
/// the grid when [`TransformParams::exact`]: integer scaling about the
/// pivot's cell corner and Phase 8 quarter turns, moved so they turn about
/// the pivot; otherwise [`transform_cells`].
pub fn apply_transform(
    cells: &[(IVec3, Voxel)],
    params: &TransformParams,
    pivot: Vec3,
) -> Vec<(IVec3, Voxel)> {
    let mut out = match params.exact() {
        Some((turns, up, down)) => {
            let mut cur = scale_cells(cells, up, down, pivot.floor().as_ivec3());
            for (axis, &t) in turns.iter().enumerate() {
                if t == 0 {
                    continue;
                }
                let Some((min, size)) = cells_box(cur.iter().map(|c| c.0)) else {
                    break;
                };
                cur = rotate90(&cur, axis, t, min, size);
                // `rotate90` turns about the box center; shift onto a turn
                // about the pivot.
                let mut angles = Vec3::ZERO;
                angles[axis] = t as f32 * 90.0;
                let r = TransformParams {
                    rotate: angles,
                    ..TransformParams::default()
                }
                .matrix();
                let center = min.as_vec3() + size.as_vec3() * 0.5;
                let shift = (r * (center - pivot) - (center - pivot)).round().as_ivec3();
                for c in &mut cur {
                    c.0 += shift;
                }
            }
            cur
        }
        None => transform_cells(cells, Affine3A::from_mat3(params.matrix()), pivot),
    };
    for c in &mut out {
        c.0 += params.offset;
    }
    out.sort_by_key(|(p, _)| (p.z, p.y, p.x));
    out
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

    fn center_of(cells: &[(IVec3, Voxel)]) -> Vec3 {
        let (min, size) = boxed(cells);
        min.as_vec3() + size.as_vec3() * 0.5
    }

    fn rotated(axis: usize, degrees: f32) -> TransformParams {
        let mut p = TransformParams::default();
        p.rotate[axis] = degrees;
        p
    }

    #[test]
    fn quarter_turns_match_rotate90_and_four_are_the_identity() {
        let long = vec![(IVec3::new(5, 5, 5), 1), (IVec3::new(8, 5, 6), 2)];
        for cells in [shape(), long] {
            for axis in 0..3 {
                let (min, size) = boxed(&cells);
                let p = rotated(axis, 90.0);
                let once = apply_transform(&cells, &p, center_of(&cells));
                assert_eq!(once, sorted(rotate90(&cells, axis, 1, min, size)));
                let mut c = cells.clone();
                for _ in 0..4 {
                    c = apply_transform(&c, &p, center_of(&c));
                }
                assert_eq!(c, sorted(cells.clone()), "axis {axis}");
                // -90 is the inverse, 360 the identity.
                let back = apply_transform(&once, &rotated(axis, -90.0), center_of(&once));
                assert_eq!(back, sorted(cells.clone()));
                let full = apply_transform(&cells, &rotated(axis, 360.0), center_of(&cells));
                assert_eq!(full, sorted(cells.clone()));
            }
        }
    }

    #[test]
    fn quarter_turns_about_a_corner_pivot() {
        // A bar along +X turned 90° about Y around its first cell's corner
        // ends up along -Z from that corner.
        let bar: Vec<_> = (0..3).map(|x| (IVec3::new(10 + x, 0, 10), 1)).collect();
        let out = apply_transform(&bar, &rotated(1, 90.0), Vec3::new(10.0, 0.0, 10.0));
        let cells: Vec<IVec3> = out.iter().map(|c| c.0).collect();
        assert_eq!(
            cells,
            vec![
                IVec3::new(10, 0, 7),
                IVec3::new(10, 0, 8),
                IVec3::new(10, 0, 9)
            ]
        );
    }

    #[test]
    fn integer_scales_are_exact() {
        let cells = shape();
        let pivot = center_of(&cells);
        let up = TransformParams {
            scale: Vec3::splat(2.0),
            ..TransformParams::default()
        };
        let big = apply_transform(&cells, &up, pivot);
        assert_eq!(big.len(), cells.len() * 8);
        // Each voxel became a 2×2×2 block of its color.
        let colors: HashMap<IVec3, Voxel> = big.iter().copied().collect();
        for &(p, v) in &big {
            let corner = p - (p - pivot.floor().as_ivec3()).rem_euclid(IVec3::splat(2));
            assert_eq!(colors.get(&corner), Some(&v));
        }
        let down = TransformParams {
            scale: Vec3::splat(0.5),
            ..TransformParams::default()
        };
        assert_eq!(apply_transform(&big, &down, pivot), sorted(cells.clone()));
        // A model made of 2×2×2 blocks survives ½ then 2×.
        let small = apply_transform(&big, &down, pivot);
        assert_eq!(apply_transform(&small, &up, pivot), big);
        // Per axis: 3× along X only.
        let wide = TransformParams {
            scale: Vec3::new(3.0, 1.0, 1.0),
            ..TransformParams::default()
        };
        assert_eq!(apply_transform(&cells, &wide, pivot).len(), cells.len() * 3);
        assert!(wide.exact().is_some());
        assert!(TransformParams {
            scale: Vec3::new(1.5, 1.0, 1.0),
            ..TransformParams::default()
        }
        .exact()
        .is_none());
        assert!(rotated(0, 45.0).exact().is_none());
    }

    #[test]
    fn majority_color_wins_when_scaling_down() {
        // One 2×2×2 block: 5 of color 3, 2 of color 4, one empty.
        let mut cells = Vec::new();
        for (i, p) in (0..8)
            .map(|i| IVec3::new(i & 1, (i >> 1) & 1, (i >> 2) & 1))
            .enumerate()
        {
            match i {
                0..=4 => cells.push((p, 3)),
                5 | 6 => cells.push((p, 4)),
                _ => {}
            }
        }
        let out = scale_cells(&cells, IVec3::ONE, IVec3::splat(2), IVec3::ZERO);
        assert_eq!(out, vec![(IVec3::ZERO, 3)]);
        // Less than half solid: dropped.
        let sparse = scale_cells(&cells[..3], IVec3::ONE, IVec3::splat(2), IVec3::ZERO);
        assert!(sparse.is_empty());
    }

    #[test]
    fn free_rotation_leaves_no_holes() {
        let mut cube = Vec::new();
        for z in 0..8 {
            for y in 0..4 {
                for x in 0..8 {
                    cube.push((IVec3::new(20 + x, 10 + y, 20 + z), 1));
                }
            }
        }
        let p = rotated(1, 45.0);
        let out = apply_transform(&cube, &p, center_of(&cube));
        // About the same volume.
        let n = out.len() as f32;
        assert!(
            (n - cube.len() as f32).abs() < cube.len() as f32 * 0.15,
            "{n}"
        );
        // Every row of every layer is one solid run: no holes inside.
        let set: HashSet<IVec3> = out.iter().map(|c| c.0).collect();
        for &(q, _) in &out {
            let mut far = q;
            while set.contains(&(far + IVec3::X)) {
                far += IVec3::X;
            }
            let mut gap = far + IVec3::X * 2;
            let (min, size) = boxed(&out);
            while gap.x < min.x + size.x {
                assert!(!set.contains(&gap), "hole before {gap}");
                gap += IVec3::X;
            }
        }
        // The rotated layers keep their height.
        assert!(out.iter().all(|c| (10..14).contains(&c.0.y)));
    }

    #[test]
    fn identity_and_offset() {
        let cells = shape();
        let id = TransformParams::default();
        assert!(id.is_identity());
        assert_eq!(
            apply_transform(&cells, &id, Vec3::ZERO),
            sorted(cells.clone())
        );
        let moved = TransformParams {
            offset: IVec3::new(1, -2, 3),
            ..TransformParams::default()
        };
        let out = apply_transform(&cells, &moved, Vec3::ZERO);
        assert_eq!(out[0].0, sorted(cells.clone())[0].0 + IVec3::new(1, -2, 3));
    }
}

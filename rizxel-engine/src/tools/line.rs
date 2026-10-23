//! Line tool paths: straight lines, polylines and Bézier curves through
//! voxel centers, and the connectivity filters that make them clean pixel
//! art. A path is a list of cells, each one stamped with the brush.

use std::collections::HashSet;

use glam::{IVec3, Vec3};

use super::brush::{line_cells, stamp_path};
use super::{Connectivity, ToolContext, VoxelChange};

/// Deepest subdivision of a curve (2^12 pieces).
const MAX_CURVE_DEPTH: u32 = 12;

/// Cells of the straight line from `a` to `b` (both included), one per
/// step of the longest axis (26-connected).
pub fn line_path(a: IVec3, b: IVec3) -> Vec<IVec3> {
    line_cells(a, b)
}

/// Cells of the segments through `points`, joined without repeating the
/// shared corners.
pub fn polyline_path(points: &[IVec3]) -> Vec<IVec3> {
    let mut out: Vec<IVec3> = points.first().copied().into_iter().collect();
    for w in points.windows(2) {
        out.extend(line_cells(w[0], w[1]).into_iter().skip(1));
    }
    out
}

fn bezier_point(c: &[Vec3], t: f32) -> Vec3 {
    // De Casteljau.
    let mut p: Vec<Vec3> = c.to_vec();
    while p.len() > 1 {
        p = p.windows(2).map(|w| w[0].lerp(w[1], t)).collect();
    }
    p[0]
}

/// Largest distance of the inner control points from the chord of `c`.
fn deviation(c: &[Vec3]) -> f32 {
    let (a, b) = (c[0], c[c.len() - 1]);
    let ab = b - a;
    let len2 = ab.length_squared();
    c[1..c.len() - 1]
        .iter()
        .map(|&p| {
            if len2 < 1e-9 {
                return p.distance(a);
            }
            let t = (p - a).dot(ab) / len2;
            // Control points beyond the chord's ends count fully.
            p.distance(a + ab * t.clamp(0.0, 1.0))
        })
        .fold(0.0, f32::max)
}

/// Splits the Bézier `c` at `t = 0.5` into its two halves.
fn split(c: &[Vec3]) -> (Vec<Vec3>, Vec<Vec3>) {
    let mut left = vec![c[0]];
    let mut right = vec![c[c.len() - 1]];
    let mut p: Vec<Vec3> = c.to_vec();
    while p.len() > 1 {
        p = p.windows(2).map(|w| w[0].lerp(w[1], 0.5)).collect();
        left.push(p[0]);
        right.push(p[p.len() - 1]);
    }
    right.reverse();
    (left, right)
}

fn flatten(c: &[Vec3], flatness: f32, depth: u32, out: &mut Vec<Vec3>) {
    if depth >= MAX_CURVE_DEPTH || deviation(c) <= flatness {
        out.push(c[c.len() - 1]);
        return;
    }
    let (l, r) = split(c);
    flatten(&l, flatness, depth + 1, out);
    flatten(&r, flatness, depth + 1, out);
}

/// Cells of the Bézier curve with control cells `points` (2: a line, 3: a
/// quadratic, 4: a cubic curve). The curve is cut into pieces that stay
/// within `flatness` voxels of their chord, and the rounded piece ends
/// are joined with [`line_path`]. Collinear control points give exactly
/// the straight line.
pub fn bezier_path(points: &[IVec3], flatness: f32) -> Vec<IVec3> {
    let Some(&first) = points.first() else {
        return Vec::new();
    };
    let c: Vec<Vec3> = points.iter().map(|p| p.as_vec3()).collect();
    let mut ends = vec![c[0]];
    if c.len() > 1 {
        flatten(&c, flatness.max(0.01), 0, &mut ends);
    }
    let mut corners: Vec<IVec3> = Vec::with_capacity(ends.len());
    for e in ends {
        let cell = e.round().as_ivec3();
        if corners.last() != Some(&cell) {
            corners.push(cell);
        }
    }
    // The end cell is exact even when rounding drifts.
    if let (Some(last), Some(&end)) = (corners.last_mut(), points.last()) {
        *last = end;
    }
    if corners.is_empty() {
        corners.push(first);
    }
    polyline_path(&corners)
}

/// Evaluates the curve at `t` (for drawing its preview).
pub fn bezier_at(points: &[IVec3], t: f32) -> Vec3 {
    let c: Vec<Vec3> = points.iter().map(|p| p.as_vec3()).collect();
    if c.is_empty() {
        return Vec3::ZERO;
    }
    bezier_point(&c, t)
}

fn chebyshev(a: IVec3, b: IVec3) -> i32 {
    (a - b).abs().max_element()
}

/// Pixel-perfect path: drops every "L-corner" cell whose neighbours on
/// the path are already within one cell of each other on every axis (the
/// 3D form of Aseprite's rule). The ends are kept; the path stays
/// 26-connected.
pub fn pixel_perfect(path: &[IVec3]) -> Vec<IVec3> {
    let mut out: Vec<IVec3> = Vec::with_capacity(path.len());
    for &p in path {
        if out.last() == Some(&p) {
            continue;
        }
        out.push(p);
        while out.len() >= 3 && chebyshev(out[out.len() - 3], out[out.len() - 1]) <= 1 {
            let n = out.len();
            out.remove(n - 2);
            if out[n - 3] == out[n - 2] {
                out.pop();
            }
        }
    }
    out
}

/// Solid path: every diagonal step is split into steps along one axis, so
/// the line is 6-connected and has no gaps seen from any side. The axis
/// the whole path moves along most goes first.
pub fn solid(path: &[IVec3]) -> Vec<IVec3> {
    let (Some(&first), Some(&last)) = (path.first(), path.last()) else {
        return Vec::new();
    };
    let span = (last - first).abs();
    let mut order = [0usize, 1, 2];
    order.sort_by_key(|&a| std::cmp::Reverse(span[a]));
    let mut out = vec![first];
    for w in path.windows(2) {
        let mut cur = w[0];
        for &axis in &order {
            if cur[axis] != w[1][axis] {
                cur[axis] = w[1][axis];
                out.push(cur);
            }
        }
    }
    out
}

/// `path` filtered by `mode`.
pub fn connect(path: Vec<IVec3>, mode: Connectivity) -> Vec<IVec3> {
    match mode {
        Connectivity::Diagonal => path,
        Connectivity::PixelPerfect => pixel_perfect(&path),
        Connectivity::Solid => solid(&path),
    }
}

/// Changes of one brush stamp per cell of `path` (Brush settings: shape,
/// size, antialiasing, symmetry, surface paint).
pub fn line_stroke(ctx: &ToolContext, path: &[IVec3]) -> Vec<VoxelChange> {
    stamp_path(ctx, path.iter().copied())
}

/// Cells the stamps along `path` cover (preview), symmetry included.
pub fn line_cells_stamped(ctx: &ToolContext, path: &[IVec3]) -> Vec<IVec3> {
    let mut seen = HashSet::new();
    path.iter()
        .flat_map(|&c| super::brush_positions(c, &ctx.options.brush, ctx.bounds))
        .filter(|p| seen.insert(*p))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::{apply_symmetry, mirror, ToolOptions, VoxelOp};
    use rizxel_math::{BoundSize, VoxelVolume};

    fn v(x: i32, y: i32, z: i32) -> IVec3 {
        IVec3::new(x, y, z)
    }

    fn is_26_connected(p: &[IVec3]) -> bool {
        p.windows(2).all(|w| chebyshev(w[0], w[1]) == 1)
    }

    fn is_6_connected(p: &[IVec3]) -> bool {
        p.windows(2).all(|w| (w[1] - w[0]).abs().element_sum() == 1)
    }

    fn samples() -> Vec<(IVec3, IVec3)> {
        vec![
            (v(0, 0, 0), v(9, 3, 0)),
            (v(2, 5, 1), v(-4, 0, 7)),
            (v(0, 0, 0), v(5, 5, 5)),
            (v(3, 3, 3), v(3, 3, 3)),
            (v(0, 0, 0), v(1, 7, -2)),
        ]
    }

    #[test]
    fn every_mode_keeps_the_ends() {
        for (a, b) in samples() {
            for mode in Connectivity::ALL {
                let p = connect(line_path(a, b), mode);
                assert_eq!((p[0], *p.last().unwrap()), (a, b), "{mode:?} {a} {b}");
            }
        }
    }

    #[test]
    fn pixel_perfect_has_no_l_corners() {
        for (a, b) in samples() {
            // A wobbly polyline has many L-corners to remove.
            let wobble = polyline_path(&[a, a + v(1, 1, 0), a + v(2, 0, 1), b]);
            for path in [line_path(a, b), wobble] {
                let p = pixel_perfect(&path);
                assert!(is_26_connected(&p), "{p:?}");
                for w in p.windows(3) {
                    assert!(chebyshev(w[0], w[2]) > 1, "corner at {}", w[1]);
                }
            }
        }
        // A staircase loses its corners and becomes the diagonal.
        let stairs = vec![v(0, 0, 0), v(1, 0, 0), v(1, 1, 0), v(2, 1, 0), v(2, 2, 0)];
        assert_eq!(
            pixel_perfect(&stairs),
            vec![v(0, 0, 0), v(1, 1, 0), v(2, 2, 0)]
        );
        // A clean diagonal and a backtrack are handled.
        let back = vec![v(0, 0, 0), v(1, 0, 0), v(0, 0, 0)];
        assert_eq!(pixel_perfect(&back), vec![v(0, 0, 0)]);
    }

    #[test]
    fn solid_paths_are_6_connected() {
        for (a, b) in samples() {
            let p = solid(&line_path(a, b));
            assert!(is_6_connected(&p), "{p:?}");
            // Every cell of the plain line is still there.
            for c in line_path(a, b) {
                assert!(p.contains(&c));
            }
        }
    }

    #[test]
    fn axis_lines_are_the_same_in_every_mode() {
        let line = line_path(v(1, 2, 3), v(1, 9, 3));
        assert_eq!(line.len(), 8);
        for mode in Connectivity::ALL {
            assert_eq!(connect(line.clone(), mode), line, "{mode:?}");
        }
    }

    #[test]
    fn collinear_bezier_is_the_straight_line() {
        let (a, b) = (v(0, 0, 0), v(12, 4, 0));
        let mid = v(6, 2, 0);
        assert_eq!(bezier_path(&[a, mid, b], 0.25), line_path(a, b));
        assert_eq!(
            bezier_path(&[a, v(3, 1, 0), v(9, 3, 0), b], 0.25),
            line_path(a, b)
        );
        assert_eq!(bezier_path(&[a, b], 0.25), line_path(a, b));
        assert_eq!(bezier_path(&[a], 0.25), vec![a]);
    }

    #[test]
    fn curves_bend_toward_the_control_point() {
        let (a, b, c) = (v(0, 0, 0), v(16, 0, 0), v(8, 16, 0));
        let p = bezier_path(&[a, c, b], 0.25);
        assert!(is_26_connected(&p));
        assert_eq!((p[0], *p.last().unwrap()), (a, b));
        // The apex of a quadratic is halfway to its control point.
        let top = p.iter().map(|q| q.y).max().unwrap();
        assert_eq!(top, 8);
        assert!((bezier_at(&[a, c, b], 0.5) - Vec3::new(8.0, 8.0, 0.0)).length() < 1e-4);
        // A cubic through two controls stays connected too.
        let cubic = bezier_path(&[a, v(0, 12, 3), v(16, 12, -3), b], 0.25);
        assert!(is_26_connected(&cubic));
        assert_eq!(*cubic.last().unwrap(), b);
    }

    #[test]
    fn symmetric_curves_mirror_exactly() {
        let bounds = BoundSize::new(32, 32, 32);
        let volume = VoxelVolume::new(bounds);
        let mut options = ToolOptions::default();
        options.brush.symmetry.x = true;
        let ctx = ToolContext {
            volume: &volume,
            bounds,
            color: 3,
            op: VoxelOp::Add,
            options: &options,
            hit: None,
        };
        let path = connect(
            bezier_path(&[v(2, 2, 4), v(6, 20, 4), v(12, 3, 4)], 0.25),
            Connectivity::PixelPerfect,
        );
        let changes = line_stroke(&ctx, &path);
        let cells: HashSet<IVec3> = changes.iter().map(|c| c.pos).collect();
        assert_eq!(cells.len(), path.len() * 2, "no overlap with the mirror");
        for &p in &path {
            assert!(cells.contains(&mirror(p, bounds, 0)));
        }
        let mirrored: HashSet<IVec3> = apply_symmetry(path.clone(), bounds, options.brush.symmetry)
            .into_iter()
            .collect();
        assert_eq!(cells, mirrored);
        assert_eq!(line_cells_stamped(&ctx, &path).len(), cells.len());
    }
}

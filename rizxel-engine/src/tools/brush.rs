//! Brush: stamps a cube, sphere or cylinder at the picked face and walks a
//! line between consecutive drag samples so quick strokes leave no gaps.

use std::collections::HashSet;

use glam::{IVec3, Vec3};
use rizxel_math::{BoundSize, VoxelVolume, EMPTY_VOXEL};

use super::{
    apply_op, apply_symmetry, BrushOptions, BrushShape, ToolContext, VoxelChange, VoxelOp,
};
use crate::raycast::RayHit;

/// Stamp center for an operation: additive ops build in front of the hit
/// face, the others act on the hit voxel itself.
pub fn brush_center(hit: &RayHit, op: VoxelOp) -> IVec3 {
    match op {
        VoxelOp::Add | VoxelOp::Join => hit.adjacent(),
        VoxelOp::Subtract | VoxelOp::Paint => hit.voxel,
    }
}

/// Box of a stamp of `size` voxels per axis around `center`: even sizes
/// extend one more voxel toward negative coordinates.
fn stamp_box(center: IVec3, size: [u32; 3]) -> (IVec3, IVec3) {
    let s = IVec3::from_array(size.map(|v| v.clamp(1, BrushOptions::MAX_SIZE) as i32));
    let lo = center - s / 2;
    (lo, lo + s)
}

/// Normalized distance of cell `p` from the stamp center: `<= 1` inside.
/// Cubes use the max norm, spheres the Euclidean norm and cylinders (Y up)
/// the larger of the radial and height norms.
fn stamp_distance(p: IVec3, lo: IVec3, hi: IVec3, shape: BrushShape) -> f32 {
    let center = (lo + hi).as_vec3() * 0.5;
    let half = (hi - lo).as_vec3() * 0.5;
    let d = ((p.as_vec3() + Vec3::splat(0.5) - center) / half).abs();
    match shape {
        BrushShape::Cube => d.max_element(),
        BrushShape::Sphere => d.length(),
        BrushShape::Cylinder => (d.x * d.x + d.z * d.z).sqrt().max(d.y),
    }
}

/// Cells of a stamp centred on `center`. Size semantics match
/// [`BrushOptions::size`] (diameter per axis, clamped to `MAX_SIZE`).
pub fn stamp_shape(
    center: IVec3,
    size: [u32; 3],
    shape: BrushShape,
) -> impl Iterator<Item = IVec3> {
    let (lo, hi) = stamp_box(center, size);
    box_shape(lo, hi, shape)
}

/// Cells of `shape` inscribed in the box `lo..hi` (max exclusive).
pub(super) fn box_shape(lo: IVec3, hi: IVec3, shape: BrushShape) -> impl Iterator<Item = IVec3> {
    (lo.z..hi.z).flat_map(move |z| {
        (lo.y..hi.y).flat_map(move |y| {
            (lo.x..hi.x).filter_map(move |x| {
                let p = IVec3::new(x, y, z);
                (stamp_distance(p, lo, hi, shape) <= 1.0).then_some(p)
            })
        })
    })
}

/// Deterministic `[0, 1)` value per cell, used for antialiasing dither.
fn hash01(p: IVec3) -> f32 {
    let mut h = (p.x as u32).wrapping_mul(0x8da6_b343)
        ^ (p.y as u32).wrapping_mul(0xd816_3841)
        ^ (p.z as u32).wrapping_mul(0xcb1a_b31f);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h >> 8) as f32 / (1u32 << 24) as f32
}

/// Stamp cells after antialiasing and symmetry. Antialiasing drops a
/// position-seeded dithered subset of the outer `antialiasing` voxels so the
/// edge looks soft; it never drops the center.
pub fn brush_positions(center: IVec3, opts: &BrushOptions, bounds: BoundSize) -> Vec<IVec3> {
    let (lo, hi) = stamp_box(center, opts.size);
    let cells = soft_box_shape(lo, hi, opts.shape, opts.antialiasing);
    apply_symmetry(cells, bounds, opts.symmetry)
}

/// [`box_shape`] with the dithered antialiasing band of `antialiasing`
/// voxels (shared by Brush and Shape).
pub(super) fn soft_box_shape(
    lo: IVec3,
    hi: IVec3,
    shape: BrushShape,
    antialiasing: u32,
) -> Vec<IVec3> {
    let min_half = ((hi - lo).min_element() as f32 * 0.5).max(0.5);
    let band = (antialiasing as f32 / min_half).min(1.0);
    box_shape(lo, hi, shape)
        .filter(|&p| {
            if band <= 0.0 {
                return true;
            }
            let d = stamp_distance(p, lo, hi, shape);
            d <= 1.0 - band || hash01(p) < (1.0 - d) / band
        })
        .collect()
}

/// Cells on the 3D line from `a` to `b` (both included), one per step of
/// the longest axis.
pub fn line_cells(a: IVec3, b: IVec3) -> Vec<IVec3> {
    let d = b - a;
    let steps = d.abs().max_element();
    if steps == 0 {
        return vec![a];
    }
    (0..=steps)
        .map(|i| {
            let t = i as f32 / steps as f32;
            (a.as_vec3() + d.as_vec3() * t).round().as_ivec3()
        })
        .collect()
}

/// True when `p` has at least one empty face neighbour.
fn on_surface(volume: &VoxelVolume, p: IVec3) -> bool {
    [
        IVec3::X,
        IVec3::NEG_X,
        IVec3::Y,
        IVec3::NEG_Y,
        IVec3::Z,
        IVec3::NEG_Z,
    ]
    .iter()
    .any(|&n| volume.get(p + n) == EMPTY_VOXEL)
}

/// Changes for one brush segment from the previous stamp center `from`
/// (`None` at the start of a drag) to `to`.
pub fn brush_stroke(ctx: &ToolContext, from: Option<IVec3>, to: IVec3) -> Vec<VoxelChange> {
    let opts = &ctx.options.brush;
    let centers = match from {
        Some(a) => line_cells(a, to),
        None => vec![to],
    };
    let mut seen = HashSet::new();
    let mut cells = Vec::new();
    for c in centers {
        for p in brush_positions(c, opts, ctx.bounds) {
            if seen.insert(p) {
                cells.push(p);
            }
        }
    }
    if opts.surface_paint && ctx.op == VoxelOp::Paint {
        cells.retain(|&p| on_surface(ctx.volume, p));
    }
    apply_op(ctx.volume, cells, ctx.op, ctx.color)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ToolOptions;

    fn count(size: u32, shape: BrushShape) -> usize {
        stamp_shape(IVec3::splat(20), [size; 3], shape).count()
    }

    #[test]
    fn stamp_counts_per_shape() {
        for shape in BrushShape::ALL {
            assert_eq!(count(1, shape), 1, "{shape:?}");
        }
        assert_eq!(count(2, BrushShape::Cube), 8);
        assert_eq!(count(4, BrushShape::Cube), 64);
        // Size 2 sphere/cylinder: every cell center is inside.
        assert_eq!(count(2, BrushShape::Sphere), 8);
        assert_eq!(count(2, BrushShape::Cylinder), 8);
        // Size 4: the sphere drops cells with two or more outer coordinates,
        // the cylinder its 4 vertical edge columns.
        assert_eq!(count(4, BrushShape::Sphere), 32);
        assert_eq!(count(4, BrushShape::Cylinder), 48);
        let r = 8.0f32;
        let sphere = count(16, BrushShape::Sphere) as f32;
        let expected = 4.0 / 3.0 * std::f32::consts::PI * r * r * r;
        assert!((sphere - expected).abs() < expected * 0.05);
    }

    #[test]
    fn stamp_is_centered_on_odd_sizes() {
        let cells: Vec<IVec3> = stamp_shape(IVec3::splat(5), [3, 1, 1], BrushShape::Cube).collect();
        assert_eq!(
            cells,
            vec![
                IVec3::new(4, 5, 5),
                IVec3::new(5, 5, 5),
                IVec3::new(6, 5, 5)
            ]
        );
    }

    #[test]
    fn antialiasing_is_deterministic_and_keeps_core() {
        let b = BoundSize::new(64, 64, 64);
        let mut opts = BrushOptions {
            size: [12; 3],
            shape: BrushShape::Sphere,
            ..BrushOptions::default()
        };
        let hard = brush_positions(IVec3::splat(20), &opts, b).len();
        opts.antialiasing = 2;
        let soft = brush_positions(IVec3::splat(20), &opts, b);
        assert!(soft.len() < hard && soft.len() > hard / 2);
        assert_eq!(soft, brush_positions(IVec3::splat(20), &opts, b));
        assert!(soft.contains(&IVec3::splat(20)));
    }

    #[test]
    fn line_walk_has_no_gaps() {
        let cells = line_cells(IVec3::ZERO, IVec3::new(5, 2, -3));
        assert_eq!(cells.len(), 6);
        assert_eq!(cells[0], IVec3::ZERO);
        assert_eq!(cells[5], IVec3::new(5, 2, -3));
        for w in cells.windows(2) {
            assert!((w[1] - w[0]).abs().max_element() <= 1);
        }
    }

    #[test]
    fn stroke_adds_in_front_and_erases_the_hit() {
        let b = BoundSize::new(16, 16, 16);
        let mut v = VoxelVolume::new(b);
        v.set(IVec3::new(4, 0, 4), 3);
        let hit = RayHit {
            voxel: IVec3::new(4, 0, 4),
            normal: IVec3::Y,
            t: 1.0,
        };
        let options = ToolOptions::default();
        let mut ctx = ToolContext {
            volume: &v,
            bounds: b,
            color: 7,
            op: VoxelOp::Add,
            options: &options,
            hit: Some(hit),
        };
        let add = brush_stroke(&ctx, None, brush_center(&hit, ctx.op));
        assert_eq!(add.len(), 1);
        assert_eq!((add[0].pos, add[0].new), (IVec3::new(4, 1, 4), 7));

        // A drag from x=4 to x=8 paints a gapless row.
        let row = brush_stroke(&ctx, Some(IVec3::new(4, 1, 4)), IVec3::new(8, 1, 4));
        assert_eq!(row.len(), 5);

        ctx.op = VoxelOp::Subtract;
        let erase = brush_stroke(&ctx, None, brush_center(&hit, ctx.op));
        assert_eq!(erase.len(), 1);
        assert_eq!((erase[0].old, erase[0].new), (3, 0));
    }

    #[test]
    fn surface_paint_skips_buried_voxels() {
        let b = BoundSize::new(8, 8, 8);
        let mut v = VoxelVolume::new(b);
        for z in 1..4 {
            for y in 1..4 {
                for x in 1..4 {
                    v.set(IVec3::new(x, y, z), 1);
                }
            }
        }
        let mut options = ToolOptions::default();
        options.brush.size = [3; 3];
        options.brush.surface_paint = true;
        let ctx = ToolContext {
            volume: &v,
            bounds: b,
            color: 4,
            op: VoxelOp::Paint,
            options: &options,
            hit: None,
        };
        let out = brush_stroke(&ctx, None, IVec3::splat(2));
        assert_eq!(out.len(), 26, "the center voxel is buried");
    }
}

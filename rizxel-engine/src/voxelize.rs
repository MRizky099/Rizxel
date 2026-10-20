//! Converts triangle meshes (OBJ, STL imports) into colored voxels.
//!
//! - **Grid detection.** Meshes exported by voxel editors (Rizxel,
//!   MagicaVoxel, Goxel) have only axis-aligned faces whose vertices sit on a
//!   regular grid. When that is detected, the grid step becomes the voxel
//!   size and the import is exact.
//! - **Fit mode.** Any other mesh is scaled so its longest side spans
//!   `resolution` voxels.
//! - **Filling.** The inside is filled by ray parity along +Y. Rays are
//!   jittered off the cell centers so they never graze shared triangle
//!   edges. In fit mode the surface is also rasterized, so thin or open
//!   meshes keep their shell.
//! - **Coloring.** Surface voxels take the color of the nearest triangle.
//!   Interior voxels take the color of the face where their column entered
//!   the mesh.

use std::collections::HashMap;

use glam::{IVec3, UVec3, Vec3};
use rizxel_math::{BoundSize, Rgba8};

/// One input triangle with its color.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Triangle {
    pub p: [Vec3; 3],
    pub color: Rgba8,
}

/// How the voxel grid is chosen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VoxelizeOptions {
    /// Use the mesh's own grid when it is a voxel mesh.
    pub detect_grid: bool,
    /// Voxels along the longest side in fit mode (`1..=256`).
    pub resolution: u32,
}

impl Default for VoxelizeOptions {
    fn default() -> Self {
        Self {
            detect_grid: true,
            resolution: 64,
        }
    }
}

/// Voxelized model: grid size and the colored cells inside it.
#[derive(Debug, Clone, PartialEq)]
pub struct ColorGrid {
    pub size: UVec3,
    /// Mesh-space size of one voxel.
    pub voxel_size: f32,
    /// True when the mesh's own voxel grid was detected (exact import).
    pub exact: bool,
    pub cells: Vec<(IVec3, Rgba8)>,
}

/// Voxelizes `tris`. Fails for empty or degenerate input, or when a
/// detected grid exceeds the largest bounds.
pub fn voxelize(tris: &[Triangle], options: VoxelizeOptions) -> Result<ColorGrid, String> {
    let tris: Vec<Triangle> = tris
        .iter()
        .copied()
        .filter(|t| t.p.iter().all(|p| p.is_finite()))
        .collect();
    let (lo, hi) = tris.iter().flat_map(|t| t.p).fold(
        (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
        |(lo, hi), p| (lo.min(p), hi.max(p)),
    );
    if tris.is_empty() || !(hi - lo).max_element().is_finite() || (hi - lo).max_element() <= 0.0 {
        return Err("the mesh has no usable triangles".into());
    }

    let detected = if options.detect_grid {
        detect_grid(&tris, lo, hi)
    } else {
        None
    };
    let (voxel_size, exact) = match detected {
        Some(step) => (step, true),
        None => {
            let res = options.resolution.clamp(1, BoundSize::MAX) as f32;
            ((hi - lo).max_element() / res, false)
        }
    };
    let size_f = ((hi - lo) / voxel_size).round().max(Vec3::ONE);
    if size_f.max_element() > BoundSize::MAX as f32 {
        return Err(format!(
            "the model is {}×{}×{} voxels; Rizxel supports up to {} per axis",
            size_f.x,
            size_f.y,
            size_f.z,
            BoundSize::MAX
        ));
    }
    let size = size_f.as_uvec3();
    // Mesh space -> grid space (one unit per voxel).
    let to_grid = |p: Vec3| (p - lo) / voxel_size;
    let gtris: Vec<Triangle> = tris
        .iter()
        .map(|t| Triangle {
            p: t.p.map(to_grid),
            color: t.color,
        })
        .collect();

    let mut cells: HashMap<IVec3, Rgba8> = HashMap::new();
    fill_by_parity(&gtris, size, &mut cells);
    if !exact {
        rasterize_surface(&gtris, size, &mut cells);
    }
    color_surface(&gtris, &mut cells);

    let mut cells: Vec<(IVec3, Rgba8)> = cells.into_iter().collect();
    cells.sort_by_key(|(p, _)| (p.z, p.y, p.x));
    Ok(ColorGrid {
        size,
        voxel_size,
        exact,
        cells,
    })
}

/// Grid step of a voxel mesh: all faces axis-aligned and every vertex
/// coordinate a multiple of one step from the minimum corner.
fn detect_grid(tris: &[Triangle], lo: Vec3, hi: Vec3) -> Option<f32> {
    let extent = (hi - lo).max_element();
    let eps = extent * 1e-5;
    for t in tris {
        let n = (t.p[1] - t.p[0]).cross(t.p[2] - t.p[0]);
        let len = n.length();
        if len <= eps * eps {
            continue; // degenerate
        }
        let n = n / len;
        if n.abs().max_element() < 1.0 - 1e-4 {
            return None;
        }
    }
    // Smallest positive gap between distinct coordinates on any axis.
    let mut step = f32::MAX;
    for axis in 0..3 {
        let mut values: Vec<f32> = tris.iter().flat_map(|t| t.p.map(|p| p[axis])).collect();
        values.sort_by(f32::total_cmp);
        for w in values.windows(2) {
            let gap = w[1] - w[0];
            if gap > eps {
                step = step.min(gap);
            }
        }
    }
    if step == f32::MAX {
        return None;
    }
    let on_grid = tris.iter().flat_map(|t| t.p).all(|p| {
        let g = (p - lo) / step;
        (g - g.round()).abs().max_element() < 1e-3
    });
    on_grid.then_some(step)
}

/// Jitter of the parity rays inside a cell, so rays never hit triangle
/// edges or the diagonals of quads exactly.
const JITTER: (f32, f32) = (0.5123, 0.4871);

/// Fills cells between pairs of +Y ray crossings (parity rule).
fn fill_by_parity(tris: &[Triangle], size: UVec3, cells: &mut HashMap<IVec3, Rgba8>) {
    // Bin triangles by the XZ columns their footprint covers.
    let mut columns: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    for (i, t) in tris.iter().enumerate() {
        let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for p in t.p {
            lo = lo.min(p);
            hi = hi.max(p);
        }
        if (hi.x - lo.x) <= 0.0 || (hi.z - lo.z) <= 0.0 {
            continue; // parallel to the ray: never crossed
        }
        for x in (lo.x.floor() as i32).max(0)..=(hi.x.floor() as i32).min(size.x as i32 - 1) {
            for z in (lo.z.floor() as i32).max(0)..=(hi.z.floor() as i32).min(size.z as i32 - 1) {
                columns.entry((x, z)).or_default().push(i);
            }
        }
    }
    let mut hits: Vec<(f32, Rgba8)> = Vec::new();
    for (&(x, z), list) in &columns {
        let (px, pz) = (x as f32 + JITTER.0, z as f32 + JITTER.1);
        hits.clear();
        for &i in list {
            if let Some(y) = ray_y(&tris[i], px, pz) {
                hits.push((y, tris[i].color));
            }
        }
        hits.sort_by(|a, b| a.0.total_cmp(&b.0));
        for pair in hits.chunks_exact(2) {
            let (enter, exit) = (pair[0].0, pair[1].0);
            // Cells whose center lies between the crossings.
            let first = (enter - 0.5).ceil().max(0.0) as i32;
            let last = ((exit - 0.5).floor() as i32).min(size.y as i32 - 1);
            for y in first..=last {
                cells.insert(IVec3::new(x, y, z), pair[0].1);
            }
        }
    }
}

/// Height where the vertical line at `(x, z)` crosses the triangle.
fn ray_y(t: &Triangle, x: f32, z: f32) -> Option<f32> {
    let [a, b, c] = t.p;
    let area = (b.x - a.x) * (c.z - a.z) - (c.x - a.x) * (b.z - a.z);
    if area.abs() < 1e-12 {
        return None;
    }
    let w0 = ((b.x - x) * (c.z - z) - (c.x - x) * (b.z - z)) / area;
    let w1 = ((c.x - x) * (a.z - z) - (a.x - x) * (c.z - z)) / area;
    let w2 = 1.0 - w0 - w1;
    (w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0).then_some(w0 * a.y + w1 * b.y + w2 * c.y)
}

/// Marks cells the surface passes through (fit mode), sampling each
/// triangle at half-voxel spacing.
fn rasterize_surface(tris: &[Triangle], size: UVec3, cells: &mut HashMap<IVec3, Rgba8>) {
    let max = size.as_ivec3() - IVec3::ONE;
    for t in tris {
        let [a, b, c] = t.p;
        let longest = (b - a).length().max((c - a).length()).max((c - b).length());
        let n = (longest * 2.0).ceil().clamp(1.0, 4096.0) as u32;
        for i in 0..=n {
            for j in 0..=(n - i) {
                let (u, v) = (i as f32 / n as f32, j as f32 / n as f32);
                let p = a + (b - a) * u + (c - a) * v;
                let cell = p.floor().as_ivec3().clamp(IVec3::ZERO, max);
                cells.entry(cell).or_insert(t.color);
            }
        }
    }
}

/// Recolors surface cells with the color of the nearest triangle.
fn color_surface(tris: &[Triangle], cells: &mut HashMap<IVec3, Rgba8>) {
    // Bin triangles into the cells their bounding boxes touch (+1 margin).
    let mut bins: HashMap<IVec3, Vec<usize>> = HashMap::new();
    for (i, t) in tris.iter().enumerate() {
        let lo = t.p[0].min(t.p[1]).min(t.p[2]).floor().as_ivec3() - IVec3::ONE;
        let hi = t.p[0].max(t.p[1]).max(t.p[2]).floor().as_ivec3() + IVec3::ONE;
        let span = (hi - lo + IVec3::ONE).as_i64vec3();
        if span.x * span.y * span.z > 4_000_000 {
            continue; // absurdly large triangle: keep entry colors
        }
        for z in lo.z..=hi.z {
            for y in lo.y..=hi.y {
                for x in lo.x..=hi.x {
                    bins.entry(IVec3::new(x, y, z)).or_default().push(i);
                }
            }
        }
    }
    let neighbours = [
        IVec3::X,
        IVec3::NEG_X,
        IVec3::Y,
        IVec3::NEG_Y,
        IVec3::Z,
        IVec3::NEG_Z,
    ];
    let surface: Vec<IVec3> = cells
        .keys()
        .copied()
        .filter(|p| neighbours.iter().any(|d| !cells.contains_key(&(*p + *d))))
        .collect();
    for p in surface {
        let center = p.as_vec3() + Vec3::splat(0.5);
        let best = bins.get(&p).and_then(|list| {
            list.iter()
                .map(|&i| {
                    (
                        i,
                        (closest_point(&tris[i], center) - center).length_squared(),
                    )
                })
                .min_by(|a, b| a.1.total_cmp(&b.1))
        });
        if let Some((i, _)) = best {
            cells.insert(p, tris[i].color);
        }
    }
}

/// Closest point on a triangle to `p` (Ericson, Real-Time Collision Detection).
fn closest_point(t: &Triangle, p: Vec3) -> Vec3 {
    let [a, b, c] = t.p;
    let (ab, ac, ap) = (b - a, c - a, p - a);
    let (d1, d2) = (ab.dot(ap), ac.dot(ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let (d3, d4) = (ab.dot(bp), ac.dot(bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return a + ab * (d1 / (d1 - d3));
    }
    let cp = p - c;
    let (d5, d6) = (ab.dot(cp), ac.dot(cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return a + ac * (d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let denom = 1.0 / (va + vb + vc);
    a + ab * (vb * denom) + ac * (vc * denom)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quads::greedy_quads;
    use rizxel_math::VoxelVolume;

    /// Triangles of a voxel volume's export quads at `scale`, `offset`.
    fn voxel_mesh(
        volume: &VoxelVolume,
        scale: f32,
        offset: Vec3,
        color: impl Fn(u8) -> Rgba8,
    ) -> Vec<Triangle> {
        greedy_quads(volume)
            .iter()
            .flat_map(|q| {
                let [a, b, c, d] = q.corners().map(|p| p.as_vec3() * scale + offset);
                let col = color(q.value);
                [
                    Triangle {
                        p: [a, b, c],
                        color: col,
                    },
                    Triangle {
                        p: [c, b, d],
                        color: col,
                    },
                ]
            })
            .collect()
    }

    #[test]
    fn voxel_meshes_round_trip_exactly() {
        let mut v = VoxelVolume::new(BoundSize::new(32, 32, 32));
        // An L-shape with two colors, a hole and an overhang.
        for x in 0..6 {
            for y in 0..3 {
                for z in 0..4 {
                    if (x, z) != (2, 1) {
                        v.set(IVec3::new(x, y, z), if y == 2 { 2 } else { 1 });
                    }
                }
            }
        }
        v.set(IVec3::new(5, 3, 0), 2);
        v.set(IVec3::new(5, 4, 0), 1);
        v.set(IVec3::new(6, 4, 0), 1);
        let pal = |i: u8| Rgba8::rgb(i * 100, 0, 255 - i * 100);
        let tris = voxel_mesh(&v, 0.1, Vec3::new(-1.0, 0.0, 0.3), pal);
        let grid = voxelize(&tris, VoxelizeOptions::default()).unwrap();
        assert!(grid.exact);
        assert!((grid.voxel_size - 0.1).abs() < 1e-5);
        assert_eq!(grid.size, UVec3::new(7, 5, 4));
        let expected: HashMap<IVec3, Rgba8> = v.iter().map(|(p, c)| (p, pal(c))).collect();
        let got: HashMap<IVec3, Rgba8> = grid.cells.into_iter().collect();
        assert_eq!(got, expected);
    }

    #[test]
    fn arbitrary_meshes_are_fit_to_the_resolution() {
        // A tetrahedron is not a voxel mesh.
        let p = [Vec3::ZERO, Vec3::X * 10.0, Vec3::Z * 10.0, Vec3::Y * 10.0];
        let red = Rgba8::rgb(255, 0, 0);
        let tris: Vec<Triangle> = [[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]]
            .iter()
            .map(|f| Triangle {
                p: f.map(|i| p[i]),
                color: red,
            })
            .collect();
        let grid = voxelize(
            &tris,
            VoxelizeOptions {
                detect_grid: true,
                resolution: 20,
            },
        )
        .unwrap();
        assert!(!grid.exact);
        assert_eq!(grid.size, UVec3::splat(20));
        // Roughly a sixth of the cube, plus the rasterized shell.
        let n = grid.cells.len() as f32;
        assert!(n > 8000.0 / 6.0 * 0.8 && n < 8000.0 / 6.0 * 2.0, "{n}");
        assert!(grid.cells.iter().all(|(_, c)| *c == red));
    }

    #[test]
    fn oversized_grids_and_empty_meshes_fail() {
        let mut v = VoxelVolume::new(BoundSize::new(256, 1, 1));
        v.set(IVec3::ZERO, 1);
        v.set(IVec3::new(255, 0, 0), 1);
        let mut tris = voxel_mesh(&v, 1.0, Vec3::ZERO, |_| Rgba8::WHITE);
        assert!(voxelize(&tris, VoxelizeOptions::default()).is_ok());
        for t in &mut tris {
            for p in &mut t.p {
                p.x *= 2.0; // still on a 1.0 grid, now 511 wide
            }
        }
        assert!(voxelize(&tris, VoxelizeOptions::default())
            .unwrap_err()
            .contains("up to 256"));
        assert!(voxelize(&[], VoxelizeOptions::default()).is_err());
    }
}

//! Optional sample files for tests that compare against real-world files.
//!
//! `RIZXEL_SAMPLES` lists the folders to search, separated like `PATH`
//! (`;` on Windows, `:` elsewhere). Tests that need a sample return early
//! when the variable is unset or the file is missing, so CI and fresh
//! clones skip them.

use std::path::PathBuf;

/// Environment variable naming the sample folders.
pub const SAMPLES_ENV: &str = "RIZXEL_SAMPLES";

/// The first `RIZXEL_SAMPLES` folder that contains `name`, joined with it.
pub fn sample(name: &str) -> Option<PathBuf> {
    let dirs = std::env::var_os(SAMPLES_ENV)?;
    std::env::split_paths(&dirs)
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
}

/// Unit faces covered by `triangles`, for comparing mesh surfaces that are
/// split into triangles differently. `units` is the number of mesh units
/// per voxel; faces must be axis-aligned on that grid. Each entry is a cell
/// `(x, y, z)` plus the face direction (`axis * 2 + positive`).
pub fn surface_cells(
    triangles: &[[glam::Vec3; 3]],
    units: f32,
) -> std::collections::BTreeSet<(i64, i64, i64, i64)> {
    use glam::Vec3;
    let mut out = std::collections::BTreeSet::new();
    for t in triangles {
        let [a, b, c] = t.map(|v: Vec3| (v * units).round());
        let n = (b - a).cross(c - a).normalize();
        let lo = a.min(b).min(c);
        let hi = a.max(b).max(c);
        for x in lo.x as i64..(hi.x as i64).max(lo.x as i64 + 1) {
            for y in lo.y as i64..(hi.y as i64).max(lo.y as i64 + 1) {
                for z in lo.z as i64..(hi.z as i64).max(lo.z as i64 + 1) {
                    // Cell center projected onto the face plane must lie in the triangle.
                    let mut p = Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5);
                    for k in 0..3 {
                        if n[k].abs() > 0.5 {
                            p[k] = a[k];
                        }
                    }
                    let inside = [(a, b), (b, c), (c, a)]
                        .iter()
                        .all(|(u, v)| (*v - *u).cross(p - *u).dot(n) >= -1e-4);
                    if inside {
                        let dir =
                            n.abs().max_position() as i64 * 2 + (n.max_element() > 0.5) as i64;
                        out.insert((x, y, z, dir));
                    }
                }
            }
        }
    }
    out
}

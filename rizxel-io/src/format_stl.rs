//! STL export (binary) and import (binary or ASCII).
//!
//! - **Export**: Z-up, as 3D printers and slicers expect, centered on X/Y
//!   and standing on Z = 0. One voxel is `scale` units (default 1 mm).
//!   Faces are the same merged quads as the OBJ export, split into
//!   outward-facing triangles. STL has no standard color, so none is
//!   written.
//! - **Import**: binary files are recognized by their exact size
//!   (`84 + 50·n` bytes); anything starting with `solid` is parsed as
//!   ASCII. Triangles come back in their own Z-up frame;
//!   [`crate::mesh_import`] converts and voxelizes them.

use glam::{IVec3, Vec3};
use rizxel_engine::Triangle;
use rizxel_math::{BoundSize, Rgba8, VoxelVolume};

use crate::format_obj::zup_faces;
use crate::RzxlError;

pub const STL_EXTENSION: &str = "stl";
/// Default export scale: one voxel = 1 mm.
pub const DEFAULT_STL_SCALE: f32 = 1.0;
const MAX_TRIANGLES: usize = 40_000_000;

/// Encodes the visible model as binary STL.
pub fn write_stl(
    volume: &VoxelVolume,
    bounds: BoundSize,
    scale: f32,
) -> Result<Vec<u8>, RzxlError> {
    if volume.is_empty() {
        return Err(RzxlError::Invalid(
            "nothing to export: the visible layers contain no voxels".into(),
        ));
    }
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        DEFAULT_STL_SCALE
    };
    let faces = zup_faces(volume, bounds);
    let half = IVec3::new(faces.size.x / 2, faces.size.y / 2, 0);
    let count: usize = faces.groups.iter().map(|g| g.triangles.len()).sum();

    let mut out = Vec::with_capacity(84 + 50 * count);
    let mut header = format!("Rizxel {} binary STL", env!("CARGO_PKG_VERSION")).into_bytes();
    header.resize(80, b' ');
    out.extend_from_slice(&header);
    out.extend_from_slice(&(count as u32).to_le_bytes());
    fn put(out: &mut Vec<u8>, v: Vec3) {
        for c in v.to_array() {
            out.extend_from_slice(&(c + 0.0).to_le_bytes());
        }
    }
    for g in &faces.groups {
        let mut n = Vec3::ZERO;
        n[g.axis] = if g.positive { 1.0 } else { -1.0 };
        for (t, _) in &g.triangles {
            put(&mut out, n);
            for &i in t {
                put(&mut out, (g.vertices[i] - half).as_vec3() * scale);
            }
            out.extend_from_slice(&[0, 0]); // attribute byte count
        }
    }
    Ok(out)
}

fn invalid(msg: impl Into<String>) -> RzxlError {
    RzxlError::Invalid(format!("STL: {}", msg.into()))
}

/// Parses binary or ASCII STL into Z-up triangles of color `color`.
pub fn read_stl(bytes: &[u8], color: Rgba8) -> Result<Vec<Triangle>, RzxlError> {
    let binary_count = (bytes.len() >= 84)
        .then(|| u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize)
        .filter(|&n| n.checked_mul(50).and_then(|b| b.checked_add(84)) == Some(bytes.len()));
    let tris = match binary_count {
        Some(n) => {
            if n > MAX_TRIANGLES {
                return Err(invalid("too many triangles"));
            }
            let f =
                |o: usize| f32::from_le_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]);
            (0..n)
                .map(|i| {
                    let o = 84 + 50 * i + 12;
                    let v =
                        |k: usize| Vec3::new(f(o + 12 * k), f(o + 12 * k + 4), f(o + 12 * k + 8));
                    Triangle {
                        p: [v(0), v(1), v(2)],
                        color,
                    }
                })
                .collect()
        }
        None if bytes.starts_with(b"solid") => read_ascii(&String::from_utf8_lossy(bytes), color)?,
        None => {
            return Err(invalid(
                "not an STL file (size does not match a binary STL)",
            ))
        }
    };
    if tris.is_empty() {
        return Err(invalid("the file contains no triangles"));
    }
    Ok(tris)
}

fn read_ascii(text: &str, color: Rgba8) -> Result<Vec<Triangle>, RzxlError> {
    let mut tris = Vec::new();
    let mut corners: Vec<Vec3> = Vec::with_capacity(3);
    for line in text.lines() {
        let mut it = line.split_whitespace();
        match it.next() {
            Some("vertex") => {
                let f: Vec<f32> = it.filter_map(|s| s.parse().ok()).collect();
                if f.len() != 3 {
                    return Err(invalid(format!("bad vertex line {line:?}")));
                }
                corners.push(Vec3::new(f[0], f[1], f[2]));
            }
            Some("endloop") => {
                if corners.len() >= 3 {
                    // Fan-triangulate loops with more than three vertices.
                    for k in 1..corners.len() - 1 {
                        tris.push(Triangle {
                            p: [corners[0], corners[k], corners[k + 1]],
                            color,
                        });
                    }
                }
                corners.clear();
                if tris.len() > MAX_TRIANGLES {
                    return Err(invalid("too many triangles"));
                }
            }
            _ => {}
        }
    }
    Ok(tris)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> (VoxelVolume, BoundSize) {
        let bounds = BoundSize::new(4, 3, 2);
        let mut v = VoxelVolume::new(bounds);
        v.set(IVec3::new(0, 0, 0), 1);
        v.set(IVec3::new(1, 0, 0), 2);
        v.set(IVec3::new(1, 2, 1), 2);
        (v, bounds)
    }

    #[test]
    fn binary_roundtrip_and_outward_normals() {
        let (v, b) = model();
        let bytes = write_stl(&v, b, 2.0).unwrap();
        assert!(
            !bytes.starts_with(b"solid"),
            "binary header must not look like ASCII"
        );
        let tris = read_stl(&bytes, Rgba8::WHITE).unwrap();
        // A two-color 2×1×1 bar (colors never merge: 10 quads) and a
        // separate single voxel (6 quads).
        assert_eq!(tris.len(), (10 + 6) * 2);
        // Stored normals agree with the winding.
        for (i, tri) in tris.iter().enumerate() {
            let o = 84 + 50 * i;
            let f = |k: usize| f32::from_le_bytes(bytes[o + k..o + k + 4].try_into().unwrap());
            let n = Vec3::new(f(0), f(4), f(8));
            let [a, bb, c] = tri.p;
            assert!((bb - a).cross(c - a).dot(n) > 0.0);
        }
        // Z-up: the model stands on z = 0 and is 3 voxels (6 mm) tall.
        let zs = tris.iter().flat_map(|t| t.p.map(|p| p.z));
        let (lo, hi) = zs.fold((f32::MAX, f32::MIN), |(l, h), z| (l.min(z), h.max(z)));
        assert_eq!((lo, hi), (0.0, 6.0));
    }

    #[test]
    fn ascii_files_are_read() {
        let text = "solid t\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid t\n";
        let tris = read_stl(text.as_bytes(), Rgba8::WHITE).unwrap();
        assert_eq!(tris.len(), 1);
        assert_eq!(tris[0].p[1], Vec3::X);
        assert!(read_stl(b"garbage", Rgba8::WHITE).is_err());
        assert!(read_stl(b"solid empty\nendsolid\n", Rgba8::WHITE).is_err());
    }
}

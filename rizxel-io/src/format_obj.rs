//! Wavefront OBJ export.
//!
//! Vertex colors are written using the widely supported extension
//! `v x y z r g b` (components in `0..=1`), understood by Blender, MeshLab, etc.

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;

use rizxel_engine::Mesh;

/// Writes `mesh` as OBJ text. Indices in OBJ are 1-based.
pub fn export_obj<W: Write>(mesh: &Mesh, mut w: W) -> io::Result<()> {
    if !mesh.is_valid() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "mesh has inconsistent attributes or out-of-range indices",
        ));
    }

    writeln!(w, "# Exported by Rizxel {}", env!("CARGO_PKG_VERSION"))?;
    writeln!(
        w,
        "# vertices: {}, triangles: {}",
        mesh.vertex_count(),
        mesh.triangle_count()
    )?;

    let has_colors = !mesh.colors.is_empty();
    for (i, p) in mesh.positions.iter().enumerate() {
        if has_colors {
            let c = mesh.colors[i];
            writeln!(
                w,
                "v {} {} {} {:.4} {:.4} {:.4}",
                p.x,
                p.y,
                p.z,
                c.r as f32 / 255.0,
                c.g as f32 / 255.0,
                c.b as f32 / 255.0
            )?;
        } else {
            writeln!(w, "v {} {} {}", p.x, p.y, p.z)?;
        }
    }

    let has_normals = !mesh.normals.is_empty();
    for n in &mesh.normals {
        writeln!(w, "vn {} {} {}", n.x, n.y, n.z)?;
    }

    for tri in mesh.indices.chunks_exact(3) {
        let [a, b, c] = [tri[0] + 1, tri[1] + 1, tri[2] + 1];
        if has_normals {
            writeln!(w, "f {a}//{a} {b}//{b} {c}//{c}")?;
        } else {
            writeln!(w, "f {a} {b} {c}")?;
        }
    }

    w.flush()
}

/// Convenience wrapper that writes an OBJ file to disk.
pub fn write_obj_file(mesh: &Mesh, path: &Path) -> io::Result<()> {
    export_obj(mesh, BufWriter::new(File::create(path)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rizxel_math::Rgba8;

    #[test]
    fn cube_has_8_vertices_and_12_faces() {
        let mut out = Vec::new();
        export_obj(&Mesh::unit_cube(Rgba8::WHITE), &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();

        assert_eq!(text.lines().filter(|l| l.starts_with("v ")).count(), 8);
        assert_eq!(text.lines().filter(|l| l.starts_with("f ")).count(), 12);
        assert!(text.contains("v 0 0 0 1.0000 1.0000 1.0000"));
        assert!(text.contains("f 1 3 4"));
    }

    #[test]
    fn rejects_invalid_mesh() {
        let mut m = Mesh::unit_cube(Rgba8::WHITE);
        m.indices.push(99);
        assert!(export_obj(&m, Vec::new()).is_err());
    }
}

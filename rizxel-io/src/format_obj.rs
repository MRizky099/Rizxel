//! Wavefront OBJ export and import.
//!
//! ## Export (MagicaVoxel layout)
//!
//! [`write_obj_set`] writes `name.obj`, `name.mtl` and `name.png` in the same
//! layout as MagicaVoxel's OBJ export (reference: MagicaVoxel's own
//! `chr_knight.obj` export of its `chr_knight.vox` sample):
//!
//! - **Text**: CRLF lines. A `# Rizxel @ Abati Technology` header, then the
//!   `# group`, `# material`, `# normals`, `# texcoords`, `# verts` and
//!   `# faces` sections. Numbers are formatted like C's `%g`.
//! - **Material**: `o ` with an empty name, `mtllib name.mtl` and
//!   `usemtl palette`. The MTL's `map_Kd` is a 256×1 RGBA palette PNG where
//!   pixel `p` is color index `p + 1`.
//! - **Normals**: always the six axis normals, in MagicaVoxel's order
//!   (−X, +X, +Z, −Z, −Y, +Y in OBJ space).
//! - **Texcoords**: one `vt (i - 0.5) / 256 0.5` per used color index `i`,
//!   ascending.
//! - **Axes**: Y-up. Z-up voxel `(x, y, z)` maps to
//!   `((x - ⌊sx/2⌋)·s, z·s, -(y - ⌊sy/2⌋)·s)` with `s` the scale
//!   (MagicaVoxel's default is 0.1). A model imported from MagicaVoxel
//!   therefore exports to the same coordinates.
//! - **Vertices**: shared within each normal group and sorted per group by
//!   (face axis, then the other two axes in a fixed rotation).
//! - **Faces**: quad corners A=(u0,v0), B=(u1,v0), C=(u0,v1), D=(u1,v1)
//!   become triangles `A B C` + `C B D` (reversed when that would face
//!   inward). Faces are `f v/vt/vn`, ordered within each group by their C
//!   or D vertex.
//! - **Merging**: same-color faces merge greedily across the whole model.
//!   This yields fewer triangles than MagicaVoxel, whose own merge pattern
//!   differs, but the format and geometry are the same.
//!
//! ## Import
//!
//! [`read_obj`] reads vertices (with optional `r g b` vertex colors), UVs
//! and polygon faces (`v`, `v/vt`, `v//vn`, `v/vt/vn`, negative indices),
//! following `mtllib`/`usemtl`. Each triangle's color comes from, in order:
//! 1. its vertex colors;
//! 2. the material's `map_Kd` texture at the face's UV;
//! 3. the material's `Kd`;
//! 4. white.
//!
//! Voxelization happens in [`crate::mesh_import`].

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use glam::{IVec3, Vec2, Vec3};
use rizxel_engine::{greedy_quads, Mesh, Triangle};
use rizxel_math::{BoundSize, Palette, Rgba8, Voxel, VoxelVolume};

use crate::format_mtl::{
    fmt_g, read_mtl, write_palette_mtl, MtlMaterial, EXPORT_HEADER, PALETTE_MATERIAL,
};
use crate::interchange::{to_zup, zup_size};
use crate::png_io::{encode_rgba, RgbaImage};
use crate::RzxlError;

pub const OBJ_EXTENSION: &str = "obj";
/// MagicaVoxel's default OBJ scale (`config.txt`, `io_obj.scale`).
pub const DEFAULT_OBJ_SCALE: f32 = 0.1;

// ------------------------------------------------------------ generic mesh

/// Writes `mesh` as OBJ text with `v x y z r g b` vertex colors. Indices in
/// OBJ are 1-based.
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

/// Convenience wrapper that writes a generic OBJ file to disk.
pub fn write_obj_file(mesh: &Mesh, path: &Path) -> io::Result<()> {
    export_obj(mesh, BufWriter::new(File::create(path)?))
}

// ------------------------------------------------- Z-up face groups

/// Exposed faces of a model in the Z-up (MagicaVoxel) frame, grouped by
/// direction in the order −X, +X, −Y, +Y, −Z, +Z.
pub(crate) struct ZupFaces {
    /// Z-up model size.
    pub size: IVec3,
    pub groups: Vec<FaceGroup>,
}

pub(crate) struct FaceGroup {
    pub axis: usize,
    pub positive: bool,
    /// Corner points, deduplicated and sorted.
    pub vertices: Vec<IVec3>,
    /// Triangles as indices into `vertices`, with their palette index.
    pub triangles: Vec<([usize; 3], Voxel)>,
}

/// Builds the Z-up face groups of a Rizxel volume.
pub(crate) fn zup_faces(volume: &VoxelVolume, bounds: BoundSize) -> ZupFaces {
    let size = zup_size(bounds);
    let mut zup = VoxelVolume::new(BoundSize::new(size.x as u32, size.y as u32, size.z as u32));
    for (p, v) in volume.iter() {
        zup.set(to_zup(bounds, p), v);
    }
    let quads = greedy_quads(&zup);
    let mut groups = Vec::with_capacity(6);
    for axis in 0..3 {
        for positive in [false, true] {
            let group: Vec<_> = quads
                .iter()
                .filter(|q| q.axis == axis && q.positive == positive)
                .collect();
            // Sort key: face axis, then the other two in a fixed rotation.
            let key = |p: &IVec3| (p[axis], p[(axis + 2) % 3], p[(axis + 1) % 3]);
            let mut vertices: Vec<IVec3> = group.iter().flat_map(|q| q.corners()).collect();
            vertices.sort_by_key(key);
            vertices.dedup();
            let index: HashMap<IVec3, usize> =
                vertices.iter().enumerate().map(|(i, &p)| (p, i)).collect();
            // (sort key = index of the C/D corner, emission order).
            let mut tris: Vec<(usize, usize, [usize; 3], Voxel)> = Vec::new();
            for q in &group {
                let [a, b, c, d] = q.corners().map(|p| index[&p]);
                let (t1, t2) = if q.abc_is_ccw() {
                    ([a, b, c], [c, b, d])
                } else {
                    ([c, b, a], [d, b, c])
                };
                let n = tris.len();
                tris.push((c, n, t1, q.value));
                tris.push((d, n + 1, t2, q.value));
            }
            tris.sort_by_key(|&(k, order, ..)| (k, order));
            groups.push(FaceGroup {
                axis,
                positive,
                vertices,
                triangles: tris.into_iter().map(|(.., t, v)| (t, v)).collect(),
            });
        }
    }
    ZupFaces { size, groups }
}

// ------------------------------------------------ MagicaVoxel-style OBJ

/// Text and texture of an OBJ export (before writing to disk).
#[derive(Debug, Clone, PartialEq)]
pub struct ObjExport {
    pub obj: String,
    pub mtl: String,
    pub png: Vec<u8>,
    pub triangles: usize,
}

/// Palette texture: 256×1 RGBA, pixel `p` = palette index `p + 1`; the last
/// pixel is transparent (MagicaVoxel has no color 256).
pub fn palette_texture(palette: &Palette) -> Result<Vec<u8>, RzxlError> {
    let pixels: Vec<u8> = crate::format_vox::export_palette(palette)
        .iter()
        .flat_map(|c| [c.r, c.g, c.b, c.a])
        .collect();
    encode_rgba(256, 1, &pixels)
}

/// Builds the OBJ/MTL/PNG triple for `volume` (the merged visible layers).
/// `stem` names the companion files (`stem.mtl`, `stem.png`).
pub fn build_obj_export(
    volume: &VoxelVolume,
    bounds: BoundSize,
    palette: &Palette,
    stem: &str,
    scale: f32,
) -> Result<ObjExport, RzxlError> {
    if volume.is_empty() {
        return Err(RzxlError::Invalid(
            "nothing to export: the visible layers contain no voxels".into(),
        ));
    }
    let faces = zup_faces(volume, bounds);
    let half = IVec3::new(faces.size.x / 2, faces.size.y / 2, 0);
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        DEFAULT_OBJ_SCALE
    };
    let coord = |c: i32| fmt_g(c as f32 * scale + 0.0);

    let mut used: Vec<Voxel> = faces
        .groups
        .iter()
        .flat_map(|g| g.triangles.iter().map(|(_, v)| *v))
        .collect();
    used.sort_unstable();
    used.dedup();
    let vt_of: HashMap<Voxel, usize> = used.iter().enumerate().map(|(i, &v)| (v, i + 1)).collect();

    let mut s = String::new();
    let mut line = |l: &str| {
        s.push_str(l);
        s.push_str("\r\n");
    };
    line(EXPORT_HEADER);
    line("");
    line("# group");
    line("o ");
    line("");
    line("# material");
    line(&format!("mtllib {stem}.mtl"));
    line(&format!("usemtl {PALETTE_MATERIAL}"));
    line("");
    line("# normals");
    for g in &faces.groups {
        // Z-up direction -> OBJ (x, z, -y).
        let mut d = IVec3::ZERO;
        d[g.axis] = if g.positive { 1 } else { -1 };
        let n = IVec3::new(d.x, d.z, -d.y);
        line(&format!("vn {} {} {}", n.x, n.y, n.z));
    }
    line("");
    line("# texcoords");
    for &v in &used {
        let u = (v as f32 - 0.5) / 256.0;
        line(&format!("vt {} 0.5", fmt_g(u)));
    }
    line("");
    line("# verts");
    for g in &faces.groups {
        for p in &g.vertices {
            let l = *p - half;
            line(&format!("v {} {} {}", coord(l.x), coord(l.z), coord(-l.y)));
        }
    }
    line("");
    line("# faces");
    let mut base = 1;
    let mut triangles = 0;
    for (n, g) in faces.groups.iter().enumerate() {
        for (t, value) in &g.triangles {
            let vt = vt_of[value];
            let [a, b, c] = t.map(|i| i + base);
            line(&format!(
                "f {a}/{vt}/{} {b}/{vt}/{} {c}/{vt}/{}",
                n + 1,
                n + 1,
                n + 1
            ));
            triangles += 1;
        }
        base += g.vertices.len();
    }
    Ok(ObjExport {
        obj: s,
        mtl: write_palette_mtl(&format!("{stem}.png")),
        png: palette_texture(palette)?,
        triangles,
    })
}

/// Writes `path` (`.obj`) plus `.mtl` and `.png` with the same stem.
/// Returns the number of triangles.
pub fn write_obj_set(
    path: &Path,
    volume: &VoxelVolume,
    bounds: BoundSize,
    palette: &Palette,
    scale: f32,
) -> Result<usize, RzxlError> {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "model".to_owned());
    let export = build_obj_export(volume, bounds, palette, &stem, scale)?;
    let sibling = |ext: &str| path.with_file_name(format!("{stem}.{ext}"));
    std::fs::write(path, export.obj.as_bytes())?;
    std::fs::write(
        sibling(crate::format_mtl::MTL_EXTENSION),
        export.mtl.as_bytes(),
    )?;
    std::fs::write(sibling("png"), &export.png)?;
    Ok(export.triangles)
}

// ------------------------------------------------------------------ import

/// Largest accepted counts when reading OBJ text.
const MAX_VERTICES: usize = 20_000_000;
const MAX_TRIANGLES: usize = 40_000_000;
const MAX_POLYGON: usize = 256;

/// Triangles of an OBJ file plus the palette texture it uses, if any.
#[derive(Debug, Clone, Default)]
pub struct ObjMesh {
    pub triangles: Vec<Triangle>,
    /// Set when the materials use a 256×1 palette texture (MagicaVoxel or
    /// Rizxel exports): colors then map one to one onto palette indices.
    pub palette: Option<Palette>,
}

/// Parses OBJ text. `load` returns a companion file's bytes by the path
/// written in the file (relative to the OBJ), for `mtllib` and textures.
pub fn read_obj(text: &str, load: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<ObjMesh, RzxlError> {
    let fail = |msg: String| Err(RzxlError::Invalid(format!("OBJ: {msg}")));
    let mut positions: Vec<Vec3> = Vec::new();
    let mut vcolors: Vec<Option<Rgba8>> = Vec::new();
    let mut uvs: Vec<Vec2> = Vec::new();
    let mut materials: HashMap<String, MtlMaterial> = HashMap::new();
    let mut textures: HashMap<String, Option<RgbaImage>> = HashMap::new();
    let mut palette: Option<Palette> = None;
    let mut current: Option<String> = None;
    let mut triangles: Vec<Triangle> = Vec::new();

    for (n, raw) in text.lines().enumerate() {
        let line = raw.trim();
        let mut it = line.split_whitespace();
        let Some(key) = it.next() else { continue };
        match key {
            "v" => {
                let f: Vec<f32> = it.filter_map(|s| s.parse().ok()).collect();
                if f.len() < 3 {
                    return fail(format!("line {}: bad vertex", n + 1));
                }
                if positions.len() >= MAX_VERTICES {
                    return fail("too many vertices".into());
                }
                positions.push(Vec3::new(f[0], f[1], f[2]));
                vcolors.push((f.len() >= 6).then(|| {
                    let c = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
                    Rgba8::rgb(c(f[3]), c(f[4]), c(f[5]))
                }));
            }
            "vt" => {
                let f: Vec<f32> = it.filter_map(|s| s.parse().ok()).collect();
                if uvs.len() >= MAX_VERTICES {
                    return fail("too many texture coordinates".into());
                }
                uvs.push(Vec2::new(
                    f.first().copied().unwrap_or(0.0),
                    f.get(1).copied().unwrap_or(0.0),
                ));
            }
            "mtllib" => {
                let name = line[6..].trim();
                if let Some(bytes) = load(name) {
                    for m in read_mtl(&String::from_utf8_lossy(&bytes)) {
                        materials.insert(m.name.clone(), m);
                    }
                }
            }
            "usemtl" => current = Some(line[6..].trim().to_owned()),
            "f" => {
                let mut corners: Vec<(usize, Option<usize>)> = Vec::new();
                for token in it.take(MAX_POLYGON) {
                    let mut parts = token.split('/');
                    let resolve = |s: Option<&str>, len: usize| -> Option<usize> {
                        let i: i64 = s.filter(|s| !s.is_empty())?.parse().ok()?;
                        let idx = if i < 0 { len as i64 + i } else { i - 1 };
                        usize::try_from(idx).ok().filter(|&i| i < len)
                    };
                    let Some(v) = resolve(parts.next(), positions.len()) else {
                        return fail(format!("line {}: bad vertex index {token:?}", n + 1));
                    };
                    corners.push((v, resolve(parts.next(), uvs.len())));
                }
                if corners.len() < 3 {
                    continue;
                }
                let material = current.as_ref().and_then(|m| materials.get(m));
                let texture = material.and_then(|m| m.map_kd.clone()).and_then(|path| {
                    textures
                        .entry(path.clone())
                        .or_insert_with(|| {
                            load(&path).and_then(|b| crate::png_io::decode_rgba(&b).ok())
                        })
                        .clone()
                });
                if palette.is_none() {
                    palette = texture
                        .as_ref()
                        .and_then(crate::format_mtl::palette_from_texture);
                }
                for k in 1..corners.len() - 1 {
                    if triangles.len() >= MAX_TRIANGLES {
                        return fail("too many faces".into());
                    }
                    let tri = [corners[0], corners[k], corners[k + 1]];
                    let p = tri.map(|(v, _)| positions[v]);
                    let color = face_color(&tri, &vcolors, &uvs, material, texture.as_ref());
                    triangles.push(Triangle { p, color });
                }
            }
            _ => {}
        }
    }
    if triangles.is_empty() {
        return fail("the file contains no faces".into());
    }
    Ok(ObjMesh { triangles, palette })
}

fn face_color(
    tri: &[(usize, Option<usize>); 3],
    vcolors: &[Option<Rgba8>],
    uvs: &[Vec2],
    material: Option<&MtlMaterial>,
    texture: Option<&RgbaImage>,
) -> Rgba8 {
    let colors: Vec<Rgba8> = tri.iter().filter_map(|(v, _)| vcolors[*v]).collect();
    if colors.len() == 3 {
        let avg = |f: fn(&Rgba8) -> u8| (colors.iter().map(|c| f(c) as u32).sum::<u32>() / 3) as u8;
        return Rgba8::rgb(avg(|c| c.r), avg(|c| c.g), avg(|c| c.b));
    }
    if let Some(img) = texture {
        let uv: Vec<Vec2> = tri.iter().filter_map(|(_, t)| t.map(|t| uvs[t])).collect();
        if uv.len() == 3 {
            let c = (uv[0] + uv[1] + uv[2]) / 3.0;
            let x = (c.x.rem_euclid(1.0) * img.width as f32) as u32;
            let y = ((1.0 - c.y).rem_euclid(1.0) * img.height as f32) as u32;
            let [r, g, b, _] = img.pixel(x, y);
            return Rgba8::rgb(r, g, b);
        }
    }
    material.map(MtlMaterial::color).unwrap_or(Rgba8::WHITE)
}

/// Loader for `read_obj` that reads companion files next to `obj_path`.
pub fn sibling_loader(obj_path: &Path) -> impl Fn(&str) -> Option<Vec<u8>> {
    let dir: PathBuf = obj_path.parent().map(Path::to_path_buf).unwrap_or_default();
    move |name: &str| {
        let rel = Path::new(name);
        let path = if rel.is_absolute() {
            rel.to_path_buf()
        } else {
            dir.join(rel)
        };
        // Companion files are small; refuse anything absurd.
        let meta = std::fs::metadata(&path).ok()?;
        (meta.len() <= 256 * 1024 * 1024).then(|| std::fs::read(&path).ok())?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rizxel_math::Rgba8;

    #[test]
    fn generic_cube_has_8_vertices_and_12_faces() {
        let mut out = Vec::new();
        export_obj(&Mesh::unit_cube(Rgba8::WHITE), &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert_eq!(text.lines().filter(|l| l.starts_with("v ")).count(), 8);
        assert_eq!(text.lines().filter(|l| l.starts_with("f ")).count(), 12);
        assert!(text.contains("v 0 0 0 1.0000 1.0000 1.0000"));
        assert!(text.contains("f 1 3 4"));
        let mut m = Mesh::unit_cube(Rgba8::WHITE);
        m.indices.push(99);
        assert!(export_obj(&m, Vec::new()).is_err());
    }

    fn one_voxel() -> (VoxelVolume, BoundSize) {
        let bounds = BoundSize::new(20, 20, 21);
        let mut v = VoxelVolume::new(bounds);
        v.set(IVec3::new(0, 0, 0), 9);
        (v, bounds)
    }

    #[test]
    fn layout_matches_magicavoxel() {
        let (v, bounds) = one_voxel();
        let e = build_obj_export(&v, bounds, &Palette::default(), "chr_knight", 0.1).unwrap();
        let lines: Vec<&str> = e.obj.split("\r\n").collect();
        assert_eq!(
            &lines[..17],
            &[
                "# Rizxel @ Abati Technology",
                "",
                "# group",
                "o ",
                "",
                "# material",
                "mtllib chr_knight.mtl",
                "usemtl palette",
                "",
                "# normals",
                "vn -1 0 0",
                "vn 1 0 0",
                "vn 0 0 1",
                "vn 0 0 -1",
                "vn 0 -1 0",
                "vn 0 1 0",
                "",
            ]
        );
        assert_eq!(lines[17], "# texcoords");
        assert_eq!(lines[18], "vt 0.0332031 0.5", "color 9 -> pixel 8");
        assert!(e.obj.ends_with("\r\n") && !e.obj.ends_with("\r\n\r\n"));
        assert!(!e.obj.contains("\n\n") && !e.obj.replace("\r\n", "").contains('\n'));
        assert_eq!(e.triangles, 12);
        assert_eq!(lines.iter().filter(|l| l.starts_with("v ")).count(), 24);
        // Voxel (0,0,0) at the back-left of the 20×21 footprint: Rizxel
        // z = 0 is MagicaVoxel y = 20, so x -1..-0.9, y 0..0.1 and
        // z -(20-10)·0.1 .. -(21-10)·0.1 = -1..-1.1.
        assert!(lines.contains(&"v -1 0 -1.1"), "{lines:?}");
        assert!(lines.contains(&"v -0.9 0.1 -1"));
        // First face uses normal 1 and the single texcoord.
        assert!(lines
            .iter()
            .any(|l| l.starts_with("f ") && l.ends_with("/1/1")));
    }

    #[test]
    fn triangles_face_outward() {
        let bounds = BoundSize::new(8, 8, 8);
        let mut v = VoxelVolume::new(bounds);
        for p in [
            IVec3::ZERO,
            IVec3::X,
            IVec3::new(1, 1, 0),
            IVec3::new(3, 2, 5),
        ] {
            v.set(p, 3);
        }
        let e = build_obj_export(&v, bounds, &Palette::default(), "m", 1.0).unwrap();
        let mesh = read_obj(&e.obj, &|_| None).unwrap();
        let normals = [
            Vec3::NEG_X,
            Vec3::X,
            Vec3::Z,
            Vec3::NEG_Z,
            Vec3::NEG_Y,
            Vec3::Y,
        ];
        let faces: Vec<&str> = e
            .obj
            .split("\r\n")
            .filter(|l| l.starts_with("f "))
            .collect();
        for (tri, f) in mesh.triangles.iter().zip(faces) {
            let n: usize = f.rsplit('/').next().unwrap().parse().unwrap();
            let [a, b, c] = tri.p;
            assert!((b - a).cross(c - a).dot(normals[n - 1]) > 0.0, "{f}");
        }
    }

    #[test]
    fn export_then_import_restores_palette_indices() {
        let bounds = BoundSize::new(6, 4, 5);
        let mut v = VoxelVolume::new(bounds);
        v.set(IVec3::new(1, 0, 1), 9);
        v.set(IVec3::new(2, 0, 1), 40);
        v.set(IVec3::new(2, 1, 1), 40);
        let palette = Palette::default();
        let e = build_obj_export(&v, bounds, &palette, "m", 0.1).unwrap();
        let mtl = e.mtl.clone();
        let png = e.png.clone();
        let load = move |name: &str| match name {
            "m.mtl" => Some(mtl.clone().into_bytes()),
            "m.png" => Some(png.clone()),
            _ => None,
        };
        let mesh = read_obj(&e.obj, &load).unwrap();
        let pal = mesh.palette.expect("palette texture detected");
        assert_eq!(pal.get(40), palette.get(40));
        let c40 = palette.get(40).unwrap();
        assert!(mesh
            .triangles
            .iter()
            .any(|t| t.color == Rgba8::rgb(c40.r, c40.g, c40.b)));
    }

    #[test]
    fn reads_polygons_negative_indices_and_vertex_colors() {
        let text = "v 0 0 0 1 0 0\nv 1 0 0 1 0 0\nv 1 1 0 1 0 0\nv 0 1 0 1 0 0\nf -4 -3 -2 -1\n";
        let mesh = read_obj(text, &|_| None).unwrap();
        assert_eq!(mesh.triangles.len(), 2);
        assert!(mesh
            .triangles
            .iter()
            .all(|t| t.color == Rgba8::rgb(255, 0, 0)));
        assert!(read_obj("v 0 0 0\nf 1 2 3\n", &|_| None).is_err());
        assert!(read_obj("# nothing\n", &|_| None).is_err());
    }

    #[test]
    fn matches_magicavoxel_reference_when_present() {
        // The knight imported from MagicaVoxel and exported again lands on
        // exactly the same surface as MagicaVoxel's own OBJ export.
        // Both files come from `RIZXEL_SAMPLES` (see `test_samples`).
        let (Some(vox), Some(obj)) = (
            crate::test_samples::sample("chr_knight.vox"),
            crate::test_samples::sample("chr_knight.obj"),
        ) else {
            return;
        };
        let (Ok(vox), Ok(reference)) = (std::fs::read(vox), std::fs::read_to_string(obj)) else {
            return;
        };
        let project = crate::read_vox(&vox, "chr_knight").unwrap();
        let mut merged = VoxelVolume::new(project.bounds);
        for l in &project.layers {
            for (p, c) in crate::volume_from_runs(project.bounds, &l.voxels)
                .unwrap()
                .iter()
            {
                merged.set(p, c);
            }
        }
        let ours =
            build_obj_export(&merged, project.bounds, &project.palette, "chr_knight", 0.1).unwrap();
        // Same header layout, normals and texcoords.
        let section = |t: &str, prefix: &str| -> Vec<String> {
            t.split("\r\n")
                .filter(|l| l.starts_with(prefix))
                .map(str::to_owned)
                .collect()
        };
        assert_eq!(section(&ours.obj, "vn "), section(&reference, "vn "));
        assert_eq!(section(&ours.obj, "vt "), section(&reference, "vt "));
        // Same covered surface: rasterize both meshes' faces into unit cells.
        let cells = |text: &str| -> std::collections::BTreeSet<(i64, i64, i64, i64, u32)> {
            let mesh = read_obj(text, &|_| None).unwrap();
            let mut out = std::collections::BTreeSet::new();
            for t in &mesh.triangles {
                let q = |v: Vec3| (v * 10.0).round();
                let [a, b, c] = t.p.map(q);
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
                                let dir = n.abs().max_position() as i64 * 2
                                    + (n.max_element() > 0.5) as i64;
                                out.insert((x, y, z, dir, 0));
                            }
                        }
                    }
                }
            }
            out
        };
        assert_eq!(cells(&ours.obj), cells(&reference));
        assert!(ours.triangles <= section(&reference, "f ").len());
    }
}

//! Wavefront OBJ export and import.
//!
//! [`write_obj`] writes one of two layouts ([`ObjLayout`]).
//!
//! ## Export (Fast load layout, the default)
//!
//! Follows BrickLink Studio's OBJ export, which 3D apps load quickly:
//!
//! - Every quad corner is its own vertex with a position, a texcoord and a
//!   normal, all at the same index, so faces read `f 1/1/1 2/2/2 3/3/3`
//!   and loaders copy them straight into vertex buffers.
//! - Faces are grouped by palette color (and layer material) with
//!   `usemtl color_<index>`; the `.mtl` gives each group a plain linear
//!   `Kd`, plus `Ke` for emission, `d` for opacity and `Pr`/`Pm`. No
//!   texture is needed. Texcoords still point into the palette strip, so
//!   the MagicaVoxel `.png` can be applied by hand.
//! - The second header line ([`FAST_LOAD_MARK`]) tells the importer that
//!   `Kd` is linear.
//! - Files are about 3–5× the size of the MagicaVoxel layout.
//!
//! ## Export (MagicaVoxel layout)
//!
//! Writes `name.obj`, `name.mtl` and `name.png` in the same
//! layout as MagicaVoxel's OBJ export (reference: MagicaVoxel's own
//! `chr_knight.obj` export of its `chr_knight.vox` sample):
//!
//! - **Text**: CRLF lines. A `# Rizxel @ MRizky099` header, then the
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
use rizxel_math::{BoundSize, Material, Palette, Rgba8, Voxel, VoxelVolume, EMPTY_VOXEL};

use crate::format_mtl::{
    fmt_g, read_mtl, write_palette_mtl, MtlMaterial, EXPORT_HEADER, PALETTE_MATERIAL,
};
use crate::interchange::{to_zup, zup_size, ExportScene};
use crate::mesh_export::{linear_to_srgb, srgb_to_linear, Pivot};
use crate::png_io::{encode_rgba, RgbaImage};
use crate::RzxlError;

pub const OBJ_EXTENSION: &str = "obj";
/// MagicaVoxel's default OBJ scale (`config.txt`, `io_obj.scale`).
pub const DEFAULT_OBJ_SCALE: f32 = 0.1;
/// Second line of Fast load OBJ files: their `Kd` values are linear.
pub const FAST_LOAD_MARK: &str = "# layout: fast load (per-corner vertices, linear Kd)";

/// How an OBJ export is laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ObjLayout {
    /// Per-corner vertices, one material per color, no texture.
    #[default]
    FastLoad,
    /// MagicaVoxel's layout: shared vertices, six normals, palette PNG.
    MagicaVoxel,
}

impl ObjLayout {
    pub const ALL: [ObjLayout; 2] = [ObjLayout::FastLoad, ObjLayout::MagicaVoxel];

    pub const fn label(self) -> &'static str {
        match self {
            ObjLayout::FastLoad => "Fast load",
            ObjLayout::MagicaVoxel => "MagicaVoxel",
        }
    }

    pub const fn note(self) -> &'static str {
        match self {
            ObjLayout::FastLoad => "one index per corner, a color per material, no texture",
            ObjLayout::MagicaVoxel => "shared vertices and a palette PNG; smallest files",
        }
    }
}

/// OBJ export settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ObjOptions {
    pub layout: ObjLayout,
    /// Units per voxel.
    pub scale: f32,
    /// Rizxel point placed at the origin (see [`Pivot`]).
    pub origin: Vec3,
}

impl ObjOptions {
    /// Defaults for `bounds`: Fast load, scale 0.1, bottom-center pivot.
    pub fn new(bounds: BoundSize) -> Self {
        Self {
            layout: ObjLayout::default(),
            scale: DEFAULT_OBJ_SCALE,
            origin: Pivot::default().origin(bounds),
        }
    }
}

fn valid_scale(scale: f32) -> f32 {
    if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        DEFAULT_OBJ_SCALE
    }
}

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

/// Builds the MagicaVoxel-layout OBJ/MTL/PNG triple for `volume` (the
/// merged visible layers), placed like MagicaVoxel. `stem` names the
/// companion files (`stem.mtl`, `stem.png`).
pub fn build_obj_export(
    volume: &VoxelVolume,
    bounds: BoundSize,
    palette: &Palette,
    stem: &str,
    scale: f32,
) -> Result<ObjExport, RzxlError> {
    build_magica_obj(
        volume,
        bounds,
        palette,
        stem,
        scale,
        Pivot::BottomCenter.origin(bounds),
    )
}

fn build_magica_obj(
    volume: &VoxelVolume,
    bounds: BoundSize,
    palette: &Palette,
    stem: &str,
    scale: f32,
    origin: Vec3,
) -> Result<ObjExport, RzxlError> {
    if volume.is_empty() {
        return Err(RzxlError::Invalid(
            "nothing to export: the visible layers contain no voxels".into(),
        ));
    }
    let faces = zup_faces(volume, bounds);
    let scale = valid_scale(scale);
    let origin = if origin.is_finite() {
        origin
    } else {
        Vec3::ZERO
    };
    let depth = bounds.z() as i32;
    // Z-up corner (x, y, z) is Rizxel corner (x, z, depth - y).
    let position = |g: IVec3| {
        let r = IVec3::new(g.x, g.z, depth - g.y).as_vec3() - origin;
        let c = |v: f32| fmt_g(v * scale + 0.0);
        format!("v {} {} {}", c(r.x), c(r.y), c(r.z))
    };

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
            line(&position(*p));
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

/// Builds the Fast load OBJ and MTL for the visible layers of `scene`.
fn build_fast_obj(
    scene: &ExportScene,
    stem: &str,
    options: &ObjOptions,
) -> Result<ObjExport, RzxlError> {
    let bounds = scene.bounds;
    let scale = valid_scale(options.scale);
    let origin = if options.origin.is_finite() {
        options.origin
    } else {
        Vec3::ZERO
    };
    // Merged colors plus the material of the layer that set each voxel.
    let mut colors = VoxelVolume::new(bounds);
    let mut slots = VoxelVolume::new(bounds);
    let mut materials: Vec<(Option<Material>, f32)> = vec![(None, 1.0)];
    for l in scene.layers.iter().filter(|l| l.visible) {
        let key = (l.material.clone(), l.opacity.clamp(0.0, 1.0));
        let slot = match materials.iter().position(|m| *m == key) {
            Some(i) => i,
            None if materials.len() < 255 => {
                materials.push(key);
                materials.len() - 1
            }
            None => 0,
        };
        for (p, v) in l.volume.iter() {
            colors.set(p, v);
            slots.set(p, slot as Voxel + 1);
        }
    }
    if colors.is_empty() {
        return Err(RzxlError::Invalid(
            "nothing to export: the visible layers contain no voxels".into(),
        ));
    }
    // Greedy quads over (color, material) keys; with more than 255 keys
    // the material falls back to each color's first one.
    let mut keys: Vec<(Voxel, u8)> = colors
        .iter()
        .map(|(p, v)| (v, slots.get(p).saturating_sub(1)))
        .collect();
    keys.sort_unstable();
    keys.dedup();
    if keys.len() > 255 {
        keys.dedup_by_key(|k| k.0);
    }
    let id_of: HashMap<(Voxel, u8), Voxel> = keys
        .iter()
        .enumerate()
        .map(|(i, &k)| (k, i as Voxel + 1))
        .collect();
    let first_of: HashMap<Voxel, Voxel> = keys
        .iter()
        .enumerate()
        .rev()
        .map(|(i, &(c, _))| (c, i as Voxel + 1))
        .collect();
    let mut keyed = VoxelVolume::new(bounds);
    for (p, v) in colors.iter() {
        let k = (v, slots.get(p).saturating_sub(1));
        let id = id_of.get(&k).or_else(|| first_of.get(&v)).copied();
        keyed.set(p, id.unwrap_or(EMPTY_VOXEL));
    }
    let mut quads = greedy_quads(&keyed);
    quads.sort_by_key(|q| q.value);

    let name_of = |id: Voxel| {
        let (color, slot) = keys[id as usize - 1];
        if slot == 0 {
            format!("color_{color}")
        } else {
            format!("color_{color}_m{slot}")
        }
    };
    let mut s = String::new();
    let mut line = |l: &str| {
        s.push_str(l);
        s.push_str("\r\n");
    };
    line(EXPORT_HEADER);
    line(FAST_LOAD_MARK);
    line("");
    line(&format!("mtllib {stem}.mtl"));
    line(&format!("o {stem}"));
    line("");
    let corner = |p: IVec3| (p.as_vec3() - origin) * scale;
    for q in &quads {
        for p in q.corners() {
            let v = corner(p);
            line(&format!(
                "v {} {} {}",
                fmt_g(v.x + 0.0),
                fmt_g(v.y + 0.0),
                fmt_g(v.z + 0.0)
            ));
        }
    }
    for q in &quads {
        let (color, _) = keys[q.value as usize - 1];
        let u = fmt_g((color as f32 - 0.5) / 256.0);
        for _ in 0..4 {
            line(&format!("vt {u} 0.5"));
        }
    }
    for q in &quads {
        let mut n = IVec3::ZERO;
        n[q.axis] = if q.positive { 1 } else { -1 };
        for _ in 0..4 {
            line(&format!("vn {} {} {}", n.x, n.y, n.z));
        }
    }
    let mut current = None;
    let mut triangles = 0;
    for (i, q) in quads.iter().enumerate() {
        if current != Some(q.value) {
            current = Some(q.value);
            line("");
            line(&format!("usemtl {}", name_of(q.value)));
        }
        let b = i * 4 + 1;
        let [a, bb, c, d] = [b, b + 1, b + 2, b + 3];
        let tris = if q.abc_is_ccw() {
            [[a, bb, c], [c, bb, d]]
        } else {
            [[c, bb, a], [d, bb, c]]
        };
        for t in tris {
            line(&format!(
                "f {0}/{0}/{0} {1}/{1}/{1} {2}/{2}/{2}",
                t[0], t[1], t[2]
            ));
            triangles += 1;
        }
    }

    // One material per used key.
    let mut m = String::new();
    let mut mline = |l: &str| {
        m.push_str(l);
        m.push_str("\r\n");
    };
    mline(EXPORT_HEADER);
    mline("# Kd is linear RGB");
    let default = Material::default();
    for (id, &(color, slot)) in keys.iter().enumerate() {
        let (mat, opacity) = &materials[slot as usize];
        let mat = mat.as_ref().unwrap_or(&default);
        let c = scene.palette.get(color as usize).unwrap_or(Rgba8::WHITE);
        let kd = [c.r, c.g, c.b]
            .iter()
            .zip(mat.tint)
            .map(|(&v, t)| format!("{:.6}", srgb_to_linear(v) * t))
            .collect::<Vec<_>>()
            .join(" ");
        mline("");
        mline(&format!("newmtl {}", name_of(id as Voxel + 1)));
        mline(&format!("Kd {kd}"));
        if mat.emission.iter().any(|&e| e > 0.0) {
            let [r, g, b] = mat.emission;
            mline(&format!("Ke {r:.6} {g:.6} {b:.6}"));
        }
        let d = (mat.tint[3] * opacity).clamp(0.0, 1.0);
        if d < 1.0 {
            mline(&format!("d {d:.6}"));
        }
        mline(&format!("Pr {:.6}", mat.roughness));
        mline(&format!("Pm {:.6}", mat.metallic));
    }
    Ok(ObjExport {
        obj: s,
        mtl: m,
        png: Vec::new(),
        triangles,
    })
}

/// Builds the OBJ export of the visible layers of `scene` in the chosen
/// layout. `png` is empty for the Fast load layout.
pub fn build_obj(
    scene: &ExportScene,
    stem: &str,
    options: &ObjOptions,
) -> Result<ObjExport, RzxlError> {
    match options.layout {
        ObjLayout::FastLoad => build_fast_obj(scene, stem, options),
        ObjLayout::MagicaVoxel => build_magica_obj(
            &scene.merged_visible(),
            scene.bounds,
            &scene.palette,
            stem,
            options.scale,
            options.origin,
        ),
    }
}

/// Writes `path` (`.obj`) plus `.mtl` (and `.png` for the MagicaVoxel
/// layout) with the same stem.
pub fn write_obj(
    path: &Path,
    scene: &ExportScene,
    options: &ObjOptions,
) -> Result<ObjExport, RzxlError> {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "model".to_owned());
    let export = build_obj(scene, &stem, options)?;
    let sibling = |ext: &str| path.with_file_name(format!("{stem}.{ext}"));
    std::fs::write(path, export.obj.as_bytes())?;
    std::fs::write(
        sibling(crate::format_mtl::MTL_EXTENSION),
        export.mtl.as_bytes(),
    )?;
    if !export.png.is_empty() {
        std::fs::write(sibling("png"), &export.png)?;
    }
    Ok(export)
}

/// Writes `path` (`.obj`) plus `.mtl` and `.png` with the same stem, in the
/// MagicaVoxel layout. Returns the number of triangles.
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
    let mut linear_kd = false;

    for (n, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line == FAST_LOAD_MARK {
            linear_kd = true;
        }
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
                    for mut m in read_mtl(&String::from_utf8_lossy(&bytes)) {
                        if linear_kd {
                            m.kd = m.kd.map(|k| k.map(linear_to_srgb));
                        }
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
                "# Rizxel @ MRizky099",
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

    fn fast_scene() -> ExportScene {
        let bounds = BoundSize::new(9, 7, 6);
        let mut a = VoxelVolume::new(bounds);
        for x in 0..6 {
            a.set(IVec3::new(x, 0, 1), 9);
            a.set(IVec3::new(x, 1, 1), if x < 3 { 40 } else { 9 });
        }
        a.set(IVec3::new(2, 2, 1), 1);
        let mut b = VoxelVolume::new(bounds);
        b.set(IVec3::new(7, 5, 4), 40);
        let mut layers = vec![
            crate::interchange::ExportLayer::new("Body", true, a),
            crate::interchange::ExportLayer::new("Glow", true, b),
        ];
        layers[1].material = Some(Material {
            emission: [1.0, 0.25, 0.0],
            ..Material::new("Lamp")
        });
        layers[1].opacity = 0.5;
        ExportScene {
            name: "t".into(),
            bounds,
            palette: Palette::default(),
            layers,
            groups: Vec::new(),
        }
    }

    fn mtl_loader(e: &ObjExport) -> impl Fn(&str) -> Option<Vec<u8>> {
        let (mtl, png) = (e.mtl.clone(), e.png.clone());
        move |name: &str| match name {
            "m.mtl" => Some(mtl.clone().into_bytes()),
            "m.png" => (!png.is_empty()).then(|| png.clone()),
            _ => None,
        }
    }

    #[test]
    fn fast_layout_uses_one_index_per_corner() {
        let s = fast_scene();
        let e = build_obj(&s, "m", &ObjOptions::new(s.bounds)).unwrap();
        assert!(e.png.is_empty(), "no texture");
        let lines: Vec<&str> = e.obj.split("\r\n").collect();
        assert_eq!(lines[0], EXPORT_HEADER);
        assert_eq!(lines[1], FAST_LOAD_MARK);
        let count = |p: &str| lines.iter().filter(|l| l.starts_with(p)).count();
        let verts = count("v ");
        assert_eq!(verts, count("vt "));
        assert_eq!(verts, count("vn "));
        assert_eq!(verts % 4, 0);
        for f in lines.iter().filter(|l| l.starts_with("f ")) {
            for corner in f[2..].split(' ') {
                let parts: Vec<&str> = corner.split('/').collect();
                assert_eq!(parts.len(), 3, "{f}");
                assert!(parts.iter().all(|p| *p == parts[0]), "{f}");
                assert!(parts[0].parse::<usize>().unwrap() <= verts);
            }
        }
        assert_eq!(count("f "), e.triangles);
    }

    #[test]
    fn fast_layout_groups_faces_by_color_and_material() {
        let s = fast_scene();
        let e = build_obj(&s, "m", &ObjOptions::new(s.bounds)).unwrap();
        let groups: Vec<&str> = e
            .obj
            .split("\r\n")
            .filter_map(|l| l.strip_prefix("usemtl "))
            .collect();
        // Colors 1, 9 and 40 in the body, 40 again with the lamp material.
        assert_eq!(groups, ["color_1", "color_9", "color_40", "color_40_m1"]);
        let mtl = read_mtl(&e.mtl);
        let names: Vec<&str> = mtl.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, groups);
        let lamp = &mtl[3];
        assert_eq!(lamp.ke, Some([1.0, 0.25, 0.0]));
        assert_eq!(lamp.opacity, Some(0.5));
        assert!(mtl.iter().all(|m| m.map_kd.is_none()));
        // Kd is linear: palette color 9 comes back through the importer.
        let c9 = s.palette.get(9).unwrap();
        let kd = mtl[1].kd.unwrap();
        assert!((kd[0] - srgb_to_linear(c9.r)).abs() < 1e-5);
    }

    #[test]
    fn both_layouts_cover_the_same_surface_and_import_the_same_voxels() {
        let s = fast_scene();
        let mut voxels = Vec::new();
        let mut surfaces = Vec::new();
        for layout in ObjLayout::ALL {
            let options = ObjOptions {
                layout,
                ..ObjOptions::new(s.bounds)
            };
            let e = build_obj(&s, "m", &options).unwrap();
            let mesh = read_obj(&e.obj, &mtl_loader(&e)).unwrap();
            let tris: Vec<[Vec3; 3]> = mesh.triangles.iter().map(|t| t.p).collect();
            surfaces.push(crate::test_samples::surface_cells(&tris, 10.0));
            let (p, info) = crate::mesh_import::project_from_mesh(
                mesh.triangles,
                crate::UpAxis::Y,
                mesh.palette,
                rizxel_engine::VoxelizeOptions::default(),
                "m",
            )
            .unwrap();
            assert!(info.exact, "{layout:?}");
            let v = crate::volume_from_runs(p.bounds, &p.layers[0].voxels).unwrap();
            let mut cells: Vec<(IVec3, Rgba8)> = v
                .iter()
                .map(|(q, c)| {
                    let k = p.palette.get(c as usize).unwrap();
                    (q, Rgba8::rgb(k.r, k.g, k.b))
                })
                .collect();
            cells.sort_by_key(|(q, _)| (q.z, q.y, q.x));
            voxels.push(cells);
        }
        assert_eq!(surfaces[0], surfaces[1]);
        assert_eq!(voxels[0], voxels[1]);
        let expected: usize = s.merged_visible().voxel_count();
        assert_eq!(voxels[0].len(), expected);
        // Colors are the palette's own.
        let c40 = s.palette.get(40).unwrap();
        assert!(voxels[0].contains(&(
            IVec3::new(7, 5, 4) - IVec3::new(0, 0, 1),
            Rgba8::rgb(c40.r, c40.g, c40.b)
        )));
    }

    #[test]
    fn magicavoxel_layout_through_build_obj_matches_the_old_writer() {
        let s = fast_scene();
        let options = ObjOptions {
            layout: ObjLayout::MagicaVoxel,
            ..ObjOptions::new(s.bounds)
        };
        let new = build_obj(&s, "m", &options).unwrap();
        let old = build_obj_export(&s.merged_visible(), s.bounds, &s.palette, "m", 0.1).unwrap();
        assert_eq!(new, old);
        // Other pivots move every vertex by the same amount.
        let corner = ObjOptions {
            origin: Pivot::Corner.origin(s.bounds),
            ..options
        };
        let moved = build_obj(&s, "m", &corner).unwrap();
        let first = |e: &ObjExport| -> Vec3 {
            let l = e.obj.split("\r\n").find(|l| l.starts_with("v ")).unwrap();
            let f: Vec<f32> = l[2..].split(' ').map(|x| x.parse().unwrap()).collect();
            Vec3::new(f[0], f[1], f[2])
        };
        let shift = first(&moved) - first(&new);
        let o = Pivot::BottomCenter.origin(s.bounds) * 0.1;
        assert!((shift - o).abs().max_element() < 1e-5, "{shift} {o}");
    }

    #[test]
    fn fast_layout_refuses_empty_scenes() {
        let mut s = fast_scene();
        s.layers.iter_mut().for_each(|l| l.visible = false);
        assert!(build_obj(&s, "m", &ObjOptions::new(s.bounds)).is_err());
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
        let cells = |text: &str| {
            let tris: Vec<[Vec3; 3]> = read_obj(text, &|_| None)
                .unwrap()
                .triangles
                .iter()
                .map(|t| t.p)
                .collect();
            crate::test_samples::surface_cells(&tris, 10.0)
        };
        assert_eq!(cells(&ours.obj), cells(&reference));
        assert!(ours.triangles <= section(&reference, "f ").len());
    }
}

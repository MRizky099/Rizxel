//! Qubicle Binary Tree `.qbt` import and export.
//!
//! Layout (little-endian): `"QB 2"`, version `1.0` (two bytes), a global
//! scale (3 × `f32`), a `COLORMAP` section (`u32` count, then RGBA colors;
//! with colors, a voxel's red byte is an index into them), and a `DATATREE`
//! section holding one node. Every node starts with a `u32` type and the
//! `u32` byte size of its body:
//!
//! | Type | Node | Body |
//! |---|---|---|
//! | 0 | Matrix | name (`u32` length + bytes), position (3 × `i32`), local scale (3 × `u32`), pivot (3 × `f32`), size (3 × `u32`), zlib data (`u32` length + bytes) |
//! | 1 | Model | `u32` child count, child nodes |
//! | 2 | Compound | a matrix body (the merge of its children), `u32` child count, child nodes |
//!
//! Checked in Qubicle 3.1: a matrix sits at its position whatever its pivot,
//! children of nested models keep their absolute positions, Qubicle draws a
//! compound from its own merged matrix (placed by rules that also depend on
//! its pivot and size), and faces are drawn from the mask bits (see
//! [`crate::qubicle::face_mask`]).
//!
//! Matrix data is 4 bytes per voxel (`r g b mask`, mask 0 = empty) with Y
//! fastest, then Z, then X.
//!
//! - **Import**: matrices become layers at their positions (bounds as for
//!   `.qb`); nested models and compounds become groups, and a compound's
//!   own merged matrix is skipped when it has children. Qubicle's frame is
//!   Rizxel's, so nothing is mirrored. Unknown node types are skipped.
//! - **Export**: a root model; each visible layer is a matrix cut to its
//!   voxels (pivot at its center), each group holding exported layers a
//!   nested model. Models have no name, so group names are not kept. No
//!   color map (true colors) and the face mask of every voxel. Re-exporting
//!   a file Rizxel wrote gives the same bytes.

use std::io::Read;

use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use glam::{IVec3, Vec3};
use rizxel_math::{Palette, Rgba8, VoxelVolume, EMPTY_VOXEL};

use crate::interchange::ExportScene;
use crate::qubicle::{
    color_of, face_mask, name_bytes, project_from_matrices, put_ivec3, put_u32, ByteReader, QGroup,
    QMatrix,
};
use crate::{ProjectFile, RzxlError, MAX_GROUP_DEPTH, MAX_LAYERS};

pub const QBT_EXTENSION: &str = "qbt";
const MAGIC: &[u8; 4] = b"QB 2";
const NODE_MATRIX: u32 = 0;
const NODE_MODEL: u32 = 1;
const NODE_COMPOUND: u32 = 2;
/// Longest node name accepted when reading.
const MAX_NAME_BYTES: usize = 64 * 1024;
/// Most solid voxels accepted across all matrices.
const MAX_CELLS: usize = 64 * 1024 * 1024;

struct Tree {
    matrices: Vec<QMatrix>,
    groups: Vec<QGroup>,
    colors: Vec<Rgba8>,
    cells: usize,
}

/// One matrix body: name, position and voxels (scale and pivot are read
/// and ignored).
/// Returns the matrix and its pivot.
fn read_matrix(r: &mut ByteReader, tree: &mut Tree) -> Result<(QMatrix, Vec3), RzxlError> {
    let len = r.u32()? as usize;
    if len > MAX_NAME_BYTES {
        return Err(r.invalid(format!("node name of {len} bytes")));
    }
    let name = String::from_utf8_lossy(r.bytes(len)?).into_owned();
    let pos = r.ivec3()?;
    for _ in 0..3 {
        r.u32()?; // local scale
    }
    let pivot = Vec3::new(r.f32()?, r.f32()?, r.f32()?);
    let size = r.size()?;
    let packed = r.u32()? as usize;
    let data = r.bytes(packed)?;
    let expected = (size.x * size.y * size.z) as usize * 4;
    let mut raw = Vec::with_capacity(expected.min(packed.saturating_mul(64)));
    ZlibDecoder::new(data)
        .take(expected as u64 + 1)
        .read_to_end(&mut raw)
        .map_err(|e| r.invalid(format!("matrix {name:?}: {e}")))?;
    if raw.len() != expected {
        return Err(r.invalid(format!(
            "matrix {name:?} holds {} bytes of voxels; its size needs {expected}",
            raw.len()
        )));
    }
    let (sy, sz) = (size.y as usize, size.z as usize);
    let mut cells = Vec::new();
    for (i, v) in raw.chunks_exact(4).enumerate() {
        if v[3] == 0 {
            continue;
        }
        let color = if tree.colors.is_empty() {
            Rgba8::rgb(v[0], v[1], v[2])
        } else {
            *tree
                .colors
                .get(v[0] as usize)
                .ok_or_else(|| r.invalid(format!("color index {} past the color map", v[0])))?
        };
        let (y, z, x) = (i % sy, (i / sy) % sz, i / (sy * sz));
        cells.push((IVec3::new(x as i32, y as i32, z as i32), color));
    }
    tree.cells += cells.len();
    if tree.cells > MAX_CELLS {
        return Err(r.invalid("too many voxels"));
    }
    Ok((
        QMatrix {
            name,
            pos,
            size,
            cells,
            group: None,
        },
        pivot,
    ))
}

fn read_children(
    r: &mut ByteReader,
    tree: &mut Tree,
    depth: usize,
    group: Option<usize>,
) -> Result<(), RzxlError> {
    let count = r.u32()? as usize;
    if count > r.remaining() / 8 {
        return Err(r.invalid(format!("{count} child nodes in too little data")));
    }
    for _ in 0..count {
        read_node(r, tree, depth + 1, group)?;
    }
    Ok(())
}

fn read_node(
    r: &mut ByteReader,
    tree: &mut Tree,
    depth: usize,
    group: Option<usize>,
) -> Result<(), RzxlError> {
    if depth > MAX_GROUP_DEPTH {
        return Err(r.invalid(format!("nodes nested deeper than {MAX_GROUP_DEPTH}")));
    }
    if tree.matrices.len() + tree.groups.len() > MAX_LAYERS {
        return Err(r.invalid(format!("more than {MAX_LAYERS} nodes")));
    }
    let kind = r.u32()?;
    let size = r.u32()? as usize;
    match kind {
        NODE_MATRIX => {
            // Plain matrices sit at their position whatever the pivot.
            let (m, _) = read_matrix(r, tree)?;
            tree.matrices.push(QMatrix { group, ..m });
        }
        NODE_MODEL => {
            // The root model is the scene itself; nested ones are groups.
            let inner = if depth == 0 {
                group
            } else {
                tree.groups.push(QGroup {
                    name: format!("Model {}", tree.groups.len() + 1),
                    parent: group,
                });
                Some(tree.groups.len() - 1)
            };
            read_children(r, tree, depth, inner)?;
        }
        NODE_COMPOUND => {
            let (merged, pivot) = read_matrix(r, tree)?;
            let g = tree.groups.len();
            tree.groups.push(QGroup {
                name: merged.name.clone(),
                parent: group,
            });
            let before = tree.matrices.len();
            read_children(r, tree, depth, Some(g))?;
            if tree.matrices.len() == before {
                // No children: the compound's own voxels are its content.
                // Qubicle places a compound at `position - pivot`.
                let shift = if pivot.is_finite() && pivot.abs().max_element() <= 1024.0 {
                    pivot.round().as_ivec3()
                } else {
                    IVec3::ZERO
                };
                tree.matrices.push(QMatrix {
                    group: Some(g),
                    pos: merged.pos - shift,
                    ..merged
                });
            } else {
                tree.cells -= merged.cells.len();
            }
        }
        _ => {
            r.bytes(size)?;
        }
    }
    Ok(())
}

/// Parses a `.qbt` file into a project named `name`.
pub fn read_qbt(bytes: &[u8], name: &str) -> Result<ProjectFile, RzxlError> {
    let mut r = ByteReader::new(bytes, "Qubicle Binary Tree");
    if r.bytes(4)? != MAGIC {
        return Err(r.invalid("not a .qbt file"));
    }
    let major = r.u8()?;
    r.u8()?;
    if major != 1 {
        return Err(r.invalid(format!("unsupported version {major}")));
    }
    for _ in 0..3 {
        r.f32()?; // global scale
    }
    if r.bytes(8)? != b"COLORMAP" {
        return Err(r.invalid("missing COLORMAP section"));
    }
    let count = r.u32()? as usize;
    if count > 256 {
        return Err(r.invalid(format!("color map of {count} colors")));
    }
    let mut tree = Tree {
        matrices: Vec::new(),
        groups: Vec::new(),
        colors: Vec::with_capacity(count),
        cells: 0,
    };
    for _ in 0..count {
        let c = r.bytes(4)?;
        tree.colors.push(Rgba8::rgb(c[0], c[1], c[2]));
    }
    if r.bytes(8)? != b"DATATREE" {
        return Err(r.invalid("missing DATATREE section"));
    }
    read_node(&mut r, &mut tree, 0, None)?;
    let palette = (!tree.colors.is_empty()
        && tree.colors.len() <= rizxel_engine::quantize::MAX_PAINT_COLORS)
        .then(|| {
            let mut colors = vec![Rgba8::BLACK];
            colors.extend(&tree.colors);
            Palette::new(colors)
        });
    project_from_matrices(name, tree.matrices, &tree.groups, palette)
}

// ------------------------------------------------------------------ writing

fn node(kind: u32, body: Vec<u8>) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len() + 8);
    put_u32(&mut out, kind);
    put_u32(&mut out, body.len() as u32);
    out.extend_from_slice(&body);
    out
}

/// Matrix body for the voxels of `volume` inside `[lo, hi)`.
fn matrix_body(
    name: &str,
    volume: &VoxelVolume,
    lo: IVec3,
    hi: IVec3,
    pivot: Vec3,
    palette: &Palette,
) -> Result<Vec<u8>, RzxlError> {
    let size = hi - lo;
    let mut out = Vec::new();
    let name = name_bytes(name, MAX_NAME_BYTES);
    put_u32(&mut out, name.len() as u32);
    out.extend_from_slice(name);
    put_ivec3(&mut out, lo);
    put_ivec3(&mut out, IVec3::ONE);
    for c in (pivot + Vec3::ZERO).to_array() {
        out.extend_from_slice(&c.to_le_bytes());
    }
    put_ivec3(&mut out, size);
    let mut raw = Vec::with_capacity((size.x * size.y * size.z * 4) as usize);
    for x in 0..size.x {
        for z in 0..size.z {
            for y in 0..size.y {
                let p = lo + IVec3::new(x, y, z);
                match volume.get(p) {
                    EMPTY_VOXEL => raw.extend_from_slice(&[0; 4]),
                    v => {
                        let c = color_of(palette, v);
                        raw.extend_from_slice(&[c.r, c.g, c.b, face_mask(volume, p) as u8]);
                    }
                }
            }
        }
    }
    let mut z = ZlibEncoder::new(Vec::new(), Compression::default());
    std::io::Write::write_all(&mut z, &raw)?;
    let packed = z.finish()?;
    put_u32(&mut out, packed.len() as u32);
    out.extend_from_slice(&packed);
    Ok(out)
}

/// Child nodes of `parent` (a group index, or the root), bottom to top.
fn children(
    scene: &ExportScene,
    parent: Option<usize>,
    depth: usize,
) -> Result<Vec<Vec<u8>>, RzxlError> {
    // (sort key, node): layers at their index, groups just below `order`.
    let mut items: Vec<(usize, Vec<u8>)> = Vec::new();
    for (i, l) in scene.layers.iter().enumerate() {
        if !l.visible || l.group != parent {
            continue;
        }
        if let Some((lo, hi)) = l.volume.occupied_bounds() {
            let pivot = (hi - lo).as_vec3() / 2.0;
            let body = matrix_body(&l.name, &l.volume, lo, hi, pivot, &scene.palette)?;
            items.push((2 * i + 1, node(NODE_MATRIX, body)));
        }
    }
    if depth < MAX_GROUP_DEPTH {
        for (g, group) in scene.groups.iter().enumerate() {
            if group.parent != parent || Some(g) == parent || !group.visible {
                continue;
            }
            let kids = children(scene, Some(g), depth + 1)?;
            if kids.is_empty() {
                continue;
            }
            // A nested model, not a compound: Qubicle keeps model children
            // where they are, but redraws a compound from its own matrix.
            let mut body = Vec::new();
            put_u32(&mut body, kids.len() as u32);
            kids.iter().for_each(|k| body.extend_from_slice(k));
            items.push((2 * group.order, node(NODE_MODEL, body)));
        }
    }
    items.sort_by_key(|(k, _)| *k);
    Ok(items.into_iter().map(|(_, n)| n).collect())
}

/// Encodes the visible layers and their groups as a `.qbt` file.
pub fn write_qbt(scene: &ExportScene) -> Result<Vec<u8>, RzxlError> {
    let kids = children(scene, None, 0)?;
    if kids.is_empty() {
        return Err(RzxlError::Invalid(
            "nothing to export: the visible layers contain no voxels".into(),
        ));
    }
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&[1, 0]);
    for _ in 0..3 {
        out.extend_from_slice(&1.0f32.to_le_bytes());
    }
    out.extend_from_slice(b"COLORMAP");
    put_u32(&mut out, 0);
    out.extend_from_slice(b"DATATREE");
    let mut root = Vec::new();
    put_u32(&mut root, kids.len() as u32);
    kids.iter().for_each(|k| root.extend_from_slice(k));
    out.extend_from_slice(&node(NODE_MODEL, root));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interchange::{ExportGroup, ExportLayer};
    use crate::volume_from_runs;
    use rizxel_math::{BoundSize, LayerKind};

    fn scene() -> ExportScene {
        let bounds = BoundSize::new(10, 8, 6);
        let mut a = VoxelVolume::new(bounds);
        for x in 0..4 {
            a.set(IVec3::new(x + 1, 0, 2), 9);
        }
        let mut b = VoxelVolume::new(bounds);
        b.set(IVec3::new(7, 3, 5), 40);
        b.set(IVec3::new(7, 4, 5), 41);
        let mut c = VoxelVolume::new(bounds);
        c.set(IVec3::new(2, 7, 0), 3);
        let mut layers = vec![
            ExportLayer::new("Base", true, a),
            ExportLayer::new("Arm", true, b),
            ExportLayer::new("Hat", true, c),
        ];
        layers[1].group = Some(0);
        layers[2].group = Some(0);
        ExportScene {
            name: "t".into(),
            bounds,
            palette: Palette::default(),
            layers,
            groups: vec![ExportGroup {
                name: "Upper".into(),
                visible: true,
                parent: None,
                order: 1,
            }],
        }
    }

    fn scene_of(p: &ProjectFile) -> ExportScene {
        let groups: Vec<usize> = p
            .layers
            .iter()
            .enumerate()
            .filter(|(_, l)| l.kind.is_group())
            .map(|(i, _)| i)
            .collect();
        let group_of =
            |id: Option<u32>| id.and_then(|id| groups.iter().position(|&g| p.layers[g].id == id));
        let mut layers = Vec::new();
        let mut out_groups = Vec::new();
        for l in &p.layers {
            if l.kind.is_group() {
                out_groups.push(ExportGroup {
                    name: l.name.clone(),
                    visible: true,
                    parent: group_of(l.parent),
                    order: layers.len(),
                });
                continue;
            }
            let mut e = ExportLayer::new(
                l.name.clone(),
                true,
                volume_from_runs(p.bounds, &l.voxels).unwrap(),
            );
            e.group = group_of(l.parent);
            layers.push(e);
        }
        ExportScene {
            name: p.name.clone(),
            bounds: p.bounds,
            palette: p.palette.clone(),
            layers,
            groups: out_groups,
        }
    }

    #[test]
    fn roundtrip_keeps_groups_and_rewrites_the_same_bytes() {
        let s = scene();
        let bytes = write_qbt(&s).unwrap();
        assert_eq!(&bytes[..4], b"QB 2");
        let p = read_qbt(&bytes, "t").unwrap();
        let names: Vec<&str> = p.layers.iter().map(|l| l.name.as_str()).collect();
        // The group comes back as a model, which has no name.
        assert_eq!(names, ["Base", "Model 1", "Arm", "Hat"]);
        assert!(matches!(p.layers[1].kind, LayerKind::Group { .. }));
        assert_eq!(p.layers[2].parent, Some(p.layers[1].id));
        assert_eq!(p.layers[0].parent, None);
        let arm = volume_from_runs(p.bounds, &p.layers[2].voxels).unwrap();
        assert_eq!(arm.voxel_count(), 2);
        assert_ne!(arm.get(IVec3::new(7, 3, 5)), EMPTY_VOXEL);
        assert_eq!(write_qbt(&scene_of(&p)).unwrap(), bytes);
    }

    fn matrix_node(name: &str, size: [u32; 3], raw: &[u8]) -> Vec<u8> {
        let mut body = Vec::new();
        put_u32(&mut body, name.len() as u32);
        body.extend_from_slice(name.as_bytes());
        put_ivec3(&mut body, IVec3::ZERO);
        put_ivec3(&mut body, IVec3::ONE);
        body.extend_from_slice(&[0; 12]);
        size.iter().for_each(|&s| put_u32(&mut body, s));
        let mut z = ZlibEncoder::new(Vec::new(), Compression::default());
        std::io::Write::write_all(&mut z, raw).unwrap();
        let packed = z.finish().unwrap();
        put_u32(&mut body, packed.len() as u32);
        body.extend_from_slice(&packed);
        node(NODE_MATRIX, body)
    }

    fn file(colors: &[[u8; 4]], root: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"QB 2");
        out.extend_from_slice(&[1, 0]);
        out.extend_from_slice(&[0; 12]);
        out.extend_from_slice(b"COLORMAP");
        put_u32(&mut out, colors.len() as u32);
        colors.iter().for_each(|c| out.extend_from_slice(c));
        out.extend_from_slice(b"DATATREE");
        out.extend_from_slice(root);
        out
    }

    #[test]
    fn reads_color_maps_and_voxel_order() {
        // 1×2×1 matrix: y = 1 solid, color map index 1.
        let raw = [0, 0, 0, 0, 1, 0, 0, 1];
        let bytes = file(
            &[[9, 9, 9, 255], [200, 10, 10, 255]],
            &matrix_node("m", [1, 2, 1], &raw),
        );
        let p = read_qbt(&bytes, "t").unwrap();
        let v = volume_from_runs(p.bounds, &p.layers[0].voxels).unwrap();
        assert_eq!(v.voxel_count(), 1);
        let value = v.get(IVec3::new(0, 1, 0));
        assert_eq!(value, 2, "color map order kept");
        assert_eq!(p.palette.get(2), Some(Rgba8::rgb(200, 10, 10)));
        // X is the slowest axis: 2×1×1 with the second voxel solid -> x = 1.
        let raw = [0, 0, 0, 0, 5, 5, 5, 1];
        let p = read_qbt(&file(&[], &matrix_node("m", [2, 1, 1], &raw)), "t").unwrap();
        let v = volume_from_runs(p.bounds, &p.layers[0].voxels).unwrap();
        assert_ne!(v.get(IVec3::new(1, 0, 0)), EMPTY_VOXEL);
    }

    #[test]
    fn childless_compounds_are_placed_at_position_minus_pivot() {
        let mut body = Vec::new();
        put_u32(&mut body, 1);
        body.push(b'c');
        put_ivec3(&mut body, IVec3::new(5, 0, 0));
        put_ivec3(&mut body, IVec3::ONE);
        for c in [2.0f32, 0.0, 0.0] {
            body.extend_from_slice(&c.to_le_bytes());
        }
        [1u32, 1, 1].iter().for_each(|&s| put_u32(&mut body, s));
        let mut z = ZlibEncoder::new(Vec::new(), Compression::default());
        std::io::Write::write_all(&mut z, &[9, 9, 9, 1]).unwrap();
        let packed = z.finish().unwrap();
        put_u32(&mut body, packed.len() as u32);
        body.extend_from_slice(&packed);
        put_u32(&mut body, 0);
        let p = read_qbt(&file(&[], &node(NODE_COMPOUND, body)), "t").unwrap();
        let layer = p.layers.iter().find(|l| !l.kind.is_group()).unwrap();
        let v = volume_from_runs(p.bounds, &layer.voxels).unwrap();
        assert_ne!(v.get(IVec3::new(3, 0, 0)), EMPTY_VOXEL);
    }

    #[test]
    fn hostile_inputs_are_refused() {
        let ok = matrix_node("m", [1, 1, 1], &[1, 2, 3, 1]);
        assert!(read_qbt(&file(&[], &ok), "t").is_ok());
        // Wrong decompressed size, matrix side over 256, bad zlib.
        assert!(read_qbt(&file(&[], &matrix_node("m", [2, 1, 1], &[1, 2, 3, 1])), "t").is_err());
        assert!(read_qbt(&file(&[], &matrix_node("m", [300, 1, 1], &[0; 1200])), "t").is_err());
        let mut bad_zlib = ok.clone();
        let n = bad_zlib.len();
        bad_zlib[n - 6..].fill(0xff);
        assert!(read_qbt(&file(&[], &bad_zlib), "t").is_err());
        // Color index past the map.
        assert!(read_qbt(
            &file(
                &[[1, 1, 1, 255]],
                &matrix_node("m", [1, 1, 1], &[5, 0, 0, 1])
            ),
            "t"
        )
        .is_err());
        // Huge name length, huge child count, deep nesting, truncation.
        let mut huge_name = ok.clone();
        huge_name[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(read_qbt(&file(&[], &huge_name), "t").is_err());
        let mut model = Vec::new();
        put_u32(&mut model, u32::MAX);
        assert!(read_qbt(&file(&[], &node(NODE_MODEL, model)), "t").is_err());
        let mut deep = ok.clone();
        for _ in 0..40 {
            let mut body = Vec::new();
            put_u32(&mut body, 1);
            body.extend_from_slice(&deep);
            deep = node(NODE_MODEL, body);
        }
        assert!(read_qbt(&file(&[], &deep), "t").is_err());
        let full = file(&[], &ok);
        assert!(read_qbt(&full[..full.len() - 3], "t").is_err());
        assert!(read_qbt(b"QB 2", "t").is_err());
        assert!(read_qbt(b"QB 1\x01\x00", "t").is_err());
        // A zlib bomb stops at the expected size.
        let mut z = ZlibEncoder::new(Vec::new(), Compression::best());
        std::io::Write::write_all(&mut z, &vec![0u8; 1 << 24]).unwrap();
        let bomb = z.finish().unwrap();
        let mut body = Vec::new();
        put_u32(&mut body, 0);
        put_ivec3(&mut body, IVec3::ZERO);
        put_ivec3(&mut body, IVec3::ONE);
        body.extend_from_slice(&[0; 12]);
        [1u32, 1, 1].iter().for_each(|&s| put_u32(&mut body, s));
        put_u32(&mut body, bomb.len() as u32);
        body.extend_from_slice(&bomb);
        assert!(read_qbt(&file(&[], &node(NODE_MATRIX, body)), "t").is_err());
        // Unknown nodes are skipped.
        let mut root = Vec::new();
        put_u32(&mut root, 2);
        root.extend_from_slice(&node(9, vec![1, 2, 3]));
        root.extend_from_slice(&ok);
        assert!(read_qbt(&file(&[], &node(NODE_MODEL, root)), "t").is_ok());
    }

    #[test]
    fn empty_scenes_are_refused() {
        let mut s = scene();
        s.layers.iter_mut().for_each(|l| l.visible = false);
        assert!(write_qbt(&s).is_err());
    }
}

//! glTF 2.0 export: binary `.glb` or `.gltf` + `.bin`.
//!
//! - **Scene**: one node per visible, non-empty layer, named after it.
//!   Groups with visible content become parent nodes. A linked clone whose
//!   geometry is its source moved by whole voxels (no crop, nothing clipped)
//!   becomes a node that reuses the source's mesh with a translation.
//! - **Meshes**: the whole-layer greedy quads (as in the OBJ export), four
//!   vertices and two triangles per quad, flat normals, counter-clockwise
//!   front faces.
//! - **Frame**: Y-up like Rizxel and glTF. One voxel is `scale` meters
//!   (default 0.1). The model is centered like the OBJ export: X and Z
//!   centered with integer halves, standing on Y = 0, so a glTF and an OBJ
//!   export of the same model overlap exactly.
//! - **Colors**: [`GltfColors::Vertex`] writes linear `COLOR_0` per vertex;
//!   [`GltfColors::Texture`] writes `TEXCOORD_0` into the embedded 256×1
//!   palette PNG (same layout and UVs as the OBJ export), sampled nearest.
//! - **Materials**: one per distinct layer material and opacity, as
//!   `pbrMetallicRoughness` (tint, metallic, roughness) plus
//!   `emissiveFactor`; layers below full opacity use `alphaMode: BLEND`.
//! - **Buffers**: one buffer, every buffer view 4-byte aligned, `POSITION`
//!   accessors with `min`/`max`, `u32` indices. Written by hand with
//!   `serde_json`.

use std::collections::HashMap;
use std::path::Path;

use glam::{IVec3, Vec3};
use rizxel_engine::greedy_quads;
use rizxel_math::{BoundSize, Material, Palette, Voxel, VoxelVolume};
use serde_json::{json, Value};

use crate::interchange::ExportScene;
use crate::RzxlError;

pub const GLB_EXTENSION: &str = "glb";
pub const GLTF_EXTENSION: &str = "gltf";
/// Default size of one voxel in meters.
pub const DEFAULT_GLTF_SCALE: f32 = 0.1;

const ARRAY_BUFFER: u32 = 34962;
const ELEMENT_ARRAY_BUFFER: u32 = 34963;
const FLOAT: u32 = 5126;
const UNSIGNED_INT: u32 = 5125;
const NEAREST: u32 = 9728;
const CLAMP_TO_EDGE: u32 = 33071;
const GLB_MAGIC: u32 = 0x4654_6C67; // "glTF"
const CHUNK_JSON: u32 = 0x4E4F_534A;
const CHUNK_BIN: u32 = 0x004E_4942;

/// How vertex colors are stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GltfColors {
    /// `COLOR_0` per vertex (no texture).
    #[default]
    Vertex,
    /// `TEXCOORD_0` into an embedded 256×1 palette PNG.
    Texture,
}

impl GltfColors {
    pub const ALL: [GltfColors; 2] = [GltfColors::Vertex, GltfColors::Texture];

    pub const fn label(self) -> &'static str {
        match self {
            GltfColors::Vertex => "Vertex colors",
            GltfColors::Texture => "Palette texture",
        }
    }
}

/// glTF export settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GltfOptions {
    /// Meters per voxel.
    pub scale: f32,
    pub colors: GltfColors,
    /// Model-space offset subtracted from voxel corners before scaling
    /// (see [`crate::mesh_export::Pivot`]).
    pub origin: Vec3,
}

impl GltfOptions {
    /// Default settings for `bounds`: 0.1 m per voxel, vertex colors,
    /// centered like the OBJ export.
    pub fn new(bounds: BoundSize) -> Self {
        Self {
            scale: DEFAULT_GLTF_SCALE,
            colors: GltfColors::Vertex,
            origin: crate::mesh_export::Pivot::default().origin(bounds),
        }
    }
}

/// A built glTF asset: the JSON document and its binary buffer.
#[derive(Debug, Clone, PartialEq)]
pub struct GltfExport {
    pub json: Value,
    pub bin: Vec<u8>,
    pub triangles: usize,
    /// Nodes holding a mesh (one per exported layer).
    pub mesh_nodes: usize,
}

impl GltfExport {
    /// Binary glTF: header, JSON chunk (space padded), BIN chunk (zero
    /// padded).
    pub fn to_glb(&self) -> Vec<u8> {
        let mut json = serde_json::to_vec(&self.json).unwrap_or_default();
        while json.len() % 4 != 0 {
            json.push(b' ');
        }
        let mut bin = self.bin.clone();
        while bin.len() % 4 != 0 {
            bin.push(0);
        }
        let total = 12 + 8 + json.len() + 8 + bin.len();
        let mut out = Vec::with_capacity(total);
        for word in [GLB_MAGIC, 2, total as u32, json.len() as u32, CHUNK_JSON] {
            out.extend_from_slice(&word.to_le_bytes());
        }
        out.extend_from_slice(&json);
        out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        out.extend_from_slice(&CHUNK_BIN.to_le_bytes());
        out.extend_from_slice(&bin);
        out
    }

    /// Text glTF whose buffer lives in the file `bin_uri` (relative).
    pub fn to_gltf(&self, bin_uri: &str) -> String {
        let mut json = self.json.clone();
        json["buffers"][0]["uri"] = Value::from(bin_uri);
        serde_json::to_string_pretty(&json).unwrap_or_default()
    }
}

/// Accumulates buffer views and accessors in one 4-byte aligned buffer.
#[derive(Default)]
struct Buffer {
    bytes: Vec<u8>,
    views: Vec<Value>,
    accessors: Vec<Value>,
}

impl Buffer {
    fn view(&mut self, data: &[u8], target: Option<u32>) -> usize {
        while self.bytes.len() % 4 != 0 {
            self.bytes.push(0);
        }
        let mut view = json!({
            "buffer": 0,
            "byteOffset": self.bytes.len(),
            "byteLength": data.len(),
        });
        if let Some(t) = target {
            view["target"] = Value::from(t);
        }
        self.bytes.extend_from_slice(data);
        self.views.push(view);
        self.views.len() - 1
    }

    /// Float accessor of `kind` (`VEC2`, `VEC3`) with `N` components.
    fn floats<const N: usize>(&mut self, data: &[[f32; N]], kind: &str, bounds: bool) -> usize {
        let bytes: Vec<u8> = data
            .iter()
            .flatten()
            .flat_map(|f| f.to_le_bytes())
            .collect();
        let view = self.view(&bytes, Some(ARRAY_BUFFER));
        let mut acc = json!({
            "bufferView": view,
            "componentType": FLOAT,
            "count": data.len(),
            "type": kind,
        });
        if bounds {
            let (mut lo, mut hi) = ([f32::MAX; N], [f32::MIN; N]);
            for v in data {
                for k in 0..N {
                    lo[k] = lo[k].min(v[k]);
                    hi[k] = hi[k].max(v[k]);
                }
            }
            acc["min"] = json!(lo.to_vec());
            acc["max"] = json!(hi.to_vec());
        }
        self.accessors.push(acc);
        self.accessors.len() - 1
    }

    fn indices(&mut self, data: &[u32]) -> usize {
        let bytes: Vec<u8> = data.iter().flat_map(|i| i.to_le_bytes()).collect();
        let view = self.view(&bytes, Some(ELEMENT_ARRAY_BUFFER));
        self.accessors.push(json!({
            "bufferView": view,
            "componentType": UNSIGNED_INT,
            "count": data.len(),
            "type": "SCALAR",
        }));
        self.accessors.len() - 1
    }
}

/// sRGB channel (0..255) -> linear 0..1, as glTF vertex colors are linear.
fn linear(c: u8) -> f32 {
    let c = c as f32 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Mesh attributes of one layer volume.
struct LayerMesh {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
}

fn layer_mesh(volume: &VoxelVolume, palette: &Palette, options: &GltfOptions) -> LayerMesh {
    let quads = greedy_quads(volume);
    let mut m = LayerMesh {
        positions: Vec::with_capacity(quads.len() * 4),
        normals: Vec::with_capacity(quads.len() * 4),
        colors: Vec::new(),
        uvs: Vec::new(),
        indices: Vec::with_capacity(quads.len() * 6),
    };
    let color_of = |v: Voxel| {
        let c = palette.get(v as usize).unwrap_or(rizxel_math::Rgba8::WHITE);
        [linear(c.r), linear(c.g), linear(c.b)]
    };
    for q in &quads {
        let base = m.positions.len() as u32;
        let mut n = [0.0; 3];
        n[q.axis] = if q.positive { 1.0 } else { -1.0 };
        for p in q.corners() {
            let p = (p.as_vec3() - options.origin) * options.scale;
            m.positions.push((p + Vec3::ZERO).to_array());
            m.normals.push(n);
            match options.colors {
                GltfColors::Vertex => m.colors.push(color_of(q.value)),
                GltfColors::Texture => m.uvs.push([(q.value as f32 - 0.5) / 256.0, 0.5]),
            }
        }
        let [a, b, c, d] = [base, base + 1, base + 2, base + 3];
        if q.abc_is_ccw() {
            m.indices.extend_from_slice(&[a, b, c, c, b, d]);
        } else {
            m.indices.extend_from_slice(&[c, b, a, d, b, c]);
        }
    }
    m
}

fn material_json(material: Option<&Material>, opacity: f32, textured: bool) -> Value {
    let base = Material::default();
    let m = material.unwrap_or(&base);
    let alpha = (m.tint[3] * opacity).clamp(0.0, 1.0);
    let mut pbr = json!({
        "baseColorFactor": [m.tint[0], m.tint[1], m.tint[2], alpha],
        "metallicFactor": m.metallic,
        "roughnessFactor": m.roughness,
    });
    if textured {
        pbr["baseColorTexture"] = json!({ "index": 0 });
    }
    let mut out = json!({
        "name": material.map_or("Default", |m| m.name.as_str()),
        "pbrMetallicRoughness": pbr,
    });
    if m.emission.iter().any(|&e| e > 0.0) {
        out["emissiveFactor"] = json!(m.emission);
    }
    if alpha < 1.0 {
        out["alphaMode"] = Value::from("BLEND");
    }
    out
}

/// Builds the glTF document and buffer for the visible layers of `scene`.
pub fn build_gltf(scene: &ExportScene, options: &GltfOptions) -> Result<GltfExport, RzxlError> {
    let mut options = *options;
    if !(options.scale.is_finite() && options.scale > 0.0) {
        options.scale = DEFAULT_GLTF_SCALE;
    }
    if !options.origin.is_finite() {
        options.origin = Vec3::ZERO;
    }
    let textured = options.colors == GltfColors::Texture;
    let mut buffer = Buffer::default();
    let mut meshes: Vec<Value> = Vec::new();
    let mut materials: Vec<(Option<Material>, u32, Value)> = Vec::new();
    // Export layer -> glTF mesh (None for empty geometry).
    let mut mesh_of: HashMap<usize, Option<usize>> = HashMap::new();
    let mut triangles = 0;

    let mut mesh_for = |i: usize, buffer: &mut Buffer| -> Option<usize> {
        if let Some(&m) = mesh_of.get(&i) {
            return m;
        }
        let layer = &scene.layers[i];
        let result = (!layer.volume.is_empty()).then(|| {
            let m = layer_mesh(&layer.volume, &scene.palette, &options);
            triangles += m.indices.len() / 3;
            let opacity = layer.opacity.clamp(0.0, 1.0);
            let key = (layer.material.clone(), opacity.to_bits());
            let material = match materials
                .iter()
                .position(|(k, o, _)| (k, o) == (&key.0, &key.1))
            {
                Some(index) => index,
                None => {
                    let json = material_json(layer.material.as_ref(), opacity, textured);
                    materials.push((key.0, key.1, json));
                    materials.len() - 1
                }
            };
            let mut attributes = json!({
                "POSITION": buffer.floats(&m.positions, "VEC3", true),
                "NORMAL": buffer.floats(&m.normals, "VEC3", false),
            });
            if textured {
                attributes["TEXCOORD_0"] = Value::from(buffer.floats(&m.uvs, "VEC2", false));
            } else {
                attributes["COLOR_0"] = Value::from(buffer.floats(&m.colors, "VEC3", false));
            }
            meshes.push(json!({
                "name": layer.name,
                "primitives": [{
                    "attributes": attributes,
                    "indices": buffer.indices(&m.indices),
                    "material": material,
                    "mode": 4,
                }],
            }));
            meshes.len() - 1
        });
        mesh_of.insert(i, result);
        result
    };

    // Mesh and translation of every exported layer.
    let mut placed: Vec<(usize, usize, IVec3)> = Vec::new();
    for (i, layer) in scene.layers.iter().enumerate() {
        if !layer.visible {
            continue;
        }
        let (source, offset) = match layer.instance_of {
            Some((s, offset)) if s < scene.layers.len() && s != i => (s, offset),
            _ => (i, IVec3::ZERO),
        };
        if let Some(mesh) = mesh_for(source, &mut buffer) {
            placed.push((i, mesh, offset));
        }
    }
    if placed.is_empty() {
        return Err(RzxlError::Invalid(
            "nothing to export: the visible layers contain no voxels".into(),
        ));
    }

    // Groups holding exported layers, with their ancestors.
    let mut used = vec![false; scene.groups.len()];
    for &(i, ..) in &placed {
        let mut g = scene.layers[i].group;
        let mut guard = 0;
        while let Some(index) = g.filter(|&g| g < used.len() && guard <= used.len()) {
            used[index] = true;
            g = scene.groups[index].parent;
            guard += 1;
        }
    }
    let mut nodes: Vec<Value> = Vec::new();
    let mut group_node: HashMap<usize, usize> = HashMap::new();
    for (g, group) in scene.groups.iter().enumerate().filter(|(g, _)| used[*g]) {
        group_node.insert(g, nodes.len());
        nodes.push(json!({ "name": group.name }));
    }
    let mut roots: Vec<usize> = Vec::new();
    let mut attach = |nodes: &mut Vec<Value>, parent: Option<usize>, child: usize| match parent
        .and_then(|p| group_node.get(&p))
    {
        Some(&p) if p != child => {
            let children = nodes[p]
                .as_object_mut()
                .map(|o| o.entry("children").or_insert_with(|| json!([])));
            if let Some(Value::Array(c)) = children {
                c.push(Value::from(child));
            }
        }
        _ => roots.push(child),
    };
    for (g, group) in scene.groups.iter().enumerate() {
        if let Some(&node) = group_node.get(&g) {
            attach(&mut nodes, group.parent, node);
        }
    }
    for &(i, mesh, offset) in &placed {
        let layer = &scene.layers[i];
        let mut node = json!({ "name": layer.name, "mesh": mesh });
        if offset != IVec3::ZERO {
            node["translation"] = json!((offset.as_vec3() * options.scale).to_array());
        }
        let index = nodes.len();
        nodes.push(node);
        attach(&mut nodes, layer.group, index);
    }

    let mut json = json!({
        "asset": {
            "version": "2.0",
            "generator": format!("Rizxel {} (MRizky099)", env!("CARGO_PKG_VERSION")),
        },
        "scene": 0,
        "scenes": [{ "name": scene.name, "nodes": roots }],
        "nodes": nodes,
        "meshes": meshes,
        "materials": materials.into_iter().map(|(.., m)| m).collect::<Vec<_>>(),
    });
    if textured {
        let png = crate::format_obj::palette_texture(&scene.palette)?;
        let view = buffer.view(&png, None);
        json["images"] =
            json!([{ "bufferView": view, "mimeType": "image/png", "name": "palette" }]);
        json["samplers"] = json!([{
            "magFilter": NEAREST,
            "minFilter": NEAREST,
            "wrapS": CLAMP_TO_EDGE,
            "wrapT": CLAMP_TO_EDGE,
        }]);
        json["textures"] = json!([{ "sampler": 0, "source": 0 }]);
    }
    while buffer.bytes.len() % 4 != 0 {
        buffer.bytes.push(0);
    }
    json["buffers"] = json!([{ "byteLength": buffer.bytes.len() }]);
    json["bufferViews"] = Value::Array(buffer.views);
    json["accessors"] = Value::Array(buffer.accessors);
    Ok(GltfExport {
        json,
        bin: buffer.bytes,
        triangles,
        mesh_nodes: placed.len(),
    })
}

/// Writes `path`: a `.glb`, or a `.gltf` with a `.bin` of the same stem.
pub fn write_gltf_file(
    path: &Path,
    scene: &ExportScene,
    options: &GltfOptions,
) -> Result<GltfExport, RzxlError> {
    let export = build_gltf(scene, options)?;
    let binary = path
        .extension()
        .is_none_or(|e| !e.eq_ignore_ascii_case(GLTF_EXTENSION));
    if binary {
        std::fs::write(path, export.to_glb())?;
    } else {
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "model".to_owned());
        let bin = format!("{stem}.bin");
        std::fs::write(path.with_file_name(&bin), &export.bin)?;
        std::fs::write(path, export.to_gltf(&bin))?;
    }
    Ok(export)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::interchange::{ExportGroup, ExportLayer};

    /// Triangles of every mesh node, in model space (node translations and
    /// parent nodes applied; Rizxel never writes other transforms).
    pub(crate) fn triangles_of(export: &GltfExport) -> Vec<[Vec3; 3]> {
        let j = &export.json;
        let read = |acc: usize| -> Vec<u8> {
            let a = &j["accessors"][acc];
            let v = &j["bufferViews"][a["bufferView"].as_u64().unwrap() as usize];
            let off = v["byteOffset"].as_u64().unwrap() as usize;
            let len = v["byteLength"].as_u64().unwrap() as usize;
            export.bin[off..off + len].to_vec()
        };
        let words = |b: Vec<u8>| -> Vec<[u8; 4]> {
            b.chunks_exact(4)
                .map(|c| [c[0], c[1], c[2], c[3]])
                .collect()
        };
        let nodes = j["nodes"].as_array().unwrap();
        let mut parent = vec![None; nodes.len()];
        for (i, n) in nodes.iter().enumerate() {
            for c in n["children"].as_array().into_iter().flatten() {
                parent[c.as_u64().unwrap() as usize] = Some(i);
            }
        }
        let mut out = Vec::new();
        for (i, n) in nodes.iter().enumerate() {
            let Some(mesh) = n["mesh"].as_u64() else {
                continue;
            };
            let mut t = Vec3::ZERO;
            let mut at = Some(i);
            while let Some(k) = at {
                if let Some(tr) = nodes[k]["translation"].as_array() {
                    t += Vec3::from_array([0, 1, 2].map(|c| tr[c].as_f64().unwrap() as f32));
                }
                at = parent[k];
            }
            let prim = &j["meshes"][mesh as usize]["primitives"][0];
            let pos: Vec<f32> = words(read(
                prim["attributes"]["POSITION"].as_u64().unwrap() as usize
            ))
            .into_iter()
            .map(f32::from_le_bytes)
            .collect();
            let idx: Vec<u32> = words(read(prim["indices"].as_u64().unwrap() as usize))
                .into_iter()
                .map(u32::from_le_bytes)
                .collect();
            let p = |k: u32| Vec3::from_slice(&pos[k as usize * 3..k as usize * 3 + 3]) + t;
            for tri in idx.chunks_exact(3) {
                out.push([p(tri[0]), p(tri[1]), p(tri[2])]);
            }
        }
        out
    }

    fn scene() -> ExportScene {
        let bounds = BoundSize::new(10, 6, 7);
        let mut body = VoxelVolume::new(bounds);
        for x in 0..4 {
            body.set(IVec3::new(x, 0, 1), 9);
            body.set(IVec3::new(x, 1, 1), 40);
        }
        let clone = body.translated(IVec3::new(5, 0, 2));
        let mut hat = VoxelVolume::new(bounds);
        hat.set(IVec3::new(1, 5, 1), 200);
        let mut layers = vec![
            ExportLayer::new("Body", true, body),
            ExportLayer::new("Clone", true, clone),
            ExportLayer::new("Hidden", false, hat),
            ExportLayer::new("Empty", true, VoxelVolume::new(bounds)),
        ];
        layers[1].instance_of = Some((0, IVec3::new(5, 0, 2)));
        layers[1].group = Some(0);
        layers[0].material = Some(Material {
            emission: [1.0, 0.5, 0.0],
            ..Material::new("Glow")
        });
        ExportScene {
            name: "t".into(),
            bounds,
            palette: Palette::default(),
            layers,
            groups: vec![ExportGroup {
                name: "Copies".into(),
                visible: true,
                parent: None,
                order: 1,
            }],
        }
    }

    fn u32_at(b: &[u8], o: usize) -> u32 {
        u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
    }

    #[test]
    fn glb_header_and_chunks_are_consistent() {
        let s = scene();
        let e = build_gltf(&s, &GltfOptions::new(s.bounds)).unwrap();
        let glb = e.to_glb();
        assert_eq!(&glb[..4], b"glTF");
        assert_eq!(u32_at(&glb, 4), 2);
        assert_eq!(u32_at(&glb, 8) as usize, glb.len());
        let json_len = u32_at(&glb, 12) as usize;
        assert_eq!(u32_at(&glb, 16), CHUNK_JSON);
        assert_eq!(json_len % 4, 0);
        let parsed: Value = serde_json::from_slice(&glb[20..20 + json_len]).unwrap();
        assert_eq!(parsed, e.json);
        let bin_at = 20 + json_len;
        let bin_len = u32_at(&glb, bin_at) as usize;
        assert_eq!(u32_at(&glb, bin_at + 4), CHUNK_BIN);
        assert_eq!(bin_len % 4, 0);
        assert_eq!(bin_at + 8 + bin_len, glb.len());
        assert_eq!(&glb[bin_at + 8..bin_at + 8 + e.bin.len()], &e.bin[..]);
        assert_eq!(e.json["buffers"][0]["byteLength"], e.bin.len());
    }

    #[test]
    fn buffer_views_are_aligned_and_positions_have_bounds() {
        let s = scene();
        for colors in GltfColors::ALL {
            let e = build_gltf(
                &s,
                &GltfOptions {
                    colors,
                    ..GltfOptions::new(s.bounds)
                },
            )
            .unwrap();
            for v in e.json["bufferViews"].as_array().unwrap() {
                let off = v["byteOffset"].as_u64().unwrap() as usize;
                let len = v["byteLength"].as_u64().unwrap() as usize;
                assert_eq!(off % 4, 0);
                assert!(off + len <= e.bin.len());
            }
            for m in e.json["meshes"].as_array().unwrap() {
                let attrs = &m["primitives"][0]["attributes"];
                let pos = &e.json["accessors"][attrs["POSITION"].as_u64().unwrap() as usize];
                assert_eq!(pos["min"].as_array().unwrap().len(), 3);
                assert_eq!(pos["max"].as_array().unwrap().len(), 3);
                assert_eq!(attrs.get("COLOR_0").is_some(), colors == GltfColors::Vertex);
                assert_eq!(
                    attrs.get("TEXCOORD_0").is_some(),
                    colors == GltfColors::Texture
                );
            }
            assert_eq!(
                e.json.get("images").is_some(),
                colors == GltfColors::Texture
            );
        }
    }

    #[test]
    fn nodes_follow_visible_layers_and_clones_share_a_mesh() {
        let s = scene();
        let e = build_gltf(&s, &GltfOptions::new(s.bounds)).unwrap();
        // Body and its clone; the hidden and the empty layer are skipped.
        assert_eq!(e.mesh_nodes, 2);
        assert_eq!(e.json["meshes"].as_array().unwrap().len(), 1);
        let nodes = e.json["nodes"].as_array().unwrap();
        let by_name = |n: &str| nodes.iter().find(|x| x["name"] == n).unwrap();
        assert_eq!(by_name("Body")["mesh"], by_name("Clone")["mesh"]);
        assert_eq!(
            by_name("Clone")["translation"],
            json!([0.5, 0.0, 0.2].map(|v: f64| v as f32))
        );
        // The clone sits in its group node.
        let group = by_name("Copies");
        let clone_index = nodes.iter().position(|x| x["name"] == "Clone").unwrap();
        assert_eq!(group["children"], json!([clone_index]));
        // The layer material and its emission come along.
        let mat = &e.json["materials"][0];
        assert_eq!(mat["name"], "Glow");
        assert_eq!(mat["emissiveFactor"], json!([1.0, 0.5, 0.0]));
    }

    #[test]
    fn surface_matches_the_obj_export() {
        let s = scene();
        let gltf = build_gltf(&s, &GltfOptions::new(s.bounds)).unwrap();
        let obj = crate::build_obj_export(
            &s.merged_visible(),
            s.bounds,
            &s.palette,
            "m",
            DEFAULT_GLTF_SCALE,
        )
        .unwrap();
        let obj_tris: Vec<[Vec3; 3]> = crate::read_obj(&obj.obj, &|_| None)
            .unwrap()
            .triangles
            .iter()
            .map(|t| t.p)
            .collect();
        let cells =
            |t: &[[Vec3; 3]]| crate::test_samples::surface_cells(t, 1.0 / DEFAULT_GLTF_SCALE);
        assert_eq!(cells(&triangles_of(&gltf)), cells(&obj_tris));
        // The clone reuses the body mesh, so the file stores half the triangles.
        assert_eq!(triangles_of(&gltf).len(), obj.triangles);
        assert_eq!(gltf.triangles * 2, obj.triangles);
    }

    #[test]
    fn triangles_face_outward() {
        let s = scene();
        let e = build_gltf(&s, &GltfOptions::new(s.bounds)).unwrap();
        let j = &e.json;
        let prim = &j["meshes"][0]["primitives"][0];
        let read = |acc: &Value| -> Vec<f32> {
            let a = &j["accessors"][acc.as_u64().unwrap() as usize];
            let v = &j["bufferViews"][a["bufferView"].as_u64().unwrap() as usize];
            let off = v["byteOffset"].as_u64().unwrap() as usize;
            let len = v["byteLength"].as_u64().unwrap() as usize;
            e.bin[off..off + len]
                .chunks_exact(4)
                .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect()
        };
        let normals = read(&prim["attributes"]["NORMAL"]);
        let pos = read(&prim["attributes"]["POSITION"]);
        let idx: Vec<u32> = read(&prim["indices"]).iter().map(|f| f.to_bits()).collect();
        let p = |k: u32| Vec3::from_slice(&pos[k as usize * 3..k as usize * 3 + 3]);
        for t in idx.chunks_exact(3) {
            let n = Vec3::from_slice(&normals[t[0] as usize * 3..t[0] as usize * 3 + 3]);
            let face = (p(t[1]) - p(t[0])).cross(p(t[2]) - p(t[0]));
            assert!(face.dot(n) > 0.0);
        }
    }

    #[test]
    fn empty_scenes_and_bad_options_are_handled() {
        let mut s = scene();
        for l in &mut s.layers {
            l.visible = false;
        }
        assert!(build_gltf(&s, &GltfOptions::new(s.bounds)).is_err());
        let s = scene();
        let bad = GltfOptions {
            scale: f32::NAN,
            origin: Vec3::splat(f32::INFINITY),
            ..GltfOptions::new(s.bounds)
        };
        let e = build_gltf(&s, &bad).unwrap();
        assert!(triangles_of(&e).iter().flatten().all(|p| p.is_finite()));
        // A clone pointing at itself or past the end is written as a plain layer.
        let mut s = scene();
        s.layers[1].instance_of = Some((99, IVec3::ONE));
        let e = build_gltf(&s, &GltfOptions::new(s.bounds)).unwrap();
        assert_eq!(e.json["meshes"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn gltf_text_names_its_bin() {
        let s = scene();
        let e = build_gltf(&s, &GltfOptions::new(s.bounds)).unwrap();
        let text = e.to_gltf("m.bin");
        let parsed: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed["buffers"][0]["uri"], "m.bin");
    }
}

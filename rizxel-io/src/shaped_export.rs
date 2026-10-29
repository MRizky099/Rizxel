//! Writers for meshes of the render-only voxel shapes (Phase 11b,
//! *Export Shaped Mesh*): any triangle mesh with per-vertex colors and
//! material slots, as OBJ + MTL (one material per color and layer
//! material, linear `Kd` like the Fast load layout), binary STL (Z up, no
//! color) and GLB (vertex colors, one primitive per layer material).

use std::collections::BTreeMap;
use std::fmt::Write as _;

use glam::Vec3;
use rizxel_engine::Mesh;
use rizxel_math::{Material, Rgba8};
use serde_json::{json, Value};

use crate::format_gltf::GltfExport;
use crate::mesh_export::srgb_to_linear;
use crate::RzxlError;

/// Placement of the exported mesh.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShapedExportOptions {
    /// Units per voxel.
    pub scale: f32,
    /// Voxel-space point placed at the origin.
    pub origin: Vec3,
}

/// Triangles above which the dialog warns.
pub const WARN_TRIANGLES: usize = 2_000_000;
/// Triangles above which the export is refused.
pub const MAX_TRIANGLES: usize = 20_000_000;

fn check(mesh: &Mesh) -> Result<(), RzxlError> {
    if mesh.indices.is_empty() {
        return Err(RzxlError::Invalid(
            "nothing to export: the visible layers contain no voxels".into(),
        ));
    }
    if mesh.triangle_count() > MAX_TRIANGLES {
        return Err(RzxlError::Invalid(format!(
            "{} triangles is more than the {MAX_TRIANGLES} an export may have",
            mesh.triangle_count()
        )));
    }
    if !mesh.is_valid() || mesh.colors.len() != mesh.positions.len() {
        return Err(RzxlError::Invalid("inconsistent mesh".into()));
    }
    Ok(())
}

fn place(p: Vec3, o: &ShapedExportOptions) -> Vec3 {
    let scale = if o.scale.is_finite() && o.scale > 0.0 {
        o.scale
    } else {
        1.0
    };
    (p - o.origin) * scale
}

/// The material of slot `slot` (0 = default, `n` = `materials[n - 1]`).
fn slot_material(materials: &[Material], slot: u8) -> Material {
    match slot {
        0 => Material::default(),
        n => materials.get(n as usize - 1).cloned().unwrap_or_default(),
    }
}

/// Triangles grouped by (color, material slot), in a stable order.
fn groups(mesh: &Mesh) -> BTreeMap<([u8; 4], u8), Vec<usize>> {
    let mut out: BTreeMap<([u8; 4], u8), Vec<usize>> = BTreeMap::new();
    for (t, tri) in mesh.indices.chunks_exact(3).enumerate() {
        let v = tri[0] as usize;
        let c = mesh.colors[v];
        let slot = mesh.materials.get(v).copied().unwrap_or(0);
        out.entry(([c.r, c.g, c.b, c.a], slot)).or_default().push(t);
    }
    out
}

/// OBJ and MTL text. `mtl_name` is the MTL file name the OBJ refers to.
pub fn shaped_obj(
    mesh: &Mesh,
    materials: &[Material],
    options: &ShapedExportOptions,
    mtl_name: &str,
) -> Result<(String, String), RzxlError> {
    check(mesh)?;
    let mut obj = String::new();
    let mut mtl = String::new();
    let _ = writeln!(
        obj,
        "# Exported by Rizxel {} (shaped mesh)",
        env!("CARGO_PKG_VERSION")
    );
    let _ = writeln!(obj, "{}", crate::format_obj::FAST_LOAD_MARK);
    let _ = writeln!(obj, "mtllib {mtl_name}");
    let _ = writeln!(mtl, "# Rizxel shaped mesh materials (linear Kd)");
    for p in &mesh.positions {
        let p = place(*p, options);
        let _ = writeln!(obj, "v {} {} {}", p.x, p.y, p.z);
    }
    for n in &mesh.normals {
        let _ = writeln!(obj, "vn {} {} {}", n.x, n.y, n.z);
    }
    for ((rgba, slot), tris) in groups(mesh) {
        let name = format!("c{:02x}{:02x}{:02x}_m{slot}", rgba[0], rgba[1], rgba[2]);
        let m = slot_material(materials, slot);
        let tint = |i: usize| srgb_to_linear(rgba[i]) * m.tint[i];
        let _ = writeln!(mtl, "\nnewmtl {name}");
        let _ = writeln!(mtl, "Kd {:.6} {:.6} {:.6}", tint(0), tint(1), tint(2));
        let e = m.emission.map(|v| v * m.emission_strength);
        if e.iter().any(|&v| v > 0.0) {
            let _ = writeln!(mtl, "Ke {:.6} {:.6} {:.6}", e[0], e[1], e[2]);
        }
        if m.tint[3] < 1.0 {
            let _ = writeln!(mtl, "d {:.6}", m.tint[3]);
        }
        let _ = writeln!(mtl, "Pr {:.6}\nPm {:.6}", m.roughness, m.metallic);
        let _ = writeln!(obj, "usemtl {name}");
        for t in tris {
            let tri = &mesh.indices[t * 3..t * 3 + 3];
            let [a, b, c] = [tri[0] + 1, tri[1] + 1, tri[2] + 1];
            let _ = writeln!(obj, "f {a}//{a} {b}//{b} {c}//{c}");
        }
    }
    Ok((obj, mtl))
}

/// Binary STL in the Z-up frame (Rizxel's +Z points to −Y), no colors.
pub fn shaped_stl(mesh: &Mesh, options: &ShapedExportOptions) -> Result<Vec<u8>, RzxlError> {
    check(mesh)?;
    let zup = |p: Vec3| Vec3::new(p.x, -p.z, p.y);
    let count = mesh.triangle_count();
    let mut out = Vec::with_capacity(84 + count * 50);
    let mut header = [0u8; 80];
    let title = b"Rizxel shaped mesh";
    header[..title.len()].copy_from_slice(title);
    out.extend_from_slice(&header);
    out.extend_from_slice(&(count as u32).to_le_bytes());
    for tri in mesh.indices.chunks_exact(3) {
        let [a, b, c] = [0, 1, 2].map(|i| zup(place(mesh.positions[tri[i] as usize], options)));
        let n = (b - a).cross(c - a).normalize_or_zero();
        for v in [n, a, b, c] {
            for f in v.to_array() {
                out.extend_from_slice(&f.to_le_bytes());
            }
        }
        out.extend_from_slice(&[0, 0]);
    }
    Ok(out)
}

/// GLB with linear vertex colors and one primitive per layer material.
pub fn shaped_glb(
    mesh: &Mesh,
    materials: &[Material],
    options: &ShapedExportOptions,
    name: &str,
) -> Result<Vec<u8>, RzxlError> {
    check(mesh)?;
    let mut bin: Vec<u8> = Vec::new();
    let mut views: Vec<Value> = Vec::new();
    let mut accessors: Vec<Value> = Vec::new();
    let mut push = |bin: &mut Vec<u8>, bytes: &[u8], target: u32| -> usize {
        while bin.len() % 4 != 0 {
            bin.push(0);
        }
        views.push(json!({
            "buffer": 0,
            "byteOffset": bin.len(),
            "byteLength": bytes.len(),
            "target": target,
        }));
        bin.extend_from_slice(bytes);
        views.len() - 1
    };
    let positions: Vec<Vec3> = mesh.positions.iter().map(|p| place(*p, options)).collect();
    let (lo, hi) = positions.iter().fold(
        (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
        |(lo, hi), p| (lo.min(*p), hi.max(*p)),
    );
    let flat = |v: &[Vec3]| -> Vec<u8> {
        v.iter()
            .flat_map(|p| p.to_array())
            .flat_map(f32::to_le_bytes)
            .collect()
    };
    let count = positions.len();
    let view = push(&mut bin, &flat(&positions), 34962);
    accessors.push(json!({
        "bufferView": view, "componentType": 5126, "count": count, "type": "VEC3",
        "min": lo.to_array(), "max": hi.to_array(),
    }));
    let view = push(&mut bin, &flat(&mesh.normals), 34962);
    accessors
        .push(json!({"bufferView": view, "componentType": 5126, "count": count, "type": "VEC3"}));
    let colors: Vec<u8> = mesh
        .colors
        .iter()
        .flat_map(|c: &Rgba8| [c.r, c.g, c.b, c.a].map(srgb_to_linear))
        .flat_map(f32::to_le_bytes)
        .collect();
    let view = push(&mut bin, &colors, 34962);
    accessors
        .push(json!({"bufferView": view, "componentType": 5126, "count": count, "type": "VEC4"}));

    // One primitive per material slot.
    let mut by_slot: BTreeMap<u8, Vec<u32>> = BTreeMap::new();
    for tri in mesh.indices.chunks_exact(3) {
        let slot = mesh.materials.get(tri[0] as usize).copied().unwrap_or(0);
        by_slot.entry(slot).or_default().extend_from_slice(tri);
    }
    let mut primitives = Vec::new();
    let mut gltf_materials = Vec::new();
    for (slot, indices) in by_slot {
        let bytes: Vec<u8> = indices.iter().flat_map(|i| i.to_le_bytes()).collect();
        let view = push(&mut bin, &bytes, 34963);
        accessors.push(json!({
            "bufferView": view, "componentType": 5125, "count": indices.len(), "type": "SCALAR",
        }));
        let m = slot_material(materials, slot);
        gltf_materials.push(json!({
            "name": m.name,
            "pbrMetallicRoughness": {
                "baseColorFactor": m.tint,
                "metallicFactor": m.metallic,
                "roughnessFactor": m.roughness,
            },
            "emissiveFactor": m.emission.map(|v| (v * m.emission_strength).min(1.0)),
            "alphaMode": if m.tint[3] < 1.0 { "BLEND" } else { "OPAQUE" },
        }));
        primitives.push(json!({
            "attributes": {"POSITION": 0, "NORMAL": 1, "COLOR_0": 2},
            "indices": accessors.len() - 1,
            "material": gltf_materials.len() - 1,
        }));
    }
    let doc = json!({
        "asset": {"version": "2.0", "generator": format!("Rizxel {}", env!("CARGO_PKG_VERSION"))},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"name": name, "mesh": 0}],
        "meshes": [{"name": name, "primitives": primitives}],
        "materials": gltf_materials,
        "buffers": [{"byteLength": bin.len()}],
        "bufferViews": views,
        "accessors": accessors,
    });
    Ok(GltfExport {
        json: doc,
        bin,
        triangles: mesh.triangle_count(),
        mesh_nodes: 1,
    }
    .to_glb())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quad() -> Mesh {
        Mesh {
            positions: vec![Vec3::ZERO, Vec3::X, Vec3::new(1.0, 0.0, 1.0), Vec3::Z],
            normals: vec![Vec3::Y; 4],
            colors: vec![Rgba8::rgb(255, 0, 0); 4],
            materials: vec![1; 4],
            ao: Vec::new(),
            indices: vec![0, 2, 1, 0, 3, 2],
        }
    }

    #[test]
    fn obj_groups_by_color_and_material() {
        let opts = ShapedExportOptions {
            scale: 2.0,
            origin: Vec3::X,
        };
        let m = Material {
            metallic: 0.7,
            ..Material::new("Steel")
        };
        let (obj, mtl) = shaped_obj(&quad(), &[m], &opts, "q.mtl").unwrap();
        assert!(obj.contains("mtllib q.mtl"));
        assert!(obj.contains("v -2 0 0"), "placed and scaled");
        assert_eq!(obj.lines().filter(|l| l.starts_with("f ")).count(), 2);
        assert!(obj.contains("usemtl cff0000_m1"));
        assert!(mtl.contains("Kd 1.000000 0.000000 0.000000"));
        assert!(mtl.contains("Pm 0.700000"));
        assert!(shaped_obj(&Mesh::default(), &[], &opts, "q.mtl").is_err());
    }

    #[test]
    fn stl_and_glb_hold_every_triangle() {
        let opts = ShapedExportOptions {
            scale: 1.0,
            origin: Vec3::ZERO,
        };
        let stl = shaped_stl(&quad(), &opts).unwrap();
        assert_eq!(stl.len(), 84 + 2 * 50);
        assert_eq!(u32::from_le_bytes([stl[80], stl[81], stl[82], stl[83]]), 2);
        let glb = shaped_glb(&quad(), &[], &opts, "q").unwrap();
        assert_eq!(&glb[..4], b"glTF");
        let json_len = u32::from_le_bytes([glb[12], glb[13], glb[14], glb[15]]) as usize;
        let json: Value = serde_json::from_slice(&glb[20..20 + json_len]).unwrap();
        assert_eq!(json["accessors"][3]["count"], 6);
        assert_eq!(json["meshes"][0]["primitives"].as_array().unwrap().len(), 1);
    }
}

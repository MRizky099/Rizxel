//! Native Rizxel project format (`.rzxl`).
//!
//! Binary layout (all integers little-endian):
//!
//! | Offset | Size | Field                              |
//! |--------|------|------------------------------------|
//! | 0      | 4    | Magic `RZXL`                       |
//! | 4      | 4    | `u32` format version               |
//! | 8      | 4    | `u32` uncompressed body length     |
//! | 12     | n    | zstd-compressed JSON [`ProjectFile`] |
//!
//! Version history:
//! - **1**: palette, layers (metadata only) and bounds.
//! - **2**: each layer carries its voxels as [`VoxelRun`]s.
//! - **3**: layer ids, opacity, groups (`parent`), linked clones, shape
//!   layers, crop boxes, per-layer materials and the project material list,
//!   plus a `generator` string.
//!
//! Compatibility rules:
//! - Every field added after version 1 has a serde default, so older bodies
//!   decode into the current structs; [`read_rzxl`] then migrates them (v1/v2
//!   layers get ids `1..=n`) and validates the result.
//! - Writers always emit the current version. Files newer than this build are
//!   rejected with [`RzxlError::UnsupportedVersion`] instead of being loaded
//!   and silently losing data on the next save.
//! - Every decoded project passes [`ProjectFile::validate`]: ids, references,
//!   group cycles, value ranges and size caps are checked, so the document
//!   layer never sees dangling links or hostile sizes.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;

use rizxel_math::{
    BoundSize, IVec3, LayerId, LayerKind, Material, Palette, Voxel, VoxelVolume, EMPTY_VOXEL,
};
use serde::{Deserialize, Serialize};

pub const RZXL_MAGIC: [u8; 4] = *b"RZXL";
pub const RZXL_VERSION: u32 = 3;
pub const RZXL_EXTENSION: &str = "rzxl";

/// Hard limit on the decompressed body to guard against corrupt/malicious files.
const MAX_BODY_LEN: u32 = 512 * 1024 * 1024;
const ZSTD_LEVEL: i32 = 3;

/// Most layers a project may hold.
pub const MAX_LAYERS: usize = 1024;
/// Most materials a project may hold.
pub const MAX_MATERIALS: usize = 256;
/// Deepest allowed group nesting (a top-level layer has depth 0).
pub const MAX_GROUP_DEPTH: usize = 32;
/// Longest layer or material name, in characters.
pub const MAX_NAME_CHARS: usize = 256;
/// Largest absolute coordinate accepted in clone offsets, shape areas and
/// crop boxes (well beyond `BoundSize::MAX`, far from `i32` overflow).
pub const MAX_COORD: i32 = 4 * BoundSize::MAX as i32;

#[derive(Debug, thiserror::Error)]
pub enum RzxlError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("not a Rizxel project file (bad magic)")]
    BadMagic,
    #[error("unsupported .rzxl version {found} (this build supports up to {RZXL_VERSION})")]
    UnsupportedVersion { found: u32 },
    #[error("declared body length {0} exceeds limit")]
    BodyTooLarge(u32),
    #[error("body length mismatch: header says {expected}, got {actual}")]
    LengthMismatch { expected: u32, actual: usize },
    #[error("invalid project data: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid project: {0}")]
    Invalid(String),
}

/// Run of `len` identical voxels starting at linear index `start` inside the
/// project bounds (see [`linear_index`] for the order).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoxelRun {
    pub start: u32,
    pub len: u32,
    pub value: Voxel,
}

fn default_opacity() -> f32 {
    1.0
}

/// Per-layer data persisted in the project. Layers are listed bottom to top
/// (later layers win where visible layers overlap).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayerData {
    /// Unique non-zero id (version 3+; older files are migrated).
    #[serde(default)]
    pub id: LayerId,
    pub name: String,
    pub visible: bool,
    pub locked: bool,
    /// `0..=1`, multiplies the layer's alpha when rendering.
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    /// Id of the group layer containing this layer (`None` = top level).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<LayerId>,
    /// Index into [`ProjectFile::materials`] (`None` = default material).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<u32>,
    /// Non-destructive crop: only voxels inside this box are shown and
    /// exported. The stored voxels are kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crop: Option<rizxel_math::VoxelBox>,
    /// Voxels, group, linked clone or shape (version 3+; default voxels).
    #[serde(default)]
    pub kind: LayerKind,
    /// Voxels as sorted, non-overlapping runs (version 2+). Only voxel layers
    /// may carry runs.
    #[serde(default)]
    pub voxels: Vec<VoxelRun>,
}

impl LayerData {
    /// Plain voxel layer with default properties.
    pub fn new(id: LayerId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            visible: true,
            locked: false,
            opacity: 1.0,
            parent: None,
            material: None,
            crop: None,
            kind: LayerKind::Voxels,
            voxels: Vec::new(),
        }
    }
}

/// Serializable project document (JSON body of a `.rzxl` file).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectFile {
    /// Application that wrote the file, e.g. `Rizxel 0.1.0` (diagnostics only).
    #[serde(default)]
    pub generator: String,
    pub name: String,
    pub palette: Palette,
    pub selected_color: usize,
    /// Materials referenced by [`LayerData::material`] (version 3+).
    #[serde(default)]
    pub materials: Vec<Material>,
    pub layers: Vec<LayerData>,
    pub active_layer: usize,
    pub bounds: BoundSize,
}

/// `generator` value written by this build.
pub fn generator() -> String {
    format!("Rizxel {}", env!("CARGO_PKG_VERSION"))
}

impl ProjectFile {
    /// Upgrades a body decoded from an older `version` in place.
    pub fn migrate(&mut self, version: u32) {
        if version < 3 {
            // v1/v2 have no ids, kinds or materials: number the layers.
            for (i, layer) in self.layers.iter_mut().enumerate() {
                layer.id = i as LayerId + 1;
            }
        }
    }

    /// Checks structural rules that serde cannot express. Voxel runs are
    /// checked separately by [`volume_from_runs`] when they are decoded.
    pub fn validate(&self) -> Result<(), RzxlError> {
        let fail = |msg: String| Err(RzxlError::Invalid(msg));
        if self.layers.len() > MAX_LAYERS {
            return fail(format!("{} layers (limit {MAX_LAYERS})", self.layers.len()));
        }
        if self.materials.len() > MAX_MATERIALS {
            return fail(format!(
                "{} materials (limit {MAX_MATERIALS})",
                self.materials.len()
            ));
        }
        for (i, m) in self.materials.iter().enumerate() {
            if m.name.chars().count() > MAX_NAME_CHARS {
                return fail(format!("material {i} name is too long"));
            }
            if !m.is_valid() {
                return fail(format!(
                    "material {i} ({:?}) has values outside 0..=1",
                    m.name
                ));
            }
        }

        let coord_ok = |v: IVec3| v.abs().cmple(IVec3::splat(MAX_COORD)).all();
        let mut kinds: HashMap<LayerId, &LayerKind> = HashMap::new();
        for (i, l) in self.layers.iter().enumerate() {
            let at = format!("layer {i} ({:?})", l.name);
            if l.id == 0 {
                return fail(format!("{at} has id 0"));
            }
            if kinds.insert(l.id, &l.kind).is_some() {
                return fail(format!("{at} reuses id {}", l.id));
            }
            if l.name.chars().count() > MAX_NAME_CHARS {
                return fail(format!("{at} name is too long"));
            }
            if !(l.opacity.is_finite() && (0.0..=1.0).contains(&l.opacity)) {
                return fail(format!("{at} opacity {} is outside 0..=1", l.opacity));
            }
            if let Some(m) = l.material {
                if m as usize >= self.materials.len() {
                    return fail(format!(
                        "{at} uses material {m}, but only {} exist",
                        self.materials.len()
                    ));
                }
            }
            if let Some(c) = &l.crop {
                if !c.is_valid() || !coord_ok(c.min) || !coord_ok(c.max) {
                    return fail(format!("{at} has an empty or out-of-range crop box"));
                }
            }
            if !l.kind.stores_voxels() && !l.voxels.is_empty() {
                return fail(format!("{at} is not a voxel layer but stores voxels"));
            }
            match l.kind {
                LayerKind::Shape { area, color, .. } => {
                    if !area.is_valid() || !coord_ok(area.min) || !coord_ok(area.max) {
                        return fail(format!("{at} has an empty or out-of-range shape area"));
                    }
                    if color == EMPTY_VOXEL || color as usize >= self.palette.len() {
                        return fail(format!("{at} shape color {color} is not a palette color"));
                    }
                }
                LayerKind::Clone { offset, .. } if !coord_ok(offset) => {
                    return fail(format!("{at} clone offset is out of range"));
                }
                _ => {}
            }
        }

        let parents: HashMap<LayerId, Option<LayerId>> =
            self.layers.iter().map(|l| (l.id, l.parent)).collect();
        for (i, l) in self.layers.iter().enumerate() {
            let at = format!("layer {i} ({:?})", l.name);
            if let LayerKind::Clone { source, .. } = l.kind {
                match kinds.get(&source) {
                    _ if source == l.id => return fail(format!("{at} clones itself")),
                    None => return fail(format!("{at} clones missing layer {source}")),
                    Some(k) if !k.is_clone_source() => {
                        return fail(format!(
                            "{at} clones layer {source}, which is not a voxel or shape layer"
                        ))
                    }
                    Some(_) => {}
                }
            }
            // Walk up the group chain: every parent must be a group, and the
            // chain must end (no cycles) within the depth limit.
            let mut seen = HashSet::from([l.id]);
            let mut parent = l.parent;
            let mut depth = 0;
            while let Some(p) = parent {
                match kinds.get(&p) {
                    None => return fail(format!("{at} has missing parent {p}")),
                    Some(k) if !k.is_group() => {
                        return fail(format!("{at} has parent {p}, which is not a group"))
                    }
                    Some(_) => {}
                }
                if !seen.insert(p) {
                    return fail(format!("{at} is inside a group cycle"));
                }
                depth += 1;
                if depth > MAX_GROUP_DEPTH {
                    return fail(format!("{at} is nested deeper than {MAX_GROUP_DEPTH}"));
                }
                parent = parents.get(&p).copied().flatten();
            }
        }
        Ok(())
    }
}

/// Linear index of `v` inside `bounds`, in Z-X-Y order: `y` varies fastest
/// (vertical columns give long runs), then `x`, then `z`.
pub fn linear_index(bounds: BoundSize, v: IVec3) -> u64 {
    let (sx, sy) = (u64::from(bounds.x()), u64::from(bounds.y()));
    (v.z as u64 * sx + v.x as u64) * sy + v.y as u64
}

/// Inverse of [`linear_index`].
fn position_of(bounds: BoundSize, index: u64) -> IVec3 {
    let (sx, sy) = (u64::from(bounds.x()), u64::from(bounds.y()));
    let y = index % sy;
    let x = (index / sy) % sx;
    let z = index / (sy * sx);
    IVec3::new(x as i32, y as i32, z as i32)
}

/// Encodes a volume as sorted runs. Cost is O(n log n) in the voxel count.
pub fn runs_from_volume(bounds: BoundSize, volume: &VoxelVolume) -> Vec<VoxelRun> {
    let mut cells: Vec<(u64, Voxel)> = volume
        .iter()
        .filter(|(p, _)| p.cmpge(IVec3::ZERO).all() && p.as_uvec3().cmplt(bounds.0).all())
        .map(|(p, v)| (linear_index(bounds, p), v))
        .collect();
    cells.sort_unstable_by_key(|&(i, _)| i);

    let mut runs: Vec<VoxelRun> = Vec::new();
    for (index, value) in cells {
        // Bounds are at most 256³ = 2^24 cells, so every index fits in u32.
        let index = index as u32;
        match runs.last_mut() {
            Some(r) if r.value == value && r.start + r.len == index => r.len += 1,
            _ => runs.push(VoxelRun {
                start: index,
                len: 1,
                value,
            }),
        }
    }
    runs
}

fn invalid(msg: String) -> RzxlError {
    RzxlError::Invalid(msg)
}

/// Decodes runs into a volume, validating them against `bounds`: runs must be
/// sorted, non-overlapping, inside the bounds and use values `1..=255`.
pub fn volume_from_runs(bounds: BoundSize, runs: &[VoxelRun]) -> Result<VoxelVolume, RzxlError> {
    let total = bounds.volume();
    let mut volume = VoxelVolume::new(bounds);
    let mut next_free = 0u64;
    for (i, run) in runs.iter().enumerate() {
        let start = u64::from(run.start);
        let end = start + u64::from(run.len);
        if run.value == EMPTY_VOXEL {
            return Err(invalid(format!("voxel run {i} uses the empty value 0")));
        }
        if end > total {
            return Err(invalid(format!(
                "voxel run {i} ({start}..{end}) exceeds the bounds volume {total}"
            )));
        }
        if start < next_free {
            return Err(invalid(format!(
                "voxel run {i} starts at {start}, overlapping or out of order (expected >= {next_free})"
            )));
        }
        for index in start..end {
            volume.set(position_of(bounds, index), run.value);
        }
        next_free = end;
    }
    Ok(volume)
}

/// Encodes a project into `.rzxl` bytes. Invalid projects are refused so a
/// save can never produce a file that fails to load.
pub fn write_rzxl<W: Write>(mut w: W, project: &ProjectFile) -> Result<(), RzxlError> {
    project.validate()?;
    let json = serde_json::to_vec(project)?;
    let len = u32::try_from(json.len()).map_err(|_| RzxlError::BodyTooLarge(u32::MAX))?;
    let compressed = zstd::encode_all(json.as_slice(), ZSTD_LEVEL)?;

    w.write_all(&RZXL_MAGIC)?;
    w.write_all(&RZXL_VERSION.to_le_bytes())?;
    w.write_all(&len.to_le_bytes())?;
    w.write_all(&compressed)?;
    w.flush()?;
    Ok(())
}

/// Decodes a project from `.rzxl` bytes, validating the header.
pub fn read_rzxl<R: Read>(mut r: R) -> Result<ProjectFile, RzxlError> {
    let mut header = [0u8; 12];
    r.read_exact(&mut header).map_err(|e| match e.kind() {
        std::io::ErrorKind::UnexpectedEof => RzxlError::BadMagic,
        _ => RzxlError::Io(e),
    })?;

    if header[0..4] != RZXL_MAGIC {
        return Err(RzxlError::BadMagic);
    }
    let word =
        |i: usize| u32::from_le_bytes([header[i], header[i + 1], header[i + 2], header[i + 3]]);
    let version = word(4);
    if version == 0 || version > RZXL_VERSION {
        return Err(RzxlError::UnsupportedVersion { found: version });
    }
    let expected = word(8);
    if expected > MAX_BODY_LEN {
        return Err(RzxlError::BodyTooLarge(expected));
    }

    // Bound the decoder output to the declared length (+1 to detect overflow).
    let mut json = Vec::with_capacity(expected as usize);
    zstd::Decoder::new(r)?
        .take(u64::from(expected) + 1)
        .read_to_end(&mut json)?;

    if json.len() != expected as usize {
        return Err(RzxlError::LengthMismatch {
            expected,
            actual: json.len(),
        });
    }

    let mut project: ProjectFile = serde_json::from_slice(&json)?;
    project.migrate(version);
    project.validate()?;
    Ok(project)
}

/// Saves a project to disk atomically (write temp file, then rename).
pub fn save_rzxl(path: &Path, project: &ProjectFile) -> Result<(), RzxlError> {
    let tmp = path.with_extension("rzxl.tmp");
    {
        let file = File::create(&tmp)?;
        write_rzxl(BufWriter::new(file), project)?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Loads a project from disk.
pub fn load_rzxl(path: &Path) -> Result<ProjectFile, RzxlError> {
    read_rzxl(BufReader::new(File::open(path)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    use rizxel_math::{ShapeKind, VoxelBox};

    fn layer(
        id: LayerId,
        name: &str,
        visible: bool,
        locked: bool,
        voxels: Vec<VoxelRun>,
    ) -> LayerData {
        LayerData {
            visible,
            locked,
            voxels,
            ..LayerData::new(id, name)
        }
    }

    fn sparse_volume(bounds: BoundSize) -> VoxelVolume {
        let mut v = VoxelVolume::new(bounds);
        for (p, c) in [
            (IVec3::new(0, 0, 0), 1),
            (IVec3::new(0, 1, 0), 1),
            (IVec3::new(0, 2, 0), 2),
            (IVec3::new(15, 31, 63), 7),
            (IVec3::new(8, 16, 17), 200),
        ] {
            v.set(p, c);
        }
        v
    }

    fn sample() -> ProjectFile {
        let bounds = BoundSize::new(16, 32, 64);
        ProjectFile {
            generator: generator(),
            name: "hero_character".into(),
            palette: Palette::default(),
            selected_color: Palette::DEFAULT_SELECTED,
            materials: Vec::new(),
            layers: vec![
                layer(1, "Head", true, false, Vec::new()),
                layer(
                    2,
                    "Left Arm",
                    false,
                    true,
                    runs_from_volume(bounds, &sparse_volume(bounds)),
                ),
            ],
            active_layer: 1,
            bounds: BoundSize::new(16, 32, 64),
        }
    }

    /// Project using every version 3 layer feature.
    fn rich_sample() -> ProjectFile {
        let mut p = sample();
        p.materials = vec![
            Material::new("Matte"),
            Material {
                emission: [1.0, 0.5, 0.0],
                tint: [1.0, 1.0, 1.0, 0.5],
                ..Material::new("Glow")
            },
        ];
        p.layers.extend([
            LayerData {
                kind: LayerKind::Group { collapsed: true },
                ..LayerData::new(10, "Arms")
            },
            LayerData {
                kind: LayerKind::Clone {
                    source: 2,
                    offset: IVec3::new(-4, 0, 2),
                },
                parent: Some(10),
                opacity: 0.25,
                material: Some(1),
                ..LayerData::new(11, "Right Arm (linked)")
            },
            LayerData {
                kind: LayerKind::Shape {
                    shape: ShapeKind::Sphere,
                    area: VoxelBox::from_corners(IVec3::new(2, 2, 2), IVec3::new(9, 9, 9)),
                    color: 12,
                },
                crop: Some(VoxelBox::from_corners(IVec3::ZERO, IVec3::new(15, 5, 15))),
                material: Some(0),
                ..LayerData::new(12, "Ball")
            },
        ]);
        p.layers[1].parent = Some(10);
        p
    }

    #[test]
    fn version_3_features_roundtrip() {
        let mut buf = Vec::new();
        write_rzxl(&mut buf, &rich_sample()).unwrap();
        assert_eq!(&buf[4..8], &3u32.to_le_bytes());
        let back = read_rzxl(buf.as_slice()).unwrap();
        assert_eq!(back, rich_sample());
        assert!(back.generator.starts_with("Rizxel "));
    }

    #[test]
    fn version_2_body_is_migrated() {
        // A v2 body has runs but no ids, kinds, opacity or materials.
        let palette = serde_json::to_string(&Palette::default()).unwrap();
        let json = format!(
            r#"{{"name":"v2","palette":{palette},"selected_color":3,
                "layers":[{{"name":"A","visible":true,"locked":false,
                            "voxels":[{{"start":0,"len":3,"value":5}}]}},
                          {{"name":"B","visible":false,"locked":true}}],
                "active_layer":1,"bounds":[4,4,4]}}"#
        );
        let p = read_rzxl(raw_file(2, &json).as_slice()).unwrap();
        assert_eq!(p.layers.iter().map(|l| l.id).collect::<Vec<_>>(), [1, 2]);
        assert!(p.layers.iter().all(|l| l.opacity == 1.0
            && l.kind == LayerKind::Voxels
            && l.parent.is_none()
            && l.material.is_none()
            && l.crop.is_none()));
        assert!(p.materials.is_empty());
        assert_eq!(p.layers[0].voxels.len(), 1);
    }

    #[test]
    fn invalid_layer_structures_are_rejected() {
        type Breaker = fn(&mut ProjectFile);
        let cases: [(&str, Breaker); 13] = [
            ("id 0", |p| p.layers[0].id = 0),
            ("reuses id", |p| p.layers[1].id = 1),
            ("opacity", |p| p.layers[0].opacity = 1.5),
            ("opacity", |p| p.layers[0].opacity = f32::NAN),
            ("material 7", |p| p.layers[0].material = Some(7)),
            ("missing parent", |p| p.layers[0].parent = Some(99)),
            ("not a group", |p| p.layers[0].parent = Some(2)),
            ("clones missing", |p| {
                p.layers[3].kind = LayerKind::Clone {
                    source: 77,
                    offset: IVec3::ZERO,
                }
            }),
            ("not a voxel or shape", |p| {
                p.layers[3].kind = LayerKind::Clone {
                    source: 10,
                    offset: IVec3::ZERO,
                }
            }),
            ("clones itself", |p| {
                p.layers[3].kind = LayerKind::Clone {
                    source: 11,
                    offset: IVec3::ZERO,
                }
            }),
            ("stores voxels", |p| {
                p.layers[2].voxels = vec![VoxelRun {
                    start: 0,
                    len: 1,
                    value: 1,
                }]
            }),
            ("shape color 0", |p| {
                if let LayerKind::Shape { color, .. } = &mut p.layers[4].kind {
                    *color = 0;
                }
            }),
            ("crop box", |p| {
                p.layers[0].crop = Some(VoxelBox {
                    min: IVec3::ONE,
                    max: IVec3::ONE,
                })
            }),
        ];
        assert!(rich_sample().validate().is_ok());
        for (expect, breaker) in cases {
            let mut p = rich_sample();
            breaker(&mut p);
            match p.validate() {
                Err(RzxlError::Invalid(msg)) => {
                    assert!(msg.contains(expect), "{expect:?} not in {msg:?}")
                }
                other => panic!("{expect}: expected Invalid, got {other:?}"),
            }
            // Writers refuse the same projects.
            assert!(write_rzxl(Vec::new(), &p).is_err(), "{expect}");
        }
    }

    #[test]
    fn group_cycles_and_depth_are_rejected() {
        let mut p = sample();
        p.layers = (1..=3)
            .map(|id| LayerData {
                kind: LayerKind::Group { collapsed: false },
                parent: Some(id % 3 + 1),
                ..LayerData::new(id, format!("G{id}"))
            })
            .collect();
        let err = p.validate().unwrap_err().to_string();
        assert!(err.contains("cycle"), "{err}");

        let depth = MAX_GROUP_DEPTH as LayerId + 2;
        p.layers = (1..=depth)
            .map(|id| LayerData {
                kind: LayerKind::Group { collapsed: false },
                parent: (id > 1).then_some(id - 1),
                ..LayerData::new(id, format!("G{id}"))
            })
            .collect();
        let err = p.validate().unwrap_err().to_string();
        assert!(err.contains("deeper"), "{err}");
    }

    #[test]
    fn size_caps_are_enforced() {
        let mut p = sample();
        p.layers = (1..=MAX_LAYERS as LayerId + 1)
            .map(|id| LayerData::new(id, "L"))
            .collect();
        assert!(p.validate().unwrap_err().to_string().contains("layers"));
        let mut p = sample();
        p.layers[0].name = "x".repeat(MAX_NAME_CHARS + 1);
        assert!(p.validate().unwrap_err().to_string().contains("too long"));
    }

    #[test]
    fn roundtrip_in_memory() {
        let mut buf = Vec::new();
        write_rzxl(&mut buf, &sample()).unwrap();
        assert_eq!(&buf[0..4], b"RZXL");
        assert_eq!(read_rzxl(buf.as_slice()).unwrap(), sample());
    }

    #[test]
    fn roundtrip_on_disk() {
        let path = std::env::temp_dir().join(format!("rizxel_test_{}.rzxl", std::process::id()));
        save_rzxl(&path, &sample()).unwrap();
        let loaded = load_rzxl(&path).unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(loaded, sample());
    }

    #[test]
    fn voxels_roundtrip_through_disk() {
        let bounds = BoundSize::new(16, 32, 64);
        let volume = sparse_volume(bounds);
        let runs = runs_from_volume(bounds, &volume);
        // (0, 0..2, 0) holds 1, 1 then 2: two runs, plus two single voxels.
        assert_eq!(runs.len(), 4);
        assert_eq!(
            runs[0],
            VoxelRun {
                start: 0,
                len: 2,
                value: 1
            }
        );

        let path = std::env::temp_dir().join(format!("rizxel_vox_{}.rzxl", std::process::id()));
        save_rzxl(&path, &sample()).unwrap();
        let loaded = load_rzxl(&path).unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(loaded, sample());
        let back = volume_from_runs(loaded.bounds, &loaded.layers[1].voxels).unwrap();
        assert_eq!(back, volume);
        assert!(volume_from_runs(bounds, &loaded.layers[0].voxels)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn linear_index_roundtrip() {
        let b = BoundSize::new(3, 5, 7);
        for i in 0..b.volume() {
            assert_eq!(linear_index(b, position_of(b, i)), i);
        }
        assert_eq!(linear_index(b, IVec3::new(0, 1, 0)), 1, "y is fastest");
    }

    /// Builds a file with an explicit version, bypassing [`write_rzxl`].
    fn raw_file(version: u32, json: &str) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&RZXL_MAGIC);
        buf.extend_from_slice(&version.to_le_bytes());
        buf.extend_from_slice(&(json.len() as u32).to_le_bytes());
        buf.extend_from_slice(&zstd::encode_all(json.as_bytes(), ZSTD_LEVEL).unwrap());
        buf
    }

    #[test]
    fn version_1_body_loads_with_empty_layers() {
        let palette = serde_json::to_string(&Palette::default()).unwrap();
        let json = format!(
            r#"{{"name":"old","palette":{palette},"selected_color":3,
                "layers":[{{"name":"Layer 1","visible":true,"locked":false}}],
                "active_layer":0,"bounds":[8,8,8]}}"#
        );
        let p = read_rzxl(raw_file(1, &json).as_slice()).unwrap();
        assert_eq!(p.name, "old");
        assert_eq!(p.bounds, BoundSize::new(8, 8, 8));
        assert_eq!(p.layers.len(), 1);
        assert!(p.layers[0].voxels.is_empty());
    }

    #[test]
    fn newer_version_is_rejected() {
        let mut buf = Vec::new();
        write_rzxl(&mut buf, &sample()).unwrap();
        assert_eq!(&buf[4..8], &RZXL_VERSION.to_le_bytes());
        buf[4..8].copy_from_slice(&(RZXL_VERSION + 1).to_le_bytes());
        assert!(matches!(
            read_rzxl(buf.as_slice()).unwrap_err(),
            RzxlError::UnsupportedVersion { found } if found == RZXL_VERSION + 1
        ));
    }

    #[test]
    fn malformed_runs_are_rejected() {
        let b = BoundSize::new(2, 2, 2);
        let run = |start, len, value| VoxelRun { start, len, value };
        let err = |runs: &[VoxelRun]| match volume_from_runs(b, runs) {
            Err(RzxlError::Invalid(msg)) => msg,
            other => panic!("expected an Invalid error, got {other:?}"),
        };
        assert!(err(&[run(6, 3, 1)]).contains("exceeds the bounds"));
        assert!(err(&[run(0, 4, 1), run(3, 1, 2)]).contains("overlapping"));
        assert!(err(&[run(4, 1, 1), run(0, 1, 2)]).contains("out of order"));
        assert!(err(&[run(0, 1, 0)]).contains("empty value"));
        let ok = volume_from_runs(b, &[run(0, 4, 1), run(4, 4, 2)]).unwrap();
        assert_eq!(ok.voxel_count(), 8);
    }

    #[test]
    fn rejects_bad_magic() {
        let err = read_rzxl(&b"VOX \x01\x00\x00\x00\x00\x00\x00\x00"[..]).unwrap_err();
        assert!(matches!(err, RzxlError::BadMagic));
        assert!(matches!(
            read_rzxl(&b"RZ"[..]).unwrap_err(),
            RzxlError::BadMagic
        ));
    }

    #[test]
    fn rejects_future_version() {
        let mut buf = Vec::new();
        write_rzxl(&mut buf, &sample()).unwrap();
        buf[4..8].copy_from_slice(&99u32.to_le_bytes());
        assert!(matches!(
            read_rzxl(buf.as_slice()).unwrap_err(),
            RzxlError::UnsupportedVersion { found: 99 }
        ));
    }

    #[test]
    fn rejects_length_mismatch() {
        let mut buf = Vec::new();
        write_rzxl(&mut buf, &sample()).unwrap();
        buf[8..12].copy_from_slice(&5u32.to_le_bytes());
        assert!(matches!(
            read_rzxl(buf.as_slice()).unwrap_err(),
            RzxlError::LengthMismatch { expected: 5, .. }
        ));
    }
}

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
//! - **2**: each layer carries its voxels as [`VoxelRun`]s. Version 1 bodies
//!   still load (their layers are empty).

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;

use rizxel_math::{BoundSize, IVec3, Palette, Voxel, VoxelVolume, EMPTY_VOXEL};
use serde::{Deserialize, Serialize};

pub const RZXL_MAGIC: [u8; 4] = *b"RZXL";
pub const RZXL_VERSION: u32 = 2;
pub const RZXL_EXTENSION: &str = "rzxl";

/// Hard limit on the decompressed body to guard against corrupt/malicious files.
const MAX_BODY_LEN: u32 = 512 * 1024 * 1024;
const ZSTD_LEVEL: i32 = 3;

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
}

/// Run of `len` identical voxels starting at linear index `start` inside the
/// project bounds (see [`linear_index`] for the order).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoxelRun {
    pub start: u32,
    pub len: u32,
    pub value: Voxel,
}

/// Per-layer data persisted in the project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayerData {
    pub name: String,
    pub visible: bool,
    pub locked: bool,
    /// Voxels as sorted, non-overlapping runs (version 2+).
    #[serde(default)]
    pub voxels: Vec<VoxelRun>,
}

/// Serializable project document (JSON body of a `.rzxl` file).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectFile {
    pub name: String,
    pub palette: Palette,
    pub selected_color: usize,
    pub layers: Vec<LayerData>,
    pub active_layer: usize,
    pub bounds: BoundSize,
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
    RzxlError::Json(<serde_json::Error as serde::de::Error>::custom(msg))
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

/// Encodes a project into `.rzxl` bytes.
pub fn write_rzxl<W: Write>(mut w: W, project: &ProjectFile) -> Result<(), RzxlError> {
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
    let version = u32::from_le_bytes(header[4..8].try_into().expect("4 bytes"));
    if version == 0 || version > RZXL_VERSION {
        return Err(RzxlError::UnsupportedVersion { found: version });
    }
    let expected = u32::from_le_bytes(header[8..12].try_into().expect("4 bytes"));
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

    Ok(serde_json::from_slice(&json)?)
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

    fn layer(name: &str, visible: bool, locked: bool, voxels: Vec<VoxelRun>) -> LayerData {
        LayerData {
            name: name.into(),
            visible,
            locked,
            voxels,
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
            name: "hero_character".into(),
            palette: Palette::default(),
            selected_color: Palette::DEFAULT_SELECTED,
            layers: vec![
                layer("Head", true, false, Vec::new()),
                layer(
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
    fn version_3_is_rejected() {
        let mut buf = Vec::new();
        write_rzxl(&mut buf, &sample()).unwrap();
        assert_eq!(&buf[4..8], &2u32.to_le_bytes());
        buf[4..8].copy_from_slice(&3u32.to_le_bytes());
        assert!(matches!(
            read_rzxl(buf.as_slice()).unwrap_err(),
            RzxlError::UnsupportedVersion { found: 3 }
        ));
    }

    #[test]
    fn malformed_runs_are_rejected() {
        let b = BoundSize::new(2, 2, 2);
        let run = |start, len, value| VoxelRun { start, len, value };
        let err = |runs: &[VoxelRun]| match volume_from_runs(b, runs) {
            Err(RzxlError::Json(e)) => e.to_string(),
            other => panic!("expected a Json error, got {other:?}"),
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

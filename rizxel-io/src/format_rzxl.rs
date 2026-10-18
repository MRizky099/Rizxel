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
//! Voxel data per layer will be added in Phase 4 via a version bump.

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;

use rizxel_math::{BoundSize, Palette};
use serde::{Deserialize, Serialize};

pub const RZXL_MAGIC: [u8; 4] = *b"RZXL";
pub const RZXL_VERSION: u32 = 1;
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

/// Per-layer metadata persisted in the project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayerData {
    pub name: String,
    pub visible: bool,
    pub locked: bool,
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

    fn sample() -> ProjectFile {
        ProjectFile {
            name: "hero_character".into(),
            palette: Palette::default(),
            selected_color: Palette::DEFAULT_SELECTED,
            layers: vec![
                LayerData {
                    name: "Head".into(),
                    visible: true,
                    locked: false,
                },
                LayerData {
                    name: "Left Arm".into(),
                    visible: false,
                    locked: true,
                },
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

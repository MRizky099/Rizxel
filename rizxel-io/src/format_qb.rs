//! Qubicle Binary `.qb` import and export.
//!
//! Layout (little-endian): a header of six `u32` — version (`1.1.0.0`),
//! color format (0 RGBA, 1 BGRA), Z-axis orientation (0 left-handed,
//! 1 right-handed), compressed (RLE) flag, visibility-mask flag and matrix
//! count — then per matrix a `u8` name length, the name, `u32` size and
//! `i32` position, and the voxels. Uncompressed voxels are one 4-byte color
//! each with X fastest, then Y, then Z. Compressed matrices store each Z
//! slice as values, runs (`2`, count, color) and an end marker (`6`).
//!
//! - **Import**: each matrix becomes a layer at its position; the bounds are
//!   the union of the matrix boxes (moved to the origin when a matrix lies
//!   below zero). Alpha 0 is empty; with the visibility mask the alpha is a
//!   face mask and any non-zero value is solid. True colors are reduced to
//!   the palette. Qubicle shows a file flagged left-handed (0) the way
//!   Rizxel does (checked in Qubicle 3.1: the front of a model stays at the
//!   front, nothing mirrored), so those load as they are; right-handed (1)
//!   files are mirrored on Z.
//! - **Export**: one RLE-compressed RGBA matrix per visible layer, cut to its
//!   voxels, flagged left-handed (Qubicle's own default), at its position
//!   in the bounds. Re-exporting a
//!   file Rizxel wrote gives the same bytes.

use glam::IVec3;
use rizxel_math::{Rgba8, EMPTY_VOXEL};

use crate::interchange::ExportScene;
use crate::qubicle::{
    color_of, export_matrices, name_bytes, project_from_matrices, put_ivec3, put_u32, ByteReader,
    QMatrix,
};
use crate::{ProjectFile, RzxlError, MAX_LAYERS};

pub const QB_EXTENSION: &str = "qb";
/// `1.1.0.0` as stored (bytes `01 01 00 00`).
const VERSION: u32 = 0x0000_0101;
const CODE_FLAG: u32 = 2;
const NEXT_SLICE_FLAG: u32 = 6;

/// Parses a `.qb` file into a project named `name`.
pub fn read_qb(bytes: &[u8], name: &str) -> Result<ProjectFile, RzxlError> {
    let mut r = ByteReader::new(bytes, "Qubicle");
    let version = r.u32()?;
    if version & 0xff != 1 {
        return Err(r.invalid(format!("unsupported version {version:#x}")));
    }
    let bgra = match r.u32()? {
        0 => false,
        1 => true,
        n => return Err(r.invalid(format!("unknown color format {n}"))),
    };
    let mirrored = match r.u32()? {
        0 => false,
        1 => true,
        n => return Err(r.invalid(format!("unknown Z-axis orientation {n}"))),
    };
    let compressed = r.u32()? != 0;
    let _visibility_mask = r.u32()? != 0;
    let count = r.u32()? as usize;
    if count > MAX_LAYERS {
        return Err(r.invalid(format!(
            "{count} matrices; at most {MAX_LAYERS} are supported"
        )));
    }
    let decode = |c: u32| -> Option<Rgba8> {
        let [a, b, c, alpha] = c.to_le_bytes();
        // Alpha is opacity, or the face mask when that flag is set: 0 = empty.
        (alpha != 0).then(|| {
            if bgra {
                Rgba8::rgb(c, b, a)
            } else {
                Rgba8::rgb(a, b, c)
            }
        })
    };

    let mut matrices = Vec::with_capacity(count.min(r.remaining() / 25));
    for _ in 0..count {
        let len = r.u8()? as usize;
        let mname = String::from_utf8_lossy(r.bytes(len)?).into_owned();
        let size = r.size()?;
        let pos = r.ivec3()?;
        let (sx, sy) = (size.x as usize, size.y as usize);
        let slice = sx * sy;
        let mut cells = Vec::new();
        let mut put = |index: usize, z: i32, c: u32| {
            if let Some(color) = decode(c) {
                let p = IVec3::new((index % sx) as i32, (index / sx) as i32, z);
                cells.push((p, color));
            }
        };
        if compressed {
            for z in 0..size.z {
                let mut index = 0usize;
                loop {
                    let data = r.u32()?;
                    if data == NEXT_SLICE_FLAG {
                        break;
                    }
                    if data == CODE_FLAG {
                        let run = r.u32()? as usize;
                        let color = r.u32()?;
                        if run > slice - index {
                            return Err(r.invalid(format!(
                                "a run of {run} voxels overruns its {slice}-voxel slice"
                            )));
                        }
                        for _ in 0..run {
                            put(index, z, color);
                            index += 1;
                        }
                    } else {
                        if index >= slice {
                            return Err(r.invalid("a slice holds more voxels than its size"));
                        }
                        put(index, z, data);
                        index += 1;
                    }
                }
            }
        } else {
            let total = slice * size.z as usize;
            if r.remaining() / 4 < total {
                return Err(r.invalid("unexpected end of data"));
            }
            for i in 0..total {
                let c = r.u32()?;
                put(i % slice, (i / slice) as i32, c);
            }
        }
        let m = QMatrix {
            name: mname,
            pos,
            size,
            cells,
            group: None,
        };
        matrices.push(if mirrored { m.mirrored_z() } else { m });
    }
    project_from_matrices(name, matrices, &[], None)
}

/// Encodes the visible layers as a `.qb` file.
pub fn write_qb(scene: &ExportScene) -> Result<Vec<u8>, RzxlError> {
    let matrices = export_matrices(scene)?;
    let mut out = Vec::new();
    for v in [VERSION, 0, 0, 1, 0, matrices.len() as u32] {
        put_u32(&mut out, v);
    }
    let rgba = |v| {
        if v == EMPTY_VOXEL {
            0
        } else {
            let c = color_of(&scene.palette, v);
            u32::from_le_bytes([c.r, c.g, c.b, 255])
        }
    };
    for m in &matrices {
        let name = name_bytes(m.name, 255);
        out.push(name.len() as u8);
        out.extend_from_slice(name);
        put_ivec3(&mut out, m.size);
        put_ivec3(&mut out, m.pos);
        for z in 0..m.size.z {
            let values: Vec<u32> = (0..m.size.y)
                .flat_map(|y| (0..m.size.x).map(move |x| IVec3::new(x, y, z)))
                .map(|p| rgba(m.get(p)))
                .collect();
            let mut i = 0;
            while i < values.len() {
                let v = values[i];
                let run = values[i..].iter().take_while(|&&w| w == v).count();
                // Runs, and values that would read as a flag, use the code form.
                if run > 2 || v == CODE_FLAG || v == NEXT_SLICE_FLAG {
                    for word in [CODE_FLAG, run as u32, v] {
                        put_u32(&mut out, word);
                    }
                } else {
                    for _ in 0..run {
                        put_u32(&mut out, v);
                    }
                }
                i += run;
            }
            put_u32(&mut out, NEXT_SLICE_FLAG);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interchange::ExportLayer;
    use crate::volume_from_runs;
    use rizxel_math::{BoundSize, Palette, VoxelVolume};

    fn scene() -> ExportScene {
        let bounds = BoundSize::new(12, 8, 9);
        let mut a = VoxelVolume::new(bounds);
        for x in 1..9 {
            a.set(IVec3::new(x, 0, 2), 9);
            a.set(IVec3::new(x, 1, 2), if x % 2 == 0 { 40 } else { 9 });
        }
        a.set(IVec3::new(4, 5, 6), 50);
        let mut b = VoxelVolume::new(bounds);
        b.set(IVec3::new(10, 7, 8), 42);
        let mut hidden = VoxelVolume::new(bounds);
        hidden.set(IVec3::ZERO, 1);
        ExportScene {
            name: "t".into(),
            bounds,
            palette: Palette::default(),
            layers: vec![
                ExportLayer::new("Body", true, a),
                ExportLayer::new("Tip", true, b),
                ExportLayer::new("Hidden", false, hidden),
            ],
            groups: Vec::new(),
        }
    }

    /// Writes a hand-made file: `flags` = (bgra, Z orientation, compressed,
    /// mask), one matrix, raw `values` (RLE stream or dense colors).
    fn file(flags: [u32; 4], size: [u32; 3], pos: [i32; 3], values: &[u32]) -> Vec<u8> {
        let mut out = Vec::new();
        for v in [VERSION, flags[0], flags[1], flags[2], flags[3], 1] {
            put_u32(&mut out, v);
        }
        out.push(1);
        out.push(b'm');
        for s in size {
            put_u32(&mut out, s);
        }
        for p in pos {
            out.extend_from_slice(&p.to_le_bytes());
        }
        for &v in values {
            put_u32(&mut out, v);
        }
        out
    }

    fn cells(p: &ProjectFile) -> Vec<(IVec3, Rgba8)> {
        let mut out: Vec<(IVec3, Rgba8)> = p
            .layers
            .iter()
            .flat_map(|l| {
                volume_from_runs(p.bounds, &l.voxels)
                    .unwrap()
                    .iter()
                    .collect::<Vec<_>>()
            })
            .map(|(q, v)| (q, p.palette.get(v as usize).unwrap()))
            .collect();
        out.sort_by_key(|(q, _)| (q.z, q.y, q.x));
        out
    }

    #[test]
    fn roundtrip_keeps_layers_and_rewrites_the_same_bytes() {
        let s = scene();
        let bytes = write_qb(&s).unwrap();
        let p = read_qb(&bytes, "t").unwrap();
        assert_eq!(p.layers.len(), 2, "hidden layer left out");
        assert_eq!(p.layers[0].name, "Body");
        let back = volume_from_runs(p.bounds, &p.layers[0].voxels).unwrap();
        assert_eq!(back.voxel_count(), s.layers[0].volume.voxel_count());
        let opaque = |c: Rgba8| Rgba8::rgb(c.r, c.g, c.b);
        assert_eq!(
            p.palette
                .get(back.get(IVec3::new(4, 5, 6)) as usize)
                .map(opaque),
            s.palette.get(50).map(opaque)
        );
        // Export -> import -> export is byte-identical.
        let scene2 = ExportScene {
            name: "t".into(),
            bounds: p.bounds,
            palette: p.palette.clone(),
            layers: p
                .layers
                .iter()
                .map(|l| {
                    ExportLayer::new(
                        l.name.clone(),
                        true,
                        volume_from_runs(p.bounds, &l.voxels).unwrap(),
                    )
                })
                .collect(),
            groups: Vec::new(),
        };
        assert_eq!(write_qb(&scene2).unwrap(), bytes);
    }

    #[test]
    fn reads_both_color_formats_and_orientations() {
        let red = u32::from_le_bytes([255, 0, 0, 255]);
        let red_bgra = u32::from_le_bytes([0, 0, 255, 255]);
        // 1×1×2 matrix, voxel at z = 0 only.
        for (flags, color) in [([0, 0, 0, 0], red), ([1, 0, 0, 0], red_bgra)] {
            let p = read_qb(&file(flags, [1, 1, 2], [0, 0, 0], &[color, 0]), "t").unwrap();
            assert_eq!(cells(&p), vec![(IVec3::ZERO, Rgba8::rgb(255, 0, 0))]);
        }
        // Right-handed: the same file mirrors to z = 1.
        let p = read_qb(&file([0, 1, 0, 0], [1, 1, 2], [0, 0, 0], &[red, 0]), "t").unwrap();
        assert_eq!(
            cells(&p),
            vec![(IVec3::new(0, 0, 1), Rgba8::rgb(255, 0, 0))]
        );
        // Visibility mask: any non-zero alpha is solid.
        let masked = u32::from_le_bytes([0, 255, 0, 0x3e]);
        let p = read_qb(&file([0, 0, 0, 1], [1, 1, 1], [0, 0, 0], &[masked]), "t").unwrap();
        assert_eq!(cells(&p).len(), 1);
    }

    #[test]
    fn reads_rle_runs_and_positions() {
        let c = u32::from_le_bytes([10, 20, 30, 255]);
        // 3×1×1 at x = 2: a run of 3.
        let p = read_qb(
            &file(
                [0, 0, 1, 0],
                [3, 1, 1],
                [2, 0, 0],
                &[CODE_FLAG, 3, c, NEXT_SLICE_FLAG],
            ),
            "t",
        )
        .unwrap();
        assert_eq!(p.bounds, BoundSize::new(5, 1, 1));
        assert_eq!(cells(&p).len(), 3);
        assert_eq!(cells(&p)[0].0, IVec3::new(2, 0, 0));
    }

    #[test]
    fn hostile_inputs_are_refused() {
        let c = u32::from_le_bytes([1, 2, 3, 255]);
        // Matrix side over 256.
        assert!(read_qb(&file([0, 0, 0, 0], [300, 1, 1], [0, 0, 0], &[]), "t").is_err());
        // Zero side.
        assert!(read_qb(&file([0, 0, 0, 0], [0, 1, 1], [0, 0, 0], &[]), "t").is_err());
        // Truncated raw data.
        assert!(read_qb(&file([0, 0, 0, 0], [4, 4, 4], [0, 0, 0], &[c]), "t").is_err());
        // Truncated RLE (no end marker), and a run past the slice.
        assert!(read_qb(&file([0, 0, 1, 0], [2, 1, 1], [0, 0, 0], &[c]), "t").is_err());
        assert!(read_qb(
            &file(
                [0, 0, 1, 0],
                [2, 1, 1],
                [0, 0, 0],
                &[CODE_FLAG, 99, c, NEXT_SLICE_FLAG]
            ),
            "t"
        )
        .is_err());
        // Too many literal values for the slice.
        assert!(read_qb(
            &file([0, 0, 1, 0], [1, 1, 1], [0, 0, 0], &[c, c, NEXT_SLICE_FLAG]),
            "t"
        )
        .is_err());
        // A name length past the end of the file.
        let mut bad = file([0, 0, 0, 0], [1, 1, 1], [0, 0, 0], &[c]);
        bad[24] = 255;
        assert!(read_qb(&bad, "t").is_err());
        // Unknown flags, huge matrix counts, empty input.
        assert!(read_qb(&file([7, 0, 0, 0], [1, 1, 1], [0, 0, 0], &[c]), "t").is_err());
        let mut many = file([0, 0, 0, 0], [1, 1, 1], [0, 0, 0], &[c]);
        many[20..24].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(read_qb(&many, "t").is_err());
        assert!(read_qb(&[], "t").is_err());
        // Matrices too far apart for the bounds.
        let mut far = file([0, 0, 0, 0], [1, 1, 1], [0, 0, 0], &[c]);
        far[20..24].copy_from_slice(&2u32.to_le_bytes());
        far.extend_from_slice(&[1, b'n']);
        for v in [1u32, 1, 1] {
            put_u32(&mut far, v);
        }
        put_ivec3(&mut far, IVec3::new(1000, 0, 0));
        put_u32(&mut far, c);
        assert!(read_qb(&far, "t").is_err());
    }

    #[test]
    fn empty_scenes_are_refused() {
        let mut s = scene();
        s.layers.iter_mut().for_each(|l| l.visible = false);
        assert!(write_qb(&s).is_err());
    }
}

//! Qubicle Exchange Format `.qef` (ASCII) import and export.
//!
//! ```text
//! Qubicle Exchange Format
//! Version 0.2
//! www.minddesk.com
//! sx sy sz
//! color count
//! r g b              (0..1, one line per color)
//! x y z index mask   (one line per voxel; index into the colors)
//! ```
//!
//! One matrix, Y-up, read and written right-handed (like Rizxel). The mask
//! marks visible faces; every listed voxel is solid.
//!
//! - **Import**: up to 255 colors become the palette in file order, so
//!   indices are kept; more colors are reduced. Coordinates outside the size
//!   or unknown color indices are refused.
//! - **Export**: the merged visible layers at the full bounds size (so the
//!   model keeps its place), every palette color, and the face mask.
//!   Re-exporting a file Rizxel wrote gives the same text.

use glam::IVec3;
use rizxel_math::{Palette, Rgba8, Voxel, VoxelVolume};

use crate::interchange::{bounds_for, build_project, ExportScene, ImportLayer};
use crate::qubicle::{face_mask, project_from_matrices, QMatrix, MAX_SIDE};
use crate::{LayerData, ProjectFile, RzxlError};

pub const QEF_EXTENSION: &str = "qef";
const HEADER: [&str; 3] = ["Qubicle Exchange Format", "Version 0.2", "www.minddesk.com"];
/// Most colors accepted in a file.
const MAX_COLORS: usize = 1 << 20;

fn invalid(msg: impl Into<String>) -> RzxlError {
    RzxlError::Invalid(format!("QEF: {}", msg.into()))
}

/// Parses `.qef` text into a one-layer project named `name`.
pub fn read_qef(text: &str, name: &str) -> Result<ProjectFile, RzxlError> {
    let mut lines = text
        .lines()
        .map(str::trim)
        .enumerate()
        .filter(|(_, l)| !l.is_empty());
    let mut next = |what: &str| {
        lines
            .next()
            .ok_or_else(|| invalid(format!("missing {what}")))
    };
    let (_, magic) = next("header")?;
    if !magic.eq_ignore_ascii_case(HEADER[0]) {
        return Err(invalid("not a Qubicle Exchange file"));
    }
    next("version")?;
    next("header")?;
    let numbers = |(n, l): (usize, &str), count: usize| -> Result<Vec<f64>, RzxlError> {
        let v: Vec<f64> = l
            .split_whitespace()
            .take(count)
            .map(|s| s.parse::<f64>())
            .collect::<Result<_, _>>()
            .map_err(|_| invalid(format!("line {}: bad number", n + 1)))?;
        if v.len() < count || v.iter().any(|x| !x.is_finite()) {
            return Err(invalid(format!("line {}: expected {count} numbers", n + 1)));
        }
        Ok(v)
    };
    let size = numbers(next("size")?, 3)?;
    if size
        .iter()
        .any(|&s| s < 1.0 || s > MAX_SIDE as f64 || s.fract() != 0.0)
    {
        return Err(invalid(format!(
            "size {}×{}×{}: each side must be 1..={MAX_SIDE}",
            size[0], size[1], size[2]
        )));
    }
    let size = IVec3::new(size[0] as i32, size[1] as i32, size[2] as i32);
    let count = numbers(next("color count")?, 1)?[0];
    if !(0.0..=MAX_COLORS as f64).contains(&count) || count.fract() != 0.0 {
        return Err(invalid(format!("bad color count {count}")));
    }
    let mut colors = Vec::with_capacity(count as usize);
    for _ in 0..count as usize {
        let c = numbers(next("color")?, 3)?;
        let ch = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        colors.push(Rgba8::rgb(ch(c[0]), ch(c[1]), ch(c[2])));
    }
    let max_voxels = (size.x * size.y * size.z) as usize;
    let mut cells: Vec<(IVec3, usize)> = Vec::new();
    for line in lines {
        let v = numbers(line, 4)?;
        let p = IVec3::new(v[0] as i32, v[1] as i32, v[2] as i32);
        let index = v[3];
        if v[..3].iter().any(|c| c.fract() != 0.0)
            || p.cmplt(IVec3::ZERO).any()
            || p.cmpge(size).any()
        {
            return Err(invalid(format!(
                "line {}: voxel outside the size",
                line.0 + 1
            )));
        }
        if index < 0.0 || index.fract() != 0.0 || index as usize >= colors.len() {
            return Err(invalid(format!(
                "line {}: unknown color {index}",
                line.0 + 1
            )));
        }
        if cells.len() >= max_voxels {
            return Err(invalid("more voxels than the size holds"));
        }
        cells.push((p, index as usize));
    }

    if colors.len() > rizxel_engine::quantize::MAX_PAINT_COLORS {
        // Too many colors for the palette: reduce them.
        let matrix = QMatrix {
            name: name.to_owned(),
            pos: IVec3::ZERO,
            size,
            cells: cells.into_iter().map(|(p, i)| (p, colors[i])).collect(),
            group: None,
        };
        return project_from_matrices(name, vec![matrix], &[], None);
    }
    let bounds = bounds_for(size.as_uvec3(), "The QEF model")?;
    let mut palette_colors = vec![Rgba8::BLACK];
    palette_colors.extend(&colors);
    if palette_colors.len() < 2 {
        palette_colors.push(Rgba8::WHITE);
    }
    let mut volume = VoxelVolume::new(bounds);
    for (p, i) in cells {
        volume.set(p, (i + 1) as Voxel);
    }
    build_project(
        name,
        bounds,
        Palette::new(palette_colors),
        Vec::new(),
        vec![ImportLayer {
            data: LayerData::new(1, name),
            volume: Some(volume),
        }],
    )
}

/// Writes the merged visible layers as `.qef` text.
pub fn write_qef(scene: &ExportScene) -> Result<String, RzxlError> {
    let volume = scene.merged_visible();
    if volume.is_empty() {
        return Err(RzxlError::Invalid(
            "nothing to export: the visible layers contain no voxels".into(),
        ));
    }
    let b = scene.bounds;
    let mut s = String::new();
    for h in HEADER {
        s.push_str(h);
        s.push('\n');
    }
    s.push_str(&format!("{} {} {}\n", b.x(), b.y(), b.z()));
    let colors = &scene.palette.colors()[1.min(scene.palette.len())..];
    s.push_str(&format!("{}\n", colors.len()));
    for c in colors {
        let f = |v: u8| v as f32 / 255.0;
        s.push_str(&format!("{:.6} {:.6} {:.6}\n", f(c.r), f(c.g), f(c.b)));
    }
    let mut cells: Vec<(IVec3, Voxel)> = volume.iter().collect();
    cells.sort_by_key(|(p, _)| (p.z, p.y, p.x));
    for (p, v) in cells {
        let index = (v as usize).clamp(1, colors.len().max(1)) - 1;
        s.push_str(&format!(
            "{} {} {} {index} {}\n",
            p.x,
            p.y,
            p.z,
            face_mask(&volume, p)
        ));
    }
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interchange::ExportLayer;
    use crate::volume_from_runs;
    use rizxel_math::BoundSize;

    fn scene() -> ExportScene {
        let bounds = BoundSize::new(6, 5, 4);
        let mut v = VoxelVolume::new(bounds);
        v.set(IVec3::new(1, 0, 2), 9);
        v.set(IVec3::new(2, 0, 2), 40);
        v.set(IVec3::new(5, 4, 3), 3);
        ExportScene {
            name: "t".into(),
            bounds,
            palette: Palette::default(),
            layers: vec![ExportLayer::new("Body", true, v)],
            groups: Vec::new(),
        }
    }

    #[test]
    fn roundtrip_keeps_indices_and_text() {
        let s = scene();
        let text = write_qef(&s).unwrap();
        assert!(text.starts_with("Qubicle Exchange Format\nVersion 0.2\n"));
        let p = read_qef(&text, "t").unwrap();
        assert_eq!(p.bounds, s.bounds);
        let back = volume_from_runs(p.bounds, &p.layers[0].voxels).unwrap();
        assert_eq!(back.get(IVec3::new(2, 0, 2)), 40);
        assert_eq!(back.get(IVec3::new(5, 4, 3)), 3);
        assert_eq!(back.voxel_count(), 3);
        let again = ExportScene {
            palette: p.palette.clone(),
            layers: vec![ExportLayer::new("t", true, back)],
            ..s
        };
        assert_eq!(write_qef(&again).unwrap(), text);
    }

    #[test]
    fn face_masks_mark_open_sides() {
        let s = scene();
        let text = write_qef(&s).unwrap();
        // The lone voxel at (5, 4, 3) is open on all six sides.
        assert!(text.lines().any(|l| l == "5 4 3 2 127"), "{text}");
        // Its two neighbours share one closed side each.
        assert!(text.lines().any(|l| l == "1 0 2 8 123"));
    }

    #[test]
    fn many_colors_are_reduced() {
        let mut t =
            String::from("Qubicle Exchange Format\nVersion 0.2\nwww.minddesk.com\n256 1 2\n300\n");
        for i in 0..300 {
            t.push_str(&format!(
                "{} {} 0\n",
                i as f32 / 300.0,
                (i % 7) as f32 / 7.0
            ));
        }
        for i in 0..300 {
            t.push_str(&format!("{} 0 {} {i} 1\n", i % 256, i / 256));
        }
        let p = read_qef(&t, "t").unwrap();
        assert!(p.palette.len() <= 256);
        assert_eq!(
            volume_from_runs(p.bounds, &p.layers[0].voxels)
                .unwrap()
                .voxel_count(),
            300
        );
    }

    #[test]
    fn hostile_inputs_are_refused() {
        let head = "Qubicle Exchange Format\nVersion 0.2\nwww.minddesk.com\n";
        for bad in [
            "",
            "Something else\n",
            "Qubicle Exchange Format\nVersion 0.2\n",
            &format!("{head}300 1 1\n1\n1 1 1\n"),
            &format!("{head}0 1 1\n1\n1 1 1\n"),
            &format!("{head}2 2 2\n1\n1 1 1\n2 0 0 0 1\n"),
            &format!("{head}2 2 2\n1\n1 1 1\n-1 0 0 0 1\n"),
            &format!("{head}2 2 2\n1\n1 1 1\n0 0 0 5 1\n"),
            &format!("{head}2 2 2\n1\n1 1 1\n0 0 0 nan 1\n"),
            &format!("{head}2 2 2\n99999999\n"),
            &format!("{head}2 2 2\n3\n1 1 1\n"),
            &format!("{head}1 1 1\n1\n1 1 1\n0 0 0 0 1\n0 0 0 0 1\n"),
        ] {
            assert!(read_qef(bad, "t").is_err(), "{bad:?}");
        }
    }
}

//! Render output planning (Phase 11b): file names and folders, turntable
//! frames, sprite sheets and the files each output writes. Pure functions;
//! the UI renders the pixels and hands them to [`save_png`] and friends.

use std::path::{Path, PathBuf};

use rizxel_io::png_io::{encode_rgba, encode_rgba16, RgbaImage};
use rizxel_io::RzxlError;
use rizxel_math::render::OutputSettings;

/// Characters Windows, macOS and Linux refuse (or treat specially) in file
/// names.
const BAD_CHARS: [char; 9] = ['<', '>', ':', '"', '/', '\\', '|', '?', '*'];

/// File name (without extension) for `pattern`: `{name}` becomes the
/// project name and `{frame}` the zero-padded frame number. A frame of a
/// sequence whose pattern has no `{frame}` gets `_<frame>` appended.
/// Characters that are not allowed in file names become `_`.
pub fn output_stem(pattern: &str, project: &str, frame: Option<(u32, u32)>) -> String {
    let mut stem = pattern.replace("{name}", project);
    if let Some((index, count)) = frame {
        let width = digits(count.saturating_sub(1)).max(3);
        let number = format!("{index:0width$}");
        if stem.contains("{frame}") {
            stem = stem.replace("{frame}", &number);
        } else {
            stem = format!("{stem}_{number}");
        }
    } else {
        stem = stem.replace("_{frame}", "").replace("{frame}", "");
    }
    let stem: String = stem
        .chars()
        .map(|c| {
            if BAD_CHARS.contains(&c) || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    let stem = stem.trim().trim_end_matches('.').to_owned();
    if stem.is_empty() {
        "render".to_owned()
    } else {
        stem
    }
}

fn digits(n: u32) -> usize {
    n.max(1).ilog10() as usize + 1
}

/// Folder the files go to: the chosen folder, else the project's folder,
/// else the working folder.
pub fn output_dir(settings: &OutputSettings, project_path: Option<&Path>) -> PathBuf {
    if !settings.folder.trim().is_empty() {
        return PathBuf::from(settings.folder.trim());
    }
    project_path
        .and_then(Path::parent)
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_default()
}

/// Full path of an output file.
pub fn output_path(
    settings: &OutputSettings,
    project: &str,
    project_path: Option<&Path>,
    frame: Option<(u32, u32)>,
    extension: &str,
) -> PathBuf {
    let stem = output_stem(&settings.name, project, frame);
    output_dir(settings, project_path).join(format!("{stem}.{extension}"))
}

fn io_error(e: RzxlError) -> std::io::Error {
    std::io::Error::other(e.to_string())
}

/// Writes an 8-bit RGBA PNG, creating the folder.
pub fn save_png(path: &Path, image: &RgbaImage) -> std::io::Result<()> {
    let bytes = encode_rgba(image.width, image.height, &image.pixels).map_err(io_error)?;
    write_file(path, &bytes)
}

/// Writes a 16-bit RGBA PNG, creating the folder.
pub fn save_png16(path: &Path, width: u32, height: u32, pixels: &[u16]) -> std::io::Result<()> {
    let bytes = encode_rgba16(width, height, pixels).map_err(io_error)?;
    write_file(path, &bytes)
}

/// Writes `bytes`, creating the folder first.
pub fn write_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, bytes)
}

/// Converts premultiplied RGBA8 (an MSAA resolve over a transparent clear)
/// to straight alpha, in place.
pub fn unpremultiply(pixels: &mut [u8]) {
    for px in pixels.chunks_exact_mut(4) {
        let a = px[3];
        if a == 0 {
            px[..3].fill(0);
        } else if a < 255 {
            for c in &mut px[..3] {
                *c = ((*c as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8;
            }
        }
    }
}

/// Camera yaw (degrees) of each turntable frame, starting at `start` and
/// turning `angle` in `frames` equal steps (a full turn does not repeat
/// the first frame at the end).
pub fn turntable_yaws(start: f32, frames: u32, angle: f32) -> Vec<f32> {
    let frames = frames.max(1);
    let step = angle / frames as f32;
    (0..frames)
        .map(|i| (start + step * i as f32).rem_euclid(360.0))
        .collect()
}

/// Largest sprite sheet side in pixels.
pub const MAX_SHEET_SIDE: u32 = 16_384;

/// Grid of turntable frames in one sprite sheet image, row by row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SheetLayout {
    pub columns: u32,
    pub rows: u32,
    /// Size of one frame.
    pub frame: [u32; 2],
    pub count: u32,
}

impl SheetLayout {
    /// The squarest grid for `count` frames of `frame` pixels.
    pub fn new(count: u32, frame: [u32; 2]) -> Self {
        let count = count.max(1);
        let columns = (count as f64).sqrt().ceil() as u32;
        Self {
            columns,
            rows: count.div_ceil(columns),
            frame,
            count,
        }
    }

    /// Sheet size in pixels.
    pub fn size(&self) -> [u32; 2] {
        [self.columns * self.frame[0], self.rows * self.frame[1]]
    }

    /// `[x, y, width, height]` of frame `i`.
    pub fn rect(&self, i: u32) -> [u32; 4] {
        [
            (i % self.columns) * self.frame[0],
            (i / self.columns) * self.frame[1],
            self.frame[0],
            self.frame[1],
        ]
    }
}

/// Lays `frames` out in a sprite sheet. Refused above
/// [`MAX_SHEET_SIDE`] (the frames are still written on their own).
pub fn compose_sheet(frames: &[RgbaImage]) -> Result<(RgbaImage, SheetLayout), String> {
    let first = frames.first().ok_or("no frames")?;
    let layout = SheetLayout::new(frames.len() as u32, [first.width, first.height]);
    let [w, h] = layout.size();
    if w > MAX_SHEET_SIDE || h > MAX_SHEET_SIDE {
        return Err(format!(
            "a {w}×{h} sprite sheet is too large (limit {MAX_SHEET_SIDE} px): use a smaller size or fewer frames"
        ));
    }
    let mut pixels = vec![0u8; (w * h * 4) as usize];
    for (i, f) in frames.iter().enumerate() {
        let [x, y, fw, fh] = layout.rect(i as u32);
        if (f.width, f.height) != (fw, fh) {
            return Err(format!("frame {i} has another size"));
        }
        for row in 0..fh {
            let src = (row * fw * 4) as usize;
            let dst = (((y + row) * w + x) * 4) as usize;
            pixels[dst..dst + (fw * 4) as usize]
                .copy_from_slice(&f.pixels[src..src + (fw * 4) as usize]);
        }
    }
    Ok((
        RgbaImage {
            width: w,
            height: h,
            pixels,
        },
        layout,
    ))
}

/// JSON describing a sprite sheet: the image, frame size, grid, playback
/// speed and each frame's rectangle and camera angle.
pub fn sheet_json(layout: &SheetLayout, image: &str, fps: u32, yaws: &[f32]) -> String {
    let frames: Vec<serde_json::Value> = (0..layout.count)
        .map(|i| {
            let [x, y, w, h] = layout.rect(i);
            serde_json::json!({
                "index": i,
                "x": x,
                "y": y,
                "w": w,
                "h": h,
                "yaw": yaws.get(i as usize).copied().unwrap_or(0.0),
            })
        })
        .collect();
    let json = serde_json::json!({
        "image": image,
        "frame_width": layout.frame[0],
        "frame_height": layout.frame[1],
        "columns": layout.columns,
        "rows": layout.rows,
        "fps": fps,
        "frames": frames,
    });
    serde_json::to_string_pretty(&json).unwrap_or_default()
}

/// Writes a looping GIF of `frames`.
pub fn save_gif(path: &Path, frames: &[RgbaImage], fps: u32) -> std::io::Result<()> {
    let bytes = rizxel_io::gif_io::encode_gif(frames, fps).map_err(io_error)?;
    write_file(path, &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turntable_frames_turn_evenly() {
        let yaws = turntable_yaws(45.0, 36, 360.0);
        assert_eq!(yaws.len(), 36);
        assert_eq!(yaws[0], 45.0);
        assert!((yaws[1] - 55.0).abs() < 1e-4);
        assert!((yaws[35] - 35.0).abs() < 1e-3, "wraps past 360");
        let half = turntable_yaws(0.0, 4, 180.0);
        assert_eq!(half, vec![0.0, 45.0, 90.0, 135.0]);
        assert_eq!(turntable_yaws(10.0, 0, 360.0), vec![10.0]);
    }

    #[test]
    fn sprite_sheet_rects_and_json() {
        let layout = SheetLayout::new(36, [64, 48]);
        assert_eq!((layout.columns, layout.rows), (6, 6));
        assert_eq!(layout.size(), [384, 288]);
        assert_eq!(layout.rect(7), [64, 48, 64, 48]);
        let odd = SheetLayout::new(5, [10, 10]);
        assert_eq!((odd.columns, odd.rows), (3, 2));

        let frames: Vec<RgbaImage> = (0..5u8)
            .map(|i| RgbaImage {
                width: 2,
                height: 2,
                pixels: vec![i * 40; 16],
            })
            .collect();
        let (sheet, layout) = compose_sheet(&frames).unwrap();
        assert_eq!((sheet.width, sheet.height), (6, 4));
        assert_eq!(sheet.pixel(2, 2), [160; 4], "frame 4 is at column 1, row 1");
        assert_eq!(sheet.pixel(3, 3), [160; 4]);
        assert_eq!(sheet.pixel(4, 2), [0; 4], "the last cell stays empty");
        assert_eq!(sheet.pixel(0, 0), [0; 4]);
        let json: serde_json::Value =
            serde_json::from_str(&sheet_json(&layout, "s.png", 12, &[0.0, 72.0])).unwrap();
        assert_eq!(json["frames"].as_array().unwrap().len(), 5);
        assert_eq!(json["frames"][1]["x"], 2);
        assert_eq!(json["frames"][1]["yaw"], 72.0);
        assert_eq!(json["columns"], 3);

        let huge = vec![
            RgbaImage {
                width: 9000,
                height: 1,
                pixels: vec![0; 36_000],
            };
            4
        ];
        assert!(compose_sheet(&huge).is_err());
    }

    #[test]
    fn stems_fill_the_pattern() {
        assert_eq!(
            output_stem("{name}_render", "knight", None),
            "knight_render"
        );
        assert_eq!(output_stem("{name}_{frame}", "knight", None), "knight");
        assert_eq!(
            output_stem("{name}_{frame}", "knight", Some((7, 36))),
            "knight_007"
        );
        assert_eq!(
            output_stem("{name}", "knight", Some((12, 2000))),
            "knight_0012"
        );
        assert_eq!(output_stem("a/b:c*?", "x", None), "a_b_c__");
        assert_eq!(output_stem("{name}", "", None), "render");
        assert_eq!(output_stem("  .. ", "x", None), "render");
    }

    #[test]
    fn files_go_to_the_folder_or_next_to_the_project() {
        let mut s = OutputSettings::default();
        let project = Path::new("models").join("knight.rzxl");
        let p = output_path(&s, "knight", Some(&project), None, "png");
        assert_eq!(p, Path::new("models").join("knight_render.png"));
        s.folder = "out".into();
        let p = output_path(&s, "knight", Some(&project), Some((0, 36)), "png");
        assert_eq!(p, Path::new("out").join("knight_render_000.png"));
    }

    #[test]
    fn saving_creates_the_folder() {
        let dir = std::env::temp_dir().join(format!("rizxel_out_{}", std::process::id()));
        let path = dir.join("a").join("img.png");
        let image = RgbaImage {
            width: 2,
            height: 1,
            pixels: vec![255, 0, 0, 255, 0, 0, 255, 128],
        };
        save_png(&path, &image).unwrap();
        let back = rizxel_io::png_io::decode_rgba(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(back, image);
        save_png16(&dir.join("b.png"), 1, 1, &[65535, 0, 0, 65535]).unwrap();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unpremultiply_restores_straight_alpha() {
        let mut px = [64, 32, 0, 128, 10, 10, 10, 0, 200, 100, 50, 255];
        unpremultiply(&mut px);
        assert_eq!(px, [128, 64, 0, 128, 0, 0, 0, 0, 200, 100, 50, 255]);
    }
}

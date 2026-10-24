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

#[cfg(test)]
mod tests {
    use super::*;

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

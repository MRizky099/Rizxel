//! Palette files: GIMP `.gpl` and Lospec-style `.hex` (one `RRGGBB` per
//! line).
//!
//! Readers are lenient line by line but bounded: a file over
//! [`MAX_PALETTE_FILE_BYTES`] is refused, overlong or malformed lines are
//! skipped with a warning, and colors past [`MAX_FILE_COLORS`] are dropped
//! with a warning. A file without a single color is an error. Colors map to
//! palette slots `1..` (slot 0 is the reserved empty voxel).

use std::path::Path;

use rizxel_math::{Rgba8, MAX_PALETTE_COLORS};

use crate::RzxlError;

pub const GPL_EXTENSION: &str = "gpl";
pub const HEX_EXTENSION: &str = "hex";
/// Largest palette file read (1 MiB): far beyond any real palette.
pub const MAX_PALETTE_FILE_BYTES: u64 = 1 << 20;
/// Colors a file can fill: every slot except the reserved slot 0.
pub const MAX_FILE_COLORS: usize = MAX_PALETTE_COLORS - 1;
/// Lines longer than this are skipped (a color line is a few dozen bytes).
const MAX_LINE_CHARS: usize = 1024;
/// Warnings kept per file; the rest are summarized.
const MAX_WARNINGS: usize = 8;

/// Colors read from a palette file, plus what was skipped.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PaletteFile {
    pub name: Option<String>,
    pub colors: Vec<Rgba8>,
    pub warnings: Vec<String>,
}

impl PaletteFile {
    fn warn(&mut self, line: usize, what: &str) {
        if self.warnings.len() < MAX_WARNINGS {
            self.warnings.push(format!("line {line}: {what}"));
        } else if self.warnings.len() == MAX_WARNINGS {
            self.warnings.push("more lines skipped".to_owned());
        }
    }

    fn push(&mut self, color: Rgba8, line: usize) {
        if self.colors.len() < MAX_FILE_COLORS {
            self.colors.push(color);
        } else if self.colors.len() == MAX_FILE_COLORS {
            // Mark once; the counter below keeps the summary accurate.
            self.warn(
                line,
                &format!("more than {MAX_FILE_COLORS} colors, the rest are ignored"),
            );
            self.colors.push(color);
        }
    }

    /// Drops the overflow marker color and refuses an empty file.
    fn finish(mut self) -> Result<Self, RzxlError> {
        self.colors.truncate(MAX_FILE_COLORS);
        if self.colors.is_empty() {
            return Err(RzxlError::Invalid("the palette file has no colors".into()));
        }
        Ok(self)
    }
}

/// Lines with their 1-based numbers, overlong ones flagged.
fn lines(text: &str) -> impl Iterator<Item = (usize, &str)> {
    text.lines().enumerate().map(|(i, l)| (i + 1, l.trim()))
}

/// Reads a GIMP palette: a `GIMP Palette` header, optional `Name:` and
/// `Columns:` lines, `#` comments and `R G B [name]` color lines.
pub fn read_gpl(text: &str) -> Result<PaletteFile, RzxlError> {
    let mut out = PaletteFile::default();
    let mut rows = lines(text.trim_start_matches('\u{feff}'));
    match rows.next() {
        Some((_, "GIMP Palette")) => {}
        _ => {
            return Err(RzxlError::Invalid(
                "missing the `GIMP Palette` header".into(),
            ))
        }
    }
    for (n, line) in rows {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.len() > MAX_LINE_CHARS {
            out.warn(n, "line too long");
            continue;
        }
        if let Some(name) = line.strip_prefix("Name:") {
            out.name = Some(name.trim().chars().take(256).collect());
            continue;
        }
        if line.starts_with("Columns:") {
            continue;
        }
        let mut fields = line.split_whitespace();
        let channels: Vec<Option<u8>> = fields
            .by_ref()
            .take(3)
            .map(|f| f.parse::<u8>().ok())
            .collect();
        match channels.as_slice() {
            [Some(r), Some(g), Some(b)] => out.push(Rgba8::rgb(*r, *g, *b), n),
            _ => out.warn(n, "not an `R G B` color in 0..255"),
        }
    }
    out.finish()
}

/// Reads a `.hex` palette: one `RRGGBB` (optionally `#RRGGBB`) per line.
pub fn read_hex(text: &str) -> Result<PaletteFile, RzxlError> {
    let mut out = PaletteFile::default();
    for (n, line) in lines(text.trim_start_matches('\u{feff}')) {
        if line.is_empty() || line.starts_with("//") || line.starts_with(';') {
            continue;
        }
        if line.len() > MAX_LINE_CHARS {
            out.warn(n, "line too long");
            continue;
        }
        let digits = line.strip_prefix('#').unwrap_or(line);
        let valid = digits.len() == 6 && digits.bytes().all(|b| b.is_ascii_hexdigit());
        match Rgba8::parse_hex(digits).filter(|_| valid) {
            Some(c) => out.push(c, n),
            None => out.warn(n, "not an RRGGBB color"),
        }
    }
    out.finish()
}

/// Writes a GIMP palette named `name`, eight columns.
pub fn write_gpl(name: &str, colors: &[Rgba8]) -> String {
    let name: String = name.chars().filter(|c| !c.is_control()).collect();
    let mut s = format!("GIMP Palette\nName: {name}\nColumns: 8\n#\n");
    for c in colors {
        s.push_str(&format!(
            "{:3} {:3} {:3}\t{}\n",
            c.r,
            c.g,
            c.b,
            c.to_hex_string()
        ));
    }
    s
}

/// Writes one lowercase `rrggbb` per line (Lospec's layout).
pub fn write_hex(colors: &[Rgba8]) -> String {
    colors
        .iter()
        .map(|c| format!("{:02x}{:02x}{:02x}\n", c.r, c.g, c.b))
        .collect()
}

/// Reads a `.gpl` or `.hex` file by its extension.
pub fn load_palette_file(path: &Path) -> Result<PaletteFile, RzxlError> {
    let len = std::fs::metadata(path)?.len();
    if len > MAX_PALETTE_FILE_BYTES {
        return Err(RzxlError::Invalid(format!(
            "palette file too large ({len} bytes)"
        )));
    }
    let bytes = std::fs::read(path)?;
    let text = String::from_utf8_lossy(&bytes);
    match extension(path).as_deref() {
        Some(GPL_EXTENSION) => read_gpl(&text),
        Some(HEX_EXTENSION) => read_hex(&text),
        _ => Err(RzxlError::Invalid(
            "unknown palette format (use .gpl or .hex)".into(),
        )),
    }
}

/// Writes `colors` as `.gpl` or `.hex`, by the path's extension.
pub fn save_palette_file(path: &Path, name: &str, colors: &[Rgba8]) -> Result<(), RzxlError> {
    let text = match extension(path).as_deref() {
        Some(GPL_EXTENSION) => write_gpl(name, colors),
        Some(HEX_EXTENSION) => write_hex(colors),
        _ => {
            return Err(RzxlError::Invalid(
                "unknown palette format (use .gpl or .hex)".into(),
            ))
        }
    };
    std::fs::write(path, text)?;
    Ok(())
}

fn extension(path: &Path) -> Option<String> {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn colors(n: usize) -> Vec<Rgba8> {
        (0..n)
            .map(|i| Rgba8::rgb(i as u8, (i * 7) as u8, 255 - i as u8))
            .collect()
    }

    #[test]
    fn gpl_and_hex_round_trip() {
        let c = colors(40);
        let gpl = read_gpl(&write_gpl("Test", &c)).unwrap();
        assert_eq!(gpl.colors, c);
        assert_eq!(gpl.name.as_deref(), Some("Test"));
        assert!(gpl.warnings.is_empty());
        let hex = read_hex(&write_hex(&c)).unwrap();
        assert_eq!(hex.colors, c);
        assert!(hex.warnings.is_empty());
    }

    #[test]
    fn reads_real_world_layouts() {
        let gpl = "GIMP Palette\r\nName: Endesga\r\nColumns: 4\r\n# comment\r\n\
                   190  74  47\tUntitled\r\n  0   0   0\r\n";
        let p = read_gpl(gpl).unwrap();
        assert_eq!(p.colors, [Rgba8::rgb(190, 74, 47), Rgba8::BLACK]);
        let hex = "\u{feff}#BE4A2F\nd77643\n\n";
        let p = read_hex(hex).unwrap();
        assert_eq!(
            p.colors,
            [Rgba8::rgb(190, 74, 47), Rgba8::rgb(215, 118, 67)]
        );
    }

    #[test]
    fn hostile_input_is_bounded() {
        // Bad values and lines are skipped with warnings.
        let gpl = "GIMP Palette\n300 0 0\n-1 2 3\nfoo bar baz\n1 2\n10 20 30\n";
        let p = read_gpl(gpl).unwrap();
        assert_eq!(p.colors, [Rgba8::rgb(10, 20, 30)]);
        assert_eq!(p.warnings.len(), 4);
        let hex = "zzzzzz\n12345\n1234567\n#12g456\n\u{e9}\u{e9}\u{e9}\n0a0b0c\n";
        let p = read_hex(hex).unwrap();
        assert_eq!(p.colors, [Rgba8::rgb(10, 11, 12)]);
        assert_eq!(p.warnings.len(), 5);
        // Overlong lines.
        let long = format!("GIMP Palette\n{}\n1 1 1\n", "1 ".repeat(10_000));
        let p = read_gpl(&long).unwrap();
        assert_eq!(p.colors.len(), 1);
        assert_eq!(p.warnings, ["line 2: line too long"]);
        // Too many colors: clamped with a single warning.
        let p = read_hex(&write_hex(&colors(600))).unwrap();
        assert_eq!(p.colors.len(), MAX_FILE_COLORS);
        assert_eq!(p.colors, colors(MAX_FILE_COLORS));
        assert_eq!(p.warnings.len(), 1);
        // Many bad lines keep a bounded warning list.
        let p = read_hex(&format!("{}ffffff\n", "x\n".repeat(10_000))).unwrap();
        assert_eq!(p.warnings.len(), MAX_WARNINGS + 1);
        // No colors, no header, garbage bytes: errors, never panics.
        assert!(read_hex("").is_err());
        assert!(read_gpl("GIMP Palette\n").is_err());
        assert!(read_gpl("10 20 30\n").is_err());
        assert!(read_hex("\0\0\0\u{fffd}").is_err());
    }

    #[test]
    fn files_dispatch_on_the_extension_and_cap_the_size() {
        let dir = std::env::temp_dir().join(format!("rizxel_palette_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let c = colors(5);
        for ext in [GPL_EXTENSION, HEX_EXTENSION, "GPL"] {
            let path = dir.join(format!("p.{ext}"));
            save_palette_file(&path, "P", &c).unwrap();
            assert_eq!(load_palette_file(&path).unwrap().colors, c);
        }
        assert!(save_palette_file(&dir.join("p.txt"), "P", &c).is_err());
        let big = dir.join("big.hex");
        std::fs::write(&big, vec![b'a'; MAX_PALETTE_FILE_BYTES as usize + 1]).unwrap();
        assert!(load_palette_file(&big).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }
}

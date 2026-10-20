//! Wavefront MTL material libraries.
//!
//! - **Writing** produces the MagicaVoxel-style library that sits next to
//!   an OBJ export: one `palette` material whose `map_Kd` is the 256×1
//!   palette texture.
//! - **Reading** collects every material's diffuse color, texture,
//!   emission, opacity and PBR values. OBJ import uses them for colors.
//!   **Import MTL** uses them to replace a document's palette and materials:
//!   a 256×1 `map_Kd` texture maps one to one onto the palette, otherwise
//!   the `Kd` colors fill it in file order.

use rizxel_math::{Material, Palette, Rgba8};

use crate::png_io::RgbaImage;
use crate::RzxlError;

pub const MTL_EXTENSION: &str = "mtl";
/// Header comment of exported OBJ/MTL files (app @ author, like
/// MagicaVoxel's `# MagicaVoxel @ Ephtracy`).
pub const EXPORT_HEADER: &str = "# Rizxel @ Abati Technology";
/// Name of the single material in exported libraries.
pub const PALETTE_MATERIAL: &str = "palette";

/// One `newmtl` block.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MtlMaterial {
    pub name: String,
    pub kd: Option<[f32; 3]>,
    pub ke: Option<[f32; 3]>,
    /// `d` (or `1 - Tr`).
    pub opacity: Option<f32>,
    /// `Ns` specular exponent.
    pub ns: Option<f32>,
    /// PBR extension `Pr` / `Pm`.
    pub roughness: Option<f32>,
    pub metallic: Option<f32>,
    /// Texture path from `map_Kd`, as written in the file.
    pub map_kd: Option<String>,
}

impl MtlMaterial {
    /// Diffuse color as RGBA8 (white when absent).
    pub fn color(&self) -> Rgba8 {
        let [r, g, b] = self
            .kd
            .unwrap_or([1.0; 3])
            .map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8);
        Rgba8::rgb(r, g, b)
    }

    /// Rizxel material carrying this entry's surface values.
    pub fn to_material(&self) -> Material {
        let unit = |v: f32| {
            if v.is_finite() {
                v.clamp(0.0, 1.0)
            } else {
                0.0
            }
        };
        let mut m = Material::new(self.name.clone());
        if let Some(r) = self.roughness {
            m.roughness = unit(r);
        } else if let Some(ns) = self.ns {
            // Blinn-Phong exponent 0..1000 -> roughness 1..0.
            m.roughness = unit(1.0 - (ns.max(0.0) / 1000.0).sqrt());
        }
        if let Some(v) = self.metallic {
            m.metallic = unit(v);
        }
        if let Some(e) = self.ke {
            m.emission = e.map(unit);
        }
        if let Some(d) = self.opacity {
            m.tint[3] = unit(d);
        }
        m
    }
}

fn floats<const N: usize>(rest: &[&str]) -> Option<[f32; N]> {
    let v: Vec<f32> = rest.iter().take(N).filter_map(|s| s.parse().ok()).collect();
    match v.len() {
        n if n == N => v.try_into().ok(),
        // `Kd 0.5` means gray.
        1 => Some([v[0]; N]),
        _ => None,
    }
}

/// Texture path of a `map_*` statement: the whole remainder of the line,
/// or the last token when options such as `-s 1 1 1` precede it.
fn map_path(line: &str) -> Option<String> {
    let rest = line.split_once(char::is_whitespace)?.1.trim();
    if rest.is_empty() {
        return None;
    }
    if rest.starts_with('-') {
        rest.split_whitespace().last().map(str::to_owned)
    } else {
        Some(rest.to_owned())
    }
}

/// Parses an MTL library. Unknown statements are ignored.
pub fn read_mtl(text: &str) -> Vec<MtlMaterial> {
    let mut out: Vec<MtlMaterial> = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        let mut it = line.split_whitespace();
        let Some(key) = it.next() else { continue };
        let rest: Vec<&str> = it.collect();
        if key == "newmtl" {
            out.push(MtlMaterial {
                name: line[6..].trim().to_owned(),
                ..Default::default()
            });
            continue;
        }
        let Some(m) = out.last_mut() else { continue };
        match key {
            "Kd" => m.kd = floats::<3>(&rest),
            "Ke" => m.ke = floats::<3>(&rest),
            "d" => m.opacity = floats::<1>(&rest).map(|[d]| d),
            "Tr" => m.opacity = floats::<1>(&rest).map(|[t]| 1.0 - t),
            "Ns" => m.ns = floats::<1>(&rest).map(|[v]| v),
            "Pr" => m.roughness = floats::<1>(&rest).map(|[v]| v),
            "Pm" => m.metallic = floats::<1>(&rest).map(|[v]| v),
            "map_Kd" => m.map_kd = map_path(line),
            _ => {}
        }
    }
    out
}

/// Writes the exported library: a single `palette` material textured with
/// `texture` (the palette PNG's file name). CRLF line endings, like
/// MagicaVoxel.
pub fn write_palette_mtl(texture: &str) -> String {
    [
        EXPORT_HEADER,
        "",
        &format!("newmtl {PALETTE_MATERIAL}"),
        "illum 1",
        "Ka 0.000 0.000 0.000",
        "Kd 1.000 1.000 1.000",
        "Ks 0.000 0.000 0.000",
        &format!("map_Kd {texture}"),
    ]
    .iter()
    .map(|l| format!("{l}\r\n"))
    .collect()
}

/// Palette from a 256×1 palette texture: pixel `p` is color index `p + 1`.
pub fn palette_from_texture(img: &RgbaImage) -> Option<Palette> {
    if (img.width, img.height) != (256, 1) {
        return None;
    }
    let mut colors = vec![Rgba8::BLACK];
    colors.extend((0..255).map(|p| {
        let [r, g, b, _] = img.pixel(p, 0);
        Rgba8::rgb(r, g, b)
    }));
    Some(Palette::new(colors))
}

/// Palette and materials described by an MTL library. `load` returns a
/// texture's decoded image by its `map_Kd` path.
pub fn palette_from_mtl(
    materials: &[MtlMaterial],
    load: &dyn Fn(&str) -> Option<RgbaImage>,
) -> Result<(Palette, Vec<Material>), RzxlError> {
    if materials.is_empty() {
        return Err(RzxlError::Invalid(
            "MTL: the library defines no materials".into(),
        ));
    }
    let texture_palette = materials
        .iter()
        .filter_map(|m| m.map_kd.as_deref())
        .filter_map(load)
        .find_map(|img| palette_from_texture(&img));
    let palette = texture_palette.unwrap_or_else(|| {
        let mut colors = vec![Rgba8::BLACK];
        colors.extend(materials.iter().take(255).map(MtlMaterial::color));
        Palette::new(colors)
    });
    let mats = materials
        .iter()
        .take(crate::MAX_MATERIALS)
        .map(MtlMaterial::to_material)
        .collect();
    Ok((palette, mats))
}

/// C `printf("%g")` formatting (6 significant digits, trailing zeros
/// trimmed), matching the numbers in MagicaVoxel's OBJ files.
pub fn fmt_g(v: f32) -> String {
    if v == 0.0 || !v.is_finite() {
        return "0".to_owned();
    }
    let sci = format!("{v:.5e}");
    let (mantissa, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let trim = |s: &str| -> String {
        if s.contains('.') {
            s.trim_end_matches('0').trim_end_matches('.').to_owned()
        } else {
            s.to_owned()
        }
    };
    if !(-4..6).contains(&exp) {
        let sign = if exp < 0 { '-' } else { '+' };
        format!("{}e{sign}{:02}", trim(mantissa), exp.abs())
    } else {
        trim(&format!("{v:.*}", (5 - exp) as usize))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_g_matches_c_printf() {
        for (v, s) in [
            (0.0, "0"),
            (-0.0, "0"),
            (-1.0, "-1"),
            (0.7, "0.7"),
            (0.1 * 7.0, "0.7"),
            (1.5, "1.5"),
            (-2.980_232_2e-8, "-2.98023e-08"),
            (0.033_203_125, "0.0332031"),
            (0.994_140_6, "0.994141"),
            (0.5, "0.5"),
            (123_456.0, "123456"),
            (1_234_567.0, "1.23457e+06"),
            (0.000_123_4, "0.0001234"),
        ] {
            assert_eq!(fmt_g(v), s, "{v}");
        }
    }

    #[test]
    fn palette_mtl_matches_magicavoxel_layout() {
        let mtl = write_palette_mtl("chr_knight.png");
        assert_eq!(
            mtl,
            "# Rizxel @ Abati Technology\r\n\r\nnewmtl palette\r\nillum 1\r\n\
             Ka 0.000 0.000 0.000\r\nKd 1.000 1.000 1.000\r\nKs 0.000 0.000 0.000\r\n\
             map_Kd chr_knight.png\r\n"
        );
        let back = read_mtl(&mtl);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].name, "palette");
        assert_eq!(back[0].map_kd.as_deref(), Some("chr_knight.png"));
    }

    #[test]
    fn reads_colors_textures_and_pbr() {
        let text = "newmtl Red Brick\nKd 1 0 0\nKe 0.5 0.5 0.5\nd 0.25\nPr 0.3\nPm 1\n\
                    newmtl Gray\nKd 0.5\nNs 1000\nmap_Kd -s 1 1 1 tex/gray.png\n\
                    newmtl Spaced\nmap_Kd my texture.png\n";
        let m = read_mtl(text);
        assert_eq!(m.len(), 3);
        assert_eq!(m[0].name, "Red Brick");
        assert_eq!(m[0].color(), Rgba8::rgb(255, 0, 0));
        let r = m[0].to_material();
        assert_eq!((r.roughness, r.metallic, r.tint[3]), (0.3, 1.0, 0.25));
        assert_eq!(m[1].color(), Rgba8::rgb(128, 128, 128));
        assert_eq!(m[1].to_material().roughness, 0.0);
        assert_eq!(m[1].map_kd.as_deref(), Some("tex/gray.png"));
        assert_eq!(m[2].map_kd.as_deref(), Some("my texture.png"));

        let (pal, mats) = palette_from_mtl(&m, &|_| None).unwrap();
        assert_eq!(pal.len(), 4);
        assert_eq!(pal.get(1), Some(Rgba8::rgb(255, 0, 0)));
        assert_eq!(mats.len(), 3);
    }

    #[test]
    fn palette_texture_maps_pixel_to_index_plus_one() {
        let mut pixels = vec![0u8; 256 * 4];
        pixels[8 * 4..8 * 4 + 4].copy_from_slice(&[255, 204, 153, 255]);
        let img = RgbaImage {
            width: 256,
            height: 1,
            pixels,
        };
        let p = palette_from_texture(&img).unwrap();
        assert_eq!(p.len(), 256);
        assert_eq!(p.get(9), Some(Rgba8::rgb(255, 204, 153)));
    }
}

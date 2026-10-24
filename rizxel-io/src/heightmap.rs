//! Heightmap import: a grayscale image becomes a terrain of voxel columns.
//!
//! - Image X runs along Rizxel +X and image Y (downwards) along +Z, so the
//!   image reads like a map seen from the top.
//! - A pixel's brightness (Rec. 601 luma; transparent pixels count as
//!   black) sets its column height: `round(luma / 255 · max_height)`
//!   voxels, standing on Y = 0. Black columns stay empty.
//! - Images wider or taller than 256 pixels are scaled down to fit.
//! - Colors come from an optional color image of any size (sampled at the
//!   same relative position and reduced to the palette), otherwise from
//!   the default palette's highlighted swatch.

use glam::IVec3;
use rizxel_math::{BoundSize, Palette, Rgba8, Voxel, VoxelVolume};

use crate::interchange::{build_project, ImportLayer};
use crate::png_io::RgbaImage;
use crate::{LayerData, ProjectFile, RzxlError};

/// Heightmap import settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeightmapOptions {
    /// Height of a white pixel, `1..=256` voxels.
    pub max_height: u32,
}

impl Default for HeightmapOptions {
    fn default() -> Self {
        Self { max_height: 32 }
    }
}

fn luma(p: [u8; 4]) -> f32 {
    let [r, g, b, a] = p.map(f32::from);
    (0.299 * r + 0.587 * g + 0.114 * b) * a / 255.0
}

/// Builds a one-layer project from a decoded heightmap (at most 256 per
/// side; see [`crate::image_io::decode_fitted`]) and an optional color
/// image.
pub fn project_from_heightmap(
    height: &RgbaImage,
    color: Option<&RgbaImage>,
    options: HeightmapOptions,
    name: &str,
) -> Result<ProjectFile, RzxlError> {
    let (w, d) = (height.width, height.height);
    if w == 0 || d == 0 || w > BoundSize::MAX || d > BoundSize::MAX {
        return Err(RzxlError::Invalid(format!(
            "heightmap is {w}×{d} pixels; up to {0}×{0} is supported",
            BoundSize::MAX
        )));
    }
    let max_height = options.max_height.clamp(1, BoundSize::MAX);
    let heights: Vec<u32> = (0..d)
        .flat_map(|z| (0..w).map(move |x| (x, z)))
        .map(|(x, z)| (luma(height.pixel(x, z)) / 255.0 * max_height as f32).round() as u32)
        .map(|h| h.min(max_height))
        .collect();
    let top = heights.iter().copied().max().unwrap_or(0);
    if top == 0 {
        return Err(RzxlError::Invalid(
            "the heightmap is black: every column has height 0".into(),
        ));
    }
    let bounds = BoundSize::new(w, top, d);

    // Column colors.
    let sample = |x: u32, z: u32| -> Option<Rgba8> {
        let img = color?;
        let px = ((x as f32 + 0.5) / w as f32 * img.width as f32) as u32;
        let pz = ((z as f32 + 0.5) / d as f32 * img.height as f32) as u32;
        let [r, g, b, _] = img.pixel(px, pz);
        Some(Rgba8::rgb(r, g, b))
    };
    let (palette, value_of): (Palette, Box<dyn Fn(u32, u32) -> Voxel>) = if color.is_some() {
        let mut counts: Vec<(Rgba8, u64)> = Vec::new();
        let mut index = std::collections::HashMap::new();
        for z in 0..d {
            for x in 0..w {
                if heights[(z * w + x) as usize] == 0 {
                    continue;
                }
                let c = sample(x, z).unwrap_or(Rgba8::WHITE);
                let i = *index.entry(c).or_insert_with(|| {
                    counts.push((c, 0));
                    counts.len() - 1
                });
                counts[i].1 += 1;
            }
        }
        let map = rizxel_engine::build_color_map(&counts);
        let palette = map.palette.clone();
        (
            palette,
            Box::new(move |x, z| map.value(sample(x, z).unwrap_or(Rgba8::WHITE))),
        )
    } else {
        let selected = Palette::DEFAULT_SELECTED as Voxel;
        (Palette::default(), Box::new(move |_, _| selected))
    };

    let mut volume = VoxelVolume::new(bounds);
    for z in 0..d {
        for x in 0..w {
            let h = heights[(z * w + x) as usize];
            if h == 0 {
                continue;
            }
            let v = value_of(x, z);
            for y in 0..h {
                volume.set(IVec3::new(x as i32, y as i32, z as i32), v);
            }
        }
    }
    build_project(
        name,
        bounds,
        palette,
        Vec::new(),
        vec![ImportLayer {
            data: LayerData::new(1, name),
            volume: Some(volume),
        }],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::volume_from_runs;

    fn image(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> RgbaImage {
        let pixels = (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .flat_map(|(x, y)| f(x, y))
            .collect();
        RgbaImage {
            width: w,
            height: h,
            pixels,
        }
    }

    #[test]
    fn brightness_sets_column_heights() {
        // A 3×2 ramp: black, gray, white on the top row; white below.
        let h = image(3, 2, |x, y| {
            let v = if y == 1 {
                255
            } else {
                [0, 128, 255][x as usize]
            };
            [v, v, v, 255]
        });
        let p = project_from_heightmap(&h, None, HeightmapOptions { max_height: 10 }, "t").unwrap();
        assert_eq!(p.bounds, BoundSize::new(3, 10, 2));
        let v = volume_from_runs(p.bounds, &p.layers[0].voxels).unwrap();
        let column = |x: i32, z: i32| (0..10).filter(|&y| v.get(IVec3::new(x, y, z)) != 0).count();
        assert_eq!(column(0, 0), 0);
        assert_eq!(column(1, 0), 5);
        assert_eq!(column(2, 0), 10);
        assert_eq!(column(0, 1), 10);
        assert_eq!(
            v.get(IVec3::new(2, 0, 0)) as usize,
            Palette::DEFAULT_SELECTED
        );
    }

    #[test]
    fn color_image_colors_columns() {
        let h = image(2, 1, |_, _| [255; 4]);
        let c = image(4, 2, |x, _| {
            if x < 2 {
                [255, 0, 0, 255]
            } else {
                [0, 0, 255, 255]
            }
        });
        let p =
            project_from_heightmap(&h, Some(&c), HeightmapOptions { max_height: 3 }, "t").unwrap();
        let v = volume_from_runs(p.bounds, &p.layers[0].voxels).unwrap();
        let color = |x: i32| p.palette.get(v.get(IVec3::new(x, 2, 0)) as usize).unwrap();
        assert_eq!(color(0), Rgba8::rgb(255, 0, 0));
        assert_eq!(color(1), Rgba8::rgb(0, 0, 255));
    }

    #[test]
    fn bad_heightmaps_are_refused() {
        let black = image(4, 4, |_, _| [0, 0, 0, 255]);
        assert!(project_from_heightmap(&black, None, HeightmapOptions::default(), "t").is_err());
        let clear = image(4, 4, |_, _| [255, 255, 255, 0]);
        assert!(project_from_heightmap(&clear, None, HeightmapOptions::default(), "t").is_err());
        let wide = image(300, 1, |_, _| [255; 4]);
        assert!(project_from_heightmap(&wide, None, HeightmapOptions::default(), "t").is_err());
        let empty = RgbaImage {
            width: 0,
            height: 0,
            pixels: Vec::new(),
        };
        assert!(project_from_heightmap(&empty, None, HeightmapOptions::default(), "t").is_err());
        // Wild max heights are clamped.
        let white = image(1, 1, |_, _| [255; 4]);
        let p = project_from_heightmap(&white, None, HeightmapOptions { max_height: 9999 }, "t")
            .unwrap();
        assert_eq!(p.bounds.y(), 256);
        let p =
            project_from_heightmap(&white, None, HeightmapOptions { max_height: 0 }, "t").unwrap();
        assert_eq!(p.bounds.y(), 1);
    }
}

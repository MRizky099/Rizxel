//! Export Slices: one PNG per voxel slice along an axis.
//!
//! Every slice covers the whole bounds, so all images have the same size
//! and stack back into the model. Empty cells are transparent; each voxel
//! is `scale × scale` pixels of its palette color. Slice 0 is the lowest
//! coordinate on the axis. Images are oriented as seen from the positive
//! end of the axis, with up (+Y) at the top for X and Z slices:
//!
//! | Axis | Seen from | Image right | Image down |
//! |---|---|---|---|
//! | X | right (+X) | −Z | −Y |
//! | Y | top (+Y) | +X | +Z |
//! | Z | front (+Z) | +X | −Y |

use glam::IVec3;
use rizxel_math::{BoundSize, Palette, VoxelVolume, EMPTY_VOXEL};

use crate::png_io::encode_rgba;
use crate::RzxlError;

/// Largest pixels-per-voxel scale.
pub const MAX_SLICE_SCALE: u32 = 16;

/// Axis the slices are cut along.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SliceAxis {
    X,
    /// Horizontal slices, bottom to top (the usual choice).
    #[default]
    Y,
    Z,
}

impl SliceAxis {
    pub const ALL: [SliceAxis; 3] = [SliceAxis::X, SliceAxis::Y, SliceAxis::Z];

    pub const fn label(self) -> &'static str {
        match self {
            SliceAxis::X => "X",
            SliceAxis::Y => "Y",
            SliceAxis::Z => "Z",
        }
    }

    const fn index(self) -> usize {
        match self {
            SliceAxis::X => 0,
            SliceAxis::Y => 1,
            SliceAxis::Z => 2,
        }
    }

    /// Image size in voxels for `bounds`.
    pub fn image_size(self, bounds: BoundSize) -> (u32, u32) {
        match self {
            SliceAxis::X => (bounds.z(), bounds.y()),
            SliceAxis::Y => (bounds.x(), bounds.z()),
            SliceAxis::Z => (bounds.x(), bounds.y()),
        }
    }

    /// Voxel shown at image cell `(u, v)` of slice `s`.
    fn cell(self, bounds: BoundSize, s: i32, u: i32, v: i32) -> IVec3 {
        let (sy, sz) = (bounds.y() as i32, bounds.z() as i32);
        match self {
            SliceAxis::X => IVec3::new(s, sy - 1 - v, sz - 1 - u),
            SliceAxis::Y => IVec3::new(u, s, v),
            SliceAxis::Z => IVec3::new(u, sy - 1 - v, s),
        }
    }
}

/// One encoded slice.
#[derive(Debug, Clone, PartialEq)]
pub struct Slice {
    /// `stem_y000.png` style file name.
    pub file_name: String,
    pub png: Vec<u8>,
}

/// Slices of `volume` along `axis` at `scale` pixels per voxel (clamped to
/// `1..=MAX_SLICE_SCALE`). Slices are numbered with at least three digits.
pub fn export_slices(
    volume: &VoxelVolume,
    palette: &Palette,
    axis: SliceAxis,
    scale: u32,
    stem: &str,
) -> Result<Vec<Slice>, RzxlError> {
    if volume.is_empty() {
        return Err(RzxlError::Invalid(
            "nothing to export: the visible layers contain no voxels".into(),
        ));
    }
    let bounds = volume.bounds();
    let scale = scale.clamp(1, MAX_SLICE_SCALE);
    let (w, h) = axis.image_size(bounds);
    let count = bounds.0[axis.index()];
    let digits = count.to_string().len().max(3);
    let tag = axis.label().to_ascii_lowercase();
    let (pw, ph) = (w * scale, h * scale);
    (0..count as i32)
        .map(|s| {
            let mut pixels = vec![0u8; (pw * ph * 4) as usize];
            for v in 0..h as i32 {
                for u in 0..w as i32 {
                    let value = volume.get(axis.cell(bounds, s, u, v));
                    if value == EMPTY_VOXEL {
                        continue;
                    }
                    let c = palette
                        .get(value as usize)
                        .unwrap_or(rizxel_math::Rgba8::WHITE);
                    for dy in 0..scale {
                        let row = (v as u32 * scale + dy) * pw;
                        for dx in 0..scale {
                            let i = ((row + u as u32 * scale + dx) * 4) as usize;
                            pixels[i..i + 4].copy_from_slice(&[c.r, c.g, c.b, 255]);
                        }
                    }
                }
            }
            Ok(Slice {
                file_name: format!("{stem}_{tag}{s:0digits$}.png"),
                png: encode_rgba(pw, ph, &pixels)?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::png_io::decode_rgba;

    fn model() -> (VoxelVolume, Palette) {
        let mut v = VoxelVolume::new(BoundSize::new(4, 3, 2));
        v.set(IVec3::new(0, 0, 0), 9);
        v.set(IVec3::new(3, 2, 1), 40);
        (v, Palette::default())
    }

    #[test]
    fn one_png_per_slice_with_the_right_size() {
        let (v, p) = model();
        for (axis, count, size) in [
            (SliceAxis::X, 4, (2, 3)),
            (SliceAxis::Y, 3, (4, 2)),
            (SliceAxis::Z, 2, (4, 3)),
        ] {
            let slices = export_slices(&v, &p, axis, 2, "m").unwrap();
            assert_eq!(slices.len(), count, "{axis:?}");
            let img = decode_rgba(&slices[0].png).unwrap();
            assert_eq!((img.width, img.height), (size.0 * 2, size.1 * 2));
        }
        let names: Vec<String> = export_slices(&v, &p, SliceAxis::Y, 1, "m")
            .unwrap()
            .into_iter()
            .map(|s| s.file_name)
            .collect();
        assert_eq!(names, ["m_y000.png", "m_y001.png", "m_y002.png"]);
    }

    #[test]
    fn pixels_carry_palette_colors_in_the_documented_orientation() {
        let (v, p) = model();
        let px = |axis, slice: usize, x: u32, y: u32| {
            let s = export_slices(&v, &p, axis, 3, "m").unwrap();
            decode_rgba(&s[slice].png).unwrap().pixel(x, y)
        };
        let c9 = p.get(9).unwrap();
        let c40 = p.get(40).unwrap();
        // Top view, bottom slice: voxel (0, 0, 0) is the top-left cell.
        assert_eq!(px(SliceAxis::Y, 0, 1, 1), [c9.r, c9.g, c9.b, 255]);
        assert_eq!(px(SliceAxis::Y, 0, 4, 1), [0, 0, 0, 0]);
        // Front view, back slice: (0, 0, 0) sits at the bottom left.
        assert_eq!(px(SliceAxis::Z, 0, 0, 8), [c9.r, c9.g, c9.b, 255]);
        // Front view, front slice: (3, 2, 1) at the top right.
        assert_eq!(px(SliceAxis::Z, 1, 11, 0), [c40.r, c40.g, c40.b, 255]);
        // Right view, slice x = 3: (3, 2, 1) has z = 1, the front, so it is
        // on the left, at the top.
        assert_eq!(px(SliceAxis::X, 3, 0, 0), [c40.r, c40.g, c40.b, 255]);
    }

    #[test]
    fn empty_volumes_and_wild_scales() {
        let p = Palette::default();
        let empty = VoxelVolume::new(BoundSize::new(2, 2, 2));
        assert!(export_slices(&empty, &p, SliceAxis::Y, 1, "m").is_err());
        let (v, p) = model();
        let s = export_slices(&v, &p, SliceAxis::Y, 0, "m").unwrap();
        assert_eq!(decode_rgba(&s[0].png).unwrap().width, 4);
        let s = export_slices(&v, &p, SliceAxis::Y, 1000, "m").unwrap();
        assert_eq!(decode_rgba(&s[0].png).unwrap().width, 4 * MAX_SLICE_SCALE);
    }
}

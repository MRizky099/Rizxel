//! Core value types: colors, palette and model bounds.

use glam::UVec3;
use serde::{Deserialize, Serialize};

/// Maximum number of colors a palette can hold (MagicaVoxel-compatible size).
pub const MAX_PALETTE_COLORS: usize = 256;

/// One voxel cell: a palette index. `0` is empty, `1..=255` are colors.
pub type Voxel = u8;

/// The empty voxel (palette slot 0 is reserved and never painted).
pub const EMPTY_VOXEL: Voxel = 0;

/// First palette index that can hold a voxel color.
pub const FIRST_COLOR_INDEX: Voxel = 1;

/// True when `v` is the empty voxel.
pub const fn is_empty(v: Voxel) -> bool {
    v == EMPTY_VOXEL
}

/// 8-bit RGBA color, laid out for direct GPU upload.
#[repr(C)]
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Default,
    Serialize,
    Deserialize,
    bytemuck::Pod,
    bytemuck::Zeroable,
)]
pub struct Rgba8 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba8 {
    pub const WHITE: Self = Self::rgb(255, 255, 255);
    pub const BLACK: Self = Self::rgb(0, 0, 0);

    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::new(r, g, b, 255)
    }

    /// Builds a color from a packed `0xRRGGBB` value.
    pub const fn from_hex(hex: u32) -> Self {
        Self::rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
    }

    /// Formats as `#RRGGBB` (alpha omitted).
    pub fn to_hex_string(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }

    /// Parses `#RRGGBB` or `RRGGBB`.
    pub fn parse_hex(s: &str) -> Option<Self> {
        let s = s.trim().trim_start_matches('#');
        if s.len() != 6 {
            return None;
        }
        u32::from_str_radix(s, 16).ok().map(Self::from_hex)
    }

    /// Converts HSL (hue in degrees, saturation/lightness in `0..=1`) to RGB.
    pub fn from_hsl(h: f32, s: f32, l: f32) -> Self {
        let h = h.rem_euclid(360.0) / 360.0;
        let s = s.clamp(0.0, 1.0);
        let l = l.clamp(0.0, 1.0);

        if s == 0.0 {
            let v = (l * 255.0).round() as u8;
            return Self::rgb(v, v, v);
        }

        let q = if l < 0.5 {
            l * (1.0 + s)
        } else {
            l + s - l * s
        };
        let p = 2.0 * l - q;

        // Standard HSL helper: maps a hue offset to a channel value.
        let channel = |mut t: f32| -> u8 {
            t = t.rem_euclid(1.0);
            let v = if t < 1.0 / 6.0 {
                p + (q - p) * 6.0 * t
            } else if t < 0.5 {
                q
            } else if t < 2.0 / 3.0 {
                p + (q - p) * (2.0 / 3.0 - t) * 6.0
            } else {
                p
            };
            (v * 255.0).round() as u8
        };

        Self::rgb(channel(h + 1.0 / 3.0), channel(h), channel(h - 1.0 / 3.0))
    }
}

/// Ordered list of colors available for painting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Palette {
    colors: Vec<Rgba8>,
}

impl Palette {
    /// Creates a palette, truncating to [`MAX_PALETTE_COLORS`].
    pub fn new(mut colors: Vec<Rgba8>) -> Self {
        colors.truncate(MAX_PALETTE_COLORS);
        Self { colors }
    }

    pub fn colors(&self) -> &[Rgba8] {
        &self.colors
    }

    pub fn len(&self) -> usize {
        self.colors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.colors.is_empty()
    }

    pub fn get(&self, index: usize) -> Option<Rgba8> {
        self.colors.get(index).copied()
    }

    /// Replaces a color; returns the previous value if the index is valid.
    pub fn set(&mut self, index: usize, color: Rgba8) -> Option<Rgba8> {
        self.colors
            .get_mut(index)
            .map(|slot| std::mem::replace(slot, color))
    }

    /// True when `index` can be used as a paint color: inside the palette
    /// and not the reserved empty slot `0`.
    pub fn is_selectable(&self, index: usize) -> bool {
        index >= FIRST_COLOR_INDEX as usize && index < self.colors.len()
    }

    /// Index of the swatch highlighted by default (`#38BDF8`).
    pub const DEFAULT_SELECTED: usize = 35;
}

impl Default for Palette {
    /// Mirrors the generator in `docs/rizxel_ui.html`: one greyscale row and
    /// six lightness rows of eight hues each (56 swatches).
    fn default() -> Self {
        const HUES: [f32; 14] = [
            0.0, 15.0, 30.0, 45.0, 60.0, 90.0, 120.0, 150.0, 180.0, 210.0, 240.0, 270.0, 300.0,
            330.0,
        ];
        const LIGHTNESS: [u32; 6] = [20, 40, 50, 60, 70, 80];

        let mut colors = Vec::with_capacity(56);

        for i in 0..8u32 {
            let l = (i as f32 / 7.0 * 100.0).floor() / 100.0;
            colors.push(Rgba8::from_hsl(0.0, 0.0, l));
        }

        for l in LIGHTNESS {
            for i in 0..8u32 {
                let hue = HUES[((i + l / 10) as usize) % HUES.len()];
                colors.push(Rgba8::from_hsl(hue, 0.7, l as f32 / 100.0));
            }
        }

        // The prototype swaps hsl(210, 70%, 60%) for the brand sky blue.
        colors[Self::DEFAULT_SELECTED] = Rgba8::from_hex(0x38BDF8);

        Self { colors }
    }
}

/// Size of the editable model volume ("Bound Model" panel), in voxels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BoundSize(pub UVec3);

impl BoundSize {
    pub const MIN: u32 = 1;
    pub const MAX: u32 = 256;

    /// Creates a bound size clamped to `MIN..=MAX` on every axis.
    pub fn new(x: u32, y: u32, z: u32) -> Self {
        let c = |v: u32| v.clamp(Self::MIN, Self::MAX);
        Self(UVec3::new(c(x), c(y), c(z)))
    }

    pub fn x(self) -> u32 {
        self.0.x
    }

    pub fn y(self) -> u32 {
        self.0.y
    }

    pub fn z(self) -> u32 {
        self.0.z
    }

    /// Total number of voxel cells inside the bounds.
    pub fn volume(self) -> u64 {
        self.0.x as u64 * self.0.y as u64 * self.0.z as u64
    }
}

impl Default for BoundSize {
    fn default() -> Self {
        Self::new(32, 32, 32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_palette_matches_prototype() {
        let p = Palette::default();
        assert_eq!(p.len(), 56);
        assert_eq!(p.get(0), Some(Rgba8::BLACK));
        assert_eq!(p.get(7), Some(Rgba8::WHITE));
        assert_eq!(
            p.get(Palette::DEFAULT_SELECTED),
            Some(Rgba8::from_hex(0x38BDF8))
        );
    }

    #[test]
    fn hex_roundtrip() {
        let c = Rgba8::from_hex(0x38BDF8);
        assert_eq!(c.to_hex_string(), "#38BDF8");
        assert_eq!(Rgba8::parse_hex("#38bdf8"), Some(c));
        assert_eq!(Rgba8::parse_hex("xyz"), None);
    }

    #[test]
    fn hsl_primaries() {
        assert_eq!(Rgba8::from_hsl(0.0, 1.0, 0.5), Rgba8::rgb(255, 0, 0));
        assert_eq!(Rgba8::from_hsl(120.0, 1.0, 0.5), Rgba8::rgb(0, 255, 0));
        assert_eq!(Rgba8::from_hsl(240.0, 1.0, 0.5), Rgba8::rgb(0, 0, 255));
    }

    #[test]
    fn slot_zero_is_not_selectable() {
        let p = Palette::default();
        assert!(!p.is_selectable(0));
        assert!(p.is_selectable(1));
        assert!(p.is_selectable(p.len() - 1));
        assert!(!p.is_selectable(p.len()));
        assert!(is_empty(EMPTY_VOXEL) && !is_empty(FIRST_COLOR_INDEX));
    }

    #[test]
    fn bounds_are_clamped() {
        let b = BoundSize::new(0, 999, 16);
        assert_eq!((b.x(), b.y(), b.z()), (1, 256, 16));
        assert_eq!(BoundSize::default().volume(), 32 * 32 * 32);
    }
}

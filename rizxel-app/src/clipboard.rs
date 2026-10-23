//! App-level voxel clipboard. Colors are stored as RGBA so a paste into a
//! document with another palette maps them onto that palette.

use std::collections::HashMap;

use glam::{IVec3, UVec3};
use rizxel_engine::quantize;
use rizxel_math::{Palette, Rgba8, Voxel, EMPTY_VOXEL};

use crate::document::Document;

/// Copied voxels, relative to the box they came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clipboard {
    /// Cells relative to `origin` (all inside `0..size`) with their color.
    pub cells: Vec<(IVec3, Rgba8)>,
    /// Minimum corner of the copied box in the source document.
    pub origin: IVec3,
    pub size: UVec3,
}

impl Clipboard {
    /// The active layer's selected voxels, or the whole layer without a
    /// selection. `None` when there is nothing to copy (or the layer stores
    /// no voxels).
    pub fn copy(doc: &Document) -> Option<Self> {
        let layer = doc.active_layer().filter(|l| l.kind().stores_voxels())?;
        let palette = &doc.data.palette;
        let color = |v: Voxel| palette.get(v as usize).unwrap_or(Rgba8::WHITE);
        let cells: Vec<(IVec3, Rgba8)> = if layer.selection.is_empty() {
            layer.volume.iter().map(|(p, v)| (p, color(v))).collect()
        } else {
            layer
                .selection
                .iter()
                .map(|p| (p, layer.volume.get(p)))
                .filter(|&(_, v)| v != EMPTY_VOXEL)
                .map(|(p, v)| (p, color(v)))
                .collect()
        };
        let (lo, hi) = cells
            .iter()
            .map(|&(p, _)| (p, p + IVec3::ONE))
            .reduce(|(a0, a1), (b0, b1)| (a0.min(b0), a1.max(b1)))?;
        let mut cells: Vec<(IVec3, Rgba8)> = cells.into_iter().map(|(p, c)| (p - lo, c)).collect();
        cells.sort_by_key(|(p, _)| (p.z, p.y, p.x));
        Some(Self {
            cells,
            origin: lo,
            size: (hi - lo).as_uvec3(),
        })
    }

    /// Box origin that puts the clipboard's bottom center on `cell` (paste
    /// at the cursor).
    pub fn origin_at(&self, cell: IVec3) -> IVec3 {
        bottom_center_origin(self.size.as_ivec3(), cell)
    }

    /// Cells relative to the box with colors mapped onto `palette`: exact
    /// matches first, else the nearest color.
    pub fn mapped(&self, palette: &Palette) -> Vec<(IVec3, Voxel)> {
        let mut map: HashMap<Rgba8, Voxel> = HashMap::new();
        self.cells
            .iter()
            .map(|&(p, c)| {
                let v = *map.entry(c).or_insert_with(|| map_color(palette, c));
                (p, v)
            })
            .collect()
    }
}

/// Origin of a `size` box whose bottom center is `cell`.
pub fn bottom_center_origin(size: IVec3, cell: IVec3) -> IVec3 {
    cell - IVec3::new(size.x / 2, 0, size.z / 2)
}

/// Palette index of `c`: the first exact match (never slot 0), else the
/// nearest color.
pub fn map_color(palette: &Palette, c: Rgba8) -> Voxel {
    palette
        .colors()
        .iter()
        .enumerate()
        .skip(1)
        .find(|(_, &p)| p == c)
        .map_or_else(|| quantize::nearest(palette, c), |(i, _)| i as Voxel)
}

//! # rizxel-math (L0)
//!
//! Foundation layer for Rizxel. Contains plain data types with no knowledge of
//! the engine, file formats or UI: colors, palettes, bound sizes, AABBs,
//! the tiled voxel volume and reference images.

pub mod aabb;
pub mod base64;
pub mod layer;
pub mod reference;
pub mod types;
pub mod volume;

pub use aabb::Aabb;
pub use glam::{IVec3, UVec3, Vec2, Vec3};
pub use layer::{LayerId, LayerKind, Material, ShapeKind, VoxelBox};
pub use reference::{DepthMode, ImageData, RefPlane, ReferenceId, ReferenceImage, SliceFilter};
pub use types::{
    is_empty, BoundSize, Palette, Rgba8, Voxel, EMPTY_VOXEL, FIRST_COLOR_INDEX, MAX_PALETTE_COLORS,
};
pub use volume::{Tile, TilePos, VoxelVolume, TILE_SIZE, TILE_VOLUME};

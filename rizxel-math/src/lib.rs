//! # rizxel-math (L0)
//!
//! Foundation layer for Rizxel. Contains plain data types with no knowledge of
//! the engine, file formats or UI: colors, palettes, bound sizes, AABBs and
//! the tiled voxel volume.

pub mod aabb;
pub mod types;
pub mod volume;

pub use aabb::Aabb;
pub use glam::{IVec3, UVec3, Vec3};
pub use types::{
    is_empty, BoundSize, Palette, Rgba8, Voxel, EMPTY_VOXEL, FIRST_COLOR_INDEX, MAX_PALETTE_COLORS,
};
pub use volume::{Tile, TilePos, VoxelVolume, TILE_SIZE, TILE_VOLUME};

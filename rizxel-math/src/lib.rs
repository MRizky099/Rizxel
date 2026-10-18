//! # rizxel-math (L0)
//!
//! Foundation layer for Rizxel. Contains plain data types with no knowledge of
//! the engine, file formats or UI: colors, palettes, bound sizes and AABBs.

pub mod aabb;
pub mod types;

pub use aabb::Aabb;
pub use glam::{IVec3, UVec3, Vec3};
pub use types::{BoundSize, Palette, Rgba8, MAX_PALETTE_COLORS};

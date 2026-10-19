//! # rizxel-engine (L1)
//!
//! Core voxel logic independent of any UI or file format: tool definitions,
//! greedy meshing of voxel volumes and (from Phase 5) raycasting.

pub mod mesher;
pub mod raycast;
pub mod shapes;
pub mod tools;

pub use mesher::{greedy_mesh, mesh_volume, Mesh};
pub use shapes::rasterize_shape;
pub use tools::{
    BrushOptions, BrushShape, ColorPickerOptions, CursorOptions, DragMode, ExtrudeOptions,
    FillOptions, FuzzySelectOptions, MaskMode, MoveOptions, NamesMode, PlaneMode, PlaneOptions,
    SelectionOptions, ShapeOptions, Symmetry, Tool, ToolOptions, VoxelOp,
};

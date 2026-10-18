//! # rizxel-engine (L1)
//!
//! Core voxel logic independent of any UI or file format: tool definitions,
//! mesh data structures and (from Phase 4/5) meshing and raycasting.

pub mod mesher;
pub mod raycast;
pub mod tools;

pub use mesher::Mesh;
pub use tools::{
    BrushOptions, BrushShape, ColorPickerOptions, CursorOptions, DragMode, ExtrudeOptions,
    FillOptions, FuzzySelectOptions, MaskMode, MoveOptions, NamesMode, PlaneMode, PlaneOptions,
    SelectionOptions, ShapeOptions, Symmetry, Tool, ToolOptions, VoxelOp,
};

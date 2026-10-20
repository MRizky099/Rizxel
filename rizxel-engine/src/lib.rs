//! # rizxel-engine (L1)
//!
//! Core voxel logic independent of any UI or file format: tool definitions,
//! greedy meshing of voxel volumes, raycasting and the tool algorithms.

pub mod mesher;
pub mod raycast;
pub mod shapes;
pub mod tools;

pub use mesher::{greedy_mesh, mesh_volume, Mesh};
pub use raycast::{Ray, RayHit};
pub use shapes::rasterize_shape;
pub use tools::{
    BrushOptions, BrushShape, ColorPickerOptions, CursorOptions, DragMode, ExtrudeOptions,
    FillOptions, FuzzySelectOptions, KeepSide, MaskMode, MoveOptions, NamesMode, PlaneOptions,
    SelectionOptions, ShapeOptions, SliceRequest, Symmetry, Tool, ToolContext, ToolOptions,
    VoxelChange, VoxelOp, WorkPlane,
};

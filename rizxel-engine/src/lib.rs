//! # rizxel-engine (L1)
//!
//! Core voxel logic independent of any UI or file format: tool definitions,
//! greedy meshing of voxel volumes, raycasting, the tool algorithms, and the
//! geometry helpers behind file import/export (export quads, color
//! quantization, mesh voxelization).

pub mod mesher;
pub mod quads;
pub mod quantize;
pub mod raycast;
pub mod shapes;
pub mod tools;
pub mod voxelize;

pub use mesher::{
    greedy_mesh, mesh_tile, mesh_volume, mesh_volume_with, MaterialSlots, Mesh, MeshParams,
    TileMeshCache,
};
pub use quads::{greedy_quads, Quad};
pub use quantize::{build_color_map, nearest, ColorMap};
pub use raycast::{Ray, RayHit};
pub use shapes::rasterize_shape;
pub use tools::{
    BrushOptions, BrushShape, ColorPickerOptions, CursorOptions, DragMode, ExtrudeOptions,
    FillMode, FillOptions, FuzzySelectOptions, KeepSide, MaskMode, MoveOptions, NamesMode,
    PlaneOptions, SelectionOptions, ShapeOptions, SliceRequest, Symmetry, Tool, ToolContext,
    ToolOptions, VoxelChange, VoxelOp, WorkPlane,
};
pub use voxelize::{voxelize, ColorGrid, Triangle, VoxelizeOptions};

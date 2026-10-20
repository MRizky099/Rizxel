//! Per-tool option sets shown in the option bar.
//!
//! Modeled on Goxel's tool panels (`src/tools/*.c`, `tool_gui_*` helpers):
//! brush/shape size and shape, antialiasing, symmetry, mask modes, color
//! thresholds, etc. Each tool keeps its own settings so switching tools does
//! not reset them.

use serde::{Deserialize, Serialize};

use super::Tool;

/// Brush / shape primitive (Goxel `shape_cube`, `shape_sphere`, `shape_cylinder`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum BrushShape {
    #[default]
    Cube,
    Sphere,
    Cylinder,
}

impl BrushShape {
    pub const ALL: [BrushShape; 3] = [BrushShape::Cube, BrushShape::Sphere, BrushShape::Cylinder];

    pub const fn label(self) -> &'static str {
        match self {
            BrushShape::Cube => "Cube",
            BrushShape::Sphere => "Sphere",
            BrushShape::Cylinder => "Cylinder",
        }
    }
}

/// How a new selection combines with the existing one (Goxel `tool_gui_mask_mode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum MaskMode {
    #[default]
    Set,
    Add,
    Subtract,
}

impl MaskMode {
    pub const ALL: [MaskMode; 3] = [MaskMode::Set, MaskMode::Add, MaskMode::Subtract];

    pub const fn label(self) -> &'static str {
        match self {
            MaskMode::Set => "Set",
            MaskMode::Add => "Add",
            MaskMode::Subtract => "Sub",
        }
    }
}

/// Move tool drag behavior (Goxel `tool_gui_drag_mode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum DragMode {
    #[default]
    Move,
    Resize,
}

impl DragMode {
    pub const ALL: [DragMode; 2] = [DragMode::Move, DragMode::Resize];

    pub const fn label(self) -> &'static str {
        match self {
            DragMode::Move => "Move",
            DragMode::Resize => "Resize",
        }
    }
}

/// When the Cursor tool shows layer names (Goxel cursor "Show names").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum NamesMode {
    #[default]
    OnHover,
    Always,
}

impl NamesMode {
    pub const ALL: [NamesMode; 2] = [NamesMode::OnHover, NamesMode::Always];

    pub const fn label(self) -> &'static str {
        match self {
            NamesMode::OnHover => "On Hover",
            NamesMode::Always => "Always",
        }
    }
}

/// Mirror axes shared by the drawing tools (Goxel `tool_gui_symmetry`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Symmetry {
    pub x: bool,
    pub y: bool,
    pub z: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CursorOptions {
    pub show_names: NamesMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrushOptions {
    pub shape: BrushShape,
    /// Diameter per axis, in voxels.
    pub size: [u32; 3],
    /// Paint only voxels on the surface under the cursor.
    pub surface_paint: bool,
    /// Edge softness: 0 = hard edges.
    pub antialiasing: u32,
    pub symmetry: Symmetry,
}

impl BrushOptions {
    pub const MAX_SIZE: u32 = 128;
    pub const MAX_ANTIALIASING: u32 = 16;
}

impl Default for BrushOptions {
    fn default() -> Self {
        Self {
            shape: BrushShape::Cube,
            size: [1; 3],
            surface_paint: false,
            antialiasing: 0,
            symmetry: Symmetry::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ShapeOptions {
    pub shape: BrushShape,
    /// Keep the shape on the plane where the drag started.
    pub planar: bool,
    pub antialiasing: u32,
    pub symmetry: Symmetry,
}

/// Which half of the model survives a Slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum KeepSide {
    /// Keep voxels with `coord >= cut`.
    #[default]
    Positive,
    /// Keep voxels with `coord < cut`.
    Negative,
}

impl KeepSide {
    pub const ALL: [KeepSide; 2] = [KeepSide::Positive, KeepSide::Negative];

    pub const fn label(self) -> &'static str {
        match self {
            KeepSide::Positive => "+ side",
            KeepSide::Negative => "- side",
        }
    }

    pub const fn flipped(self) -> Self {
        match self {
            KeepSide::Positive => KeepSide::Negative,
            KeepSide::Negative => KeepSide::Positive,
        }
    }

    /// True when a voxel at `coord` survives a cut at `cut`.
    pub const fn keeps(self, coord: i32, cut: i32) -> bool {
        match self {
            KeepSide::Positive => coord >= cut,
            KeepSide::Negative => coord < cut,
        }
    }
}

/// Options for the Plane tool. The plane itself (axis, position,
/// visibility, slice) is per-document view state: see `WorkPlane`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PlaneOptions {
    /// Close the opening left by "Cut" with the current color.
    pub cap: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct MoveOptions {
    pub drag_mode: DragMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ColorPickerOptions {
    /// Sample the topmost visible voxel across all layers instead of the active one.
    pub sample_all_layers: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SelectionOptions {
    pub mask: MaskMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FuzzySelectOptions {
    /// Select matching voxels anywhere in the layer, not only connected ones.
    pub entire_layer: bool,
    /// Stop at voxels whose color differs by more than `threshold`.
    pub use_color: bool,
    pub threshold: u8,
    pub mask: MaskMode,
}

impl Default for FuzzySelectOptions {
    fn default() -> Self {
        Self {
            entire_layer: false,
            use_color: true,
            threshold: 0,
            mask: MaskMode::Set,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtrudeOptions {
    /// Find the face region in the active layer only (otherwise in every
    /// visible layer). Writes always go to the active layer.
    pub confine_to_layer: bool,
    pub use_color: bool,
    pub threshold: u8,
    /// Depth of a click without drag; dragging adds to it.
    pub depth: u32,
}

impl ExtrudeOptions {
    pub const MAX_DEPTH: u32 = 256;
}

impl Default for ExtrudeOptions {
    fn default() -> Self {
        Self {
            confine_to_layer: false,
            use_color: false,
            threshold: 0,
            depth: 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FillOptions {
    pub current_layer_only: bool,
    pub threshold: u8,
}

impl Default for FillOptions {
    fn default() -> Self {
        Self {
            current_layer_only: true,
            threshold: 0,
        }
    }
}

/// Settings of every tool, kept side by side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ToolOptions {
    pub cursor: CursorOptions,
    pub brush: BrushOptions,
    pub shape: ShapeOptions,
    pub plane: PlaneOptions,
    pub moving: MoveOptions,
    pub color_picker: ColorPickerOptions,
    pub selection: SelectionOptions,
    pub fuzzy_select: FuzzySelectOptions,
    pub extrude: ExtrudeOptions,
    pub fill: FillOptions,
}

impl Tool {
    /// Whether the Add/Subtract/Join/Paint operation applies to this tool.
    pub const fn uses_voxel_op(self) -> bool {
        matches!(self, Tool::Brush | Tool::Shape)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_are_independent_per_tool() {
        let mut opts = ToolOptions::default();
        opts.brush.shape = BrushShape::Sphere;
        assert_eq!(opts.shape.shape, BrushShape::Cube);
        assert!(!opts.plane.cap);
    }

    #[test]
    fn only_drawing_tools_use_voxel_op() {
        assert!(Tool::Brush.uses_voxel_op());
        assert!(!Tool::Plane.uses_voxel_op());
        assert!(!Tool::Fill.uses_voxel_op());
        assert!(!Tool::Selection.uses_voxel_op());
    }
}

//! Tool, operation and tool-option definitions, plus the Phase 5a tool
//! algorithms.
//!
//! Every algorithm is a pure function from a read-only view of the volume
//! (and the pick result) to a list of [`VoxelChange`]s. The application
//! layer applies those changes inside an undo transaction, so one drag is
//! one undo step. Selection-driven tools (Selection, Fuzzy Select, Fill,
//! Shape) arrive in Phase 5b.

mod brush;
mod extrude;
mod moving;
mod options;
mod pick;
mod quality;
mod slice;
mod symmetry;
mod work_plane;

use glam::IVec3;
use rizxel_math::{BoundSize, Voxel, VoxelVolume};
use serde::{Deserialize, Serialize};

pub use brush::{brush_center, brush_positions, brush_stroke, line_cells, stamp_shape};
pub use extrude::{extrude_changes, extrude_region};
pub use moving::{drag_axis, move_changes, resize_bounds};
pub use options::*;
pub use pick::pick_voxel;
pub use quality::{apply_op, filter_placeable};
pub use slice::{slice_layers, slice_volume, SliceRequest};
pub use symmetry::{apply_symmetry, mirror};
pub use work_plane::WorkPlane;

use crate::raycast::RayHit;

/// One voxel write: `old` is the value before, `new` the value after.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VoxelChange {
    pub pos: IVec3,
    pub old: Voxel,
    pub new: Voxel,
}

/// Inputs shared by the drawing tools.
#[derive(Debug, Clone, Copy)]
pub struct ToolContext<'a> {
    /// Layer being edited (read-only; changes are returned, not applied).
    pub volume: &'a VoxelVolume,
    pub bounds: BoundSize,
    /// Palette index written by add/paint operations.
    pub color: Voxel,
    pub op: VoxelOp,
    pub options: &'a ToolOptions,
    pub hit: Option<RayHit>,
}

/// Editing tools shown in the left toolbar, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Tool {
    Cursor,
    Brush,
    Shape,
    #[default]
    Plane,
    Move,
    ColorPicker,
    Selection,
    FuzzySelect,
    Extrude,
    Fill,
}

impl Tool {
    /// Primary drawing tools (above the toolbar separator).
    pub const PRIMARY: [Tool; 4] = [Tool::Cursor, Tool::Brush, Tool::Shape, Tool::Plane];

    /// Secondary manipulation tools (below the toolbar separator).
    pub const SECONDARY: [Tool; 6] = [
        Tool::Move,
        Tool::ColorPicker,
        Tool::Selection,
        Tool::FuzzySelect,
        Tool::Extrude,
        Tool::Fill,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Tool::Cursor => "Cursor",
            Tool::Brush => "Brush",
            Tool::Shape => "Shape",
            Tool::Plane => "Plane",
            Tool::Move => "Move",
            Tool::ColorPicker => "Color Picker",
            Tool::Selection => "Selection",
            Tool::FuzzySelect => "Fuzzy Select",
            Tool::Extrude => "Extrude",
            Tool::Fill => "Fill",
        }
    }

    /// Every tool, in toolbar order.
    pub const ALL: [Tool; 10] = [
        Tool::Cursor,
        Tool::Brush,
        Tool::Shape,
        Tool::Plane,
        Tool::Move,
        Tool::ColorPicker,
        Tool::Selection,
        Tool::FuzzySelect,
        Tool::Extrude,
        Tool::Fill,
    ];

    /// Single-letter keyboard shortcut (Goxel / Photoshop conventions:
    /// B brush, S shape, P plane, M move, C color picker, R rectangle
    /// select, W magic wand, E extrude, F fill, V cursor).
    pub const fn shortcut(self) -> char {
        match self {
            Tool::Cursor => 'V',
            Tool::Brush => 'B',
            Tool::Shape => 'S',
            Tool::Plane => 'P',
            Tool::Move => 'M',
            Tool::ColorPicker => 'C',
            Tool::Selection => 'R',
            Tool::FuzzySelect => 'W',
            Tool::Extrude => 'E',
            Tool::Fill => 'F',
        }
    }

    /// Short usage hint displayed in the status bar.
    pub const fn hint(self) -> &'static str {
        match self {
            Tool::Cursor => "Hover to inspect a voxel. Right-drag to orbit, middle-drag to pan.",
            Tool::Brush => "Click and drag to paint. Shift: erase, Alt: pick color, Esc: cancel.",
            Tool::Shape => "Click and drag to draw a box-shaped volume of voxels (Phase 5b).",
            Tool::Plane => "Click a face to place the work plane; other tools then draw on it.",
            Tool::Move => "Drag to move the active layer (Resize: drag to grow the bounds).",
            Tool::ColorPicker => {
                "Click a voxel to pick its color. Ctrl+click: copy it into the swatch."
            }
            Tool::Selection => "Click and drag to select a box region (Phase 5b).",
            Tool::FuzzySelect => "Click to select connected voxels of the same color (Phase 5b).",
            Tool::Extrude => {
                "Click or drag a face to extrude it. Shift: carve inward, Esc: cancel."
            }
            Tool::Fill => "Click to flood-fill connected voxels with the active color (Phase 5b).",
        }
    }

    /// Hint for the Plane tool while Slice is on.
    pub const PLANE_SLICE_HINT: &'static str =
        "Drag to move the slice plane along its normal. Click a face to move it there.";

    /// Status bar hint taking the slice state into account.
    pub fn hint_with(self, slicing: bool) -> &'static str {
        match self {
            Tool::Plane if slicing => Self::PLANE_SLICE_HINT,
            _ => self.hint(),
        }
    }

    /// True for the tools implemented in Phase 5a.
    pub const fn is_available(self) -> bool {
        !matches!(
            self,
            Tool::Shape | Tool::Selection | Tool::FuzzySelect | Tool::Fill
        )
    }
}

/// How a tool affects the voxel volume.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum VoxelOp {
    #[default]
    Add,
    Subtract,
    Join,
    Paint,
}

impl VoxelOp {
    pub const ALL: [VoxelOp; 4] = [
        VoxelOp::Add,
        VoxelOp::Subtract,
        VoxelOp::Join,
        VoxelOp::Paint,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            VoxelOp::Add => "Add",
            VoxelOp::Subtract => "Subtract",
            VoxelOp::Join => "Join",
            VoxelOp::Paint => "Paint",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcuts_are_unique_letters() {
        for (i, a) in Tool::ALL.iter().enumerate() {
            assert!(a.shortcut().is_ascii_uppercase());
            for b in &Tool::ALL[i + 1..] {
                assert_ne!(a.shortcut(), b.shortcut(), "{a:?} / {b:?}");
            }
        }
        let toolbar: Vec<Tool> = Tool::PRIMARY
            .iter()
            .chain(&Tool::SECONDARY)
            .copied()
            .collect();
        assert_eq!(toolbar, Tool::ALL);
    }

    #[test]
    fn toolbar_lists_every_tool_once() {
        let all: Vec<Tool> = Tool::PRIMARY
            .iter()
            .chain(&Tool::SECONDARY)
            .copied()
            .collect();
        assert_eq!(all.len(), 10);
        for (i, a) in all.iter().enumerate() {
            assert!(!all[i + 1..].contains(a), "duplicate tool {a:?}");
        }
    }

    #[test]
    fn plane_is_default_with_prototype_hint() {
        assert_eq!(Tool::default(), Tool::Plane);
        assert_eq!(Tool::Plane.hint_with(false), Tool::Plane.hint());
        assert_eq!(Tool::Plane.hint_with(true), Tool::PLANE_SLICE_HINT);
        assert_eq!(Tool::Brush.hint_with(true), Tool::Brush.hint());
    }
}

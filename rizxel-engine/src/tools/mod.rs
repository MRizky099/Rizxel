//! Tool, operation and tool-option definitions, plus the tool algorithms.
//!
//! Every algorithm is a pure function from a read-only view of the volume
//! (and the pick result) to a list of [`VoxelChange`]s or of cells. The
//! application layer applies those changes inside an undo transaction, so
//! one drag is one undo step. Selection tools (Selection, Fuzzy Select)
//! return cells; the selection type lives in the application layer.

mod boolean;
mod brush;
mod components;
mod core_ops;
mod extrude;
mod fill;
mod fuzzy;
mod line;
mod moving;
mod options;
mod pick;
mod quality;
mod select;
mod shape;
mod slice;
mod symmetry;
mod transform;
mod work_plane;

use glam::IVec3;
use rizxel_math::{BoundSize, Voxel, VoxelVolume};
use serde::{Deserialize, Serialize};

pub use boolean::{boolean_changes, BooleanOp};
pub use brush::{brush_center, brush_positions, brush_stroke, line_cells, stamp_shape};
pub use components::connected_components;
pub use core_ops::{fill_hollows, hollow, hollow_advanced, MAX_CORE_CELLS};
pub use extrude::{extrude_changes, extrude_region, extruded_cells, selection_region};
pub use fill::{fill_cells, FillError};
pub use fuzzy::fuzzy_select;
pub use line::{
    bezier_at, bezier_path, connect, line_cells_stamped, line_path, line_stroke, pixel_perfect,
    polyline_path, solid,
};
pub use moving::{drag_axis, move_changes, move_selection_changes, resize_bounds};
pub use options::*;
pub use pick::pick_voxel;
pub use quality::{
    apply_op, color_distance, colors_match, filter_placeable, flood, FloodCapped, MAX_FLOOD_CELLS,
    NEIGHBORS_6,
};
pub use select::{
    box_cells, box_face_hit, fit_box, move_box, resize_face, select_box, selection_box,
    MAX_LIVE_BOX,
};
pub use shape::{shape_box, shape_cells, shape_cells_toward};
pub use slice::{slice_layers, slice_volume, SliceRequest};
pub use symmetry::{apply_symmetry, mirror};
pub use transform::{
    apply_transform, cells_box, flip, mirror as mirror_changes, relocate_changes, rotate90,
    scale_cells, transform_cells, Transform, TransformParams, MAX_SCALE, MIN_SCALE,
};
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
    Line,
    Shape,
    #[default]
    Plane,
    Move,
    Transform,
    ColorPicker,
    Selection,
    FuzzySelect,
    Extrude,
    Fill,
}

impl Tool {
    /// Primary drawing tools (above the toolbar separator).
    pub const PRIMARY: [Tool; 5] = [
        Tool::Cursor,
        Tool::Brush,
        Tool::Line,
        Tool::Shape,
        Tool::Plane,
    ];

    /// Secondary manipulation tools (below the toolbar separator).
    pub const SECONDARY: [Tool; 7] = [
        Tool::Move,
        Tool::Transform,
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
            Tool::Line => "Line",
            Tool::Shape => "Shape",
            Tool::Plane => "Plane",
            Tool::Move => "Move",
            Tool::Transform => "Transform",
            Tool::ColorPicker => "Color Picker",
            Tool::Selection => "Selection",
            Tool::FuzzySelect => "Fuzzy Select",
            Tool::Extrude => "Extrude",
            Tool::Fill => "Fill",
        }
    }

    /// Every tool, in toolbar order.
    pub const ALL: [Tool; 12] = [
        Tool::Cursor,
        Tool::Brush,
        Tool::Line,
        Tool::Shape,
        Tool::Plane,
        Tool::Move,
        Tool::Transform,
        Tool::ColorPicker,
        Tool::Selection,
        Tool::FuzzySelect,
        Tool::Extrude,
        Tool::Fill,
    ];

    /// Single-letter keyboard shortcut (Goxel / Photoshop conventions:
    /// B brush, L line, S shape, P plane, M move, T transform, C color
    /// picker, R rectangle select, W magic wand, E extrude, F fill, V
    /// cursor).
    pub const fn shortcut(self) -> char {
        match self {
            Tool::Cursor => 'V',
            Tool::Brush => 'B',
            Tool::Line => 'L',
            Tool::Shape => 'S',
            Tool::Plane => 'P',
            Tool::Move => 'M',
            Tool::Transform => 'T',
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
            Tool::Brush => {
                "Click and drag to paint. Ctrl+click: line from the last click. Shift: erase, Alt: pick color."
            }
            Tool::Line => {
                "Drag a line. Polyline: click points, Enter or right-click ends. Curve: drag, then drag to bend. Shift: erase."
            }
            Tool::Shape => {
                "Drag on a face to draw a cube, sphere, cylinder, pyramid or cone. Shift: carve, Esc: cancel."
            }
            Tool::Plane => "Click a face to place the work plane; other tools then draw on it.",
            Tool::Move => {
                "Drag to move the selection (or the whole layer). Resize: drag to grow the bounds."
            }
            Tool::Transform => {
                "Drag the rings to rotate, arrows and corners to scale, the box to move. Shift/Ctrl: finer steps. Enter: apply, Esc: cancel."
            }
            Tool::ColorPicker => {
                "Click a voxel to pick its color. Ctrl+click: copy it into the swatch."
            }
            Tool::Selection => {
                "Click or drag a box; drag its arrows or faces to adjust it. Enter: apply, Esc: cancel. Shift: add, Ctrl: subtract."
            }
            Tool::FuzzySelect => {
                "Click to select connected voxels of a similar color. Shift: add, Ctrl: subtract."
            }
            Tool::Extrude => {
                "Click or drag a face (or the selection) to extrude it. Shift: carve inward."
            }
            Tool::Fill => {
                "Click to flood-fill with the active color (inside the selection if any)."
            }
        }
    }

    /// Hint for the Plane tool while Slice is on.
    pub const PLANE_SLICE_HINT: &'static str =
        "Drag to move the slice along its normal. Click a voxel to slice there.";

    /// Status bar hint taking the slice state into account.
    pub fn hint_with(self, slicing: bool) -> &'static str {
        match self {
            Tool::Plane if slicing => Self::PLANE_SLICE_HINT,
            _ => self.hint(),
        }
    }

    /// Tools that build a selection and use the mask modifiers.
    pub const fn selects(self) -> bool {
        matches!(self, Tool::Selection | Tool::FuzzySelect)
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
        assert_eq!(all.len(), Tool::ALL.len());
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

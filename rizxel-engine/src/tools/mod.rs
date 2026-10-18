//! Tool, operation and tool-option definitions.
//!
//! Only metadata lives here for Phase 2. Algorithms (plane slicing, extrude,
//! fill) are implemented in Phase 5.

mod options;

use serde::{Deserialize, Serialize};

pub use options::*;

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

    /// Short usage hint displayed in the status bar.
    pub const fn hint(self) -> &'static str {
        match self {
            Tool::Cursor => "Click to inspect a voxel. Right-drag to orbit, middle-drag to pan.",
            Tool::Brush => "Click and drag to paint voxels one by one.",
            Tool::Shape => "Click and drag to draw a box-shaped volume of voxels.",
            Tool::Plane => "Click and drag to create a flat plane of voxels.",
            Tool::Move => "Drag to move the selected voxels.",
            Tool::ColorPicker => "Click a voxel to pick its color.",
            Tool::Selection => "Click and drag to select a box region.",
            Tool::FuzzySelect => "Click to select connected voxels of the same color.",
            Tool::Extrude => "Drag a face to extrude it along its normal.",
            Tool::Fill => "Click to flood-fill connected voxels with the active color.",
        }
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

/// Reference frame for the plane offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum PlaneMode {
    /// Offset is relative to the clicked face.
    #[default]
    Relative,
    /// Offset is an absolute coordinate on the plane axis.
    Absolute,
}

impl PlaneMode {
    pub const ALL: [PlaneMode; 2] = [PlaneMode::Relative, PlaneMode::Absolute];

    pub const fn label(self) -> &'static str {
        match self {
            PlaneMode::Relative => "Relative",
            PlaneMode::Absolute => "Absolute",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(
            Tool::Plane.hint(),
            "Click and drag to create a flat plane of voxels."
        );
    }
}

//! Undo/redo history based on reversible edit commands.

use std::collections::HashMap;

use glam::IVec3;
use rizxel_math::{BoundSize, Rgba8, Voxel};

use crate::document::{DocData, Layer};

/// Maximum number of undo steps kept per document.
const MAX_UNDO: usize = 512;

/// One voxel write inside a layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VoxelChange {
    pub pos: IVec3,
    pub old: Voxel,
    pub new: Voxel,
}

/// A voxel removed from a layer when the bounds shrank, kept so the shrink
/// can be undone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClippedVoxel {
    pub layer: usize,
    pub pos: IVec3,
    pub value: Voxel,
}

/// A reversible change to [`DocData`]. Each variant stores enough information
/// to both apply and revert itself.
#[derive(Debug, Clone, PartialEq)]
pub enum Edit {
    InsertLayer {
        index: usize,
        layer: Layer,
    },
    RemoveLayer {
        index: usize,
        layer: Layer,
    },
    RenameLayer {
        index: usize,
        old: String,
        new: String,
    },
    SetLayerVisible {
        index: usize,
        old: bool,
        new: bool,
    },
    SetLayerLocked {
        index: usize,
        old: bool,
        new: bool,
    },
    /// Resizes the bounds and every layer volume. `clipped` holds the voxels
    /// the shrink removed (restored on revert).
    SetBounds {
        old: BoundSize,
        new: BoundSize,
        clipped: Vec<ClippedVoxel>,
    },
    SetPaletteColor {
        index: usize,
        old: Rgba8,
        new: Rgba8,
    },
    /// Voxel writes to one layer. Built without no-op changes.
    Voxels {
        layer: usize,
        changes: Vec<VoxelChange>,
    },
}

impl Edit {
    pub fn apply(&self, data: &mut DocData) {
        match self {
            Edit::InsertLayer { index, layer } => data.layers.insert(*index, layer.clone()),
            Edit::RemoveLayer { index, .. } => {
                data.layers.remove(*index);
            }
            Edit::RenameLayer { index, new, .. } => data.layers[*index].name = new.clone(),
            Edit::SetLayerVisible { index, new, .. } => data.layers[*index].visible = *new,
            Edit::SetLayerLocked { index, new, .. } => data.layers[*index].locked = *new,
            Edit::SetBounds { new, clipped, .. } => {
                set_bounds(data, *new);
                // A merged drag may have clipped voxels that the final size
                // would keep; they were gone in the live document, so redo
                // removes them too.
                for c in clipped {
                    if let Some(l) = data.layers.get_mut(c.layer) {
                        l.volume.clear_at(c.pos);
                    }
                }
            }
            Edit::SetPaletteColor { index, new, .. } => {
                data.palette.set(*index, *new);
            }
            Edit::Voxels { layer, changes } => {
                if let Some(l) = data.layers.get_mut(*layer) {
                    for c in changes {
                        l.volume.set(c.pos, c.new);
                    }
                }
            }
        }
    }

    pub fn revert(&self, data: &mut DocData) {
        match self {
            Edit::InsertLayer { index, .. } => {
                data.layers.remove(*index);
            }
            Edit::RemoveLayer { index, layer } => data.layers.insert(*index, layer.clone()),
            Edit::RenameLayer { index, old, .. } => data.layers[*index].name = old.clone(),
            Edit::SetLayerVisible { index, old, .. } => data.layers[*index].visible = *old,
            Edit::SetLayerLocked { index, old, .. } => data.layers[*index].locked = *old,
            Edit::SetBounds { old, clipped, .. } => {
                set_bounds(data, *old);
                for c in clipped {
                    if let Some(l) = data.layers.get_mut(c.layer) {
                        l.volume.set(c.pos, c.value);
                    }
                }
            }
            Edit::SetPaletteColor { index, old, .. } => {
                data.palette.set(*index, *old);
            }
            Edit::Voxels { layer, changes } => {
                if let Some(l) = data.layers.get_mut(*layer) {
                    for c in changes.iter().rev() {
                        l.volume.set(c.pos, c.old);
                    }
                }
            }
        }
    }

    /// Tries to fold `next` into `self` (same kind and target). On success
    /// `self` spans from its original `old` to `next.new`.
    fn try_merge(&mut self, next: &Edit) -> bool {
        match (self, next) {
            (
                Edit::SetBounds { new, clipped, .. },
                Edit::SetBounds {
                    new: n, clipped: c, ..
                },
            ) => {
                *new = *n;
                clipped.extend_from_slice(c);
                true
            }
            (
                Edit::Voxels { layer, changes },
                Edit::Voxels {
                    layer: l,
                    changes: next_changes,
                },
            ) if layer == l => {
                merge_voxel_changes(changes, next_changes);
                true
            }
            (
                Edit::SetPaletteColor { index, new, .. },
                Edit::SetPaletteColor {
                    index: i, new: n, ..
                },
            ) if index == i => {
                *new = *n;
                true
            }
            _ => false,
        }
    }
}

/// Sets the document bounds and resizes every layer volume to match.
fn set_bounds(data: &mut DocData, bounds: BoundSize) {
    data.bounds = bounds;
    for layer in &mut data.layers {
        layer.volume.resize(bounds);
    }
}

/// Folds `next` into `acc`: a position changed twice keeps its first `old`
/// and its last `new`; positions that end up unchanged are dropped.
pub fn merge_voxel_changes(acc: &mut Vec<VoxelChange>, next: &[VoxelChange]) {
    let mut index: HashMap<IVec3, usize> =
        acc.iter().enumerate().map(|(i, c)| (c.pos, i)).collect();
    for c in next {
        match index.get(&c.pos) {
            Some(&i) => acc[i].new = c.new,
            None => {
                index.insert(c.pos, acc.len());
                acc.push(*c);
            }
        }
    }
    acc.retain(|c| c.old != c.new);
}

/// Linear undo/redo stack.
#[derive(Debug, Default)]
pub struct History {
    undo: Vec<Edit>,
    redo: Vec<Edit>,
}

impl History {
    /// Records an already-applied edit. Clears the redo stack.
    pub fn push(&mut self, edit: Edit, merge: bool) {
        self.redo.clear();
        if merge {
            if let Some(last) = self.undo.last_mut() {
                if last.try_merge(&edit) {
                    return;
                }
            }
        }
        self.undo.push(edit);
        if self.undo.len() > MAX_UNDO {
            self.undo.remove(0);
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self, data: &mut DocData) -> bool {
        let Some(edit) = self.undo.pop() else {
            return false;
        };
        edit.revert(data);
        self.redo.push(edit);
        true
    }

    pub fn redo(&mut self, data: &mut DocData) -> bool {
        let Some(edit) = self.redo.pop() else {
            return false;
        };
        edit.apply(data);
        self.undo.push(edit);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rizxel_math::Palette;

    fn data() -> DocData {
        DocData {
            palette: Palette::default(),
            layers: vec![Layer::new("A", BoundSize::default())],
            bounds: BoundSize::default(),
        }
    }

    fn change(x: i32, old: Voxel, new: Voxel) -> VoxelChange {
        VoxelChange {
            pos: IVec3::new(x, 0, 0),
            old,
            new,
        }
    }

    #[test]
    fn voxel_edits_apply_and_revert() {
        let mut d = data();
        let e = Edit::Voxels {
            layer: 0,
            changes: vec![change(0, 0, 3), change(1, 0, 4)],
        };
        e.apply(&mut d);
        assert_eq!(d.layers[0].volume.voxel_count(), 2);
        assert_eq!(d.layers[0].volume.get(IVec3::X), 4);
        e.revert(&mut d);
        assert!(d.layers[0].volume.is_empty());
    }

    #[test]
    fn voxel_edits_merge_per_layer() {
        let mut h = History::default();
        let voxels = |layer, changes| Edit::Voxels { layer, changes };
        h.push(voxels(0, vec![change(0, 0, 3), change(1, 0, 3)]), false);
        // Repaints x=0 and adds x=2: same layer, one step.
        h.push(voxels(0, vec![change(0, 3, 5), change(2, 0, 3)]), true);
        // Restores x=1: the change cancels out and is dropped.
        h.push(voxels(0, vec![change(1, 3, 0)]), true);
        // Another layer never merges.
        h.push(voxels(1, vec![change(0, 0, 1)]), true);
        assert_eq!(h.undo.len(), 2);
        assert_eq!(h.undo[0], voxels(0, vec![change(0, 0, 5), change(2, 0, 3)]));
    }

    #[test]
    fn bounds_shrink_restores_clipped_voxels() {
        let mut d = data();
        d.layers[0].volume.set(IVec3::new(20, 0, 0), 7);
        let small = BoundSize::new(10, 32, 32);
        let e = Edit::SetBounds {
            old: d.bounds,
            new: small,
            clipped: vec![ClippedVoxel {
                layer: 0,
                pos: IVec3::new(20, 0, 0),
                value: 7,
            }],
        };
        e.apply(&mut d);
        assert_eq!(d.layers[0].volume.bounds(), small);
        assert!(d.layers[0].volume.is_empty());
        e.revert(&mut d);
        assert_eq!(d.layers[0].volume.bounds(), BoundSize::default());
        assert_eq!(d.layers[0].volume.get(IVec3::new(20, 0, 0)), 7);
    }

    #[test]
    fn new_edit_clears_redo() {
        let mut d = data();
        let mut h = History::default();
        let e = Edit::SetBounds {
            old: d.bounds,
            new: BoundSize::new(1, 1, 1),
            clipped: Vec::new(),
        };
        e.apply(&mut d);
        h.push(e, false);
        h.undo(&mut d);
        assert!(h.can_redo());

        let e = Edit::RenameLayer {
            index: 0,
            old: "A".into(),
            new: "B".into(),
        };
        e.apply(&mut d);
        h.push(e, false);
        assert!(!h.can_redo());
    }

    #[test]
    fn palette_merge_only_for_same_index() {
        let mut h = History::default();
        let c = Rgba8::WHITE;
        h.push(
            Edit::SetPaletteColor {
                index: 0,
                old: c,
                new: c,
            },
            false,
        );
        h.push(
            Edit::SetPaletteColor {
                index: 1,
                old: c,
                new: c,
            },
            true,
        );
        h.push(
            Edit::SetPaletteColor {
                index: 1,
                old: c,
                new: c,
            },
            true,
        );
        assert_eq!(h.undo.len(), 2);
    }
}

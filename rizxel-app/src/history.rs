//! Undo/redo history based on reversible edit commands, with transactions
//! that fold a whole tool drag into one undo step.

use std::collections::{BTreeMap, HashMap};

use glam::IVec3;
use rizxel_math::{BoundSize, Material, Palette, Rgba8, Voxel};

use crate::document::{DocData, Layer, LayerProps};
use crate::selection::Selection;

/// Voxel diffs are produced by the engine's tool algorithms.
pub use rizxel_engine::VoxelChange;
use rizxel_engine::WorkPlane;

/// Maximum number of undo steps kept per document.
const MAX_UNDO: usize = 512;

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
    /// Reorders a layer: removes it at `from` and re-inserts it at `to`.
    MoveLayer {
        from: usize,
        to: usize,
    },
    /// Several edits recorded as one undo step (applied in order).
    Batch(Vec<Edit>),
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
    /// Opacity, group, material, crop or kind of one layer.
    SetLayerProps {
        index: usize,
        old: LayerProps,
        new: LayerProps,
    },
    /// Replaces the project material list.
    SetMaterials {
        old: Vec<Material>,
        new: Vec<Material>,
    },
    /// Replaces the whole palette (e.g. importing one from an MTL file).
    SetPalette {
        old: Palette,
        new: Palette,
    },
    /// Replaces the selection of one layer.
    SetSelection {
        layer: usize,
        old: Selection,
        new: Selection,
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
            Edit::MoveLayer { from, to } => move_layer(data, *from, *to),
            Edit::Batch(edits) => edits.iter().for_each(|e| e.apply(data)),
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
            Edit::SetLayerProps { index, new, .. } => {
                if let Some(l) = data.layers.get_mut(*index) {
                    l.props = new.clone();
                }
            }
            Edit::SetMaterials { new, .. } => data.materials = new.clone(),
            Edit::SetPalette { new, .. } => data.palette = new.clone(),
            Edit::SetSelection { layer, new, .. } => {
                if let Some(l) = data.layers.get_mut(*layer) {
                    l.selection = new.clone();
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
            Edit::MoveLayer { from, to } => move_layer(data, *to, *from),
            Edit::Batch(edits) => edits.iter().rev().for_each(|e| e.revert(data)),
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
            Edit::SetLayerProps { index, old, .. } => {
                if let Some(l) = data.layers.get_mut(*index) {
                    l.props = old.clone();
                }
            }
            Edit::SetMaterials { old, .. } => data.materials = old.clone(),
            Edit::SetPalette { old, .. } => data.palette = old.clone(),
            Edit::SetSelection { layer, old, .. } => {
                if let Some(l) = data.layers.get_mut(*layer) {
                    l.selection = old.clone();
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
            // Slider drags (opacity, crop) on one layer become one step.
            (
                Edit::SetLayerProps { index, new, .. },
                Edit::SetLayerProps {
                    index: i, new: n, ..
                },
            ) if index == i => {
                *new = n.clone();
                true
            }
            _ => false,
        }
    }
}

fn move_layer(data: &mut DocData, from: usize, to: usize) {
    let layer = data.layers.remove(from);
    data.layers.insert(to, layer);
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

/// Tentative voxel changes of one layer inside a transaction: each position
/// once, with its value before the transaction and its latest value.
#[derive(Debug, Default)]
struct PendingLayer {
    changes: Vec<VoxelChange>,
    index: HashMap<IVec3, usize>,
}

impl PendingLayer {
    fn record(&mut self, c: VoxelChange) {
        match self.index.get(&c.pos) {
            Some(&i) => self.changes[i].new = c.new,
            None => {
                self.index.insert(c.pos, self.changes.len());
                self.changes.push(c);
            }
        }
    }
}

/// An open transaction: already-applied voxel changes, per layer.
#[derive(Debug, Default)]
struct Transaction {
    layers: BTreeMap<usize, PendingLayer>,
}

/// One undo step and the slice it was made on.
#[derive(Debug)]
struct Step {
    edit: Edit,
    /// Work plane while slicing when the step was recorded; undo and redo
    /// move the slice back there (Qubicle Slice Mode).
    plane: Option<WorkPlane>,
}

/// Linear undo/redo stack.
#[derive(Debug, Default)]
pub struct History {
    undo: Vec<Step>,
    redo: Vec<Step>,
    transaction: Option<Transaction>,
    /// Slice recorded with the steps pushed from now on (`None` when not
    /// slicing). Set by the document before it records edits.
    pub tag: Option<WorkPlane>,
}

impl History {
    /// Opens a transaction: voxel edits recorded until
    /// [`History::commit_transaction`] become a single undo step. Does
    /// nothing when one is already open.
    pub fn begin_transaction(&mut self) {
        self.transaction.get_or_insert_with(Transaction::default);
    }

    pub fn in_transaction(&self) -> bool {
        self.transaction.is_some()
    }

    /// Records already-applied voxel changes into the open transaction,
    /// keeping the earliest `old` and the latest `new` per `(layer, pos)`.
    /// Returns `false` (and records nothing) when no transaction is open.
    pub fn record(&mut self, layer: usize, changes: &[VoxelChange]) -> bool {
        let Some(t) = &mut self.transaction else {
            return false;
        };
        let pending = t.layers.entry(layer).or_default();
        for c in changes {
            pending.record(*c);
        }
        true
    }

    /// Number of positions the open transaction actually changes.
    pub fn pending_count(&self) -> usize {
        self.transaction.as_ref().map_or(0, |t| {
            t.layers
                .values()
                .map(|l| l.changes.iter().filter(|c| c.old != c.new).count())
                .sum()
        })
    }

    /// Restores every tentative change of the open transaction and empties
    /// it, keeping it open (tools that recompute from the drag anchor).
    /// Returns whether anything was reverted.
    pub fn revert_pending(&mut self, data: &mut DocData) -> bool {
        let Some(t) = &mut self.transaction else {
            return false;
        };
        let pending = std::mem::take(&mut t.layers);
        let mut any = false;
        for (layer, p) in pending {
            if let Some(l) = data.layers.get_mut(layer) {
                for c in p.changes.iter().filter(|c| c.old != c.new) {
                    l.volume.set(c.pos, c.old);
                    any = true;
                }
            }
        }
        any
    }

    /// Closes the transaction and pushes its changes as one undo step
    /// (`Edit::Voxels`, or a `Batch` of them for several layers). Returns
    /// `false` when it changed nothing, in which case no step is recorded.
    pub fn commit_transaction(&mut self) -> bool {
        self.commit_transaction_with(Vec::new())
    }

    /// Like [`History::commit_transaction`], with `extra` already-applied
    /// edits (e.g. the selection following a move) joining the same undo
    /// step after the voxel changes. A transaction that changed no voxels
    /// still records `extra`. Returns `false` when nothing was recorded.
    pub fn commit_transaction_with(&mut self, extra: Vec<Edit>) -> bool {
        let Some(t) = self.transaction.take() else {
            return false;
        };
        let mut edits: Vec<Edit> = t
            .layers
            .into_iter()
            .filter_map(|(layer, p)| {
                let changes: Vec<VoxelChange> =
                    p.changes.into_iter().filter(|c| c.old != c.new).collect();
                (!changes.is_empty()).then_some(Edit::Voxels { layer, changes })
            })
            .chain(extra)
            .collect();
        let edit = match edits.len() {
            0 => return false,
            1 => edits.pop().unwrap_or(Edit::Batch(Vec::new())),
            _ => Edit::Batch(edits),
        };
        self.push(edit, false);
        true
    }

    /// Closes the transaction and restores the data it changed. Returns
    /// whether anything was reverted.
    pub fn cancel_transaction(&mut self, data: &mut DocData) -> bool {
        let any = self.revert_pending(data);
        self.transaction = None;
        any
    }

    /// Records an already-applied edit. Clears the redo stack. While a
    /// transaction is open, voxel edits join it; any other edit commits the
    /// transaction first so history order matches the order of changes.
    pub fn push(&mut self, edit: Edit, merge: bool) {
        if self.transaction.is_some() {
            if let Edit::Voxels { layer, changes } = &edit {
                self.record(*layer, changes);
                self.redo.clear();
                return;
            }
            self.commit_transaction();
        }
        self.redo.clear();
        if merge {
            if let Some(last) = self.undo.last_mut() {
                if last.edit.try_merge(&edit) {
                    return;
                }
            }
        }
        self.undo.push(Step {
            edit,
            plane: self.tag,
        });
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

    /// Undoes the last step. An open transaction is committed first, so the
    /// drag in progress is what gets undone.
    pub fn undo(&mut self, data: &mut DocData) -> bool {
        self.commit_transaction();
        let Some(step) = self.undo.pop() else {
            return false;
        };
        step.edit.revert(data);
        self.redo.push(step);
        true
    }

    pub fn redo(&mut self, data: &mut DocData) -> bool {
        self.commit_transaction();
        let Some(step) = self.redo.pop() else {
            return false;
        };
        step.edit.apply(data);
        self.undo.push(step);
        true
    }

    /// Slice of the step the last [`History::undo`] reverted.
    pub fn undone_plane(&self) -> Option<WorkPlane> {
        self.redo.last().and_then(|s| s.plane)
    }

    /// Slice of the step the last [`History::redo`] re-applied.
    pub fn redone_plane(&self) -> Option<WorkPlane> {
        self.undo.last().and_then(|s| s.plane)
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
            materials: Vec::new(),
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
        let sel = Edit::SetSelection {
            layer: 0,
            old: Selection::default(),
            new: [IVec3::ZERO].into_iter().collect(),
        };
        sel.apply(&mut d);
        assert_eq!(d.layers[0].selection.len(), 1);
        sel.revert(&mut d);
        assert!(d.layers[0].selection.is_empty());
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
        assert_eq!(
            h.undo[0].edit,
            voxels(0, vec![change(0, 0, 5), change(2, 0, 3)])
        );
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

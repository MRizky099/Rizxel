//! Undo/redo history based on reversible edit commands.

use rizxel_math::{BoundSize, Rgba8};

use crate::document::{DocData, Layer};

/// Maximum number of undo steps kept per document.
const MAX_UNDO: usize = 512;

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
    SetBounds {
        old: BoundSize,
        new: BoundSize,
    },
    SetPaletteColor {
        index: usize,
        old: Rgba8,
        new: Rgba8,
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
            Edit::SetBounds { new, .. } => data.bounds = *new,
            Edit::SetPaletteColor { index, new, .. } => {
                data.palette.set(*index, *new);
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
            Edit::SetBounds { old, .. } => data.bounds = *old,
            Edit::SetPaletteColor { index, old, .. } => {
                data.palette.set(*index, *old);
            }
        }
    }

    /// Tries to fold `next` into `self` (same kind and target). On success
    /// `self` spans from its original `old` to `next.new`.
    fn try_merge(&mut self, next: &Edit) -> bool {
        match (self, next) {
            (Edit::SetBounds { new, .. }, Edit::SetBounds { new: n, .. }) => {
                *new = *n;
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
            layers: vec![Layer::new("A")],
            bounds: BoundSize::default(),
        }
    }

    #[test]
    fn new_edit_clears_redo() {
        let mut d = data();
        let mut h = History::default();
        let e = Edit::SetBounds {
            old: d.bounds,
            new: BoundSize::new(1, 1, 1),
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

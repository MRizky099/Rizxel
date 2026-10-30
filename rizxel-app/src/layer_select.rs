//! Multi-selection in the Layers panel (Phase 12c): `Ctrl`+click toggles a
//! layer, `Shift`+click selects a range of rows, and the structure
//! operations act on the whole selection as one undo step (*Group
//! Layers*, *Move Layers*, delete, duplicate, show, lock, merge).
//!
//! The selection is a list of layer ids on the [`Document`], view state
//! like the active layer. It always holds the active layer: when the
//! active layer changes to one outside it (a new layer, undo), the
//! selection is that layer alone. Ids that no longer exist are skipped.

use std::collections::HashSet;

use rizxel_math::{LayerId, LayerKind};

use crate::document::{Document, Layer, LayerDrop, LayerProps};
use crate::history::Edit;
use crate::layer_tree::{LayerTree, Link};
use crate::selection::Selection;

/// History name of *select on click* (consecutive clicks fold into one).
pub const SELECT_LAYER_LABEL: &str = "Select Layer";

impl Document {
    // ------------------------------------------------------------ selection

    /// Ids of the selected layers, in the order they were picked; the
    /// active layer is always one of them.
    pub fn selected_layer_ids(&self) -> Vec<LayerId> {
        let Some(active) = self.active_layer().map(|l| l.id) else {
            return Vec::new();
        };
        if !self.selected_layers.contains(&active) {
            return vec![active];
        }
        self.selected_layers
            .iter()
            .copied()
            .filter(|&id| self.data.layer_index(id).is_some())
            .collect()
    }

    /// Indices of the selected layers, bottom to top.
    pub fn selected_layer_indices(&self) -> Vec<usize> {
        let mut out: Vec<usize> = self
            .selected_layer_ids()
            .into_iter()
            .filter_map(|id| self.data.layer_index(id))
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    pub fn selected_layer_count(&self) -> usize {
        self.selected_layer_indices().len()
    }

    pub fn is_layer_selected(&self, index: usize) -> bool {
        self.data
            .layers
            .get(index)
            .is_some_and(|l| self.selected_layer_ids().contains(&l.id))
    }

    /// The selection without layers whose group is selected too (the group
    /// carries its subtree), bottom to top.
    pub fn normalized_layer_selection(&self) -> Vec<usize> {
        let sel = self.selected_layer_indices();
        let tree = self.data.tree();
        sel.iter()
            .copied()
            .filter(|&i| !tree.ancestors(i).any(|a| sel.contains(&a)))
            .collect()
    }

    /// Plain click: layer `index` alone, and active.
    pub fn select_layer(&mut self, index: usize) {
        let Some(id) = self.data.layers.get(index).map(|l| l.id) else {
            return;
        };
        self.active_layer = index;
        self.selected_layers = vec![id];
        self.layer_anchor = Some(id);
    }

    /// `Ctrl`+click: adds layer `index` (it becomes active) or removes it
    /// (the last one picked becomes active). The last selected layer stays.
    pub fn toggle_layer_selected(&mut self, index: usize) {
        let Some(id) = self.data.layers.get(index).map(|l| l.id) else {
            return;
        };
        let mut ids = self.selected_layer_ids();
        self.layer_anchor = Some(id);
        if let Some(pos) = ids.iter().position(|&s| s == id) {
            if ids.len() == 1 {
                return;
            }
            ids.remove(pos);
            if let Some(i) = ids.last().and_then(|&last| self.data.layer_index(last)) {
                self.active_layer = i;
            }
        } else {
            ids.push(id);
            self.active_layer = index;
        }
        self.selected_layers = ids;
    }

    /// `Shift`+click: the rows from the anchor to `index` in panel order
    /// (a collapsed group counts as its row); `add` (`Ctrl+Shift`) keeps
    /// the current selection too. Layer `index` becomes active.
    pub fn select_layer_range(&mut self, index: usize, add: bool) {
        if index >= self.data.layers.len() {
            return;
        }
        let tree = self.data.tree();
        let collapsed = |i: usize| {
            matches!(
                self.data.layers[i].kind(),
                LayerKind::Group { collapsed: true }
            )
        };
        let rows = tree.visible_rows(collapsed);
        let anchor = self
            .layer_anchor
            .and_then(|id| self.data.layer_index(id))
            .unwrap_or(self.active_layer);
        let (a, b) = (
            tree.shown_row(anchor, collapsed),
            tree.shown_row(index, collapsed),
        );
        let (Some(pa), Some(pb)) = (
            rows.iter().position(|&r| r == a),
            rows.iter().position(|&r| r == b),
        ) else {
            return;
        };
        let mut ids = if add {
            self.selected_layer_ids()
        } else {
            Vec::new()
        };
        for &r in &rows[pa.min(pb)..=pa.max(pb)] {
            let id = self.data.layers[r].id;
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        // The clicked row is the active one, so it goes last.
        let clicked = self.data.layers[b].id;
        ids.retain(|&id| id != clicked);
        ids.push(clicked);
        self.active_layer = b;
        self.selected_layers = ids;
        if self.layer_anchor.is_none() {
            self.layer_anchor = Some(self.data.layers[anchor].id);
        }
    }

    /// `Ctrl+A` over the Layers panel: every layer.
    pub fn select_all_layers(&mut self) {
        let mut ids: Vec<LayerId> = self.data.layers.iter().map(|l| l.id).collect();
        // The active layer stays active (last).
        if let Some(active) = self.active_layer().map(|l| l.id) {
            ids.retain(|&id| id != active);
            ids.push(active);
        }
        self.selected_layers = ids;
    }

    /// `Esc` over the Layers panel: back to the active layer alone.
    pub fn clear_layer_selection(&mut self) {
        self.selected_layers = self.active_layer().map(|l| l.id).into_iter().collect();
    }

    /// Selects layers `ids` (the last one becomes active).
    fn set_layer_selection(&mut self, ids: Vec<LayerId>) {
        if let Some(i) = ids.last().and_then(|&id| self.data.layer_index(id)) {
            self.active_layer = i;
        }
        self.layer_anchor = ids.last().copied();
        self.selected_layers = ids;
    }

    // --------------------------------------------------------- select voxels

    /// *Select on click*: every voxel layer of `indices` gets all its voxels
    /// selected, as one undo step that also takes in the clicks right
    /// before it ([`SELECT_LAYER_LABEL`]). Groups, shapes, clones and empty
    /// layers are skipped. Returns whether a selection changed.
    pub fn select_layer_voxels(&mut self, indices: &[usize]) -> bool {
        let mut edits: Vec<Edit> = indices
            .iter()
            .filter_map(|&i| {
                let l = self.data.layers.get(i)?;
                if !l.kind().stores_voxels() || l.volume.voxel_count() == 0 {
                    return None;
                }
                let all: Selection = l.volume.iter().map(|(p, _)| p).collect();
                (l.selection != all).then(|| Edit::SetSelection {
                    layer: i,
                    old: l.selection.clone(),
                    new: all,
                })
            })
            .collect();
        let edit = match edits.len() {
            0 => return false,
            1 => edits.pop().unwrap_or(Edit::Batch(Vec::new())),
            _ => Edit::Batch(edits),
        };
        self.apply_folded(edit, SELECT_LAYER_LABEL);
        true
    }

    // ------------------------------------------------------------ structure

    /// One edit that turns the layer list into `order` (ids bottom to top,
    /// each with its group) after appending `added`. `None` when `order`
    /// breaks the tree rules or changes nothing.
    fn restructure_edit(
        &self,
        added: Vec<Layer>,
        order: &[(LayerId, Option<LayerId>)],
    ) -> Option<Edit> {
        let kind_group = |id: LayerId| {
            self.data
                .layers
                .iter()
                .chain(&added)
                .find(|l| l.id == id)
                .map(|l| l.kind().is_group())
        };
        let links: Vec<Link> = order
            .iter()
            .map(|&(id, parent)| {
                Some(Link {
                    id,
                    parent,
                    group: kind_group(id)?,
                })
            })
            .collect::<Option<_>>()?;
        if links.len() != self.data.layers.len() + added.len()
            || !LayerTree::from_links(&links).is_valid()
        {
            return None;
        }
        let mut sim: Vec<LayerId> = self
            .data
            .layers
            .iter()
            .chain(&added)
            .map(|l| l.id)
            .collect();
        let mut props: Vec<(LayerId, LayerProps)> = self
            .data
            .layers
            .iter()
            .chain(&added)
            .map(|l| (l.id, l.props.clone()))
            .collect();
        let base = self.data.layers.len();
        let mut edits: Vec<Edit> = added
            .into_iter()
            .enumerate()
            .map(|(k, layer)| Edit::InsertLayer {
                index: base + k,
                layer,
            })
            .collect();
        for (k, &(id, _)) in order.iter().enumerate() {
            let from = sim.iter().position(|&s| s == id)?;
            if from != k {
                sim.remove(from);
                sim.insert(k, id);
                edits.push(Edit::MoveLayer { from, to: k });
            }
        }
        for (k, &(id, parent)) in order.iter().enumerate() {
            let (_, old) = props.iter_mut().find(|(p, _)| *p == id)?;
            if old.parent != parent {
                let new = LayerProps {
                    parent,
                    ..old.clone()
                };
                edits.push(Edit::SetLayerProps {
                    index: k,
                    old: std::mem::replace(old, new.clone()),
                    new,
                });
            }
        }
        (!edits.is_empty()).then_some(Edit::Batch(edits))
    }

    /// The layers in `roots` with their subtrees, and the rest, both in
    /// list order.
    fn split_blocks(&self, roots: &[usize]) -> (Vec<usize>, Vec<usize>) {
        let tree = self.data.tree();
        let blocks: Vec<_> = roots.iter().map(|&r| tree.subtree(r)).collect();
        (0..self.data.layers.len()).partition(|i| blocks.iter().any(|b| b.contains(i)))
    }

    /// The list order (ids with groups) after putting `moved` (blocks, in
    /// order) at position `at` of `rest`, the roots of `roots` into group
    /// `parent`.
    fn order_with(
        &self,
        rest: &[usize],
        moved: &[usize],
        roots: &[usize],
        at: usize,
        parent: Option<LayerId>,
    ) -> Vec<(LayerId, Option<LayerId>)> {
        let entry = |i: usize| {
            let l = &self.data.layers[i];
            let p = if roots.contains(&i) {
                parent
            } else {
                l.props.parent
            };
            (l.id, p)
        };
        let at = at.min(rest.len());
        rest[..at]
            .iter()
            .chain(moved)
            .chain(&rest[at..])
            .map(|&i| entry(i))
            .collect()
    }

    /// *Group Selected* (`Ctrl+G`): a new group at the place of the
    /// top-most selected layer, in the deepest group holding the whole
    /// selection, with the selected layers (and their subtrees) inside in
    /// their order. One undo step; the new group becomes active, selected
    /// and expanded. With one layer selected this is *Group Layer*. `None`
    /// when the nesting would get too deep.
    pub fn group_selected_layers(&mut self) -> Option<LayerId> {
        let sel = self.normalized_layer_selection();
        if sel.len() < 2 {
            let index = sel.first().copied().unwrap_or(self.active_layer);
            let id = self.group_layer(index)?;
            self.set_layer_selection(vec![id]);
            return Some(id);
        }
        let tree = self.data.tree();
        let parent = tree.common_parent(&sel);
        let parent_id = parent.map(|p| self.data.layers[p].id);
        let top = *sel.last()?;
        // The child of `parent` holding the top-most selected layer.
        let anchor = std::iter::once(top)
            .chain(tree.ancestors(top))
            .find(|&a| tree.parent(a) == parent)?;
        let (moved, rest) = self.split_blocks(&sel);
        let at = rest.iter().filter(|&&i| i <= anchor).count();

        let mut group = Layer::new(self.unique_name_from("Group"), self.data.bounds);
        group.props.kind = LayerKind::Group { collapsed: false };
        group.props.parent = parent_id;
        group.id = self.next_layer_id();
        let id = group.id;
        let mut order = self.order_with(&rest, &moved, &sel, at, Some(id));
        order.insert(at + moved.len(), (id, parent_id));
        let edit = self.restructure_edit(vec![group], &order)?;
        self.labeled("Group Layers", |d| d.apply(edit, false));
        self.set_layer_selection(vec![id]);
        Some(id)
    }

    /// Whether the dragged row `from` carries the whole selection: it is
    /// selected and other layers are too.
    pub fn drags_selection(&self, from: usize) -> bool {
        self.is_layer_selected(from) && self.normalized_layer_selection().len() > 1
    }

    /// The list order after dropping the selection at `target`, `None` when
    /// refused (into a selected subtree, breaking the tree rules) or a
    /// no-op.
    fn selection_drop_order(&self, target: LayerDrop) -> Option<Vec<(LayerId, Option<LayerId>)>> {
        let sel = self.normalized_layer_selection();
        let tree = self.data.tree();
        let row = target.row();
        if row >= self.data.layers.len() || sel.iter().any(|&s| tree.subtree(s).contains(&row)) {
            return None;
        }
        let (moved, rest) = self.split_blocks(&sel);
        let parent_of = |i: usize| self.data.layers[i].props.parent;
        let pos = |i: usize| rest.iter().position(|&r| r == i);
        let (at, parent) = match target {
            LayerDrop::Into(g) if self.data.layers[g].kind().is_group() => {
                (pos(g)?, Some(self.data.layers[g].id))
            }
            LayerDrop::Into(_) => return None,
            LayerDrop::Above(r) => (pos(r)? + 1, parent_of(r)),
            LayerDrop::Below(r) => {
                let start = tree.subtree(r).start;
                (rest.iter().filter(|&&i| i < start).count(), parent_of(r))
            }
        };
        let order = self.order_with(&rest, &moved, &sel, at, parent);
        let unchanged = order
            .iter()
            .zip(&self.data.layers)
            .all(|(&(id, p), l)| id == l.id && p == l.props.parent);
        (!unchanged).then_some(order)
    }

    /// Whether [`Document::drop_selected_layers`] would act on `target`.
    pub fn can_drop_selection(&self, target: LayerDrop) -> bool {
        self.selection_drop_order(target)
            .is_some_and(|order| self.restructure_edit(Vec::new(), &order).is_some())
    }

    /// Drops the whole selection above, below or into row `target`,
    /// keeping its order, as one undo step ("Move Layers"). Refused into a
    /// selected group's subtree.
    pub fn drop_selected_layers(&mut self, target: LayerDrop) -> bool {
        let Some(edit) = self
            .selection_drop_order(target)
            .and_then(|order| self.restructure_edit(Vec::new(), &order))
        else {
            return false;
        };
        let ids = self.selected_layer_ids();
        self.labeled("Move Layers", |d| d.apply(edit, false));
        self.set_layer_selection(ids);
        true
    }

    /// Whether [`Document::delete_selected_layers`] would act: at least one
    /// layer survives and no survivor is a linked clone of a deleted one.
    pub fn can_delete_selected_layers(&self) -> bool {
        let sel = self.selected_layer_indices();
        if sel.is_empty() || sel.len() >= self.data.layers.len() {
            return false;
        }
        let ids: HashSet<LayerId> = sel.iter().map(|&i| self.data.layers[i].id).collect();
        !self.data.layers.iter().any(|l| {
            !ids.contains(&l.id)
                && matches!(l.props.kind, LayerKind::Clone { source, .. } if ids.contains(&source))
        })
    }

    /// Deletes every selected layer as one undo step. Children of a deleted
    /// group that are not selected move up to the nearest group that stays.
    pub fn delete_selected_layers(&mut self) -> bool {
        let sel = self.selected_layer_indices();
        if sel.len() == 1 {
            self.active_layer = sel[0];
            return self.delete_active_layer();
        }
        if !self.can_delete_selected_layers() {
            return false;
        }
        let tree = self.data.tree();
        let ids: HashSet<LayerId> = sel.iter().map(|&i| self.data.layers[i].id).collect();
        let mut edits: Vec<Edit> = Vec::new();
        for (i, l) in self.data.layers.iter().enumerate() {
            if ids.contains(&l.id) || !l.props.parent.is_some_and(|p| ids.contains(&p)) {
                continue;
            }
            let parent = tree
                .ancestors(i)
                .map(|a| self.data.layers[a].id)
                .find(|a| !ids.contains(a));
            edits.push(Edit::SetLayerProps {
                index: i,
                old: l.props.clone(),
                new: LayerProps {
                    parent,
                    ..l.props.clone()
                },
            });
        }
        edits.extend(sel.iter().rev().map(|&index| Edit::RemoveLayer {
            index,
            layer: self.data.layers[index].clone(),
        }));
        // The layer right below the lowest deleted one becomes active.
        let below = sel[0].checked_sub(1);
        self.labeled("Delete Layers", |d| d.apply(Edit::Batch(edits), false));
        self.active_layer = below.unwrap_or(0);
        self.clear_layer_selection();
        true
    }

    /// Copies every selected layer (groups with their contents) just above
    /// itself as one undo step; the copies become the selection.
    pub fn duplicate_selected_layers(&mut self) -> bool {
        let sel = self.normalized_layer_selection();
        if sel.len() < 2 {
            if let Some(&i) = sel.first() {
                self.active_layer = i;
            }
            return self.duplicate_active_layer();
        }
        let active = self.active_layer().map(|l| l.id);
        let mut next = self.next_layer_id();
        let mut names = Vec::new();
        let mut edits = Vec::new();
        let mut copies = Vec::new();
        // Top first: copies of a higher block never shift a lower one.
        for &i in sel.iter().rev() {
            let block = self.duplicate_edits(i, &mut next, &mut names);
            if let Some(Edit::InsertLayer { layer, .. }) = block.last() {
                copies.push((self.data.layers[i].id, layer.id));
            }
            edits.extend(block);
        }
        self.labeled("Duplicate Layers", |d| d.apply(Edit::Batch(edits), false));
        // Bottom to top, the active layer's copy last.
        copies.reverse();
        if let Some(pos) = copies.iter().position(|&(src, _)| Some(src) == active) {
            let c = copies.remove(pos);
            copies.push(c);
        }
        self.set_layer_selection(copies.into_iter().map(|(_, copy)| copy).collect());
        true
    }

    /// Shows or hides every selected layer as one undo step.
    pub fn set_selected_layers_visible(&mut self, visible: bool) -> bool {
        let edits: Vec<Edit> = self
            .normalized_layer_selection()
            .into_iter()
            .filter(|&i| self.data.layers[i].visible != visible)
            .map(|index| Edit::SetLayerVisible {
                index,
                old: !visible,
                new: visible,
            })
            .collect();
        if edits.is_empty() {
            return false;
        }
        let label = if visible {
            "Show Layers"
        } else {
            "Hide Layers"
        };
        self.labeled(label, |d| d.apply(Edit::Batch(edits), false));
        true
    }

    /// Locks or unlocks every selected layer as one undo step.
    pub fn set_selected_layers_locked(&mut self, locked: bool) -> bool {
        let edits: Vec<Edit> = self
            .normalized_layer_selection()
            .into_iter()
            .filter(|&i| self.data.layers[i].locked != locked)
            .map(|index| Edit::SetLayerLocked {
                index,
                old: !locked,
                new: locked,
            })
            .collect();
        if edits.is_empty() {
            return false;
        }
        let label = if locked {
            "Lock Layers"
        } else {
            "Unlock Layers"
        };
        self.labeled(label, |d| d.apply(Edit::Batch(edits), false));
        true
    }

    /// Whether [`Document::merge_selected_layers`] would act: no groups,
    /// and the lowest selected layer holds voxels.
    pub fn can_merge_selected_layers(&self) -> bool {
        let sel = self.selected_layer_indices();
        sel.len() > 1
            && !sel.iter().any(|&i| self.data.layers[i].kind().is_group())
            && self.mergeable(sel[0], &sel[1..])
    }

    /// Merges the selected layers into the lowest one (a voxel layer),
    /// upper layers winning, as one undo step.
    pub fn merge_selected_layers(&mut self) -> bool {
        if !self.can_merge_selected_layers() {
            return false;
        }
        let sel = self.selected_layer_indices();
        let edit = self.merge_into(sel[0], &sel[1..]);
        self.labeled("Merge Layers", |d| d.apply(edit, false));
        self.select_layer(sel[0]);
        true
    }
}

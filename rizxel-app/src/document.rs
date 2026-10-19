//! A single open project (one tab) and its undoable data.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use glam::IVec3;
use rizxel_engine::{greedy_mesh, Mesh};
use rizxel_io::{LayerData, ProjectFile, RzxlError};
use rizxel_math::{BoundSize, Palette, Rgba8, Voxel, VoxelVolume, FIRST_COLOR_INDEX};

use crate::history::{ClippedVoxel, Edit, History, VoxelChange};

/// A named group of voxels. Its volume always has the document's bounds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layer {
    pub name: String,
    pub visible: bool,
    pub locked: bool,
    pub volume: VoxelVolume,
}

impl Layer {
    pub fn new(name: impl Into<String>, bounds: BoundSize) -> Self {
        Self {
            name: name.into(),
            visible: true,
            locked: false,
            volume: VoxelVolume::new(bounds),
        }
    }
}

/// Source of [`Document::id`]s; unique for the lifetime of the process.
static NEXT_DOC_ID: AtomicU64 = AtomicU64::new(1);

/// Document content that is modified exclusively through [`Edit`]s so every
/// change can be undone.
#[derive(Debug, Clone, PartialEq)]
pub struct DocData {
    pub palette: Palette,
    pub layers: Vec<Layer>,
    pub bounds: BoundSize,
}

/// An open project: undoable data plus view/selection state.
#[derive(Debug)]
pub struct Document {
    /// Process-unique id, used together with [`Document::revision`] to key
    /// caches (e.g. the viewport mesh) per open tab.
    id: u64,
    pub name: String,
    pub path: Option<PathBuf>,
    pub data: DocData,
    /// Index into `data.layers`. Not part of undo history.
    pub active_layer: usize,
    /// Index into `data.palette`. Not part of undo history.
    pub selected_color: usize,
    history: History,
    /// Incremented on every data change; compared with `saved_revision`.
    revision: u64,
    saved_revision: u64,
}

impl Document {
    /// Empty document with one layer and the default palette.
    pub fn new(name: impl Into<String>) -> Self {
        Self::with_layers(name, Vec::new())
    }

    /// Document with the default bounds. Layer volumes are resized to them;
    /// an empty list gets one default layer.
    pub fn with_layers(name: impl Into<String>, layers: Vec<Layer>) -> Self {
        Self::with_bounds(name, BoundSize::default(), layers)
    }

    fn with_bounds(name: impl Into<String>, bounds: BoundSize, mut layers: Vec<Layer>) -> Self {
        if layers.is_empty() {
            layers.push(Layer::new("Layer 1", bounds));
        }
        for layer in &mut layers {
            layer.volume.resize(bounds);
        }
        Self {
            id: NEXT_DOC_ID.fetch_add(1, Ordering::Relaxed),
            name: name.into(),
            path: None,
            data: DocData {
                palette: Palette::default(),
                layers,
                bounds,
            },
            active_layer: 0,
            selected_color: Palette::DEFAULT_SELECTED,
            history: History::default(),
            revision: 0,
            saved_revision: 0,
        }
    }

    // ---------------------------------------------------------------- state

    pub fn is_dirty(&self) -> bool {
        self.revision != self.saved_revision
    }

    /// Process-unique document id.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Bumped on every data change (edit, undo, redo, load). Caches derived
    /// from [`DocData`] are valid while `(id, revision)` is unchanged.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Visible layers as mesher input.
    fn mesh_layers(&self) -> Vec<(&VoxelVolume, bool)> {
        self.data
            .layers
            .iter()
            .map(|l| (&l.volume, l.visible))
            .collect()
    }

    /// Greedy mesh of the visible layers, in model space.
    pub fn visible_mesh(&self) -> Mesh {
        greedy_mesh(&self.mesh_layers(), &self.data.palette)
    }

    /// Voxel count summed over visible layers (overlaps count once per layer).
    pub fn visible_voxel_count(&self) -> usize {
        self.data
            .layers
            .iter()
            .filter(|l| l.visible)
            .map(|l| l.volume.voxel_count())
            .sum()
    }

    /// Box around the voxels of the visible layers: `(min, max exclusive)`.
    pub fn occupied_bounds(&self) -> Option<(IVec3, IVec3)> {
        self.data
            .layers
            .iter()
            .filter(|l| l.visible)
            .filter_map(|l| l.volume.occupied_bounds())
            .reduce(|(a0, a1), (b0, b1)| (a0.min(b0), a1.max(b1)))
    }

    /// Display title: file name if saved, otherwise the document name.
    pub fn title(&self) -> String {
        self.path
            .as_deref()
            .and_then(Path::file_name)
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("{}.{}", self.name, rizxel_io::RZXL_EXTENSION))
    }

    pub fn selected_rgba(&self) -> Rgba8 {
        self.data
            .palette
            .get(self.selected_color)
            .unwrap_or(Rgba8::WHITE)
    }

    pub fn active_layer(&self) -> Option<&Layer> {
        self.data.layers.get(self.active_layer)
    }

    // -------------------------------------------------------------- editing

    /// Applies an edit and records it for undo.
    ///
    /// When `merge` is true and the previous edit is of the same kind and
    /// target (e.g. continuous drag of a bound value), the two are combined
    /// into a single undo step.
    pub fn apply(&mut self, edit: Edit, merge: bool) {
        edit.apply(&mut self.data);
        self.history.push(edit, merge);
        self.touch();
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    pub fn undo(&mut self) -> bool {
        let done = self.history.undo(&mut self.data);
        if done {
            self.touch();
        }
        done
    }

    pub fn redo(&mut self) -> bool {
        let done = self.history.redo(&mut self.data);
        if done {
            self.touch();
        }
        done
    }

    fn touch(&mut self) {
        self.revision += 1;
        // Keep selection indices valid after structural changes.
        let last = self.data.layers.len().saturating_sub(1);
        self.active_layer = self.active_layer.min(last);
        // Palette slot 0 is the reserved "empty" voxel and never selected.
        let last = self.data.palette.len().saturating_sub(1);
        self.selected_color = self.selected_color.clamp(
            FIRST_COLOR_INDEX as usize,
            last.max(FIRST_COLOR_INDEX as usize),
        );
    }

    /// Selects a paint color; ignored for the reserved slot 0 or bad indices.
    pub fn select_color(&mut self, index: usize) -> bool {
        let ok = self.data.palette.is_selectable(index);
        if ok {
            self.selected_color = index;
        }
        ok
    }

    // ------------------------------------------------------ voxel helpers

    /// Writes voxels into a layer as one undoable edit. No-op and out-of-bounds
    /// writes are skipped; returns `false` when nothing changed. With `merge`,
    /// consecutive writes to the same layer become one undo step (a drag).
    pub fn set_voxels(
        &mut self,
        layer: usize,
        writes: impl IntoIterator<Item = (IVec3, Voxel)>,
        merge: bool,
    ) -> bool {
        let Some(l) = self.data.layers.get(layer) else {
            return false;
        };
        let mut changes: Vec<VoxelChange> = Vec::new();
        for (pos, new) in writes {
            if !l.volume.in_bounds(pos) {
                continue;
            }
            let old = l.volume.get(pos);
            if old != new {
                changes.push(VoxelChange { pos, old, new });
            }
        }
        // Repeated positions inside one batch collapse to first-old/last-new.
        let mut deduped = Vec::with_capacity(changes.len());
        crate::history::merge_voxel_changes(&mut deduped, &changes);
        if deduped.is_empty() {
            return false;
        }
        self.apply(
            Edit::Voxels {
                layer,
                changes: deduped,
            },
            merge,
        );
        true
    }

    // ------------------------------------------------------ layer helpers

    /// Adds a new layer above the active one and selects it.
    pub fn add_layer(&mut self) {
        let index = (self.active_layer + 1).min(self.data.layers.len());
        let name = self.unique_layer_name();
        self.apply(
            Edit::InsertLayer {
                index,
                layer: Layer::new(name, self.data.bounds),
            },
            false,
        );
        self.active_layer = index;
    }

    /// Deletes the active layer. The last remaining layer cannot be deleted.
    pub fn delete_active_layer(&mut self) -> bool {
        if self.data.layers.len() <= 1 {
            return false;
        }
        let index = self.active_layer;
        let layer = self.data.layers[index].clone();
        self.apply(Edit::RemoveLayer { index, layer }, false);
        true
    }

    pub fn toggle_layer_visible(&mut self, index: usize) {
        if let Some(l) = self.data.layers.get(index) {
            let (old, new) = (l.visible, !l.visible);
            self.apply(Edit::SetLayerVisible { index, old, new }, false);
        }
    }

    pub fn toggle_layer_locked(&mut self, index: usize) {
        if let Some(l) = self.data.layers.get(index) {
            let (old, new) = (l.locked, !l.locked);
            self.apply(Edit::SetLayerLocked { index, old, new }, false);
        }
    }

    pub fn rename_layer(&mut self, index: usize, new: String) {
        if let Some(l) = self.data.layers.get(index) {
            if l.name != new && !new.trim().is_empty() {
                let old = l.name.clone();
                self.apply(Edit::RenameLayer { index, old, new }, false);
            }
        }
    }

    /// Moves a layer to a new index; the active layer follows it. Higher
    /// indices draw on top.
    pub fn move_layer(&mut self, from: usize, to: usize) -> bool {
        let len = self.data.layers.len();
        if from >= len || to >= len || from == to {
            return false;
        }
        let active = self.active_layer;
        self.apply(Edit::MoveLayer { from, to }, false);
        self.active_layer = if active == from {
            to
        } else if from < active && active <= to {
            active - 1
        } else if to <= active && active < from {
            active + 1
        } else {
            active
        };
        true
    }

    /// Moves the active layer one step up (`true`, drawn above) or down.
    pub fn move_active_layer(&mut self, up: bool) -> bool {
        let from = self.active_layer;
        let to = if up {
            from + 1
        } else {
            match from.checked_sub(1) {
                Some(to) => to,
                None => return false,
            }
        };
        self.move_layer(from, to)
    }

    /// Copies the active layer (voxels and flags) just above itself.
    pub fn duplicate_active_layer(&mut self) -> bool {
        let Some(source) = self.data.layers.get(self.active_layer) else {
            return false;
        };
        let mut layer = source.clone();
        layer.name = self.unique_name_from(&format!("{} copy", source.name));
        let index = self.active_layer + 1;
        self.apply(Edit::InsertLayer { index, layer }, false);
        self.active_layer = index;
        true
    }

    /// Merges the active layer into the one below it (upper voxels win).
    pub fn merge_active_layer_down(&mut self) -> bool {
        let upper = self.active_layer;
        let Some(lower) = upper.checked_sub(1) else {
            return false;
        };
        if upper >= self.data.layers.len() {
            return false;
        }
        let edit = self.merge_into(lower, &[upper]);
        self.apply(edit, false);
        self.active_layer = lower;
        true
    }

    /// Merges every visible layer into the lowest visible one.
    pub fn merge_visible_layers(&mut self) -> bool {
        let visible: Vec<usize> = (0..self.data.layers.len())
            .filter(|&i| self.data.layers[i].visible)
            .collect();
        let [target, rest @ ..] = visible.as_slice() else {
            return false;
        };
        if rest.is_empty() {
            return false;
        }
        let edit = self.merge_into(*target, rest);
        self.apply(edit, false);
        self.active_layer = *target;
        true
    }

    /// Builds one undo step that paints `sources` (ascending, all above
    /// `target`) onto `target` in draw order, then removes them.
    fn merge_into(&self, target: usize, sources: &[usize]) -> Edit {
        let mut working = self.data.layers[target].volume.clone();
        let mut changes = Vec::new();
        for &i in sources {
            for (pos, new) in self.data.layers[i].volume.iter() {
                let old = working.get(pos);
                if old != new {
                    working.set(pos, new);
                    changes.push(VoxelChange { pos, old, new });
                }
            }
        }
        // Folds repeated positions into one change each.
        let mut merged = Vec::new();
        crate::history::merge_voxel_changes(&mut merged, &changes);

        let mut edits = vec![Edit::Voxels {
            layer: target,
            changes: merged,
        }];
        edits.extend(sources.iter().rev().map(|&index| Edit::RemoveLayer {
            index,
            layer: self.data.layers[index].clone(),
        }));
        Edit::Batch(edits)
    }

    /// Deletes every hidden layer. Refused when all layers are hidden, since
    /// a document keeps at least one layer.
    pub fn delete_hidden_layers(&mut self) -> bool {
        let hidden: Vec<usize> = (0..self.data.layers.len())
            .filter(|&i| !self.data.layers[i].visible)
            .collect();
        if hidden.is_empty() || hidden.len() == self.data.layers.len() {
            return false;
        }
        let active = self.active_layer;
        let removed_below = hidden.iter().filter(|&&i| i < active).count();
        let edits = hidden
            .iter()
            .rev()
            .map(|&index| Edit::RemoveLayer {
                index,
                layer: self.data.layers[index].clone(),
            })
            .collect();
        self.apply(Edit::Batch(edits), false);
        self.active_layer = active.saturating_sub(removed_below);
        true
    }

    /// Shows only layer `index`; when it already is the only visible one,
    /// shows every layer again (Goxel's solo toggle).
    pub fn solo_layer(&mut self, index: usize) {
        if index >= self.data.layers.len() {
            return;
        }
        let others_hidden = self
            .data
            .layers
            .iter()
            .enumerate()
            .all(|(i, l)| i == index || !l.visible);
        let edits: Vec<Edit> = self
            .data
            .layers
            .iter()
            .enumerate()
            .filter_map(|(i, l)| {
                let new = i == index || others_hidden;
                (l.visible != new).then_some(Edit::SetLayerVisible {
                    index: i,
                    old: l.visible,
                    new,
                })
            })
            .collect();
        if !edits.is_empty() {
            self.apply(Edit::Batch(edits), false);
        }
    }

    /// Locks (or unlocks) every layer as one undo step.
    pub fn set_all_locked(&mut self, locked: bool) {
        let edits: Vec<Edit> = self
            .data
            .layers
            .iter()
            .enumerate()
            .filter(|(_, l)| l.locked != locked)
            .map(|(index, l)| Edit::SetLayerLocked {
                index,
                old: l.locked,
                new: locked,
            })
            .collect();
        if !edits.is_empty() {
            self.apply(Edit::Batch(edits), false);
        }
    }

    pub fn set_bounds(&mut self, new: BoundSize, merge: bool) {
        let old = self.data.bounds;
        if old == new {
            return;
        }
        // Voxels outside the new bounds are dropped by the resize; remember
        // them so the shrink is undoable. Only tiles are visited, not cells.
        let limit = new.0;
        let clipped = self
            .data
            .layers
            .iter()
            .enumerate()
            .flat_map(|(layer, l)| {
                l.volume
                    .iter()
                    .filter(move |(pos, _)| pos.as_uvec3().cmpge(limit).any())
                    .map(move |(pos, value)| ClippedVoxel { layer, pos, value })
            })
            .collect();
        self.apply(Edit::SetBounds { old, new, clipped }, merge);
    }

    pub fn set_palette_color(&mut self, index: usize, new: Rgba8, merge: bool) {
        if let Some(old) = self.data.palette.get(index) {
            if old != new {
                self.apply(Edit::SetPaletteColor { index, old, new }, merge);
            }
        }
    }

    fn unique_layer_name(&self) -> String {
        (1..)
            .map(|n| format!("Layer {n}"))
            .find(|name| !self.data.layers.iter().any(|l| &l.name == name))
            .expect("infinite iterator")
    }

    /// `base`, or `base 2`, `base 3`... when that name is taken.
    fn unique_name_from(&self, base: &str) -> String {
        let taken = |name: &str| self.data.layers.iter().any(|l| l.name == name);
        if !taken(base) {
            return base.to_owned();
        }
        (2..)
            .map(|n| format!("{base} {n}"))
            .find(|name| !taken(name))
            .expect("infinite iterator")
    }

    // ---------------------------------------------------------- file I/O

    pub fn to_project(&self) -> ProjectFile {
        ProjectFile {
            name: self.name.clone(),
            palette: self.data.palette.clone(),
            selected_color: self.selected_color,
            layers: self
                .data
                .layers
                .iter()
                .map(|l| LayerData {
                    name: l.name.clone(),
                    visible: l.visible,
                    locked: l.locked,
                    voxels: rizxel_io::runs_from_volume(self.data.bounds, &l.volume),
                })
                .collect(),
            active_layer: self.active_layer,
            bounds: self.data.bounds,
        }
    }

    /// Builds a document from a decoded project. Fails when a layer's voxel
    /// runs do not fit the project bounds.
    pub fn from_project(project: ProjectFile, path: Option<PathBuf>) -> Result<Self, RzxlError> {
        // Re-clamp: the file may carry out-of-range bounds.
        let bounds = BoundSize::new(project.bounds.x(), project.bounds.y(), project.bounds.z());
        let layers = project
            .layers
            .into_iter()
            .map(|l| {
                Ok(Layer {
                    volume: rizxel_io::volume_from_runs(bounds, &l.voxels)?,
                    name: l.name,
                    visible: l.visible,
                    locked: l.locked,
                })
            })
            .collect::<Result<Vec<_>, RzxlError>>()?;
        let mut doc = Self::with_bounds(project.name, bounds, layers);
        doc.data.palette = project.palette;
        doc.active_layer = project.active_layer;
        doc.selected_color = project.selected_color;
        doc.path = path;
        doc.touch();
        doc.saved_revision = doc.revision;
        Ok(doc)
    }

    pub fn load(path: &Path) -> Result<Self, RzxlError> {
        let project = rizxel_io::load_rzxl(path)?;
        Self::from_project(project, Some(path.to_path_buf()))
    }

    /// Saves to `path` and makes it the document's path.
    pub fn save_to(&mut self, path: &Path) -> Result<(), RzxlError> {
        rizxel_io::save_rzxl(path, &self.to_project())?;
        self.path = Some(path.to_path_buf());
        if let Some(stem) = path.file_stem() {
            self.name = stem.to_string_lossy().into_owned();
        }
        self.saved_revision = self.revision;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Document with layers named `names`, each holding one voxel at x = index.
    fn doc_with(names: &[&str]) -> Document {
        let layers = names
            .iter()
            .enumerate()
            .map(|(i, n)| {
                let mut l = Layer::new(*n, BoundSize::default());
                l.volume.set(IVec3::new(i as i32, 0, 0), 2 + i as Voxel);
                l
            })
            .collect();
        Document::with_layers("t", layers)
    }

    fn names(doc: &Document) -> Vec<&str> {
        doc.data.layers.iter().map(|l| l.name.as_str()).collect()
    }

    #[test]
    fn move_layer_reorders_and_undoes() {
        let mut d = doc_with(&["A", "B", "C"]);
        d.active_layer = 0;
        assert!(d.move_active_layer(true));
        assert_eq!(names(&d), ["B", "A", "C"]);
        assert_eq!(d.active_layer, 1);
        assert!(d.move_layer(2, 0));
        assert_eq!(names(&d), ["C", "B", "A"]);
        assert_eq!(d.active_layer, 2, "active follows its layer");
        d.undo();
        d.undo();
        assert_eq!(names(&d), ["A", "B", "C"]);
        d.active_layer = 0;
        assert!(!d.move_active_layer(false));
    }

    #[test]
    fn duplicate_copies_voxels_with_unique_name() {
        let mut d = doc_with(&["A"]);
        assert!(d.duplicate_active_layer());
        assert!(d.duplicate_active_layer());
        assert_eq!(names(&d), ["A", "A copy", "A copy copy"]);
        assert_eq!(d.data.layers[2].volume.get(IVec3::ZERO), 2);
        d.active_layer = 0;
        d.duplicate_active_layer();
        assert_eq!(d.data.layers[1].name, "A copy 2");
    }

    #[test]
    fn merge_down_combines_and_undoes_in_one_step() {
        let mut d = doc_with(&["A", "B"]);
        // B also covers A's voxel with its own color: the upper layer wins.
        d.data.layers[1].volume.set(IVec3::ZERO, 9);
        d.active_layer = 1;
        assert!(d.merge_active_layer_down());
        assert_eq!(names(&d), ["A"]);
        assert_eq!(d.data.layers[0].volume.get(IVec3::ZERO), 9);
        assert_eq!(d.data.layers[0].volume.get(IVec3::X), 3);
        assert!(d.undo());
        assert_eq!(names(&d), ["A", "B"]);
        assert_eq!(d.data.layers[0].volume.voxel_count(), 1);
        assert_eq!(d.data.layers[0].volume.get(IVec3::ZERO), 2);
        d.active_layer = 0;
        assert!(!d.merge_active_layer_down());
    }

    #[test]
    fn merge_visible_skips_hidden_layers() {
        let mut d = doc_with(&["A", "B", "C"]);
        d.toggle_layer_visible(1);
        assert!(d.merge_visible_layers());
        assert_eq!(names(&d), ["A", "B"]);
        assert_eq!(d.data.layers[0].volume.voxel_count(), 2);
        d.undo();
        assert_eq!(names(&d), ["A", "B", "C"]);
    }

    #[test]
    fn delete_hidden_keeps_one_layer() {
        let mut d = doc_with(&["A", "B", "C"]);
        d.active_layer = 2;
        d.toggle_layer_visible(0);
        assert!(d.delete_hidden_layers());
        assert_eq!(names(&d), ["B", "C"]);
        assert_eq!(d.data.layers[d.active_layer].name, "C");
        d.toggle_layer_visible(0);
        d.toggle_layer_visible(1);
        assert!(!d.delete_hidden_layers(), "all hidden: refused");
    }

    #[test]
    fn solo_toggles_between_one_and_all() {
        let mut d = doc_with(&["A", "B", "C"]);
        d.solo_layer(1);
        let vis = |d: &Document| d.data.layers.iter().map(|l| l.visible).collect::<Vec<_>>();
        assert_eq!(vis(&d), [false, true, false]);
        d.solo_layer(1);
        assert_eq!(vis(&d), [true, true, true]);
        d.solo_layer(1);
        d.undo();
        assert_eq!(vis(&d), [true, true, true]);
    }

    #[test]
    fn layer_ops_are_undoable() {
        let mut d = Document::new("t");
        d.add_layer();
        assert_eq!(d.data.layers.len(), 2);
        assert_eq!(d.active_layer, 1);
        assert_eq!(d.data.layers[1].name, "Layer 2");
        assert!(d.is_dirty());

        d.toggle_layer_visible(1);
        assert!(!d.data.layers[1].visible);

        assert!(d.undo());
        assert!(d.data.layers[1].visible);
        assert!(d.undo());
        assert_eq!(d.data.layers.len(), 1);
        assert!(!d.undo());

        assert!(d.redo());
        assert_eq!(d.data.layers.len(), 2);
    }

    #[test]
    fn last_layer_cannot_be_deleted() {
        let mut d = Document::new("t");
        assert!(!d.delete_active_layer());
        d.add_layer();
        assert!(d.delete_active_layer());
        assert_eq!(d.active_layer, 0);
        assert!(d.undo());
        assert_eq!(d.data.layers.len(), 2);
    }

    #[test]
    fn bound_drags_merge_into_one_step() {
        let mut d = Document::new("t");
        d.set_bounds(BoundSize::new(33, 32, 32), false);
        d.set_bounds(BoundSize::new(40, 32, 32), true);
        d.set_bounds(BoundSize::new(48, 32, 32), true);
        assert!(d.undo());
        assert_eq!(d.data.bounds, BoundSize::default());
        assert!(!d.can_undo());
    }

    #[test]
    fn project_roundtrip_preserves_state() {
        let mut d = Document::new("hero");
        d.add_layer();
        d.toggle_layer_locked(0);
        d.set_bounds(BoundSize::new(8, 16, 24), false);
        d.selected_color = 3;

        d.set_voxels(
            1,
            [(IVec3::new(1, 2, 3), 5), (IVec3::new(7, 0, 0), 9)],
            false,
        );

        let back = Document::from_project(d.to_project(), None).unwrap();
        assert_eq!(back.data, d.data);
        assert_eq!(back.data.layers[1].volume.voxel_count(), 2);
        assert_eq!(back.active_layer, d.active_layer);
        assert_eq!(back.selected_color, 3);
        assert!(!back.is_dirty());
        assert!(!back.can_undo());
        assert_ne!(back.id(), d.id());
    }

    #[test]
    fn voxel_writes_are_undoable_and_bump_revision() {
        let mut d = Document::new("t");
        let r0 = d.revision();
        assert!(d.set_voxels(0, [(IVec3::ZERO, 4), (IVec3::X, 4)], false));
        assert!(d.revision() > r0);
        assert!(d.is_dirty());
        // No-op and out-of-bounds writes produce no edit.
        let r1 = d.revision();
        assert!(!d.set_voxels(0, [(IVec3::ZERO, 4), (IVec3::splat(99), 1)], false));
        assert_eq!(d.revision(), r1);
        // A drag merges into one undo step.
        d.set_voxels(0, [(IVec3::Y, 2)], true);
        d.set_voxels(0, [(IVec3::Z, 2)], true);
        assert_eq!(d.visible_voxel_count(), 4);
        assert!(d.undo());
        assert_eq!(d.visible_voxel_count(), 0);
        assert!(!d.can_undo());
        assert!(d.redo());
        assert_eq!(d.data.layers[0].volume.get(IVec3::Z), 2);
    }

    #[test]
    fn hidden_layers_leave_the_mesh() {
        let mut d = Document::new("t");
        d.set_voxels(0, [(IVec3::ZERO, 4)], false);
        assert_eq!(d.visible_mesh().triangle_count(), 12);
        d.toggle_layer_visible(0);
        assert!(d.visible_mesh().is_empty());
        assert_eq!(d.visible_voxel_count(), 0);
        assert_eq!(d.occupied_bounds(), None);
    }

    #[test]
    fn shrinking_bounds_is_undoable() {
        let mut d = Document::new("t");
        d.set_voxels(
            0,
            [(IVec3::new(2, 2, 2), 1), (IVec3::new(20, 2, 2), 2)],
            false,
        );
        // Drag 32 -> 16 -> 30: the voxel at x=20 is clipped mid-drag.
        d.set_bounds(BoundSize::new(16, 32, 32), false);
        d.set_bounds(BoundSize::new(30, 32, 32), true);
        assert_eq!(d.data.layers[0].volume.voxel_count(), 1);
        assert_eq!(d.data.layers[0].volume.bounds(), BoundSize::new(30, 32, 32));
        assert!(d.undo());
        assert_eq!(d.data.bounds, BoundSize::default());
        assert_eq!(d.data.layers[0].volume.get(IVec3::new(20, 2, 2)), 2);
        // Redo reproduces the live state, including the clipped voxel.
        assert!(d.redo());
        assert_eq!(d.data.layers[0].volume.voxel_count(), 1);
    }

    #[test]
    fn slot_zero_is_never_selected() {
        let mut d = Document::new("t");
        assert!(!d.select_color(0));
        assert!(d.select_color(1));
        let mut p = d.to_project();
        p.selected_color = 0;
        assert_eq!(Document::from_project(p, None).unwrap().selected_color, 1);
    }
}

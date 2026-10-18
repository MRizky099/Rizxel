//! A single open project (one tab) and its undoable data.

use std::path::{Path, PathBuf};

use rizxel_io::{LayerData, ProjectFile, RzxlError};
use rizxel_math::{BoundSize, Palette, Rgba8};

use crate::history::{Edit, History};

/// A named group of voxels. Voxel storage is added in Phase 4.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layer {
    pub name: String,
    pub visible: bool,
    pub locked: bool,
}

impl Layer {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            visible: true,
            locked: false,
        }
    }
}

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
        Self::with_layers(name, vec![Layer::new("Layer 1")])
    }

    pub fn with_layers(name: impl Into<String>, layers: Vec<Layer>) -> Self {
        let layers = if layers.is_empty() {
            vec![Layer::new("Layer 1")]
        } else {
            layers
        };
        Self {
            name: name.into(),
            path: None,
            data: DocData {
                palette: Palette::default(),
                layers,
                bounds: BoundSize::default(),
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
        let last = self.data.palette.len().saturating_sub(1);
        self.selected_color = self.selected_color.min(last);
    }

    // ------------------------------------------------------ layer helpers

    /// Adds a new layer above the active one and selects it.
    pub fn add_layer(&mut self) {
        let index = (self.active_layer + 1).min(self.data.layers.len());
        let name = self.unique_layer_name();
        self.apply(
            Edit::InsertLayer {
                index,
                layer: Layer::new(name),
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

    pub fn set_bounds(&mut self, new: BoundSize, merge: bool) {
        let old = self.data.bounds;
        if old != new {
            self.apply(Edit::SetBounds { old, new }, merge);
        }
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
                })
                .collect(),
            active_layer: self.active_layer,
            bounds: self.data.bounds,
        }
    }

    pub fn from_project(project: ProjectFile, path: Option<PathBuf>) -> Self {
        let layers = project
            .layers
            .into_iter()
            .map(|l| Layer {
                name: l.name,
                visible: l.visible,
                locked: l.locked,
            })
            .collect();
        let mut doc = Self::with_layers(project.name, layers);
        doc.data.palette = project.palette;
        doc.data.bounds =
            BoundSize::new(project.bounds.x(), project.bounds.y(), project.bounds.z());
        doc.active_layer = project.active_layer;
        doc.selected_color = project.selected_color;
        doc.path = path;
        doc.touch();
        doc.saved_revision = doc.revision;
        doc
    }

    pub fn load(path: &Path) -> Result<Self, RzxlError> {
        let project = rizxel_io::load_rzxl(path)?;
        Ok(Self::from_project(project, Some(path.to_path_buf())))
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

        let back = Document::from_project(d.to_project(), None);
        assert_eq!(back.data, d.data);
        assert_eq!(back.active_layer, d.active_layer);
        assert_eq!(back.selected_color, 3);
        assert!(!back.is_dirty());
        assert!(!back.can_undo());
    }
}

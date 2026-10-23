//! A single open project (one tab) and its undoable data.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use glam::IVec3;
use rizxel_engine::tools::slice_layers;
use rizxel_engine::{
    greedy_mesh, rasterize_shape, MaskMode, Mesh, SliceRequest, TileMeshCache, WorkPlane,
};
use rizxel_io::{LayerData, ProjectFile, RzxlError};
use rizxel_math::{
    BoundSize, LayerId, LayerKind, Material, Palette, Rgba8, ShapeKind, Voxel, VoxelBox,
    VoxelVolume, EMPTY_VOXEL, FIRST_COLOR_INDEX,
};

use crate::clipboard::Clipboard;
use crate::history::{ClippedVoxel, Edit, History, VoxelChange};
use crate::selection::Selection;

/// Layer properties other than name, visibility, lock and voxels. Changed as
/// one undoable unit through [`Document::set_layer_props`].
#[derive(Debug, Clone, PartialEq)]
pub struct LayerProps {
    /// `0..=1`, multiplies the layer's alpha when rendering.
    pub opacity: f32,
    /// Group layer containing this layer.
    pub parent: Option<LayerId>,
    /// Index into [`DocData::materials`].
    pub material: Option<u32>,
    /// Non-destructive crop box.
    pub crop: Option<VoxelBox>,
    pub kind: LayerKind,
}

impl Default for LayerProps {
    fn default() -> Self {
        Self {
            opacity: 1.0,
            parent: None,
            material: None,
            crop: None,
            kind: LayerKind::Voxels,
        }
    }
}

/// One layer of the document. `volume` always has the document's bounds and
/// holds voxels only for [`LayerKind::Voxels`] layers.
#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    /// Stable id (non-zero once the layer is in a document).
    pub id: LayerId,
    pub name: String,
    pub visible: bool,
    pub locked: bool,
    pub props: LayerProps,
    pub volume: VoxelVolume,
    /// Selected cells (Selection tools). Travels with the layer through
    /// reorders and undo, changes only through [`Edit::SetSelection`] and is
    /// never saved to `.rzxl`.
    pub selection: Selection,
}

impl Layer {
    /// Voxel layer with default properties and id 0 (assigned on insertion).
    pub fn new(name: impl Into<String>, bounds: BoundSize) -> Self {
        Self {
            id: 0,
            name: name.into(),
            visible: true,
            locked: false,
            props: LayerProps::default(),
            volume: VoxelVolume::new(bounds),
            selection: Selection::default(),
        }
    }

    pub fn kind(&self) -> &LayerKind {
        &self.props.kind
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
    /// Materials referenced by [`LayerProps::material`].
    pub materials: Vec<Material>,
}

impl DocData {
    pub fn layer_index(&self, id: LayerId) -> Option<usize> {
        self.layers.iter().position(|l| l.id == id)
    }

    /// Visible itself and inside visible groups only.
    pub fn effectively_visible(&self, index: usize) -> bool {
        let Some(mut layer) = self.layers.get(index) else {
            return false;
        };
        // Depth-capped walk; validated files cannot cycle, but stay safe.
        for _ in 0..=rizxel_io::MAX_GROUP_DEPTH {
            if !layer.visible {
                return false;
            }
            match layer
                .props
                .parent
                .and_then(|p| self.layer_index(p))
                .and_then(|i| self.layers.get(i))
            {
                Some(parent) => layer = parent,
                None => return true,
            }
        }
        false
    }

    /// Geometry a layer contributes before visibility: own voxels, the
    /// rasterized shape or the moved clone source, then the crop box.
    /// `None` for groups and broken clones.
    fn evaluate(&self, index: usize) -> Option<Derived> {
        let layer = self.layers.get(index)?;
        let mut derived = match layer.props.kind {
            LayerKind::Group { .. } => return None,
            LayerKind::Voxels => Derived::Own,
            LayerKind::Shape { shape, area, color } => {
                Derived::Owned(rasterize_shape(shape, &area, color, self.bounds))
            }
            LayerKind::Clone { source, offset } => {
                let src = self.layer_index(source)?;
                let base = match self.layers.get(src)?.props.kind {
                    LayerKind::Voxels => self.layers.get(src)?.volume.translated(offset),
                    LayerKind::Shape { shape, area, color } => {
                        rasterize_shape(shape, &area, color, self.bounds).translated(offset)
                    }
                    _ => return None,
                };
                Derived::Owned(base)
            }
        };
        if let Some(crop) = &layer.props.crop {
            let cropped = match &derived {
                Derived::Own => layer.volume.cropped(crop),
                Derived::Owned(v) => v.cropped(crop),
            };
            derived = Derived::Owned(cropped);
        }
        Some(derived)
    }
}

/// Why a layer cannot take voxel edits from a tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditBlock {
    NoLayer,
    Locked,
    Hidden,
    /// Groups, shapes and linked clones do not store voxels.
    NotVoxels,
}

impl EditBlock {
    pub const fn message(self) -> &'static str {
        match self {
            EditBlock::NoLayer => "No active layer",
            EditBlock::Locked => "The active layer is locked",
            EditBlock::Hidden => "The active layer is hidden",
            EditBlock::NotVoxels => {
                "The active layer has no editable voxels (group, shape or clone)"
            }
        }
    }
}

/// Summary of a [`Document::slice`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SliceOutcome {
    /// Voxels cleared.
    pub removed: usize,
    /// Cap voxels written.
    pub capped: usize,
    /// Layers that changed.
    pub layers: usize,
    /// Visible, non-empty voxel layers skipped because they are locked.
    pub locked_skipped: usize,
}

/// A paste that follows the cursor (like a selection Move) until it is
/// committed (`Enter`, click) or cancelled (`Esc`). Its voxels are written
/// tentatively inside an open edit transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FloatingPaste {
    /// Target layer index.
    pub layer: usize,
    /// Minimum corner of the pasted box.
    pub origin: IVec3,
    /// Size of the pasted box.
    pub size: IVec3,
    /// Follows the cursor (Paste); Paste in Place stays where it was copied.
    pub follows_cursor: bool,
    /// Cells relative to `origin`, mapped onto the document palette.
    cells: Vec<(IVec3, Voxel)>,
}

impl FloatingPaste {
    /// Box origin that puts the paste's bottom center on `cell`.
    pub fn origin_at(&self, cell: IVec3) -> IVec3 {
        crate::clipboard::bottom_center_origin(self.size, cell)
    }

    /// Cells at their current position.
    pub fn placed(&self) -> impl Iterator<Item = (IVec3, Voxel)> + '_ {
        self.cells.iter().map(|&(p, v)| (p + self.origin, v))
    }
}

/// Result of [`DocData::evaluate`]: the layer's own volume or a computed one.
#[derive(Debug)]
enum Derived {
    Own,
    Owned(VoxelVolume),
}

/// Geometry of the effectively visible layers for one revision, in layer
/// order. `None` entries mean "use the layer's own volume".
#[derive(Debug)]
struct GeometryCache {
    revision: u64,
    layers: Vec<(usize, Option<VoxelVolume>)>,
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
    /// Plane tool work plane / slice view. Not part of undo history.
    pub plane: WorkPlane,
    history: History,
    /// Incremented on every data change; compared with `saved_revision`.
    revision: u64,
    saved_revision: u64,
    /// Undo steps recorded, undone or redone so far (autosave counts them).
    edits: u64,
    /// Paste in progress (see [`Document::begin_paste`]).
    paste: Option<FloatingPaste>,
    /// Derived layer geometry, rebuilt lazily when `revision` changes. Code
    /// that mutates `data` directly (bypassing [`Edit`]s) must not rely on it.
    geometry: RefCell<Option<GeometryCache>>,
    /// Per-tile meshes of the last [`Document::view_mesh`], so a drag
    /// re-meshes only the tiles it touched.
    view_tiles: RefCell<TileMeshCache>,
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
        let mut next_id = layers.iter().map(|l| l.id).max().unwrap_or(0);
        for layer in &mut layers {
            layer.volume.resize(bounds);
            if layer.id == 0 {
                next_id += 1;
                layer.id = next_id;
            }
        }
        Self {
            id: NEXT_DOC_ID.fetch_add(1, Ordering::Relaxed),
            name: name.into(),
            path: None,
            data: DocData {
                palette: Palette::default(),
                layers,
                bounds,
                materials: Vec::new(),
            },
            active_layer: 0,
            selected_color: Palette::DEFAULT_SELECTED,
            plane: WorkPlane::default(),
            history: History::default(),
            revision: 0,
            saved_revision: 0,
            edits: 0,
            paste: None,
            geometry: RefCell::new(None),
            view_tiles: RefCell::new(TileMeshCache::default()),
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

    /// Undo steps recorded, undone or redone since the document was created
    /// (a whole drag counts once).
    pub fn edit_count(&self) -> u64 {
        self.edits
    }

    /// Bumped on every data change (edit, undo, redo, load). Caches derived
    /// from [`DocData`] are valid while `(id, revision)` is unchanged.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Runs `f` with the final geometry of every effectively visible layer,
    /// bottom to top: voxel layers, rasterized shapes and moved linked
    /// clones, each clipped by its crop box. Groups contribute nothing; a
    /// hidden group hides its children. Cached per revision.
    pub fn with_visible_geometry<R>(&self, f: impl FnOnce(&[&VoxelVolume]) -> R) -> R {
        let mut cache = self.geometry.borrow_mut();
        if cache.as_ref().is_none_or(|c| c.revision != self.revision) {
            let layers = (0..self.data.layers.len())
                .filter(|&i| self.data.effectively_visible(i))
                .filter_map(|i| {
                    self.data.evaluate(i).map(|d| match d {
                        Derived::Own => (i, None),
                        Derived::Owned(v) => (i, Some(v)),
                    })
                })
                .collect();
            *cache = Some(GeometryCache {
                revision: self.revision,
                layers,
            });
        }
        let volumes: Vec<&VoxelVolume> = cache
            .as_ref()
            .map(|c| {
                c.layers
                    .iter()
                    .filter_map(|(i, v)| v.as_ref().or(self.data.layers.get(*i).map(|l| &l.volume)))
                    .collect()
            })
            .unwrap_or_default();
        f(&volumes)
    }

    /// Greedy mesh of the visible layers, in model space.
    pub fn visible_mesh(&self) -> Mesh {
        self.with_visible_geometry(|volumes| {
            let layers: Vec<(&VoxelVolume, bool)> = volumes.iter().map(|v| (*v, true)).collect();
            greedy_mesh(&layers, &self.data.palette)
        })
    }

    /// Region shown in the viewport: the slice layer and the far side of
    /// it, or `None` (everything) when not slicing.
    pub fn view_clip(&self) -> Option<VoxelBox> {
        self.plane.clip_box(self.data.bounds)
    }

    /// Region tools may pick and edit: the slice layer while slicing, or
    /// `None` (everything) otherwise.
    pub fn edit_clip(&self) -> Option<VoxelBox> {
        self.plane.slice_box(self.data.bounds)
    }

    /// Viewport mesh: like [`Document::visible_mesh`] but clipped to
    /// [`Document::view_clip`]. The mesher closes the cut, so the sliced
    /// model shows a solid cut face. Incremental: only tiles that changed
    /// since the previous call are re-meshed (same output as a full mesh).
    pub fn view_mesh(&self) -> Mesh {
        let clip = self.view_clip();
        self.with_visible_geometry(|volumes| {
            let palette = &self.data.palette;
            let mut tiles = self.view_tiles.borrow_mut();
            let merged;
            let volume = match (volumes, clip) {
                ([], _) => return Mesh::default(),
                ([single], None) => *single,
                _ => {
                    let mut out = VoxelVolume::new(self.data.bounds);
                    for v in volumes {
                        for (pos, value) in v.iter() {
                            if clip.is_none_or(|c| c.contains(pos)) {
                                out.set(pos, value);
                            }
                        }
                    }
                    merged = out;
                    &merged
                }
            };
            tiles.mesh(volume, palette)
        })
    }

    /// Voxel count summed over visible layers (overlaps count once per layer).
    pub fn visible_voxel_count(&self) -> usize {
        self.with_visible_geometry(|volumes| volumes.iter().map(|v| v.voxel_count()).sum())
    }

    /// Box around the voxels of the visible layers: `(min, max exclusive)`.
    pub fn occupied_bounds(&self) -> Option<(IVec3, IVec3)> {
        self.with_visible_geometry(|volumes| {
            volumes
                .iter()
                .filter_map(|v| v.occupied_bounds())
                .reduce(|(a0, a1), (b0, b1)| (a0.min(b0), a1.max(b1)))
        })
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
        self.tag_history();
        edit.apply(&mut self.data);
        self.history.push(edit, merge);
        self.edits += 1;
        self.touch();
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// Undoes the last step (an edit in progress is committed first, so it
    /// is what gets undone).
    /// While slicing, the slice moves back to the one the step was made on.
    /// A floating paste is cancelled instead (it is the step in progress).
    pub fn undo(&mut self) -> bool {
        if self.paste.is_some() {
            self.cancel_paste();
            return true;
        }
        let done = self.history.undo(&mut self.data);
        if done {
            self.edits += 1;
            self.touch();
            if let Some(p) = self.history.undone_plane() {
                self.plane.restore_slice(&p, self.data.bounds);
            }
        }
        done
    }

    pub fn redo(&mut self) -> bool {
        if self.paste.is_some() {
            return false;
        }
        let done = self.history.redo(&mut self.data);
        if done {
            self.edits += 1;
            self.touch();
            if let Some(p) = self.history.redone_plane() {
                self.plane.restore_slice(&p, self.data.bounds);
            }
        }
        done
    }

    /// Records the current slice with the next undo steps.
    fn tag_history(&mut self) {
        self.history.tag = self.plane.slice.then_some(self.plane);
    }

    // ---------------------------------------------------------- transactions

    /// Opens an edit transaction: every voxel change until
    /// [`Document::commit_edit`] becomes one undo step (one tool drag).
    pub fn begin_edit(&mut self) {
        if !self.is_editing() {
            self.tag_history();
        }
        self.history.begin_transaction();
    }

    /// True while a transaction is open.
    pub fn is_editing(&self) -> bool {
        self.history.in_transaction()
    }

    /// Number of voxels the open transaction changes so far.
    pub fn pending_changes(&self) -> usize {
        self.history.pending_count()
    }

    /// Writes `changes` (their `new` values; `old` is re-read from the layer)
    /// into a voxel layer. Inside a transaction they are tentative; without
    /// one they form their own undo step. Locked layers, non-voxel layers and
    /// out-of-bounds positions are refused. Returns the number of voxels that
    /// actually changed.
    pub fn edit_voxels(&mut self, layer: usize, changes: &[VoxelChange]) -> usize {
        if self
            .editable_layer(layer)
            .is_err_and(|e| e != EditBlock::Hidden)
        {
            return 0;
        }
        let auto = !self.is_editing();
        if auto {
            self.begin_edit();
        }
        let mut applied = Vec::with_capacity(changes.len());
        if let Some(l) = self.data.layers.get_mut(layer) {
            for c in changes {
                if !l.volume.in_bounds(c.pos) {
                    continue;
                }
                let old = l.volume.set(c.pos, c.new);
                if old != c.new {
                    applied.push(VoxelChange {
                        pos: c.pos,
                        old,
                        new: c.new,
                    });
                }
            }
        }
        self.history.record(layer, &applied);
        if !applied.is_empty() {
            self.touch();
        }
        if auto {
            self.commit_edit();
        }
        applied.len()
    }

    /// Restores everything the open transaction changed but keeps it open,
    /// for tools that recompute their result from the drag anchor.
    pub fn revert_pending(&mut self) {
        if self.history.revert_pending(&mut self.data) {
            self.touch();
        }
    }

    /// Closes the transaction as one undo step. Returns `false` when it
    /// changed nothing (no undo entry is recorded then).
    pub fn commit_edit(&mut self) -> bool {
        self.paste = None;
        if !self.is_editing() {
            return false;
        }
        let done = self.history.commit_transaction();
        self.edits += u64::from(done);
        self.touch();
        done
    }

    /// Closes the transaction as one undo step that also replaces layer
    /// `layer`'s selection with `selection` (Move and Extrude carry the
    /// selection along). Returns `false` when nothing changed.
    pub fn commit_edit_with_selection(&mut self, layer: usize, selection: Selection) -> bool {
        self.paste = None;
        if !self.is_editing() {
            return false;
        }
        let extra = match self.data.layers.get(layer) {
            Some(l) if l.selection != selection => {
                let edit = Edit::SetSelection {
                    layer,
                    old: l.selection.clone(),
                    new: selection,
                };
                edit.apply(&mut self.data);
                vec![edit]
            }
            _ => Vec::new(),
        };
        let done = self.history.commit_transaction_with(extra);
        self.edits += u64::from(done);
        self.touch();
        done
    }

    /// Closes the transaction and restores every voxel it changed (a
    /// floating paste disappears).
    pub fn cancel_edit(&mut self) {
        self.paste = None;
        if self.is_editing() {
            self.history.cancel_transaction(&mut self.data);
            self.touch();
        }
    }

    /// Whether tools may write into layer `index`. Hidden layers are
    /// reported so the UI can warn; [`Document::edit_voxels`] still allows
    /// them.
    pub fn editable_layer(&self, index: usize) -> Result<(), EditBlock> {
        let layer = self.data.layers.get(index).ok_or(EditBlock::NoLayer)?;
        if !layer.kind().stores_voxels() {
            Err(EditBlock::NotVoxels)
        } else if layer.locked {
            Err(EditBlock::Locked)
        } else if !self.data.effectively_visible(index) {
            Err(EditBlock::Hidden)
        } else {
            Ok(())
        }
    }

    /// Union of the visible geometry in one volume (upper layers win), used
    /// as the pick target while a drag modifies the document.
    pub fn merged_visible_volume(&self) -> VoxelVolume {
        self.with_visible_geometry(|volumes| {
            let mut out = VoxelVolume::new(self.data.bounds);
            for v in volumes {
                for (pos, value) in v.iter() {
                    out.set(pos, value);
                }
            }
            out
        })
    }

    /// Plane tool, Slice mode: cuts every visible, unlocked voxel layer at
    /// `req` as one undo step. Hidden layers are skipped silently; locked
    /// ones are counted in the outcome.
    pub fn slice(&mut self, req: &SliceRequest, cap_color: Voxel) -> SliceOutcome {
        let mut outcome = SliceOutcome::default();
        let mut targets: Vec<(usize, &VoxelVolume)> = Vec::new();
        for (i, l) in self.data.layers.iter().enumerate() {
            if !l.kind().stores_voxels() || !self.data.effectively_visible(i) {
                continue;
            }
            if l.locked {
                outcome.locked_skipped += usize::from(!l.volume.is_empty());
                continue;
            }
            targets.push((i, &l.volume));
        }
        let per_layer = slice_layers(&targets, self.data.bounds, req, cap_color);
        if per_layer.is_empty() {
            return outcome;
        }
        outcome.layers = per_layer.len();
        for c in per_layer.iter().flat_map(|(_, changes)| changes) {
            if c.new == EMPTY_VOXEL {
                outcome.removed += 1;
            } else {
                outcome.capped += 1;
            }
        }
        let mut edits: Vec<Edit> = per_layer
            .into_iter()
            .map(|(layer, changes)| Edit::Voxels { layer, changes })
            .collect();
        let edit = match edits.len() {
            1 => edits.pop().unwrap_or(Edit::Batch(Vec::new())),
            _ => Edit::Batch(edits),
        };
        self.apply(edit, false);
        outcome
    }

    /// Destructive "Cut" of the Plane tool: deletes the side hidden by the
    /// slice plane from every visible, unlocked voxel layer (one undo step).
    pub fn cut_hidden_side(&mut self, cap: bool, cap_color: Voxel) -> SliceOutcome {
        let req = self.plane.cut_request(self.data.bounds, cap);
        self.slice(&req, cap_color)
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

    // ----------------------------------------------------------- selection

    /// Selection of the active layer.
    pub fn selection(&self) -> Option<&Selection> {
        self.active_layer().map(|l| &l.selection)
    }

    /// Replaces layer `layer`'s selection as one undo step. Returns `false`
    /// (recording nothing) when it would not change.
    pub fn set_selection(&mut self, layer: usize, new: Selection) -> bool {
        let Some(l) = self.data.layers.get(layer) else {
            return false;
        };
        if l.selection == new {
            return false;
        }
        let old = l.selection.clone();
        self.apply(Edit::SetSelection { layer, old, new }, false);
        true
    }

    /// Combines `cells` with the active layer's selection by `mode`.
    pub fn select(&mut self, cells: impl IntoIterator<Item = IVec3>, mode: MaskMode) -> bool {
        let Some(current) = self.selection() else {
            return false;
        };
        let fresh: Selection = cells.into_iter().collect();
        let new = current.combine(&fresh, mode);
        self.set_selection(self.active_layer, new)
    }

    /// Deselects everything on the active layer (`Esc`).
    pub fn clear_selection(&mut self) -> bool {
        self.set_selection(self.active_layer, Selection::default())
    }

    /// Deletes the selected voxels of the active layer and clears the
    /// selection, as one undo step. Returns the number of voxels removed.
    pub fn delete_selected(&mut self) -> usize {
        let layer = self.active_layer;
        if self
            .editable_layer(layer)
            .is_err_and(|e| e != EditBlock::Hidden)
        {
            return 0;
        }
        let Some(l) = self.data.layers.get(layer) else {
            return 0;
        };
        let changes: Vec<VoxelChange> = l
            .selection
            .iter()
            .filter_map(|pos| {
                let old = l.volume.get(pos);
                (old != EMPTY_VOXEL).then_some(VoxelChange {
                    pos,
                    old,
                    new: EMPTY_VOXEL,
                })
            })
            .collect();
        if changes.is_empty() {
            return 0;
        }
        self.begin_edit();
        let removed = self.edit_voxels(layer, &changes);
        self.commit_edit_with_selection(layer, Selection::default());
        removed
    }

    // ----------------------------------------------------------- clipboard

    /// Copies the selection (or the whole active layer), then deletes it, as
    /// one undo step. `None` when there is nothing to cut or the layer is
    /// locked or not a voxel layer.
    pub fn cut(&mut self) -> Option<Clipboard> {
        let layer = self.active_layer;
        if self
            .editable_layer(layer)
            .is_err_and(|e| e != EditBlock::Hidden)
        {
            return None;
        }
        let clip = Clipboard::copy(self)?;
        if self.selection().is_some_and(|s| !s.is_empty()) {
            self.delete_selected();
        } else {
            let changes: Vec<VoxelChange> = self.data.layers[layer]
                .volume
                .iter()
                .map(|(pos, old)| VoxelChange {
                    pos,
                    old,
                    new: EMPTY_VOXEL,
                })
                .collect();
            self.edit_voxels(layer, &changes);
        }
        Some(clip)
    }

    /// Starts a floating paste of `clip` on the active layer with its box at
    /// `origin`. Cells outside the bounds are clipped.
    pub fn begin_paste(
        &mut self,
        clip: &Clipboard,
        origin: IVec3,
        follows_cursor: bool,
    ) -> Result<(), EditBlock> {
        let layer = self.active_layer;
        if let Err(e) = self.editable_layer(layer) {
            if e != EditBlock::Hidden {
                return Err(e);
            }
        }
        self.cancel_edit();
        self.begin_edit();
        self.paste = Some(FloatingPaste {
            layer,
            origin,
            size: clip.size.as_ivec3(),
            follows_cursor,
            cells: clip.mapped(&self.data.palette),
        });
        self.write_paste();
        Ok(())
    }

    pub fn floating_paste(&self) -> Option<&FloatingPaste> {
        self.paste.as_ref()
    }

    /// Moves the floating paste so its box starts at `origin`.
    pub fn move_paste(&mut self, origin: IVec3) {
        match &mut self.paste {
            Some(p) if p.origin != origin => p.origin = origin,
            _ => return,
        }
        self.revert_pending();
        self.write_paste();
    }

    /// Runs `f` on the document as it is without the floating paste's
    /// voxels (the pick target while the paste follows the cursor).
    pub fn without_paste<R>(&mut self, f: impl FnOnce(&Self) -> R) -> R {
        if self.paste.is_none() {
            return f(self);
        }
        self.revert_pending();
        let out = f(self);
        self.write_paste();
        out
    }

    fn write_paste(&mut self) {
        let Some(p) = &self.paste else { return };
        let layer = p.layer;
        let changes: Vec<VoxelChange> = p
            .placed()
            .map(|(pos, new)| VoxelChange {
                pos,
                old: EMPTY_VOXEL,
                new,
            })
            .collect();
        self.edit_voxels(layer, &changes);
    }

    /// Commits the floating paste as one undo step; the pasted cells become
    /// the layer's selection. Returns the number of voxels pasted.
    pub fn commit_paste(&mut self) -> usize {
        let Some(p) = self.paste.take() else {
            return 0;
        };
        let bounds = self.data.bounds;
        let selection: Selection = p
            .placed()
            .map(|(pos, _)| pos)
            .filter(|&pos| VoxelBox::from_bounds(bounds).contains(pos))
            .collect();
        let count = selection.len();
        self.commit_edit_with_selection(p.layer, selection);
        count
    }

    /// Drops the floating paste; no undo step is recorded.
    pub fn cancel_paste(&mut self) {
        if self.paste.is_some() {
            self.cancel_edit();
        }
    }

    /// Pastes `clip` at its original position into a new layer above the
    /// active one, as one undo step. The pasted voxels are selected.
    pub fn paste_to_new_layer(&mut self, clip: &Clipboard) -> LayerId {
        self.cancel_edit();
        let mut layer = Layer::new(self.unique_name_from("Pasted"), self.data.bounds);
        for (p, v) in clip.mapped(&self.data.palette) {
            let pos = p + clip.origin;
            if layer.volume.in_bounds(pos) {
                layer.volume.set(pos, v);
                layer.selection.insert(pos);
            }
        }
        self.insert_above_active(layer)
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
        // Only voxel layers store voxels; shapes and clones are procedural.
        let Some(l) = self
            .data
            .layers
            .get(layer)
            .filter(|l| l.kind().stores_voxels())
        else {
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

    /// Next free layer id.
    fn next_layer_id(&self) -> LayerId {
        self.data
            .layers
            .iter()
            .map(|l| l.id)
            .max()
            .unwrap_or(0)
            .saturating_add(1)
    }

    /// Inserts `layer` above the active one (in the active layer's group),
    /// gives it a fresh id, selects it and returns the id.
    fn insert_above_active(&mut self, mut layer: Layer) -> LayerId {
        let index = (self.active_layer + 1).min(self.data.layers.len());
        layer.id = self.next_layer_id();
        if layer.props.parent.is_none() {
            layer.props.parent = self.active_layer().and_then(|a| match a.kind() {
                LayerKind::Group { .. } => Some(a.id),
                _ => a.props.parent,
            });
        }
        let id = layer.id;
        self.apply(Edit::InsertLayer { index, layer }, false);
        self.active_layer = index;
        id
    }

    /// Adds a new voxel layer above the active one and selects it.
    pub fn add_layer(&mut self) -> LayerId {
        let name = self.unique_layer_name();
        self.insert_above_active(Layer::new(name, self.data.bounds))
    }

    /// Adds an empty group above the active layer. Layers join it by setting
    /// their `parent` (see [`Document::set_layer_props`]).
    pub fn add_group(&mut self) -> LayerId {
        let mut layer = Layer::new(self.unique_name_from("Group"), self.data.bounds);
        layer.props.kind = LayerKind::Group { collapsed: false };
        self.insert_above_active(layer)
    }

    /// Adds a linked clone of layer `source` (a voxel or shape layer) moved by
    /// `offset`. The clone follows every later change to its source.
    pub fn add_linked_clone(&mut self, source: usize, offset: IVec3) -> Option<LayerId> {
        let src = self.data.layers.get(source)?;
        if !src.kind().is_clone_source() {
            return None;
        }
        let mut layer = Layer::new(
            self.unique_name_from(&format!("{} (linked)", src.name)),
            self.data.bounds,
        );
        layer.props.kind = LayerKind::Clone {
            source: src.id,
            offset,
        };
        layer.props.parent = src.props.parent;
        layer.props.material = src.props.material;
        Some(self.insert_above_active(layer))
    }

    /// Adds a procedural shape layer filling `area` with palette `color`.
    pub fn add_shape_layer(&mut self, shape: ShapeKind, area: VoxelBox, color: Voxel) -> LayerId {
        let mut layer = Layer::new(self.unique_name_from(shape.label()), self.data.bounds);
        layer.props.kind = LayerKind::Shape { shape, area, color };
        self.insert_above_active(layer)
    }

    /// Changes opacity, group, material, crop or kind of a layer as one undo
    /// step (`merge` folds slider drags). Refused when the result would break
    /// a link: parent not a group or creating a cycle, clone source missing,
    /// or a layer that clones are linked to stopping being a clone source.
    pub fn set_layer_props(&mut self, index: usize, new: LayerProps, merge: bool) -> bool {
        let Some(layer) = self.data.layers.get(index) else {
            return false;
        };
        let old = layer.props.clone();
        if old == new || !self.props_are_consistent(index, &new) {
            return false;
        }
        // Switching away from a voxel layer drops its voxels: keep them in the
        // edit so undo restores them.
        let mut edits = Vec::new();
        if old.kind.stores_voxels() && !new.kind.stores_voxels() && !layer.volume.is_empty() {
            let changes = layer
                .volume
                .iter()
                .map(|(pos, v)| VoxelChange {
                    pos,
                    old: v,
                    new: 0,
                })
                .collect();
            edits.push(Edit::Voxels {
                layer: index,
                changes,
            });
        }
        edits.push(Edit::SetLayerProps { index, old, new });
        let edit = match edits.len() {
            1 => edits.pop().unwrap_or(Edit::Batch(Vec::new())),
            _ => Edit::Batch(edits),
        };
        self.apply(edit, merge);
        true
    }

    fn props_are_consistent(&self, index: usize, new: &LayerProps) -> bool {
        let Some(layer) = self.data.layers.get(index) else {
            return false;
        };
        let opacity_ok = new.opacity.is_finite() && (0.0..=1.0).contains(&new.opacity);
        let material_ok = new
            .material
            .is_none_or(|m| (m as usize) < self.data.materials.len());
        let crop_ok = new.crop.is_none_or(|c| c.is_valid());
        // Parent chain: groups only, and never through this layer itself.
        let mut parent_ok = true;
        let mut parent = new.parent;
        for _ in 0..=rizxel_io::MAX_GROUP_DEPTH {
            let Some(p) = parent else { break };
            let group = self
                .data
                .layer_index(p)
                .and_then(|i| self.data.layers.get(i));
            match group {
                Some(g) if g.kind().is_group() && g.id != layer.id => parent = g.props.parent,
                _ => {
                    parent_ok = false;
                    break;
                }
            }
        }
        parent_ok &= parent.is_none();
        let kind_ok = match new.kind {
            LayerKind::Clone { source, .. } => {
                source != layer.id
                    && self
                        .data
                        .layer_index(source)
                        .and_then(|i| self.data.layers.get(i))
                        .is_some_and(|s| s.kind().is_clone_source())
            }
            LayerKind::Shape { area, color, .. } => {
                area.is_valid() && self.data.palette.is_selectable(color as usize)
            }
            _ => true,
        };
        // A layer others clone must stay a clone source; a group with
        // children must stay a group.
        let links_ok = (new.kind.is_clone_source() || !self.is_clone_source_in_use(index))
            && (new.kind.is_group() || !self.has_children(index));
        opacity_ok && material_ok && crop_ok && parent_ok && kind_ok && links_ok
    }

    /// True when some other layer is a linked clone of layer `index`.
    fn is_clone_source_in_use(&self, index: usize) -> bool {
        let Some(id) = self.data.layers.get(index).map(|l| l.id) else {
            return false;
        };
        self.data
            .layers
            .iter()
            .any(|l| matches!(l.props.kind, LayerKind::Clone { source, .. } if source == id))
    }

    fn has_children(&self, index: usize) -> bool {
        let Some(id) = self.data.layers.get(index).map(|l| l.id) else {
            return false;
        };
        self.data.layers.iter().any(|l| l.props.parent == Some(id))
    }

    /// Replaces the palette and material list as one undo step (MTL import).
    /// A shorter palette keeps the old colors in the slots it does not
    /// cover, so existing voxels never lose their color.
    pub fn replace_palette_and_materials(&mut self, palette: Palette, materials: Vec<Material>) {
        let mut colors = palette.colors().to_vec();
        colors.extend(self.data.palette.colors().iter().skip(colors.len()));
        let new = Palette::new(colors);
        let mut edits = Vec::new();
        if new != self.data.palette {
            edits.push(Edit::SetPalette {
                old: self.data.palette.clone(),
                new,
            });
        }
        let materials: Vec<Material> = materials
            .into_iter()
            .take(rizxel_io::MAX_MATERIALS)
            .collect();
        if materials != self.data.materials {
            // Layers pointing past the new list fall back to the default.
            edits.extend(
                self.data
                    .layers
                    .iter()
                    .enumerate()
                    .filter(|(_, l)| {
                        l.props
                            .material
                            .is_some_and(|m| m as usize >= materials.len())
                    })
                    .map(|(index, l)| Edit::SetLayerProps {
                        index,
                        old: l.props.clone(),
                        new: LayerProps {
                            material: None,
                            ..l.props.clone()
                        },
                    }),
            );
            edits.push(Edit::SetMaterials {
                old: self.data.materials.clone(),
                new: materials,
            });
        }
        if !edits.is_empty() {
            self.apply(Edit::Batch(edits), false);
        }
    }

    /// Everything exporters need: every non-group layer's final geometry
    /// (shapes rasterized, clones resolved, crops applied) with its
    /// effective visibility.
    pub fn export_scene(&self) -> rizxel_io::ExportScene {
        let layers = (0..self.data.layers.len())
            .filter_map(|i| {
                let geometry = match self.data.evaluate(i)? {
                    Derived::Own => self.data.layers[i].volume.clone(),
                    Derived::Owned(v) => v,
                };
                Some(rizxel_io::ExportLayer {
                    name: self.data.layers[i].name.clone(),
                    visible: self.data.effectively_visible(i),
                    volume: geometry,
                })
            })
            .collect();
        rizxel_io::ExportScene {
            name: self.name.clone(),
            bounds: self.data.bounds,
            palette: self.data.palette.clone(),
            layers,
        }
    }

    /// Flags the document as unsaved (imports have no `.rzxl` yet).
    pub fn mark_unsaved(&mut self) {
        self.saved_revision = u64::MAX;
    }

    /// Replaces the material list. Layers whose material index no longer
    /// exists fall back to the default material in the same undo step.
    pub fn set_materials(&mut self, new: Vec<Material>) -> bool {
        if new == self.data.materials || new.len() > rizxel_io::MAX_MATERIALS {
            return false;
        }
        let mut edits: Vec<Edit> = self
            .data
            .layers
            .iter()
            .enumerate()
            .filter(|(_, l)| l.props.material.is_some_and(|m| m as usize >= new.len()))
            .map(|(index, l)| Edit::SetLayerProps {
                index,
                old: l.props.clone(),
                new: LayerProps {
                    material: None,
                    ..l.props.clone()
                },
            })
            .collect();
        edits.push(Edit::SetMaterials {
            old: self.data.materials.clone(),
            new,
        });
        self.apply(Edit::Batch(edits), false);
        true
    }

    /// The active layer can be deleted unless it is the last layer or linked
    /// clones still point at it.
    pub fn can_delete_active_layer(&self) -> bool {
        self.data.layers.len() > 1 && !self.is_clone_source_in_use(self.active_layer)
    }

    /// Deletes the active layer. Children of a deleted group move up to the
    /// group's parent in the same undo step. Refused for the last layer and
    /// for layers that linked clones point at.
    pub fn delete_active_layer(&mut self) -> bool {
        if !self.can_delete_active_layer() {
            return false;
        }
        let index = self.active_layer;
        let Some(layer) = self.data.layers.get(index).cloned() else {
            return false;
        };
        let mut edits: Vec<Edit> = self
            .data
            .layers
            .iter()
            .enumerate()
            .filter(|(_, l)| l.props.parent == Some(layer.id))
            .map(|(i, l)| Edit::SetLayerProps {
                index: i,
                old: l.props.clone(),
                new: LayerProps {
                    parent: layer.props.parent,
                    ..l.props.clone()
                },
            })
            .collect();
        edits.push(Edit::RemoveLayer { index, layer });
        self.apply(Edit::Batch(edits), false);
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
        layer.selection = Selection::default();
        layer.name = self.unique_name_from(&format!("{} copy", source.name));
        layer.id = self.next_layer_id();
        let index = self.active_layer + 1;
        self.apply(Edit::InsertLayer { index, layer }, false);
        self.active_layer = index;
        true
    }

    /// True when layer `index` can be painted into and removed by a merge:
    /// the target must store voxels, sources must have geometry and must not
    /// be needed by layers that survive the merge.
    fn mergeable(&self, target: usize, sources: &[usize]) -> bool {
        let target_ok = self
            .data
            .layers
            .get(target)
            .is_some_and(|l| l.kind().stores_voxels());
        let sources_ok = sources.iter().all(|&i| {
            self.data.layers.get(i).is_some_and(|l| {
                let id = l.id;
                // Survivors that clone or are grouped under this source.
                let needed = self.data.layers.iter().enumerate().any(|(j, other)| {
                    !sources.contains(&j)
                        && (other.props.parent == Some(id)
                            || matches!(other.props.kind,
                                LayerKind::Clone { source, .. } if source == id))
                });
                !l.kind().is_group() && !needed
            })
        });
        target_ok && sources_ok
    }

    /// Merges the active layer into the one below it (upper voxels win).
    pub fn merge_active_layer_down(&mut self) -> bool {
        let upper = self.active_layer;
        let Some(lower) = upper.checked_sub(1) else {
            return false;
        };
        if upper >= self.data.layers.len() || !self.mergeable(lower, &[upper]) {
            return false;
        }
        let edit = self.merge_into(lower, &[upper]);
        self.apply(edit, false);
        self.active_layer = lower;
        true
    }

    /// Merges every visible layer with geometry (voxels, shapes, clones) into
    /// the lowest visible voxel layer. Groups stay.
    pub fn merge_visible_layers(&mut self) -> bool {
        let visible: Vec<usize> = (0..self.data.layers.len())
            .filter(|&i| self.data.effectively_visible(i) && !self.data.layers[i].kind().is_group())
            .collect();
        let Some(target_pos) = visible
            .iter()
            .position(|&i| self.data.layers[i].kind().stores_voxels())
        else {
            return false;
        };
        let target = &visible[target_pos];
        let rest: Vec<usize> = visible[target_pos + 1..].to_vec();
        if rest.is_empty() || !self.mergeable(*target, &rest) {
            return false;
        }
        let rest = rest.as_slice();
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
            // Shapes and clones are baked from their evaluated geometry.
            let derived = match self.data.evaluate(i) {
                Some(Derived::Owned(v)) => v,
                Some(Derived::Own) => self.data.layers[i].volume.clone(),
                None => continue,
            };
            for (pos, new) in derived.iter() {
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
        // Refused while a visible layer still needs a hidden one (clone
        // source or group parent): deleting would break the link.
        let ids: Vec<LayerId> = hidden.iter().map(|&i| self.data.layers[i].id).collect();
        let needed = self.data.layers.iter().enumerate().any(|(i, l)| {
            !hidden.contains(&i)
                && (l.props.parent.is_some_and(|p| ids.contains(&p))
                    || matches!(l.props.kind, LayerKind::Clone { source, .. } if ids.contains(&source)))
        });
        if needed {
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
            generator: rizxel_io::generator(),
            name: self.name.clone(),
            palette: self.data.palette.clone(),
            selected_color: self.selected_color,
            materials: self.data.materials.clone(),
            layers: self
                .data
                .layers
                .iter()
                .map(|l| LayerData {
                    id: l.id,
                    name: l.name.clone(),
                    visible: l.visible,
                    locked: l.locked,
                    opacity: l.props.opacity,
                    parent: l.props.parent,
                    material: l.props.material,
                    crop: l.props.crop,
                    kind: l.props.kind,
                    voxels: if l.kind().stores_voxels() {
                        rizxel_io::runs_from_volume(self.data.bounds, &l.volume)
                    } else {
                        Vec::new()
                    },
                })
                .collect(),
            active_layer: self.active_layer,
            bounds: self.data.bounds,
        }
    }

    /// Builds a document from a decoded project. Fails when the project breaks
    /// a structural rule (see `ProjectFile::validate`) or a layer's voxel runs
    /// do not fit the project bounds.
    pub fn from_project(project: ProjectFile, path: Option<PathBuf>) -> Result<Self, RzxlError> {
        // `read_rzxl` already validated; projects built in memory may not be.
        project.validate()?;
        // Re-clamp: the file may carry out-of-range bounds.
        let bounds = BoundSize::new(project.bounds.x(), project.bounds.y(), project.bounds.z());
        let layers = project
            .layers
            .into_iter()
            .map(|l| {
                Ok(Layer {
                    volume: rizxel_io::volume_from_runs(bounds, &l.voxels)?,
                    id: l.id,
                    name: l.name,
                    visible: l.visible,
                    locked: l.locked,
                    props: LayerProps {
                        opacity: l.opacity,
                        parent: l.parent,
                        material: l.material,
                        crop: l.crop,
                        kind: l.kind,
                    },
                    selection: Selection::default(),
                })
            })
            .collect::<Result<Vec<_>, RzxlError>>()?;
        let mut doc = Self::with_bounds(project.name, bounds, layers);
        doc.data.palette = project.palette;
        doc.data.materials = project.materials;
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
        // Never save the tentative data of a drag in progress.
        self.commit_edit();
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

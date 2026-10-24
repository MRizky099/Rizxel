//! A single open project (one tab) and its undoable data.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use glam::{IVec3, Vec3};
use rizxel_engine::tools::{
    boolean_changes, cells_box, connected_components, fill_hollows, hollow, hollow_advanced,
    mirror_changes, relocate_changes, slice_layers, BooleanOp, Transform,
};
use rizxel_engine::{
    greedy_mesh, rasterize_shape, MaskMode, MaterialSlots, Mesh, MeshParams, SliceRequest,
    TileMeshCache, WorkPlane,
};
use rizxel_io::{LayerData, ProjectFile, RzxlError};
use rizxel_math::{
    BoundSize, ImageData, LayerId, LayerKind, Material, Palette, RefPlane, ReferenceId,
    ReferenceImage, RenderPreset, RenderSettings, Rgba8, ShapeKind, Voxel, VoxelBox, VoxelVolume,
    EMPTY_VOXEL, FIRST_COLOR_INDEX, MAX_PALETTE_COLORS,
};

use crate::box_select::{BoxSelect, LastBox};
use crate::clipboard::Clipboard;
use crate::history::{ClippedVoxel, Edit, History, VoxelChange};
use crate::layer_tree::{canonical_order, LayerTree, Link};
use crate::selection::Selection;
use crate::transform_session::TransformSession;

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
    /// Reference images, in draw order.
    pub references: Vec<ReferenceImage>,
    /// Render-mode settings (saved in the file, edited in Render mode).
    pub render: RenderSettings,
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

    /// Locked itself or inside a locked group.
    pub fn effectively_locked(&self, index: usize) -> bool {
        self.layers.get(index).is_some_and(|l| l.locked) || self.locked_by_group(index)
    }

    /// Inside a locked group (at any depth).
    pub fn locked_by_group(&self, index: usize) -> bool {
        self.tree().ancestors(index).any(|a| self.layers[a].locked)
    }

    /// Parent/child structure of the layer list.
    pub fn tree(&self) -> LayerTree {
        LayerTree::new(&self.layers)
    }

    /// The layer list keeps the tree rules (see [`crate::layer_tree`]).
    pub fn structure_is_valid(&self) -> bool {
        self.tree().is_valid()
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
    /// The layer is inside a locked group.
    LockedGroup,
    Hidden,
    /// Groups, shapes and linked clones do not store voxels.
    NotVoxels,
}

impl EditBlock {
    pub const fn message(self) -> &'static str {
        match self {
            EditBlock::NoLayer => "No active layer",
            EditBlock::Locked => "The active layer is locked",
            EditBlock::LockedGroup => "The active layer is inside a locked group",
            EditBlock::Hidden => "The active layer is hidden",
            EditBlock::NotVoxels => {
                "The active layer has no editable voxels (group, shape or clone): select a voxel layer"
            }
        }
    }
}

/// Where a dragged row lands in the Layers panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerDrop {
    /// Right above row `i` (and its subtree), in the same group.
    Above(usize),
    /// Right below row `i`'s subtree, in the same group.
    Below(usize),
    /// Into group `i`, as its top child.
    Into(usize),
}

impl LayerDrop {
    /// The row the drop is relative to.
    pub fn row(self) -> usize {
        match self {
            LayerDrop::Above(i) | LayerDrop::Below(i) | LayerDrop::Into(i) => i,
        }
    }
}

/// Why a transform did nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransformError {
    Blocked(EditBlock),
    /// No voxels in the selection (or on the layer).
    Empty,
    /// Some voxels would leave the bounds; nothing was changed.
    OutOfBounds,
    /// Mask Mode protects the selection the transform would move.
    Masked,
}

impl TransformError {
    pub const fn message(self) -> &'static str {
        match self {
            TransformError::Blocked(e) => e.message(),
            TransformError::Empty => "Nothing to transform",
            TransformError::OutOfBounds => {
                "The result would leave the bounds: enlarge them or move the voxels first"
            }
            TransformError::Masked => MASKED_MESSAGE,
        }
    }
}

/// Why commands that act on the selection itself do nothing in Mask Mode.
pub const MASKED_MESSAGE: &str = "Mask Mode protects the selection: turn it off (Shift+M) first";

/// Core operation of *Model ▸ Core* ([`Document::core_op`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreOp {
    FillHollows,
    Hollow,
    /// Hollow keeping a shell this many voxels thick.
    HollowAdvanced(u32),
}

/// Order of *Palette ▸ Sort* ([`Document::sort_palette`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteSort {
    /// Greys first (dark to light), then colors by hue, then lightness.
    Hue,
    /// Dark to light (Rec. 709 luma).
    Luminance,
}

impl PaletteSort {
    fn key(self, c: Rgba8) -> (f32, f32) {
        let (r, g, b) = (c.r as f32, c.g as f32, c.b as f32);
        let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;
        match self {
            PaletteSort::Luminance => (luma, 0.0),
            PaletteSort::Hue => {
                let (max, min) = (r.max(g).max(b), r.min(g).min(b));
                let delta = max - min;
                if delta < 8.0 {
                    return (-1.0, luma);
                }
                let hue = if max == r {
                    ((g - b) / delta).rem_euclid(6.0)
                } else if max == g {
                    (b - r) / delta + 2.0
                } else {
                    (r - g) / delta + 4.0
                };
                (hue, luma)
            }
        }
    }
}

/// Why a structure operation (Separate Parts, Fit Bounds, Core, Boolean)
/// did nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpError {
    Blocked(EditBlock),
    /// Nothing to do, with the reason.
    Nothing(&'static str),
    /// The model is larger than the operation's cell limit.
    TooLarge(usize),
}

impl OpError {
    pub fn message(self) -> String {
        match self {
            OpError::Blocked(e) => e.message().to_owned(),
            OpError::Nothing(why) => why.to_owned(),
            OpError::TooLarge(limit) => format!("Too large (over {limit} cells): nothing changed"),
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

/// Highest material slot ([`Document::material_slot`]); slot 0 is the
/// default material.
pub const MAX_MATERIAL_SLOT: usize = 255;

/// Alpha of the layers drawn as ghosts (*View ▸ Ghosts*).
pub const GHOST_ALPHA: f32 = 0.25;

/// A layer drawn in the viewport's transparent pass.
#[derive(Debug, Clone)]
pub struct TransparentMesh {
    pub layer: LayerId,
    pub mesh: Arc<Mesh>,
    /// Opacity in `0..1` (exclusive); comes from the layer, so changing it
    /// does not touch the mesh.
    pub alpha: f32,
    /// Center of the layer's voxels in model space, for back-to-front sorting.
    pub center: Vec3,
}

/// Viewport meshes of a document: every fully opaque layer merged into one
/// mesh, and one mesh per translucent layer. A part that did not change
/// keeps its `Arc`, so the renderer can skip the upload.
#[derive(Debug, Clone, Default)]
pub struct ViewMeshes {
    pub opaque: Arc<Mesh>,
    /// Bottom to top in layer order; the viewport sorts them by depth.
    pub transparent: Vec<TransparentMesh>,
}

/// Incremental meshing state behind [`Document::view_meshes`].
#[derive(Debug, Default)]
struct ViewCache {
    opaque: TileMeshCache,
    /// Per translucent layer.
    layers: HashMap<LayerId, TileMeshCache>,
    /// The last result, updated in place by [`Document::poll_view_meshes`].
    last: ViewMeshes,
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
    /// Index into `data.references` (the References panel's selected row).
    /// Not part of undo history.
    pub active_reference: usize,
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
    /// Open box selection (see [`Document::begin_box_select`]). Not part
    /// of history until applied.
    pub(crate) box_select: Option<BoxSelect>,
    /// Box of the last applied box selection.
    pub(crate) last_box: Option<LastBox>,
    /// Open Transform tool session (see [`Document::begin_transform`]);
    /// its voxels are written inside the open transaction.
    pub(crate) transform: Option<TransformSession>,
    /// Mask Mode: tool writes skip the selected cells of their layer. Mirrors
    /// `ToolOptions::mask_selection` (set by the app). Not part of history.
    pub mask_selection: bool,
    /// Derived layer geometry, rebuilt lazily when `revision` changes. Code
    /// that mutates `data` directly (bypassing [`Edit`]s) must not rely on it.
    geometry: RefCell<Option<GeometryCache>>,
    /// Per-tile meshes of the last [`Document::view_meshes`], so a drag
    /// re-meshes only the tiles it touched.
    view_cache: RefCell<ViewCache>,
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
                references: Vec::new(),
                render: RenderSettings::default(),
            },
            active_layer: 0,
            active_reference: 0,
            selected_color: Palette::DEFAULT_SELECTED,
            plane: WorkPlane::default(),
            history: History::default(),
            revision: 0,
            saved_revision: 0,
            edits: 0,
            paste: None,
            box_select: None,
            last_box: None,
            transform: None,
            mask_selection: false,
            geometry: RefCell::new(None),
            view_cache: RefCell::new(ViewCache::default()),
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
        self.with_visible_layers(|layers| {
            let volumes: Vec<&VoxelVolume> = layers.iter().map(|(_, v)| *v).collect();
            f(&volumes)
        })
    }

    /// [`Document::with_visible_geometry`] with each volume's layer index.
    fn with_visible_layers<R>(&self, f: impl FnOnce(&[(usize, &VoxelVolume)]) -> R) -> R {
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
        let layers: Vec<(usize, &VoxelVolume)> = cache
            .as_ref()
            .map(|c| {
                c.layers
                    .iter()
                    .filter_map(|(i, v)| {
                        let volume = v.as_ref().or(self.data.layers.get(*i).map(|l| &l.volume));
                        volume.map(|v| (*i, v))
                    })
                    .collect()
            })
            .unwrap_or_default();
        f(&layers)
    }

    /// Topmost effectively visible layer with a voxel at `p` (the one the
    /// viewport shows there).
    pub fn layer_at(&self, p: IVec3) -> Option<usize> {
        self.with_visible_layers(|layers| {
            layers
                .iter()
                .rev()
                .find(|(_, v)| v.get(p) != EMPTY_VOXEL)
                .map(|(i, _)| *i)
        })
    }

    /// Opacity a layer is drawn with: its own times every enclosing
    /// group's. With `ghosts`, layers other than the active one are drawn at
    /// most [`GHOST_ALPHA`].
    pub fn view_alpha(&self, index: usize, ghosts: bool) -> f32 {
        let mut alpha = 1.0;
        let mut layer = self.data.layers.get(index);
        for _ in 0..=rizxel_io::MAX_GROUP_DEPTH {
            let Some(l) = layer else { break };
            alpha *= l.props.opacity;
            layer = l
                .props
                .parent
                .and_then(|p| self.data.layer_index(p))
                .and_then(|i| self.data.layers.get(i));
        }
        // The material tint's alpha is the material opacity.
        if let Some(m) = self.layer_material(index) {
            alpha *= m.tint[3];
        }
        if ghosts && index != self.active_layer {
            alpha = alpha.min(GHOST_ALPHA);
        }
        alpha.clamp(0.0, 1.0)
    }

    /// The material a layer uses (`None` = the default material).
    pub fn layer_material(&self, index: usize) -> Option<&Material> {
        let m = self.data.layers.get(index)?.props.material?;
        self.data.materials.get(m as usize)
    }

    /// Material slot of a layer for the mesher and the renderer's material
    /// table: 0 = default, `m + 1` = material `m`. The table has 256 slots,
    /// so the 256th material shares the last one (see [`MAX_MATERIAL_SLOT`]).
    pub fn material_slot(&self, index: usize) -> u8 {
        self.data
            .layers
            .get(index)
            .and_then(|l| l.props.material)
            .filter(|&m| (m as usize) < self.data.materials.len())
            .map_or(0, |m| (m + 1).min(MAX_MATERIAL_SLOT as u32) as u8)
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

    /// Opaque viewport mesh: [`ViewMeshes::opaque`] without ghosts (with
    /// ambient occlusion, so it has more quads than
    /// [`Document::visible_mesh`], which exports use).
    pub fn view_mesh(&self) -> Mesh {
        (*self.view_meshes(false).opaque).clone()
    }

    /// Viewport meshes clipped to [`Document::view_clip`]: fully opaque
    /// layers merged into one mesh (later layers win), and one mesh per
    /// translucent layer ([`Document::view_alpha`] below 1; fully
    /// transparent layers are skipped). The mesher closes the cut, so the
    /// sliced model shows a solid cut face. Meshes carry per-vertex ambient
    /// occlusion (the renderer decides whether to show it). Incremental:
    /// only tiles that changed since the previous call are re-meshed (same
    /// output as a full mesh), and an opacity change alone re-meshes
    /// nothing.
    pub fn view_meshes(&self, ghosts: bool) -> ViewMeshes {
        self.view_meshes_with(ghosts, false)
    }

    /// [`Document::view_meshes`] for the viewport: a large update (an
    /// import, the undo of a big edit) is meshed by the background pool and
    /// the result keeps the previous meshes of the waiting tiles; call
    /// [`Document::poll_view_meshes`] every frame while
    /// [`Document::view_meshing`]. Small edits mesh right here.
    pub fn view_meshes_background(&self, ghosts: bool) -> ViewMeshes {
        self.view_meshes_with(ghosts, true)
    }

    /// True while background meshing for the viewport is in progress.
    pub fn view_meshing(&self) -> bool {
        let cache = self.view_cache.borrow();
        cache.opaque.pending() || cache.layers.values().any(TileMeshCache::pending)
    }

    /// Takes in finished background tiles. Returns the updated meshes of
    /// the last [`Document::view_meshes_background`] call when any part
    /// changed (alphas and centers unchanged).
    pub fn poll_view_meshes(&self) -> Option<ViewMeshes> {
        let mut cache = self.view_cache.borrow_mut();
        let cache = &mut *cache;
        let mut changed = false;
        if cache.opaque.poll() {
            if let Some(mesh) = cache.opaque.output() {
                cache.last.opaque = mesh;
                changed = true;
            }
        }
        for t in &mut cache.last.transparent {
            if let Some(tiles) = cache.layers.get_mut(&t.layer) {
                if tiles.poll() {
                    if let Some(mesh) = tiles.output() {
                        t.mesh = mesh;
                        changed = true;
                    }
                }
            }
        }
        changed.then(|| cache.last.clone())
    }

    fn view_meshes_with(&self, ghosts: bool, background: bool) -> ViewMeshes {
        let clip = self.view_clip();
        let clipped = |v: &VoxelVolume| match clip {
            Some(c) => v.cropped(&c),
            None => v.clone(),
        };
        self.with_visible_layers(|layers| {
            let palette = &self.data.palette;
            let mut cache = self.view_cache.borrow_mut();
            let cache = &mut *cache;
            let mut opaque = Vec::new();
            let mut out = ViewMeshes::default();
            let mut kept = Vec::new();
            let mesh = |tiles: &mut TileMeshCache, volume: &VoxelVolume, params: &MeshParams| {
                let params = params.ao(true);
                if background {
                    tiles.mesh_background(volume, &params)
                } else {
                    tiles.mesh_shared(volume, &params)
                }
            };
            for &(index, volume) in layers {
                let alpha = self.view_alpha(index, ghosts);
                if alpha >= 1.0 {
                    opaque.push((volume, self.material_slot(index)));
                    continue;
                }
                let Some(layer) = self.data.layers.get(index).filter(|_| alpha > 0.0) else {
                    continue;
                };
                let volume = clipped(volume);
                let Some((lo, hi)) = volume.occupied_bounds() else {
                    continue;
                };
                let tiles = cache.layers.entry(layer.id).or_default();
                let slots = MaterialSlots::Uniform(self.material_slot(index));
                out.transparent.push(TransparentMesh {
                    layer: layer.id,
                    mesh: mesh(tiles, &volume, &MeshParams::new(palette).slots(slots)),
                    alpha,
                    center: (lo + hi).as_vec3() * 0.5,
                });
                kept.push(layer.id);
            }
            cache.layers.retain(|id, _| kept.contains(id));

            // Later layers win, for the palette index and for the material.
            let (merged, merged_slots);
            let (volume, slots) = match (opaque.as_slice(), clip) {
                ([], _) => (None, MaterialSlots::Uniform(0)),
                ([(single, slot)], None) => (Some(*single), MaterialSlots::Uniform(*slot)),
                _ => {
                    let mut all = VoxelVolume::new(self.data.bounds);
                    let per_voxel = opaque.iter().any(|(_, s)| *s != opaque[0].1);
                    let mut slots = VoxelVolume::new(self.data.bounds);
                    for (v, slot) in &opaque {
                        for (pos, value) in v.iter() {
                            if clip.is_none_or(|c| c.contains(pos)) {
                                all.set(pos, value);
                                if per_voxel {
                                    slots.set(pos, *slot);
                                }
                            }
                        }
                    }
                    merged = all;
                    merged_slots = slots;
                    let slots = if per_voxel {
                        MaterialSlots::PerVoxel(&merged_slots)
                    } else {
                        MaterialSlots::Uniform(opaque[0].1)
                    };
                    (Some(&merged), slots)
                }
            };
            match volume {
                Some(volume) => {
                    let params = MeshParams::new(palette).slots(slots);
                    out.opaque = mesh(&mut cache.opaque, volume, &params);
                }
                // Nothing opaque: drop the cache so no late tile lands here.
                None => cache.opaque = TileMeshCache::default(),
            }
            cache.last = out.clone();
            out
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

    /// Tab label: [`Document::title`] with `*` between the file name and its
    /// extension while there are unsaved changes (`hero*.rzxl`).
    pub fn tab_title(&self) -> String {
        let title = self.title();
        if !self.is_dirty() {
            return title;
        }
        match title.rsplit_once('.') {
            Some((stem, ext)) if !stem.is_empty() => format!("{stem}*.{ext}"),
            _ => format!("{title}*"),
        }
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
        self.check_structure();
    }

    /// Debug builds check the layer tree rules after every change.
    fn check_structure(&self) {
        debug_assert!(
            self.data.structure_is_valid(),
            "layer tree rules broken: {:?}",
            self.data
                .layers
                .iter()
                .map(|l| (l.id, l.props.parent, l.kind().is_group()))
                .collect::<Vec<_>>()
        );
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
        if self.cancel_box_select() || self.cancel_transform() {
            return true;
        }
        if self.paste.is_some() {
            self.cancel_paste();
            return true;
        }
        let done = self.history.undo(&mut self.data);
        if done {
            self.edits += 1;
            self.touch();
            self.check_structure();
            if let Some(p) = self.history.undone_plane() {
                self.plane.restore_slice(&p, self.data.bounds);
            }
        }
        done
    }

    pub fn redo(&mut self) -> bool {
        if self.paste.is_some() || self.transform.is_some() {
            return false;
        }
        let done = self.history.redo(&mut self.data);
        if done {
            self.edits += 1;
            self.touch();
            self.check_structure();
            if let Some(p) = self.history.redone_plane() {
                self.plane.restore_slice(&p, self.data.bounds);
            }
        }
        done
    }

    /// Names the next undo step in the history list.
    pub fn label_next(&mut self, label: impl Into<String>) {
        self.history.label_next(label);
    }

    /// Runs `f`, naming the undo step it records `label` (nothing is named
    /// when `f` records no step).
    pub fn labeled<R>(&mut self, label: impl Into<String>, f: impl FnOnce(&mut Self) -> R) -> R {
        self.history.label_next(label);
        let out = f(self);
        self.history.forget_label();
        out
    }

    /// The undo history list (oldest first) and how many of its steps are
    /// done; the rest are undone and can be redone.
    pub fn history_entries(&self) -> (Vec<String>, usize) {
        (self.history.entries(), self.history.position())
    }

    /// Undoes or redoes until `position` steps are done (the History panel:
    /// clicking an entry goes to the state right after it; 0 is the
    /// start). Returns whether anything changed.
    pub fn jump_to(&mut self, position: usize) -> bool {
        self.cancel_box_select();
        self.cancel_transform();
        self.cancel_paste();
        self.commit_edit();
        let mut moved = false;
        while self.history.position() > position && self.undo() {
            moved = true;
        }
        while self.history.position() < position && self.redo() {
            moved = true;
        }
        moved
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
        let mask = self.mask_selection;
        if let Some(l) = self.data.layers.get_mut(layer) {
            for c in changes {
                if !l.volume.in_bounds(c.pos) || (mask && l.selection.contains(c.pos)) {
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
        self.transform = None;
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
        self.transform = None;
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
        self.transform = None;
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
        } else if self.data.locked_by_group(index) {
            Err(EditBlock::LockedGroup)
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
            if self.data.effectively_locked(i) {
                outcome.locked_skipped += usize::from(!l.volume.is_empty());
                continue;
            }
            targets.push((i, &l.volume));
        }
        let mut per_layer = slice_layers(&targets, self.data.bounds, req, cap_color);
        if self.mask_selection {
            for (layer, changes) in &mut per_layer {
                let selection = &self.data.layers[*layer].selection;
                changes.retain(|c| !selection.contains(c.pos));
            }
            per_layer.retain(|(_, changes)| !changes.is_empty());
        }
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

    /// Selects every voxel of the active layer (`Ctrl+A`).
    pub fn select_all(&mut self) -> bool {
        let Some(l) = self.active_layer() else {
            return false;
        };
        let all: Selection = l.volume.iter().map(|(p, _)| p).collect();
        self.set_selection(self.active_layer, all)
    }

    /// Selects the active layer's voxels that are not selected, and
    /// deselects the rest (`Ctrl+I`).
    pub fn invert_selection(&mut self) -> bool {
        let Some(l) = self.active_layer() else {
            return false;
        };
        let inverted = l.selection.inverted(l.volume.iter().map(|(p, _)| p));
        self.set_selection(self.active_layer, inverted)
    }

    /// Deselects everything on the active layer (`Esc`).
    pub fn clear_selection(&mut self) -> bool {
        self.set_selection(self.active_layer, Selection::default())
    }

    /// *Selection ▸ Grow*: adds the active layer's voxels touching the
    /// selection, `steps` cells deep.
    pub fn grow_selection(&mut self, steps: u32) -> bool {
        let Some(l) = self.active_layer() else {
            return false;
        };
        let grown = l.selection.grown(&l.volume, steps);
        self.set_selection(self.active_layer, grown)
    }

    /// *Selection ▸ Shrink*: peels `steps` cells off the selection's surface.
    pub fn shrink_selection(&mut self, steps: u32) -> bool {
        let Some(l) = self.active_layer() else {
            return false;
        };
        let shrunk = l.selection.shrunk(steps);
        self.set_selection(self.active_layer, shrunk)
    }

    /// *Selection ▸ By Color*: the active layer's voxels within `threshold`
    /// of palette entry `color`, combined with the selection by `mode`.
    pub fn select_by_color(&mut self, color: Voxel, threshold: u8, mode: MaskMode) -> bool {
        let Some(l) = self.active_layer() else {
            return false;
        };
        let cells = Selection::by_color(&l.volume, &self.data.palette, color, threshold);
        self.select(cells.iter(), mode)
    }

    /// True while Mask Mode protects a non-empty selection on the active
    /// layer. Commands that act on the selection itself (Delete, Cut,
    /// transforms) are refused then.
    pub fn selection_protected(&self) -> bool {
        self.mask_selection && self.selection().is_some_and(|s| !s.is_empty())
    }

    /// The selection tools work inside (Fill, Move, Extrude): the active
    /// layer's selection, unless it is empty or Mask Mode protects it.
    pub fn region_selection(&self) -> Option<&Selection> {
        self.selection()
            .filter(|s| !s.is_empty() && !self.mask_selection)
    }

    /// Deletes the selected voxels of the active layer and clears the
    /// selection, as one undo step. Returns the number of voxels removed.
    pub fn delete_selected(&mut self) -> usize {
        let layer = self.active_layer;
        if self.selection_protected()
            || self
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

    // ---------------------------------------------------------- transforms

    /// Layer index a transform may write to, or why not (hidden is fine).
    pub(crate) fn transform_layer(&self) -> Result<usize, TransformError> {
        if self.selection_protected() {
            return Err(TransformError::Masked);
        }
        let layer = self.active_layer;
        match self.editable_layer(layer) {
            Ok(()) | Err(EditBlock::Hidden) => Ok(layer),
            Err(e) => Err(TransformError::Blocked(e)),
        }
    }

    /// Rotates or flips the selection (or, without one, the whole active
    /// layer) inside its box, as one undo step that carries the selection
    /// along. Refused as a whole when a voxel would leave the bounds.
    /// Returns the number of voxels changed.
    pub fn transform(&mut self, t: Transform) -> Result<usize, TransformError> {
        let layer = self.transform_layer()?;
        let l = &self.data.layers[layer];
        let selection = (!l.selection.is_empty()).then(|| l.selection.clone());
        let sources: Vec<(IVec3, Voxel)> = match &selection {
            Some(sel) => sel
                .iter()
                .map(|p| (p, l.volume.get(p)))
                .filter(|&(_, v)| v != EMPTY_VOXEL)
                .collect(),
            None => l.volume.iter().collect(),
        };
        if sources.is_empty() {
            return Err(TransformError::Empty);
        }
        let (min, size) = match selection.as_ref().and_then(Selection::bounds) {
            Some((lo, hi)) => (lo, hi - lo),
            None => cells_box(sources.iter().map(|c| c.0)).ok_or(TransformError::Empty)?,
        };
        let targets: Vec<(IVec3, Voxel)> = sources
            .iter()
            .map(|&(p, v)| (t.apply(p, min, size), v))
            .collect();
        let changes =
            relocate_changes(&l.volume, &sources, &targets).ok_or(TransformError::OutOfBounds)?;
        let bounds = VoxelBox::from_bounds(self.data.bounds);
        let moved: Option<Selection> = selection.map(|sel| {
            sel.iter()
                .map(|p| t.apply(p, min, size))
                .filter(|&p| bounds.contains(p))
                .collect()
        });
        Ok(self.commit_transform(layer, &changes, moved))
    }

    /// One-shot mirror across the bounds' center plane on `axis`: the
    /// selection's (or layer's) `-axis` half is copied onto the `+axis`
    /// half, as one undo step. The mirrored cells join the selection.
    pub fn mirror(&mut self, axis: usize) -> Result<usize, TransformError> {
        let layer = self.transform_layer()?;
        let bounds = self.data.bounds;
        let l = &self.data.layers[layer];
        let region = (!l.selection.is_empty()).then(|| l.selection.cells());
        let changes = mirror_changes(&l.volume, region, axis, bounds);
        let grown = region.map(|cells| {
            let mut sel = l.selection.clone();
            sel.extend(
                cells
                    .iter()
                    .map(|&p| rizxel_engine::tools::mirror(p, bounds, axis)),
            );
            sel.translated(IVec3::ZERO, bounds)
        });
        if changes.is_empty() && grown.as_ref().is_none_or(|g| *g == l.selection) {
            return Err(TransformError::Empty);
        }
        Ok(self.commit_transform(layer, &changes, grown))
    }

    fn commit_transform(
        &mut self,
        layer: usize,
        changes: &[VoxelChange],
        selection: Option<Selection>,
    ) -> usize {
        self.cancel_edit();
        self.begin_edit();
        let n = self.edit_voxels(layer, changes);
        match selection {
            Some(sel) => self.commit_edit_with_selection(layer, sel),
            None => self.commit_edit(),
        };
        n
    }

    // ---------------------------------------------------- structure ops

    /// Final geometry of layer `index` (voxels, rasterized shape, moved
    /// clone, crop applied); `None` for groups and broken clones.
    pub fn layer_geometry(&self, index: usize) -> Option<VoxelVolume> {
        match self.data.evaluate(index)? {
            Derived::Own => Some(self.data.layers[index].volume.clone()),
            Derived::Owned(v) => Some(v),
        }
    }

    /// The active layer if operations may rewrite its voxels (hidden is fine).
    fn op_layer(&self) -> Result<usize, OpError> {
        let layer = self.active_layer;
        match self.editable_layer(layer) {
            Ok(()) | Err(EditBlock::Hidden) => Ok(layer),
            Err(e) => Err(OpError::Blocked(e)),
        }
    }

    /// Writes `changes` into `layer` as one undo step (Mask Mode applies).
    fn write_op(&mut self, layer: usize, changes: &[VoxelChange]) -> Result<usize, OpError> {
        self.cancel_edit();
        self.begin_edit();
        let n = self.edit_voxels(layer, changes);
        self.commit_edit();
        if n == 0 {
            Err(OpError::Nothing("Nothing changed"))
        } else {
            Ok(n)
        }
    }

    /// Moves `parts` of `layer` (cell lists) into new layers just above it,
    /// as one undo step. Returns the number of new layers.
    fn move_parts_to_layers(&mut self, layer: usize, parts: &[(String, Vec<IVec3>)]) -> usize {
        let source = &self.data.layers[layer];
        let mut edits = Vec::new();
        let mut cleared = Vec::new();
        let first_id = self.next_layer_id();
        let mut names: Vec<String> = Vec::new();
        for (k, (suffix, cells)) in parts.iter().enumerate() {
            let base = format!("{} {suffix}", source.name);
            let mut name = self.unique_name_from(&base);
            let mut n = 2;
            while names.contains(&name) {
                name = self.unique_name_from(&format!("{base} {n}"));
                n += 1;
            }
            names.push(name.clone());
            let mut new = Layer::new(name, self.data.bounds);
            new.id = first_id + k as LayerId;
            new.props.parent = source.props.parent;
            new.props.material = source.props.material;
            new.props.opacity = source.props.opacity;
            for &p in cells {
                let v = source.volume.get(p);
                new.volume.set(p, v);
                cleared.push(VoxelChange {
                    pos: p,
                    old: v,
                    new: EMPTY_VOXEL,
                });
            }
            edits.push(Edit::InsertLayer {
                index: layer + 1 + k,
                layer: new,
            });
        }
        edits.insert(
            0,
            Edit::Voxels {
                layer,
                changes: cleared,
            },
        );
        let count = parts.len();
        self.cancel_edit();
        self.apply(Edit::Batch(edits), false);
        self.active_layer = layer;
        count
    }

    /// *Layer ▸ Separate Parts*: every 6-connected part of the active layer
    /// with at least `min_voxels` voxels gets its own layer; the largest
    /// part and the smaller bits stay. One undo step; returns the number of
    /// new layers.
    pub fn separate_parts(&mut self, min_voxels: usize) -> Result<usize, OpError> {
        let layer = self.op_layer()?;
        let parts = connected_components(&self.data.layers[layer].volume)
            .map_err(|e| OpError::TooLarge(e.limit))?;
        let big: Vec<(String, Vec<IVec3>)> = parts
            .into_iter()
            .filter(|p| p.len() >= min_voxels.max(1))
            .skip(1)
            .enumerate()
            .map(|(k, cells)| (format!("part {}", k + 2), cells))
            .collect();
        if big.is_empty() {
            return Err(OpError::Nothing("The layer has only one part"));
        }
        Ok(self.move_parts_to_layers(layer, &big))
    }

    /// *Layer ▸ Split at Work Plane*: the active layer's voxels on the
    /// plane's positive side (`coord >= plane`) move to a new layer above.
    pub fn split_at_plane(&mut self) -> Result<usize, OpError> {
        let layer = self.op_layer()?;
        let p = self.plane.clamped(self.data.bounds);
        let (axis, coord) = (p.axis.min(2), p.coord);
        let volume = &self.data.layers[layer].volume;
        let side: Vec<IVec3> = volume
            .iter()
            .filter(|(q, _)| q[axis] >= coord)
            .map(|(q, _)| q)
            .collect();
        if side.is_empty() || side.len() == volume.voxel_count() {
            return Err(OpError::Nothing(
                "The work plane does not cut through the layer",
            ));
        }
        let suffix = format!("{}{coord}+", ["X", "Y", "Z"][axis]);
        Ok(self.move_parts_to_layers(layer, &[(suffix, side)]))
    }

    /// *Selection ▸ To New Layer*: the selected voxels of the active layer
    /// go into a new voxel layer right above it (same group, material and
    /// opacity), which becomes active and carries the selection. `cut`
    /// removes them from the source (a clone or shape source is baked into
    /// voxels first); otherwise the source stays as it is. One undo step.
    pub fn selection_to_new_layer(&mut self, cut: bool) -> Result<LayerId, OpError> {
        let layer = self.active_layer;
        let source = self
            .data
            .layers
            .get(layer)
            .ok_or(OpError::Blocked(EditBlock::NoLayer))?;
        if source.kind().is_group() {
            return Err(OpError::Blocked(EditBlock::NotVoxels));
        }
        if source.selection.is_empty() {
            return Err(OpError::Nothing("Nothing is selected"));
        }
        if self.mask_selection {
            return Err(OpError::Nothing(MASKED_MESSAGE));
        }
        if cut {
            match self.editable_layer(layer) {
                Ok(()) | Err(EditBlock::Hidden) | Err(EditBlock::NotVoxels) => {}
                Err(e) => return Err(OpError::Blocked(e)),
            }
        }
        let geometry = self
            .layer_geometry(layer)
            .ok_or(OpError::Nothing("The layer has no voxels"))?;
        let picked: Vec<(IVec3, Voxel)> = source
            .selection
            .iter()
            .map(|p| (p, geometry.get(p)))
            .filter(|&(_, v)| v != EMPTY_VOXEL)
            .collect();
        if picked.is_empty() {
            return Err(OpError::Nothing(
                "The active layer has no voxels in the selection",
            ));
        }
        let mut new = Layer::new(
            self.unique_name_from(&format!("{} part", source.name)),
            self.data.bounds,
        );
        new.id = self.next_layer_id();
        new.props.parent = source.props.parent;
        new.props.material = source.props.material;
        new.props.opacity = source.props.opacity;
        for &(p, v) in &picked {
            new.volume.set(p, v);
            new.selection.insert(p);
        }
        let id = new.id;
        let mut edits = Vec::new();
        if cut {
            let baked = !source.kind().stores_voxels();
            let changes: Vec<VoxelChange> = if baked {
                // The source becomes a voxel layer holding what is left.
                let mut props = source.props.clone();
                props.kind = LayerKind::Voxels;
                edits.push(Edit::SetLayerProps {
                    index: layer,
                    old: source.props.clone(),
                    new: props,
                });
                geometry
                    .iter()
                    .filter(|(p, _)| !source.selection.contains(*p))
                    .map(|(pos, new)| VoxelChange {
                        pos,
                        old: source.volume.get(pos),
                        new,
                    })
                    .filter(|c| c.old != c.new)
                    .collect()
            } else {
                picked
                    .iter()
                    .map(|&(pos, old)| VoxelChange {
                        pos,
                        old,
                        new: EMPTY_VOXEL,
                    })
                    .collect()
            };
            edits.push(Edit::Voxels { layer, changes });
            edits.push(Edit::SetSelection {
                layer,
                old: source.selection.clone(),
                new: Selection::default(),
            });
        }
        edits.push(Edit::InsertLayer {
            index: layer + 1,
            layer: new,
        });
        let label = if cut {
            "To New Layer (Cut)"
        } else {
            "To New Layer (Copy)"
        };
        self.cancel_edit();
        self.labeled(label, |d| d.apply(Edit::Batch(edits), false));
        self.active_layer = layer + 1;
        Ok(id)
    }

    /// Box around the geometry of every layer (hidden ones too).
    pub fn content_bounds(&self) -> Option<(IVec3, IVec3)> {
        (0..self.data.layers.len())
            .filter_map(|i| self.layer_geometry(i)?.occupied_bounds())
            .reduce(|(a0, a1), (b0, b1)| (a0.min(b0), a1.max(b1)))
    }

    /// *Model ▸ Fit Bounds to Content*: moves every layer so the content's
    /// minimum corner is at the origin and shrinks the bounds to fit it, as
    /// one undo step. Shape areas and crop boxes move along.
    pub fn fit_bounds_to_content(&mut self) -> Result<BoundSize, OpError> {
        let (lo, hi) = self
            .content_bounds()
            .ok_or(OpError::Nothing("The model is empty"))?;
        let size = hi - lo;
        let new = BoundSize::new(size.x as u32, size.y as u32, size.z as u32);
        if lo == IVec3::ZERO && new == self.data.bounds {
            return Err(OpError::Nothing("The bounds already fit the model"));
        }
        let shift = -lo;
        let moved = |b: VoxelBox| VoxelBox {
            min: b.min + shift,
            max: b.max + shift,
        };
        let mut edits = Vec::new();
        for (index, l) in self.data.layers.iter().enumerate() {
            if l.kind().stores_voxels() && !l.volume.is_empty() && shift != IVec3::ZERO {
                let mut changes: Vec<VoxelChange> = l
                    .volume
                    .iter()
                    .map(|(pos, old)| VoxelChange {
                        pos,
                        old,
                        new: EMPTY_VOXEL,
                    })
                    .collect();
                let placed: Vec<VoxelChange> = l
                    .volume
                    .iter()
                    .map(|(pos, v)| VoxelChange {
                        pos: pos + shift,
                        old: EMPTY_VOXEL,
                        new: v,
                    })
                    .collect();
                crate::history::merge_voxel_changes(&mut changes, &placed);
                edits.push(Edit::Voxels {
                    layer: index,
                    changes,
                });
            }
            let mut props = l.props.clone();
            props.crop = props.crop.map(moved);
            if let LayerKind::Shape { area, .. } = &mut props.kind {
                *area = moved(*area);
            }
            if props != l.props {
                edits.push(Edit::SetLayerProps {
                    index,
                    old: l.props.clone(),
                    new: props,
                });
            }
            // Selections follow their voxels.
            if !l.selection.is_empty() && shift != IVec3::ZERO {
                edits.push(Edit::SetSelection {
                    layer: index,
                    old: l.selection.clone(),
                    new: l.selection.translated(shift, new),
                });
            }
        }
        if new != self.data.bounds {
            edits.push(Edit::SetBounds {
                old: self.data.bounds,
                new,
                clipped: Vec::new(),
            });
        }
        self.cancel_edit();
        self.apply(Edit::Batch(edits), false);
        Ok(new)
    }

    /// *Model ▸ Core* operations on the active layer, as one undo step.
    /// Fill Hollows uses the selected color. Returns the voxels changed.
    pub fn core_op(&mut self, op: CoreOp) -> Result<usize, OpError> {
        let layer = self.op_layer()?;
        let volume = &self.data.layers[layer].volume;
        let changes = match op {
            CoreOp::FillHollows => fill_hollows(volume, self.selected_color as Voxel),
            CoreOp::Hollow => hollow(volume),
            CoreOp::HollowAdvanced(t) => hollow_advanced(volume, t),
        }
        .map_err(|e| OpError::TooLarge(e.limit))?;
        if changes.is_empty() {
            return Err(OpError::Nothing(match op {
                CoreOp::FillHollows => "No closed hollows to fill",
                _ => "Nothing to hollow out",
            }));
        }
        self.write_op(layer, &changes)
    }

    /// *Layer ▸ Boolean*: the active layer becomes `active op other`
    /// (`other`'s final geometry; it is not changed). One undo step.
    pub fn boolean(&mut self, other: usize, op: BooleanOp) -> Result<usize, OpError> {
        let layer = self.op_layer()?;
        if other == layer {
            return Err(OpError::Nothing("Pick another layer"));
        }
        let geometry = self
            .layer_geometry(other)
            .ok_or(OpError::Nothing("That layer has no geometry"))?;
        let changes = boolean_changes(&self.data.layers[layer].volume, &geometry, op);
        self.write_op(layer, &changes)
    }

    // ----------------------------------------------------------- clipboard

    /// Copies the selection (or the whole active layer), then deletes it, as
    /// one undo step. `None` when there is nothing to cut or the layer is
    /// locked or not a voxel layer.
    pub fn cut(&mut self) -> Option<Clipboard> {
        let layer = self.active_layer;
        if self.selection_protected()
            || self
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
    /// gives it a fresh id and that group, selects it and returns the id.
    /// When the active layer is a group, the new layer becomes its top
    /// child (right below the group row).
    fn insert_above_active(&mut self, mut layer: Layer) -> LayerId {
        let into_group = self.active_layer().is_some_and(|a| a.kind().is_group());
        let index = if into_group {
            self.active_layer
        } else {
            (self.active_layer + 1).min(self.data.layers.len())
        };
        layer.id = self.next_layer_id();
        // The position decides the group.
        layer.props.parent = self.active_layer().and_then(|a| match a.kind() {
            LayerKind::Group { .. } => Some(a.id),
            _ => a.props.parent,
        });
        // Too deep for one more level: stay next to the group instead.
        if into_group && self.data.tree().depth(self.active_layer) >= rizxel_io::MAX_GROUP_DEPTH {
            layer.props.parent = self.active_layer().and_then(|a| a.props.parent);
            return self.insert_at(self.active_layer + 1, layer);
        }
        self.insert_at(index, layer)
    }

    fn insert_at(&mut self, index: usize, layer: Layer) -> LayerId {
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
        // The new parent must keep the subtree contiguous (move layers into
        // groups with `move_subtree`).
        let structure_ok =
            new.parent == layer.props.parent && new.kind.is_group() == layer.kind().is_group() || {
                let mut links: Vec<Link> = self.data.layers.iter().map(Link::of).collect();
                links[index].parent = new.parent;
                links[index].group = new.kind.is_group();
                LayerTree::from_links(&links).is_valid()
            };
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
        opacity_ok && material_ok && crop_ok && parent_ok && structure_ok && kind_ok && links_ok
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

    // --------------------------------------------------------- palette ops

    /// Palette indices used by some voxel layer or shape (`used[i]`).
    pub fn used_colors(&self) -> Vec<bool> {
        let mut used = vec![false; MAX_PALETTE_COLORS];
        for l in &self.data.layers {
            match l.props.kind {
                LayerKind::Voxels => {
                    for (_, v) in l.volume.iter() {
                        used[v as usize] = true;
                    }
                }
                LayerKind::Shape { color, .. } => used[color as usize] = true,
                _ => {}
            }
        }
        used
    }

    /// Edits that recolor every voxel layer and shape by `map` (old index →
    /// new index), on every layer (locked and hidden ones too: the colors
    /// move, the look does not change).
    fn remap_edits(&self, map: &[Voxel]) -> Vec<Edit> {
        let to = |v: Voxel| map.get(v as usize).copied().unwrap_or(v);
        let mut edits = Vec::new();
        for (index, l) in self.data.layers.iter().enumerate() {
            match l.props.kind {
                LayerKind::Voxels => {
                    let changes: Vec<VoxelChange> = l
                        .volume
                        .iter()
                        .filter(|&(_, v)| to(v) != v)
                        .map(|(pos, old)| VoxelChange {
                            pos,
                            old,
                            new: to(old),
                        })
                        .collect();
                    if !changes.is_empty() {
                        edits.push(Edit::Voxels {
                            layer: index,
                            changes,
                        });
                    }
                }
                LayerKind::Shape { shape, area, color } if to(color) != color => {
                    edits.push(Edit::SetLayerProps {
                        index,
                        old: l.props.clone(),
                        new: LayerProps {
                            kind: LayerKind::Shape {
                                shape,
                                area,
                                color: to(color),
                            },
                            ..l.props.clone()
                        },
                    });
                }
                _ => {}
            }
        }
        edits
    }

    /// Applies a new palette and an index remap as one undo step; the
    /// selected color follows the remap. Returns `false` when nothing would
    /// change.
    fn apply_palette_remap(&mut self, palette: Palette, map: &[Voxel]) -> bool {
        let mut edits = self.remap_edits(map);
        if palette != self.data.palette {
            edits.insert(
                0,
                Edit::SetPalette {
                    old: self.data.palette.clone(),
                    new: palette,
                },
            );
        }
        if edits.is_empty() {
            return false;
        }
        let selected = map
            .get(self.selected_color)
            .map_or(self.selected_color, |&v| v as usize);
        self.cancel_edit();
        self.apply(Edit::Batch(edits), false);
        self.select_color(selected);
        true
    }

    /// *Palette ▸ Sort*: reorders slots `1..` by `key` and remaps the voxels
    /// so the model looks the same. One undo step.
    pub fn sort_palette(&mut self, key: PaletteSort) -> bool {
        let colors = self.data.palette.colors();
        let mut order: Vec<usize> = (FIRST_COLOR_INDEX as usize..colors.len()).collect();
        order.sort_by(|&a, &b| {
            key.key(colors[a])
                .partial_cmp(&key.key(colors[b]))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut sorted = colors.to_vec();
        let mut map: Vec<Voxel> = (0..MAX_PALETTE_COLORS).map(|i| i as Voxel).collect();
        for (slot, &old) in order.iter().enumerate() {
            let new = slot + FIRST_COLOR_INDEX as usize;
            sorted[new] = colors[old];
            map[old] = new as Voxel;
        }
        self.apply_palette_remap(Palette::new(sorted), &map)
    }

    /// *Palette ▸ Remove Duplicates*: later slots with the same color as an
    /// earlier one go; the palette closes up and voxels follow. One step.
    pub fn remove_duplicate_colors(&mut self) -> usize {
        let colors = self.data.palette.colors();
        let mut kept: Vec<Rgba8> = colors
            .iter()
            .take(FIRST_COLOR_INDEX as usize)
            .copied()
            .collect();
        let mut map: Vec<Voxel> = (0..MAX_PALETTE_COLORS).map(|i| i as Voxel).collect();
        for (i, &c) in colors.iter().enumerate().skip(FIRST_COLOR_INDEX as usize) {
            let first = kept
                .iter()
                .skip(FIRST_COLOR_INDEX as usize)
                .position(|&k| k == c)
                .map(|p| p + FIRST_COLOR_INDEX as usize);
            map[i] = match first {
                Some(j) => j as Voxel,
                None => {
                    kept.push(c);
                    (kept.len() - 1) as Voxel
                }
            };
        }
        let removed = colors.len() - kept.len();
        if removed == 0 {
            return 0;
        }
        self.apply_palette_remap(Palette::new(kept), &map);
        removed
    }

    /// *Palette ▸ Remap*: every voxel (and shape) of color `from` becomes
    /// `to`, on every layer. One undo step; returns the voxels changed.
    pub fn remap_color(&mut self, from: usize, to: usize) -> usize {
        let palette = &self.data.palette;
        if from == to || !palette.is_selectable(from) || !palette.is_selectable(to) {
            return 0;
        }
        let mut map: Vec<Voxel> = (0..MAX_PALETTE_COLORS).map(|i| i as Voxel).collect();
        map[from] = to as Voxel;
        let edits = self.remap_edits(&map);
        let count = edits
            .iter()
            .map(|e| match e {
                Edit::Voxels { changes, .. } => changes.len(),
                _ => 1,
            })
            .sum();
        if !edits.is_empty() {
            self.cancel_edit();
            self.apply(Edit::Batch(edits), false);
        }
        count
    }

    /// *Palette ▸ Gradient*: the slots from `a` to `b` (either order) blend
    /// linearly between the two end colors, which stay exact. One step.
    pub fn palette_gradient(&mut self, a: usize, b: usize) -> bool {
        let (lo, hi) = (a.min(b), a.max(b));
        let palette = &self.data.palette;
        if hi - lo < 2 || !palette.is_selectable(lo) || !palette.is_selectable(hi) {
            return false;
        }
        let (start, end) = (
            palette.get(a).unwrap_or_default(),
            palette.get(b).unwrap_or_default(),
        );
        let mut colors = palette.colors().to_vec();
        let span = (b as f32) - (a as f32);
        for (i, slot) in colors.iter_mut().enumerate().take(hi + 1).skip(lo) {
            let t = (i as f32 - a as f32) / span;
            let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
            *slot = Rgba8::new(
                mix(start.r, end.r),
                mix(start.g, end.g),
                mix(start.b, end.b),
                mix(start.a, end.a),
            );
        }
        let new = Palette::new(colors);
        if new == self.data.palette {
            return false;
        }
        self.cancel_edit();
        self.apply(
            Edit::SetPalette {
                old: self.data.palette.clone(),
                new,
            },
            false,
        );
        true
    }

    /// *Palette ▸ Load*: `colors` fill slots `1..` (indices stay, so voxels
    /// take the new colors); a shorter list keeps the old trailing slots.
    /// One undo step.
    pub fn load_palette(&mut self, colors: &[Rgba8]) -> bool {
        let before = self.data.palette.clone();
        let mut new: Vec<Rgba8> = before
            .colors()
            .iter()
            .take(FIRST_COLOR_INDEX as usize)
            .copied()
            .collect();
        new.extend(colors.iter().take(MAX_PALETTE_COLORS - new.len()));
        self.replace_palette_and_materials(Palette::new(new), self.data.materials.clone());
        self.data.palette != before
    }

    /// *Palette ▸ Load and Snap Model* (Qubicle *Apply Colors*): the
    /// palette becomes `colors` (slots `1..`) and every voxel takes the
    /// nearest new color (RGB distance), so the model keeps its look as
    /// closely as the new palette allows. One undo step.
    pub fn snap_to_palette(&mut self, colors: &[Rgba8]) -> bool {
        if colors.is_empty() {
            return false;
        }
        let old = self.data.palette.colors().to_vec();
        let mut new: Vec<Rgba8> = old
            .iter()
            .take(FIRST_COLOR_INDEX as usize)
            .copied()
            .collect();
        new.extend(colors.iter().take(MAX_PALETTE_COLORS - new.len()));
        let nearest = |c: Rgba8| -> Voxel {
            let d = |k: &Rgba8| {
                let (dr, dg, db) = (
                    c.r as i32 - k.r as i32,
                    c.g as i32 - k.g as i32,
                    c.b as i32 - k.b as i32,
                );
                dr * dr + dg * dg + db * db
            };
            (FIRST_COLOR_INDEX as usize..new.len())
                .min_by_key(|&i| d(&new[i]))
                .unwrap_or(FIRST_COLOR_INDEX as usize) as Voxel
        };
        let map: Vec<Voxel> = (0..MAX_PALETTE_COLORS)
            .map(|i| match old.get(i) {
                Some(&c) if i >= FIRST_COLOR_INDEX as usize => nearest(c),
                _ => i as Voxel,
            })
            .collect();
        self.apply_palette_remap(Palette::new(new), &map)
    }

    /// Everything exporters need: every non-group layer's final geometry
    /// (shapes rasterized, clones resolved, crops applied) with its
    /// effective visibility.
    pub fn export_scene(&self) -> rizxel_io::ExportScene {
        let group_index: HashMap<LayerId, usize> = self
            .data
            .layers
            .iter()
            .filter(|l| l.kind().is_group())
            .enumerate()
            .map(|(g, l)| (l.id, g))
            .collect();
        let group_of = |i: usize| {
            self.data.layers[i]
                .props
                .parent
                .and_then(|p| group_index.get(&p).copied())
        };
        let mut layers: Vec<rizxel_io::ExportLayer> = Vec::new();
        let mut groups = Vec::new();
        // Document layer index -> export layer index, for clone links.
        let mut export_index: HashMap<usize, usize> = HashMap::new();
        for (i, l) in self.data.layers.iter().enumerate() {
            if l.kind().is_group() {
                groups.push(rizxel_io::ExportGroup {
                    name: l.name.clone(),
                    visible: self.data.effectively_visible(i),
                    parent: group_of(i),
                    order: layers.len(),
                });
                continue;
            }
            let Some(derived) = self.data.evaluate(i) else {
                continue;
            };
            let geometry = match derived {
                Derived::Own => l.volume.clone(),
                Derived::Owned(v) => v,
            };
            export_index.insert(i, layers.len());
            layers.push(rizxel_io::ExportLayer {
                name: l.name.clone(),
                visible: self.data.effectively_visible(i),
                volume: geometry,
                group: group_of(i),
                material: self.layer_material(i).cloned(),
                opacity: l.props.opacity,
                instance_of: None,
            });
        }
        // A clone shares its source's mesh when neither is cropped and the
        // move clipped nothing at the bounds.
        for (i, l) in self.data.layers.iter().enumerate() {
            let LayerKind::Clone { source, offset } = l.props.kind else {
                continue;
            };
            let (Some(&ci), Some(src)) = (export_index.get(&i), self.data.layer_index(source))
            else {
                continue;
            };
            let Some(&si) = export_index.get(&src) else {
                continue;
            };
            let uncropped = l.props.crop.is_none() && self.data.layers[src].props.crop.is_none();
            if uncropped && layers[ci].volume.voxel_count() == layers[si].volume.voxel_count() {
                layers[ci].instance_of = Some((si, offset));
            }
        }
        rizxel_io::ExportScene {
            name: self.name.clone(),
            bounds: self.data.bounds,
            palette: self.data.palette.clone(),
            layers,
            groups,
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

    /// Replaces material `index` (`merge` folds slider drags into one undo
    /// step). Refused for a missing index or invalid channels.
    pub fn set_material(&mut self, index: usize, material: Material, merge: bool) -> bool {
        if self.data.materials.get(index) == Some(&material) || !material.is_valid() {
            return false;
        }
        let Some(_) = self.data.materials.get(index) else {
            return false;
        };
        let mut new = self.data.materials.clone();
        new[index] = material;
        self.apply(
            Edit::SetMaterials {
                old: self.data.materials.clone(),
                new,
            },
            merge,
        );
        true
    }

    /// Appends a default material named after its number and returns its
    /// index (`None` when the list is full).
    pub fn add_material(&mut self) -> Option<u32> {
        if self.data.materials.len() >= rizxel_io::MAX_MATERIALS {
            return None;
        }
        let index = self.data.materials.len();
        let mut new = self.data.materials.clone();
        new.push(Material::new(format!("Material {}", index + 1)));
        self.set_materials(new);
        Some(index as u32)
    }

    /// Number of layers that use material `index`.
    pub fn material_users(&self, index: u32) -> usize {
        self.data
            .layers
            .iter()
            .filter(|l| l.props.material == Some(index))
            .count()
    }

    /// Gives layer `layer` its own copy of its material (named
    /// "<name> copy"), so editing it no longer changes the other layers.
    /// A layer on the default material gets a fresh one. One undo step;
    /// returns the new material index.
    pub fn make_material_unique(&mut self, layer: usize) -> Option<u32> {
        let props = self.data.layers.get(layer)?.props.clone();
        if self.data.materials.len() >= rizxel_io::MAX_MATERIALS {
            return None;
        }
        let copy = match props
            .material
            .and_then(|m| self.data.materials.get(m as usize))
        {
            Some(m) => Material {
                name: format!("{} copy", m.name),
                ..m.clone()
            },
            None => Material::new(format!("Material {}", self.data.materials.len() + 1)),
        };
        let index = self.data.materials.len() as u32;
        let mut materials = self.data.materials.clone();
        materials.push(copy);
        let edits = vec![
            Edit::SetMaterials {
                old: self.data.materials.clone(),
                new: materials,
            },
            Edit::SetLayerProps {
                index: layer,
                old: props.clone(),
                new: LayerProps {
                    material: Some(index),
                    ..props
                },
            },
        ];
        self.label_next("Make Material Unique");
        self.apply(Edit::Batch(edits), false);
        Some(index)
    }

    /// Replaces the render settings (`merge` folds slider drags into one
    /// undo step). Values are clamped into range first.
    pub fn set_render(&mut self, new: RenderSettings, merge: bool) -> bool {
        let new = new.sanitized();
        if new == self.data.render {
            return false;
        }
        self.apply(
            Edit::SetRender {
                old: Box::new(self.data.render.clone()),
                new: Box::new(new),
            },
            merge,
        );
        true
    }

    /// Applies a built-in render preset as one undo step.
    pub fn apply_render_preset(&mut self, preset: RenderPreset) -> bool {
        let new = preset.apply(&self.data.render);
        self.labeled(format!("Render Preset \"{}\"", preset.label()), |d| {
            d.set_render(new, false)
        })
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

    /// Moves a layer to index `to` inside its group; the active layer
    /// follows it. Higher indices draw on top. A group moves with its
    /// subtree (see [`Document::move_subtree`]).
    pub fn move_layer(&mut self, from: usize, to: usize) -> bool {
        let Some(parent) = self.data.layers.get(from).map(|l| l.props.parent) else {
            return false;
        };
        self.move_subtree(from, to, parent)
    }

    /// Moves layer `from` together with its subtree so that it lands at
    /// index `to` inside group `parent` (`None`: top level), as one undo
    /// step; the active layer stays on the layer it was on. Refused when
    /// the result breaks the tree rules: a group into itself, a group split
    /// apart, or nesting deeper than the limit.
    pub fn move_subtree(&mut self, from: usize, to: usize, parent: Option<LayerId>) -> bool {
        let len = self.data.layers.len();
        if from >= len || to >= len {
            return false;
        }
        let block = self.data.tree().subtree(from);
        let (start, count) = (block.start, block.len());
        // Index of the block's first layer once it is placed.
        let Some(place) = (to + 1).checked_sub(count) else {
            return false;
        };
        let old_parent = self.data.layers[from].props.parent;
        if place == start && parent == old_parent {
            return false;
        }
        let mut links: Vec<Link> = self.data.layers.iter().map(Link::of).collect();
        let moved: Vec<Link> = links.drain(block).collect();
        links.splice(place..place, moved);
        links[to].parent = parent;
        if !LayerTree::from_links(&links).is_valid() {
            return false;
        }
        // One `MoveLayer` per layer of the block, keeping its order.
        let mut edits: Vec<Edit> = match place.cmp(&start) {
            std::cmp::Ordering::Less => (0..count)
                .map(|j| Edit::MoveLayer {
                    from: start + j,
                    to: place + j,
                })
                .collect(),
            std::cmp::Ordering::Greater => (0..count)
                .map(|_| Edit::MoveLayer { from: start, to })
                .collect(),
            std::cmp::Ordering::Equal => Vec::new(),
        };
        if parent != old_parent {
            let old = self.data.layers[from].props.clone();
            let new = LayerProps {
                parent,
                ..old.clone()
            };
            edits.push(Edit::SetLayerProps {
                index: to,
                old,
                new,
            });
        }
        let active = self.active_layer().map(|l| l.id);
        let edit = match edits.len() {
            1 => edits.pop().unwrap_or(Edit::Batch(Vec::new())),
            _ => Edit::Batch(edits),
        };
        self.apply(edit, false);
        if let Some(i) = active.and_then(|id| self.data.layer_index(id)) {
            self.active_layer = i;
        }
        true
    }

    /// Moves layer `from` (with its subtree) next to or into row `target`
    /// of the Layers panel. Returns whether anything moved.
    pub fn drop_layer(&mut self, from: usize, target: LayerDrop) -> bool {
        match self.drop_destination(from, target) {
            Some((to, parent)) => self.move_subtree(from, to, parent),
            None => false,
        }
    }

    /// Where [`Document::drop_layer`] would put layer `from`: its final
    /// index and group. `None` when the drop is refused (onto itself or
    /// into its own subtree, or breaking the tree rules).
    pub fn drop_destination(
        &self,
        from: usize,
        target: LayerDrop,
    ) -> Option<(usize, Option<LayerId>)> {
        let len = self.data.layers.len();
        let tree = self.data.tree();
        let row = target.row();
        if from >= len || row >= len || tree.subtree(from).contains(&row) {
            return None;
        }
        let parent_of = |i: usize| self.data.layers[i].props.parent;
        // Insertion point in the full list, and the new group.
        let (at, parent) = match target {
            LayerDrop::Above(i) => (i + 1, parent_of(i)),
            LayerDrop::Below(i) => (tree.subtree(i).start, parent_of(i)),
            LayerDrop::Into(i) if self.data.layers[i].kind().is_group() => {
                (i, Some(self.data.layers[i].id))
            }
            LayerDrop::Into(_) => return None,
        };
        let count = tree.subtree(from).len();
        let place = if from < at { at - count } else { at };
        let to = place + count - 1;
        let mut links: Vec<Link> = self.data.layers.iter().map(Link::of).collect();
        let moved: Vec<Link> = links.drain(tree.subtree(from)).collect();
        links.splice(place..place, moved);
        links[to].parent = parent;
        let changed = to != from || parent != parent_of(from);
        (changed && LayerTree::from_links(&links).is_valid()).then_some((to, parent))
    }

    /// Moves the active layer (with its subtree) one step up (`true`, drawn
    /// above) or down, past its neighbor inside the same group.
    pub fn move_active_layer(&mut self, up: bool) -> bool {
        let from = self.active_layer;
        let tree = self.data.tree();
        if from >= tree.len() {
            return false;
        }
        let parent = tree.parent(from);
        let start = tree.subtree(from).start;
        let to = if up {
            // The sibling above: the ancestor of the next row in this group.
            let next = from + 1;
            if next >= tree.len() || Some(next) == parent {
                return false;
            }
            match std::iter::once(next)
                .chain(tree.ancestors(next))
                .find(|&a| tree.parent(a) == parent)
            {
                Some(sibling) => sibling,
                None => return false,
            }
        } else {
            // The row right under the subtree is the sibling below, if any.
            match start.checked_sub(1) {
                Some(below) if tree.parent(below) == parent => {
                    tree.subtree(below).start + (from - start)
                }
                _ => return false,
            }
        };
        let parent = self.data.layers[from].props.parent;
        self.move_subtree(from, to, parent)
    }

    /// Copies the active layer (voxels and flags) just above itself. A group
    /// is copied with its whole subtree; clones inside it that point at
    /// layers inside it point at the copies.
    pub fn duplicate_active_layer(&mut self) -> bool {
        let index = self.active_layer;
        if index >= self.data.layers.len() {
            return false;
        }
        let range = self.data.tree().subtree(index);
        let mut next = self.next_layer_id();
        let ids: HashMap<LayerId, LayerId> = range
            .clone()
            .map(|i| {
                let id = (self.data.layers[i].id, next);
                next = next.saturating_add(1);
                id
            })
            .collect();
        let start = range.start;
        let edits: Vec<Edit> = range
            .map(|i| {
                let source = &self.data.layers[i];
                let mut layer = source.clone();
                layer.selection = Selection::default();
                layer.id = ids[&source.id];
                if i == index {
                    layer.name = self.unique_name_from(&format!("{} copy", source.name));
                } else {
                    layer.props.parent = source.props.parent.map(|p| ids[&p]);
                }
                if let LayerKind::Clone { source, .. } = &mut layer.props.kind {
                    if let Some(&copy) = ids.get(source) {
                        *source = copy;
                    }
                }
                Edit::InsertLayer {
                    index: i + (index + 1 - start),
                    layer,
                }
            })
            .collect();
        let count = edits.len();
        self.apply(Edit::Batch(edits), false);
        self.active_layer = index + count;
        true
    }

    /// Wraps layer `index` (a group with its subtree) in a new group at the
    /// same place, as one undo step; the new group becomes active. Refused
    /// when the nesting would get too deep.
    pub fn group_layer(&mut self, index: usize) -> Option<LayerId> {
        let tree = self.data.tree();
        let layer = self.data.layers.get(index)?;
        if tree.depth(index) + tree.subtree_height(index) + 1 > rizxel_io::MAX_GROUP_DEPTH {
            return None;
        }
        let mut group = Layer::new(self.unique_name_from("Group"), self.data.bounds);
        group.props.kind = LayerKind::Group { collapsed: false };
        group.props.parent = layer.props.parent;
        group.id = self.next_layer_id();
        let id = group.id;
        let old = layer.props.clone();
        let new = LayerProps {
            parent: Some(id),
            ..old.clone()
        };
        self.labeled("Group Layer", |d| {
            d.apply(
                Edit::Batch(vec![
                    Edit::InsertLayer {
                        index: index + 1,
                        layer: group,
                    },
                    Edit::SetLayerProps { index, old, new },
                ]),
                false,
            )
        });
        self.active_layer = index + 1;
        Some(id)
    }

    /// Dissolves group `index`: its children move up one level and the
    /// group is removed, as one undo step. The top child becomes active.
    pub fn ungroup(&mut self, index: usize) -> bool {
        let Some(layer) = self.data.layers.get(index).cloned() else {
            return false;
        };
        if !layer.kind().is_group() || self.data.layers.len() < 2 {
            return false;
        }
        let mut edits = self.reparent_children(&layer);
        let had_children = !edits.is_empty();
        edits.push(Edit::RemoveLayer { index, layer });
        self.labeled("Ungroup", |d| d.apply(Edit::Batch(edits), false));
        self.active_layer = if had_children {
            index - 1
        } else {
            index.min(self.data.layers.len() - 1)
        };
        true
    }

    /// Edits moving the children of `group` to its parent.
    fn reparent_children(&self, group: &Layer) -> Vec<Edit> {
        self.data
            .layers
            .iter()
            .enumerate()
            .filter(|(_, l)| l.props.parent == Some(group.id))
            .map(|(i, l)| Edit::SetLayerProps {
                index: i,
                old: l.props.clone(),
                new: LayerProps {
                    parent: group.props.parent,
                    ..l.props.clone()
                },
            })
            .collect()
    }

    /// True when a layer outside `range` is a linked clone of one inside.
    fn range_is_cloned_from_outside(&self, range: &std::ops::Range<usize>) -> bool {
        let ids: Vec<LayerId> = range.clone().map(|i| self.data.layers[i].id).collect();
        self.data.layers.iter().enumerate().any(|(i, l)| {
            !range.contains(&i)
                && matches!(l.props.kind, LayerKind::Clone { source, .. } if ids.contains(&source))
        })
    }

    /// Whether [`Document::delete_group_with_contents`] would act on `index`.
    pub fn can_delete_group_with_contents(&self, index: usize) -> bool {
        let range = self.data.tree().subtree(index);
        self.data
            .layers
            .get(index)
            .is_some_and(|l| l.kind().is_group())
            && range.len() < self.data.layers.len()
            && !self.range_is_cloned_from_outside(&range)
    }

    /// Deletes group `index` and everything inside it as one undo step.
    /// Refused when it would remove every layer or a linked clone outside
    /// the group points inside it.
    pub fn delete_group_with_contents(&mut self, index: usize) -> bool {
        if !self.can_delete_group_with_contents(index) {
            return false;
        }
        let range = self.data.tree().subtree(index);
        let start = range.start;
        let edits = range
            .rev()
            .map(|i| Edit::RemoveLayer {
                index: i,
                layer: self.data.layers[i].clone(),
            })
            .collect();
        self.labeled("Delete Group and Contents", |d| {
            d.apply(Edit::Batch(edits), false)
        });
        self.active_layer = start.saturating_sub(1);
        true
    }

    /// Bakes the visible contents of group `index` (voxels, shapes, clones,
    /// crops) into one voxel layer that replaces the group, as one undo
    /// step. Hidden children are dropped with the group. Refused for empty
    /// groups and when a linked clone outside the group points inside it.
    pub fn merge_group(&mut self, index: usize) -> bool {
        let Some(group) = self.data.layers.get(index) else {
            return false;
        };
        let tree = self.data.tree();
        let range = tree.subtree(index);
        if !group.kind().is_group() || range.len() < 2 || self.range_is_cloned_from_outside(&range)
        {
            return false;
        }
        // Visible inside the group: the layer and its ancestors below it.
        let shown = |i: usize| {
            self.data.layers[i].visible
                && tree
                    .ancestors(i)
                    .take_while(|&a| a != index)
                    .all(|a| self.data.layers[a].visible)
        };
        let mut merged = Layer::new(group.name.clone(), self.data.bounds);
        merged.id = self.next_layer_id();
        merged.visible = group.visible;
        merged.props.parent = group.props.parent;
        for i in range.clone().filter(|&i| i != index && shown(i)) {
            let derived = match self.data.evaluate(i) {
                Some(Derived::Owned(v)) => v,
                Some(Derived::Own) => self.data.layers[i].volume.clone(),
                None => continue,
            };
            for (pos, v) in derived.iter() {
                merged.volume.set(pos, v);
            }
        }
        let start = range.start;
        let mut edits: Vec<Edit> = range
            .rev()
            .map(|i| Edit::RemoveLayer {
                index: i,
                layer: self.data.layers[i].clone(),
            })
            .collect();
        edits.push(Edit::InsertLayer {
            index: start,
            layer: merged,
        });
        self.labeled("Merge Group", |d| d.apply(Edit::Batch(edits), false));
        self.active_layer = start;
        true
    }

    /// Folds or unfolds group `index` in the Layers panel. View state like
    /// the camera: saved with the file, but no undo step and no unsaved
    /// changes.
    pub fn set_group_collapsed(&mut self, index: usize, collapsed: bool) -> bool {
        match self.data.layers.get_mut(index).map(|l| &mut l.props.kind) {
            Some(LayerKind::Group { collapsed: c }) if *c != collapsed => {
                *c = collapsed;
                true
            }
            _ => false,
        }
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

    // ------------------------------------------------------- references

    /// Next free reference id.
    fn next_reference_id(&self) -> ReferenceId {
        self.data
            .references
            .iter()
            .map(|r| r.id)
            .max()
            .unwrap_or(0)
            .saturating_add(1)
    }

    /// Adds a reference image on `plane`, fitted into the bounds face and
    /// drawn above the others, and selects it. Refused at
    /// [`rizxel_io::MAX_REFERENCES`].
    pub fn add_reference(
        &mut self,
        name: impl Into<String>,
        image: ImageData,
        plane: RefPlane,
    ) -> Option<ReferenceId> {
        if self.data.references.len() >= rizxel_io::MAX_REFERENCES {
            return None;
        }
        let id = self.next_reference_id();
        let reference = ReferenceImage::fitted(id, name, image, plane, self.data.bounds);
        let index = self.data.references.len();
        self.apply(Edit::InsertReference { index, reference }, false);
        self.active_reference = index;
        Some(id)
    }

    pub fn remove_reference(&mut self, index: usize) -> bool {
        let Some(reference) = self.data.references.get(index).cloned() else {
            return false;
        };
        self.apply(Edit::RemoveReference { index, reference }, false);
        self.active_reference = index.saturating_sub(1);
        true
    }

    /// Replaces reference `index` with `new` (same id) as one undo step;
    /// `merge` folds slider and field drags. Refused for invalid values.
    pub fn set_reference(&mut self, index: usize, new: ReferenceImage, merge: bool) -> bool {
        let Some(old) = self.data.references.get(index).cloned() else {
            return false;
        };
        if old == new || old.id != new.id || !new.is_valid() {
            return false;
        }
        self.apply(Edit::SetReference { index, old, new }, merge);
        true
    }

    /// Changes reference `index` through `f` (see [`Document::set_reference`]).
    pub fn update_reference(
        &mut self,
        index: usize,
        merge: bool,
        f: impl FnOnce(&mut ReferenceImage),
    ) -> bool {
        let Some(mut new) = self.data.references.get(index).cloned() else {
            return false;
        };
        f(&mut new);
        self.set_reference(index, new, merge)
    }

    /// Moves a reference in the draw order; the selection follows it.
    pub fn move_reference(&mut self, from: usize, to: usize) -> bool {
        let len = self.data.references.len();
        if from >= len || to >= len || from == to {
            return false;
        }
        self.apply(Edit::MoveReference { from, to }, false);
        self.active_reference = to;
        true
    }

    /// Copies reference `index` just above itself (the image bytes are
    /// shared) and selects the copy.
    pub fn duplicate_reference(&mut self, index: usize) -> bool {
        let Some(source) = self.data.references.get(index) else {
            return false;
        };
        if self.data.references.len() >= rizxel_io::MAX_REFERENCES {
            return false;
        }
        let reference = ReferenceImage {
            id: self.next_reference_id(),
            name: format!("{} copy", source.name),
            ..source.clone()
        };
        self.apply(
            Edit::InsertReference {
                index: index + 1,
                reference,
            },
            false,
        );
        self.active_reference = index + 1;
        true
    }

    /// Gives reference `index` a new image, keeping its placement and
    /// width (the height follows the new aspect ratio).
    pub fn replace_reference_image(&mut self, index: usize, image: ImageData) -> bool {
        self.update_reference(index, false, |r| {
            let aspect = image.height.max(1) as f32 / image.width.max(1) as f32;
            r.size.y = r.size.x * aspect;
            r.image = image;
        })
    }

    /// Shows or hides every reference image as one undo step.
    pub fn set_all_references_visible(&mut self, visible: bool) -> bool {
        let edits: Vec<Edit> = self
            .data
            .references
            .iter()
            .enumerate()
            .filter(|(_, r)| r.visible != visible)
            .map(|(index, r)| Edit::SetReference {
                index,
                old: r.clone(),
                new: ReferenceImage {
                    visible,
                    ..r.clone()
                },
            })
            .collect();
        if edits.is_empty() {
            return false;
        }
        let label = if visible {
            "Show All References"
        } else {
            "Hide All References"
        };
        self.labeled(label, |d| d.apply(Edit::Batch(edits), false));
        true
    }

    /// *Show All / Hide All* (`Alt+R`): hides every reference when any is
    /// visible, otherwise shows them all.
    pub fn toggle_all_references(&mut self) -> bool {
        let any = self.data.references.iter().any(|r| r.visible);
        self.set_all_references_visible(!any)
    }

    /// Puts the work plane on reference `index`'s plane and position, so
    /// the Plane tool draws straight onto it.
    pub fn align_work_plane_to_reference(&mut self, index: usize) -> bool {
        let Some(r) = self.data.references.get(index) else {
            return false;
        };
        let axis = r.plane.normal_axis();
        let max = self.data.bounds.0[axis] as i32;
        self.plane.axis = axis;
        self.plane.coord = (r.offset[axis].round() as i32).clamp(0, max);
        self.plane.visible = true;
        true
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
            references: self.data.references.clone(),
            render: self.data.render.clone(),
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
        // Files may list a group's children anywhere (Goxel puts groups
        // first): put each subtree right below its group.
        let links: Vec<Link> = layers.iter().map(Link::of).collect();
        let order = canonical_order(&links);
        let active = order
            .iter()
            .position(|&i| i == project.active_layer)
            .unwrap_or(project.active_layer);
        let mut slots: Vec<Option<Layer>> = layers.into_iter().map(Some).collect();
        let layers = order.iter().filter_map(|&i| slots[i].take()).collect();
        let mut doc = Self::with_bounds(project.name, bounds, layers);
        doc.data.palette = project.palette;
        doc.data.materials = project.materials;
        doc.data.references = project.references;
        doc.data.render = project.render.sanitized();
        doc.active_layer = active;
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
    fn view_alpha_multiplies_groups_and_caps_ghosts() {
        let mut d = Document::new("t");
        d.set_voxels(0, [(IVec3::ZERO, 4)], false);
        let group = d.add_group();
        let g = d.data.layer_index(group).unwrap();
        let set = |d: &mut Document, i: usize, f: &dyn Fn(&mut LayerProps)| {
            let mut props = d.data.layers[i].props.clone();
            f(&mut props);
            assert!(d.set_layer_props(i, props, false));
        };
        set(&mut d, g, &|p| p.opacity = 0.5);
        set(&mut d, 0, &|p| {
            p.opacity = 0.5;
            p.parent = Some(group);
        });
        assert_eq!(d.view_alpha(0, false), 0.25);
        d.active_layer = 0;
        assert_eq!(
            d.view_alpha(0, true),
            0.25,
            "the active layer is not ghosted"
        );
        set(&mut d, 0, &|p| p.opacity = 1.0);
        set(&mut d, g, &|p| p.opacity = 1.0);
        assert_eq!(d.view_alpha(0, false), 1.0);
        d.active_layer = g;
        assert_eq!(d.view_alpha(0, true), GHOST_ALPHA);

        // The translucent part matches meshing the layer alone.
        set(&mut d, 0, &|p| p.opacity = 0.6);
        let view = d.view_meshes(false);
        assert!(view.opaque.is_empty());
        assert_eq!(*view.transparent[0].mesh, d.visible_mesh());
        assert_eq!(view.transparent[0].center, Vec3::splat(0.5));
    }

    #[test]
    fn opaque_layers_carry_their_material_slots() {
        let mut d = Document::new("t");
        d.set_voxels(0, [(IVec3::ZERO, 4), (IVec3::X, 4)], false);
        assert_eq!(d.add_material(), Some(0));
        let glow = Material {
            emission: [1.0, 0.5, 0.0],
            ..d.data.materials[0].clone()
        };
        assert!(d.set_material(0, glow.clone(), false));
        assert_eq!(d.data.materials[0], glow);
        let props = LayerProps {
            material: Some(0),
            ..d.data.layers[0].props.clone()
        };
        assert!(d.set_layer_props(0, props, false));
        assert_eq!(d.material_slot(0), 1);
        let one = d.view_meshes(false);
        assert!(one.opaque.materials.iter().all(|&s| s == 1));

        // A later default-material layer wins where it overlaps.
        d.add_layer();
        d.set_voxels(1, [(IVec3::X, 6)], false);
        let two = d.view_meshes(false);
        assert!(two.opaque.materials.contains(&0) && two.opaque.materials.contains(&1));
        // Different colors already split the quads: no extra triangles.
        assert_eq!(
            two.opaque.triangle_count(),
            d.visible_mesh().triangle_count()
        );

        // Tint alpha acts as opacity; undo restores the material.
        let faded = Material {
            tint: [1.0, 1.0, 1.0, 0.5],
            ..glow
        };
        assert!(d.set_material(0, faded, false));
        assert_eq!(d.view_alpha(0, false), 0.5);
        assert!(d.undo());
        assert_eq!(d.view_alpha(0, false), 1.0);
    }

    #[test]
    fn large_import_meshes_in_the_background() {
        // A 256³ document like an import: the viewport call returns at
        // once and the meshes fill in until they equal the synchronous ones.
        let mut volume = VoxelVolume::new(BoundSize::new(256, 256, 256));
        for z in 0..256 {
            for x in 0..256 {
                let h = 4 + ((x * 7 + z * 3) % 24);
                for y in 0..h {
                    volume.set(IVec3::new(x, y, z), 1 + ((x / 16 + z / 16) % 8) as Voxel);
                }
            }
        }
        let mut layer = Layer::new("import", BoundSize::new(256, 256, 256));
        layer.volume = volume;
        let d = Document::with_bounds("big", BoundSize::new(256, 256, 256), vec![layer]);

        let start = std::time::Instant::now();
        let first = d.view_meshes_background(false);
        let returned = start.elapsed();
        assert!(d.view_meshing(), "large updates go to the pool");
        assert!(
            first.opaque.is_empty(),
            "nothing finished yet on a first call"
        );
        let mut polls = 0;
        let mut last = first;
        while d.view_meshing() {
            if let Some(view) = d.poll_view_meshes() {
                last = view;
            }
            polls += 1;
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        if let Some(view) = d.poll_view_meshes() {
            last = view;
        }
        let total = start.elapsed();
        println!("256³ import: returned in {returned:?}, meshed in {total:?} ({polls} polls)");
        let params = MeshParams::new(&d.data.palette).ao(true);
        let full = rizxel_engine::mesh_volume_with(&d.data.layers[0].volume, &params);
        assert_eq!(*last.opaque, full);
        assert!(returned < total);
    }

    #[test]
    fn undoing_a_big_edit_meshes_in_the_background() {
        let mut d = Document::new("t");
        d.set_bounds(BoundSize::new(96, 32, 96), false);
        d.view_meshes_background(false);
        let wall: Vec<_> = (0..96)
            .flat_map(|x| (0..32).map(move |y| (IVec3::new(x, y, 40), 5)))
            .collect();
        d.set_voxels(0, wall.clone(), false);
        d.view_meshes_background(false);
        while d.view_meshing() {
            d.poll_view_meshes();
        }
        // Deleting only drops tiles: nothing to mesh.
        d.set_voxels(0, wall.into_iter().map(|(p, _)| (p, EMPTY_VOXEL)), false);
        assert!(d.view_meshes_background(false).opaque.is_empty());
        assert!(!d.view_meshing());
        // Undoing the delete brings back 12 tiles: background.
        assert!(d.undo());
        d.view_meshes_background(false);
        assert!(d.view_meshing());
        while d.view_meshing() {
            d.poll_view_meshes();
        }
        // A brush-sized edit stays synchronous.
        d.set_voxels(0, [(IVec3::new(3, 3, 3), 5)], false);
        let view = d.view_meshes_background(false);
        assert!(!d.view_meshing());
        let params = MeshParams::new(&d.data.palette).ao(true);
        let full = rizxel_engine::mesh_volume_with(&d.data.layers[0].volume, &params);
        assert_eq!(*view.opaque, full);
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
    fn tab_title_marks_unsaved_changes() {
        let mut d = Document::new("hero");
        assert_eq!(d.tab_title(), "hero.rzxl");
        d.add_layer();
        assert_eq!(d.tab_title(), "hero*.rzxl");
        d.path = Some(PathBuf::from("/m/my.model.rzxl"));
        assert_eq!(d.tab_title(), "my.model*.rzxl", "only the last extension");
        let path = std::env::temp_dir().join(format!("rizxel_tab_{}.rzxl", std::process::id()));
        d.save_to(&path).unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(d.tab_title(), d.title(), "saved: no marker");
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

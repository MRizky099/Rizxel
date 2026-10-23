//! # rizxel-app (L3)
//!
//! Headless application state. The UI layer reads this state and mutates it
//! only through the methods below, so all behavior is testable without a window.

pub mod autosave;
pub mod clipboard;
pub mod close;
pub mod document;
pub mod history;
pub mod interchange;
pub mod paths;
pub mod recent;
pub mod selection;

use std::path::Path;
use std::time::Instant;

use glam::camera::rh::{
    proj::directx::{orthographic, perspective},
    view::look_at_mat4,
};
use glam::{IVec3, Mat4, Vec3};
use rizxel_engine::tools::Transform;
use rizxel_engine::{RayHit, Tool, ToolOptions, VoxelOp};
use rizxel_io::RzxlError;
use rizxel_math::BoundSize;

pub use autosave::{RecoveryEntry, RecoveryMeta, AUTOSAVE_EDITS, AUTOSAVE_INTERVAL};
pub use clipboard::Clipboard;
pub use close::{CloseAction, CloseChoice, CloseRequest, CloseStep};
pub use document::{
    DocData, Document, EditBlock, FloatingPaste, Layer, LayerProps, SliceOutcome, TransformError,
    TransparentMesh, ViewMeshes, GHOST_ALPHA, MAX_MATERIAL_SLOT,
};
pub use history::{ClippedVoxel, Edit, History, VoxelChange};
pub use interchange::FileFormat;
pub use paths::AppPaths;
pub use recent::RecentFiles;
pub use rizxel_engine::Mesh;
/// Re-exported so the UI layer does not need to depend on `rizxel-io` directly.
pub use rizxel_io::RZXL_EXTENSION;
pub use selection::Selection;

/// File extension of mesh exports.
pub const OBJ_EXTENSION: &str = "obj";

/// Slice Mode navigation commands (Qubicle's *Tools ▸ Slice* menu).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SliceStep {
    /// One slice away from the camera.
    Next,
    /// One slice toward the camera.
    Previous,
    /// Layer 0 of the slice axis.
    First,
    /// The last layer of the slice axis.
    Last,
}

impl SliceStep {
    pub const ALL: [SliceStep; 4] = [Self::Next, Self::Previous, Self::First, Self::Last];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Next => "Next Slice",
            Self::Previous => "Previous Slice",
            Self::First => "First Slice",
            Self::Last => "Last Slice",
        }
    }
}

/// Axis names for status text.
const AXIS_NAMES: [&str; 3] = ["X", "Y", "Z"];

/// Viewport presentation mode (option bar toggle).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewMode {
    #[default]
    Model,
    Render,
}

/// Camera projection (`Numpad 5` toggles it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Projection {
    #[default]
    Perspective,
    /// Parallel rays: pixel-accurate face views and isometric corners.
    Orthographic,
}

impl Projection {
    pub const fn label(self) -> &'static str {
        match self {
            Projection::Perspective => "Perspective",
            Projection::Orthographic => "Orthographic",
        }
    }
}

/// Orbit camera parameters (degrees). Driven by the placeholder viewport in
/// Phase 2 and reused by the wgpu viewport in Phase 3.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OrbitCamera {
    /// Rotation around the vertical axis.
    pub yaw: f32,
    /// Elevation, clamped to `-90..=90`.
    pub pitch: f32,
    /// Orbit distance. The orthographic view height follows it (see
    /// [`OrbitCamera::view_height`]), so zoom and framing match perspective.
    pub distance: f32,
    /// Pan offset added to the document's default orbit target (world units).
    pub pan: Vec3,
    pub projection: Projection,
    /// Orthographic was switched on by a view snap (*Auto ortho*); orbiting
    /// by hand returns to perspective.
    pub auto_ortho: bool,
}

impl OrbitCamera {
    pub const MIN_DISTANCE: f32 = 2.0;
    pub const MAX_DISTANCE: f32 = 1000.0;
    /// Vertical field of view (radians).
    pub const FOV_Y: f32 = 45.0 * std::f32::consts::PI / 180.0;
    pub const NEAR: f32 = 0.1;
    pub const FAR: f32 = 4000.0;
    /// Distance of the orthographic eye from the target: far enough that
    /// every model and pan lie in front of it.
    pub const ORTHO_EYE_DISTANCE: f32 = 2000.0;
    /// Room left around a framed box (1 = touching the viewport edges).
    pub const FRAME_MARGIN: f32 = 1.1;

    /// Rotates by a delta (degrees), clamping pitch. Leaves an orthographic
    /// view that a snap switched on.
    pub fn orbit(&mut self, d_yaw: f32, d_pitch: f32) {
        self.yaw = (self.yaw + d_yaw).rem_euclid(360.0);
        self.pitch = (self.pitch + d_pitch).clamp(-90.0, 90.0);
        if self.auto_ortho && (d_yaw != 0.0 || d_pitch != 0.0) {
            self.auto_ortho = false;
            self.projection = Projection::Perspective;
        }
    }

    pub fn zoom(&mut self, factor: f32) {
        self.distance = (self.distance * factor).clamp(Self::MIN_DISTANCE, Self::MAX_DISTANCE);
    }

    /// Snaps the camera so its view direction points at `target`.
    /// Faces keep the level horizon; edges tilt 45 deg; corners use the
    /// isometric angles ([`ISO_PITCH`]).
    pub fn snap(&mut self, target: ViewTarget) {
        let (yaw, pitch) = target.angles();
        self.yaw = yaw;
        self.pitch = pitch;
    }

    /// [`OrbitCamera::snap`] that switches a perspective camera to
    /// orthographic for faces (Qubicle's *Perpendicular View*) and corners
    /// (isometric) when `auto_ortho` is on. Edges keep the projection.
    pub fn snap_view(&mut self, target: ViewTarget, auto_ortho: bool) {
        self.snap(target);
        let flat = matches!(target, ViewTarget::Face(_) | ViewTarget::Corner(_));
        if auto_ortho && flat && !self.is_ortho() {
            self.projection = Projection::Orthographic;
            self.auto_ortho = true;
        }
    }

    /// Switches between perspective and orthographic (`Numpad 5`).
    pub fn toggle_projection(&mut self) {
        self.projection = match self.projection {
            Projection::Perspective => Projection::Orthographic,
            Projection::Orthographic => Projection::Perspective,
        };
        self.auto_ortho = false;
    }

    pub fn is_ortho(&self) -> bool {
        self.projection == Projection::Orthographic
    }

    /// World units covered by the viewport height at the orbit target. The
    /// orthographic view uses it as its height, so toggling keeps the framing.
    pub fn view_height(&self) -> f32 {
        2.0 * self.distance * (Self::FOV_Y * 0.5).tan()
    }

    /// Unit vector from the orbit target to the eye.
    pub fn eye_direction(&self) -> Vec3 {
        // Avoid the look-at singularity when looking straight up/down.
        let pitch = self.pitch.clamp(-89.5, 89.5).to_radians();
        let yaw = self.yaw.to_radians();
        Vec3::new(
            pitch.cos() * yaw.sin(),
            pitch.sin(),
            pitch.cos() * yaw.cos(),
        )
    }

    /// Moves the orbit target in the view plane. `dx`/`dy` are screen-space
    /// fractions of the viewport height (right/down positive), so the scene
    /// follows the pointer at any zoom level.
    pub fn pan_screen(&mut self, dx: f32, dy: f32, fov_y: f32) {
        let forward = -self.eye_direction();
        let right = forward.cross(Vec3::Y).normalize_or(Vec3::X);
        let up = right.cross(forward);
        // World units covered by the full viewport height at the target distance.
        let span = 2.0 * self.distance * (fov_y * 0.5).tan();
        self.pan += (-right * dx + up * dy) * span;
    }

    /// Eye position orbiting `target`. The orthographic eye sits far back
    /// ([`OrbitCamera::ORTHO_EYE_DISTANCE`]) on the same line of sight.
    pub fn eye(&self, target: Vec3) -> Vec3 {
        let distance = match self.projection {
            Projection::Perspective => self.distance,
            Projection::Orthographic => Self::ORTHO_EYE_DISTANCE,
        };
        target + self.eye_direction() * distance
    }

    /// World -> view matrix (right-handed, +Y up).
    pub fn view_matrix(&self, target: Vec3) -> Mat4 {
        look_at_mat4(self.eye(target), target, Vec3::Y)
    }

    /// View -> clip matrix using the wgpu/DirectX convention (depth in `[0, 1]`).
    pub fn projection_matrix(&self, aspect: f32) -> Mat4 {
        let aspect = aspect.max(0.01);
        match self.projection {
            Projection::Perspective => perspective(Self::FOV_Y, aspect, Self::NEAR, Self::FAR),
            Projection::Orthographic => {
                let half_h = self.view_height() * 0.5;
                let half_w = half_h * aspect;
                orthographic(-half_w, half_w, -half_h, half_h, Self::NEAR, Self::FAR)
            }
        }
    }

    /// Combined world -> clip matrix for a viewport of the given aspect ratio.
    pub fn view_proj(&self, target: Vec3, aspect: f32) -> Mat4 {
        self.projection_matrix(aspect) * self.view_matrix(target)
    }

    /// Aims at the world-space box `min..max` and zooms so it fills the
    /// viewport, keeping the angles and the projection. `base` is the orbit
    /// target without pan ([`orbit_base`]).
    pub fn frame_box(&mut self, base: Vec3, min: Vec3, max: Vec3, aspect: f32) {
        let radius = ((max - min).length() * 0.5).max(0.5);
        // The narrower field of view decides; a sphere that fits the
        // perspective view also fits the orthographic one.
        let half_y = Self::FOV_Y * 0.5;
        let half_x = (half_y.tan() * aspect.max(0.01)).atan();
        let half = half_y.min(half_x);
        self.distance = (radius * Self::FRAME_MARGIN / half.sin())
            .clamp(Self::MIN_DISTANCE, Self::MAX_DISTANCE);
        self.pan = (min + max) * 0.5 - base;
    }

    /// Restores the default orientation, zoom, pan and projection (*Home*).
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

impl Default for OrbitCamera {
    fn default() -> Self {
        // Matches the prototype's initial cube transform: rotateX(-20) rotateY(-45).
        Self {
            yaw: 45.0,
            pitch: 20.0,
            distance: 64.0,
            pan: Vec3::ZERO,
            projection: Projection::Perspective,
            auto_ortho: false,
        }
    }
}

/// World-space min/max of the model bounds: centered on the origin in XZ and
/// standing on the ground (`y = 0`). Voxel `v` spans `min + v..min + v + 1`.
pub fn world_bounds(bounds: BoundSize) -> (Vec3, Vec3) {
    let size = bounds.0.as_vec3();
    let half = Vec3::new(size.x * 0.5, 0.0, size.z * 0.5);
    (-half, Vec3::new(half.x, size.y, half.z))
}

/// Orbit target for `bounds` before [`OrbitCamera::pan`] is added.
pub fn orbit_base(bounds: BoundSize) -> Vec3 {
    Vec3::new(0.0, bounds.0.y as f32 * 0.4, 0.0)
}

/// Pitch of a true isometric view, `atan(1 / sqrt(2))` (about 35.26°).
pub const ISO_PITCH: f32 = 35.264_39;

/// View cube selection: 6 faces + 12 edges + 8 corners (26 total).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewTarget {
    Face(ViewFace),
    Edge(ViewEdge),
    Corner(ViewCorner),
}

impl ViewTarget {
    /// Camera `(yaw, pitch)` that looks at this target.
    pub const fn angles(self) -> (f32, f32) {
        match self {
            ViewTarget::Face(f) => f.angles(),
            ViewTarget::Edge(e) => e.angles(),
            ViewTarget::Corner(c) => c.angles(),
        }
    }
}

/// Faces of the view cube.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewFace {
    Front,
    Back,
    Right,
    Left,
    Top,
    Bottom,
}

impl ViewFace {
    pub const ALL: [ViewFace; 6] = [
        ViewFace::Front,
        ViewFace::Back,
        ViewFace::Right,
        ViewFace::Left,
        ViewFace::Top,
        ViewFace::Bottom,
    ];

    /// Camera `(yaw, pitch)` that looks at this face.
    pub const fn angles(self) -> (f32, f32) {
        match self {
            ViewFace::Front => (0.0, 0.0),
            ViewFace::Right => (90.0, 0.0),
            ViewFace::Back => (180.0, 0.0),
            ViewFace::Left => (270.0, 0.0),
            ViewFace::Top => (0.0, 90.0),
            ViewFace::Bottom => (0.0, -90.0),
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            ViewFace::Front => "Front",
            ViewFace::Back => "Back",
            ViewFace::Right => "Right",
            ViewFace::Left => "Left",
            ViewFace::Top => "Top",
            ViewFace::Bottom => "Bottom",
        }
    }
}

/// Edges of the view cube (12 total).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewEdge {
    FrontTop,
    FrontBottom,
    FrontRight,
    FrontLeft,
    BackTop,
    BackBottom,
    BackRight,
    BackLeft,
    TopRight,
    TopLeft,
    BottomRight,
    BottomLeft,
}

impl ViewEdge {
    pub const ALL: [ViewEdge; 12] = [
        ViewEdge::FrontTop,
        ViewEdge::FrontBottom,
        ViewEdge::FrontRight,
        ViewEdge::FrontLeft,
        ViewEdge::BackTop,
        ViewEdge::BackBottom,
        ViewEdge::BackRight,
        ViewEdge::BackLeft,
        ViewEdge::TopRight,
        ViewEdge::TopLeft,
        ViewEdge::BottomRight,
        ViewEdge::BottomLeft,
    ];

    pub const fn angles(self) -> (f32, f32) {
        match self {
            ViewEdge::FrontTop => (0.0, 45.0),
            ViewEdge::FrontBottom => (0.0, -45.0),
            ViewEdge::FrontRight => (45.0, 0.0),
            ViewEdge::FrontLeft => (315.0, 0.0),
            ViewEdge::BackTop => (180.0, 45.0),
            ViewEdge::BackBottom => (180.0, -45.0),
            ViewEdge::BackRight => (135.0, 0.0),
            ViewEdge::BackLeft => (225.0, 0.0),
            ViewEdge::TopRight => (90.0, 45.0),
            ViewEdge::TopLeft => (270.0, 45.0),
            ViewEdge::BottomRight => (90.0, -45.0),
            ViewEdge::BottomLeft => (270.0, -45.0),
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            ViewEdge::FrontTop => "Front-Top",
            ViewEdge::FrontBottom => "Front-Bottom",
            ViewEdge::FrontRight => "Front-Right",
            ViewEdge::FrontLeft => "Front-Left",
            ViewEdge::BackTop => "Back-Top",
            ViewEdge::BackBottom => "Back-Bottom",
            ViewEdge::BackRight => "Back-Right",
            ViewEdge::BackLeft => "Back-Left",
            ViewEdge::TopRight => "Top-Right",
            ViewEdge::TopLeft => "Top-Left",
            ViewEdge::BottomRight => "Bottom-Right",
            ViewEdge::BottomLeft => "Bottom-Left",
        }
    }
}

/// Corners of the view cube (8 total).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewCorner {
    FrontTopRight,
    FrontTopLeft,
    FrontBottomRight,
    FrontBottomLeft,
    BackTopRight,
    BackTopLeft,
    BackBottomRight,
    BackBottomLeft,
}

impl ViewCorner {
    pub const ALL: [ViewCorner; 8] = [
        ViewCorner::FrontTopRight,
        ViewCorner::FrontTopLeft,
        ViewCorner::FrontBottomRight,
        ViewCorner::FrontBottomLeft,
        ViewCorner::BackTopRight,
        ViewCorner::BackTopLeft,
        ViewCorner::BackBottomRight,
        ViewCorner::BackBottomLeft,
    ];

    pub const fn angles(self) -> (f32, f32) {
        match self {
            ViewCorner::FrontTopRight => (45.0, ISO_PITCH),
            ViewCorner::FrontTopLeft => (315.0, ISO_PITCH),
            ViewCorner::FrontBottomRight => (45.0, -ISO_PITCH),
            ViewCorner::FrontBottomLeft => (315.0, -ISO_PITCH),
            ViewCorner::BackTopRight => (135.0, ISO_PITCH),
            ViewCorner::BackTopLeft => (225.0, ISO_PITCH),
            ViewCorner::BackBottomRight => (135.0, -ISO_PITCH),
            ViewCorner::BackBottomLeft => (225.0, -ISO_PITCH),
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            ViewCorner::FrontTopRight => "Front-Top-Right",
            ViewCorner::FrontTopLeft => "Front-Top-Left",
            ViewCorner::FrontBottomRight => "Front-Bottom-Right",
            ViewCorner::FrontBottomLeft => "Front-Bottom-Left",
            ViewCorner::BackTopRight => "Back-Top-Right",
            ViewCorner::BackTopLeft => "Back-Top-Left",
            ViewCorner::BackBottomRight => "Back-Bottom-Right",
            ViewCorner::BackBottomLeft => "Back-Bottom-Left",
        }
    }
}

/// What the viewport shows besides the model (*View* menu). View state, not
/// stored in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewOptions {
    /// Snapping to a face or corner switches to orthographic.
    pub auto_ortho: bool,
    /// The infinite ground grid (`G`). Off still clears to the background.
    pub floor_grid: bool,
    /// Thin lines along the voxel edges on the model's faces (`Shift+G`).
    pub voxel_grid: bool,
    /// The bounds wireframe, the occupied-region box and the slice frame
    /// (`Shift+B`). The slice frame's drag handle works either way.
    pub model_bounds: bool,
    /// Layers other than the active one drawn at [`GHOST_ALPHA`].
    pub ghosts: bool,
}

impl Default for ViewOptions {
    fn default() -> Self {
        Self {
            auto_ortho: true,
            floor_grid: true,
            voxel_grid: false,
            model_bounds: true,
            ghosts: false,
        }
    }
}

/// Top-level application state shared by every panel.
#[derive(Debug)]
pub struct AppState {
    pub documents: Vec<Document>,
    pub active_doc: usize,
    pub tool: Tool,
    pub op: VoxelOp,
    pub options: ToolOptions,
    pub view_mode: ViewMode,
    pub camera: OrbitCamera,
    /// View toggles (*View* menu). View state: not saved in the file.
    pub view: ViewOptions,
    /// Width / height of the viewport, set by the viewport every frame; the
    /// frame commands fit boxes to it.
    pub viewport_aspect: f32,
    /// Voxel under the cursor (set by the viewport; `None` when off-model).
    pub cursor_voxel: Option<IVec3>,
    /// Transient message shown in the status bar (e.g. save errors).
    pub status_message: Option<String>,
    /// Live feedback of the active tool (hovered slice plane, pending
    /// change count while dragging). Set by the viewport every frame.
    pub tool_feedback: Option<String>,
    /// Open "Save changes?" prompt (tab close or exit).
    closing: Option<CloseRequest>,
    /// App data folder; `None` keeps recent files and autosave in memory
    /// only (tests, or no home directory).
    paths: Option<AppPaths>,
    /// *File ▸ Open Recent*.
    pub recent: RecentFiles,
    autosave: autosave::Autosave,
    /// Voxels copied or cut, shared by every open document.
    pub clipboard: Option<Clipboard>,
    untitled_counter: u32,
}

impl Default for AppState {
    fn default() -> Self {
        let mut state = Self {
            documents: Vec::new(),
            active_doc: 0,
            tool: Tool::default(),
            op: VoxelOp::default(),
            options: ToolOptions::default(),
            view_mode: ViewMode::default(),
            camera: OrbitCamera::default(),
            view: ViewOptions::default(),
            viewport_aspect: 16.0 / 9.0,
            cursor_voxel: None,
            status_message: None,
            tool_feedback: None,
            closing: None,
            paths: None,
            recent: RecentFiles::default(),
            autosave: autosave::Autosave::default(),
            clipboard: None,
            untitled_counter: 0,
        };
        state.new_document();
        state
    }
}

impl AppState {
    /// State backed by an app data folder: loads the recent files list.
    pub fn with_paths(paths: Option<AppPaths>) -> Self {
        let mut state = Self::default();
        if let Some(p) = &paths {
            state.recent = RecentFiles::load(&p.recent_file());
        }
        state.paths = paths;
        state
    }

    pub fn paths(&self) -> Option<&AppPaths> {
        self.paths.as_ref()
    }

    // --------------------------------------------------------- recent files

    /// Puts `path` at the top of the recent files and writes the list.
    pub fn remember_recent(&mut self, path: &Path) {
        self.recent.push(path);
        self.write_recent();
    }

    /// Drops recent files that no longer exist (when the menu opens).
    pub fn prune_recent(&mut self) {
        if self.recent.prune() {
            self.write_recent();
        }
    }

    pub fn clear_recent(&mut self) {
        self.recent.clear();
        self.write_recent();
    }

    fn write_recent(&self) {
        if let Some(p) = &self.paths {
            if let Err(e) = self.recent.save(&p.recent_file()) {
                log::warn!("recent files: {e}");
            }
        }
    }

    // ------------------------------------------------------------ documents

    pub fn doc(&self) -> Option<&Document> {
        self.documents.get(self.active_doc)
    }

    pub fn doc_mut(&mut self) -> Option<&mut Document> {
        self.documents.get_mut(self.active_doc)
    }

    /// Creates an untitled document and activates it.
    pub fn new_document(&mut self) {
        self.untitled_counter += 1;
        let name = format!("untitled_{}", self.untitled_counter);
        self.push_document(Document::new(name));
    }

    pub fn push_document(&mut self, doc: Document) {
        self.cancel_active_edit();
        self.documents.push(doc);
        self.active_doc = self.documents.len() - 1;
    }

    pub fn activate(&mut self, index: usize) {
        if index < self.documents.len() && index != self.active_doc {
            self.cancel_active_edit();
            self.active_doc = index;
        }
    }

    /// Closes a tab (its recovery file goes too). Keeps at least one
    /// document open.
    pub fn close_document(&mut self, index: usize) {
        if index >= self.documents.len() {
            return;
        }
        let doc = self.documents.remove(index);
        self.drop_recovery(doc.id());
        if self.documents.is_empty() {
            self.new_document();
        } else if self.active_doc >= index && self.active_doc > 0 {
            self.active_doc -= 1;
        }
    }

    fn doc_index(&self, id: u64) -> Option<usize> {
        self.documents.iter().position(|d| d.id() == id)
    }

    /// Closes tab `index`, asking first when it has unsaved changes. Returns
    /// `true` when the tab closed right away; otherwise the prompt is open
    /// (see [`AppState::close_prompt`]).
    pub fn close_document_checked(&mut self, index: usize) -> bool {
        let Some(doc) = self.documents.get(index) else {
            return false;
        };
        if !doc.is_dirty() {
            self.close_document(index);
            return true;
        }
        let request = CloseRequest::close_tab(doc.id());
        let step = request.start();
        self.closing = Some(request);
        self.follow_close(step);
        false
    }

    /// Exit requested (menu, title bar, `Alt+F4`, the OS). Returns `true`
    /// when nothing is unsaved and the app may quit now; otherwise the
    /// prompt walks the dirty documents one at a time.
    pub fn request_exit(&mut self) -> bool {
        if self.closing.is_some() {
            return false;
        }
        self.cancel_active_edit();
        let dirty: Vec<u64> = self
            .documents
            .iter()
            .filter(|d| d.is_dirty())
            .map(Document::id)
            .collect();
        if dirty.is_empty() {
            return true;
        }
        let request = CloseRequest::exit(dirty);
        let step = request.start();
        self.closing = Some(request);
        self.follow_close(step) == CloseStep::Quit
    }

    /// Open close request, if any.
    pub fn closing(&self) -> Option<&CloseRequest> {
        self.closing.as_ref()
    }

    /// Document the "Save changes?" prompt is about (also the active one).
    pub fn close_prompt(&self) -> Option<&Document> {
        let id = self.closing.as_ref()?.current()?;
        self.documents.iter().find(|d| d.id() == id)
    }

    /// Answers the prompt. [`CloseStep::Save`] asks the caller to save the
    /// active document (Save As when untitled) and then call
    /// [`AppState::close_after_save`]; [`CloseStep::Quit`] means the app may
    /// quit. Closing tabs and walking to the next document happen here.
    pub fn answer_close(&mut self, choice: CloseChoice) -> CloseStep {
        let Some(request) = &mut self.closing else {
            return CloseStep::Abort;
        };
        let step = request.answer(choice);
        self.follow_close(step)
    }

    /// Continues after the save asked for by [`CloseStep::Save`]: a document
    /// that is still dirty (Save As cancelled, write failed) aborts.
    pub fn close_after_save(&mut self) -> CloseStep {
        let saved = self.close_prompt().is_some_and(|d| !d.is_dirty());
        let Some(request) = &mut self.closing else {
            return CloseStep::Abort;
        };
        let step = request.saved(saved);
        self.follow_close(step)
    }

    fn follow_close(&mut self, step: CloseStep) -> CloseStep {
        match step {
            CloseStep::Ask(id) | CloseStep::Save(id) => match self.doc_index(id) {
                Some(i) => self.activate(i),
                // Gone meanwhile: nothing left to ask about.
                None => return self.answer_close(CloseChoice::DontSave),
            },
            CloseStep::Close(id) => {
                self.closing = None;
                if let Some(i) = self.doc_index(id) {
                    self.close_document(i);
                }
            }
            CloseStep::Quit | CloseStep::Abort => self.closing = None,
        }
        step
    }

    /// Opens a `.rzxl`. If it is already open, that tab is activated instead.
    pub fn open(&mut self, path: &Path) -> Result<(), RzxlError> {
        if let Some(i) = self
            .documents
            .iter()
            .position(|d| d.path.as_deref() == Some(path))
        {
            self.active_doc = i;
            return Ok(());
        }
        let doc = Document::load(path)?;
        self.cancel_active_edit();
        // Replace a pristine untitled tab instead of adding another one.
        let replace_pristine = self.documents.len() == 1
            && self.documents[0].path.is_none()
            && !self.documents[0].is_dirty();
        if replace_pristine {
            self.documents[0] = doc;
            self.active_doc = 0;
        } else {
            self.push_document(doc);
        }
        self.remember_recent(path);
        Ok(())
    }

    /// Saves the active document to `path`.
    pub fn save_active_as(&mut self, path: &Path) -> Result<(), RzxlError> {
        let id = match self.doc_mut() {
            Some(doc) => {
                doc.save_to(path)?;
                doc.id()
            }
            None => return Ok(()),
        };
        self.remember_recent(path);
        // Saved by hand: the autosave copy is obsolete.
        if let (Some(key), Some(paths)) = (self.autosave.saved(id), &self.paths) {
            autosave::remove_recovery(paths, &key);
        }
        Ok(())
    }

    // ------------------------------------------------------------ autosave

    /// Starts background autosaves of the documents that are due at `now`
    /// and collects finished ones; a failure goes to the status bar.
    pub fn autosave_tick(&mut self, now: Instant) {
        let Some(paths) = &self.paths else { return };
        for id in self.autosave.due(now, &self.documents) {
            if let Some(doc) = self.documents.iter().find(|d| d.id() == id) {
                self.autosave.write(now, doc, paths);
            }
        }
        self.collect_autosaves(false);
    }

    /// Waits for every autosave in flight.
    pub fn flush_autosave(&mut self) {
        self.collect_autosaves(true);
    }

    fn collect_autosaves(&mut self, block: bool) {
        for (id, result) in self.autosave.finished(block) {
            if let Err(e) = result {
                log::warn!("autosave: {e}");
                self.status_message = Some(format!("Autosave failed: {e}"));
                continue;
            }
            // Closed or saved while the write ran: the copy is obsolete.
            let obsolete = self
                .documents
                .iter()
                .find(|d| d.id() == id)
                .is_none_or(|d| !d.is_dirty());
            if obsolete {
                self.drop_recovery(id);
            }
        }
    }

    /// Deletes the recovery file of document `id` and forgets it.
    fn drop_recovery(&mut self, id: u64) {
        if let (Some(key), Some(paths)) = (self.autosave.forget(id), &self.paths) {
            autosave::remove_recovery(paths, &key);
        }
    }

    /// Clean exit: every document is answered for, so no recovery file stays.
    pub fn clean_exit(&mut self) {
        self.flush_autosave();
        let ids: Vec<u64> = self.documents.iter().map(Document::id).collect();
        for id in ids {
            self.drop_recovery(id);
        }
    }

    /// Recovery files left by an earlier session that did not exit cleanly.
    pub fn recoverable(&self) -> Vec<RecoveryEntry> {
        self.paths
            .as_ref()
            .map(autosave::recoverable)
            .unwrap_or_default()
    }

    /// Opens a recovery file as a dirty document with its original path.
    /// The recovery file stays until the document is saved or closed.
    pub fn restore(&mut self, entry: &RecoveryEntry) -> Result<(), RzxlError> {
        let Some(paths) = &self.paths else {
            return Err(RzxlError::Invalid("no app data folder".to_owned()));
        };
        let project = rizxel_io::load_rzxl(&entry.rzxl(paths))?;
        let mut doc = Document::from_project(project, entry.meta.original.clone())?;
        doc.name.clone_from(&entry.meta.name);
        doc.mark_unsaved();
        self.autosave.adopt_key(doc.id(), entry.key.clone());
        self.cancel_active_edit();
        let replace_pristine = self.documents.len() == 1
            && self.documents[0].path.is_none()
            && !self.documents[0].is_dirty();
        if replace_pristine {
            let old = std::mem::replace(&mut self.documents[0], doc);
            self.drop_recovery(old.id());
            self.active_doc = 0;
        } else {
            self.push_document(doc);
        }
        Ok(())
    }

    /// Deletes a recovery file the user chose not to restore.
    pub fn discard_recovery(&self, entry: &RecoveryEntry) {
        if let Some(paths) = &self.paths {
            autosave::remove_recovery(paths, &entry.key);
        }
    }

    /// Cancels a tool drag in progress on the active document (tab switch,
    /// open, new document).
    pub fn cancel_active_edit(&mut self) {
        if let Some(d) = self.doc_mut() {
            d.cancel_edit();
        }
    }

    // ----------------------------------------------------------- history

    pub fn undo(&mut self) {
        if let Some(d) = self.doc_mut() {
            d.undo();
        }
    }

    pub fn redo(&mut self) {
        if let Some(d) = self.doc_mut() {
            d.redo();
        }
    }

    pub fn can_undo(&self) -> bool {
        self.doc().is_some_and(Document::can_undo)
    }

    pub fn can_redo(&self) -> bool {
        self.doc().is_some_and(Document::can_redo)
    }

    // --------------------------------------------------------- selection

    /// Number of selected cells on the active layer of the active document.
    pub fn selected_voxels(&self) -> usize {
        self.doc()
            .and_then(Document::selection)
            .map_or(0, Selection::len)
    }

    // ------------------------------------------------------ edit menu

    /// The active layer stores voxels and has some (Select All, Invert,
    /// Copy, transforms).
    pub fn active_layer_has_voxels(&self) -> bool {
        self.doc()
            .and_then(Document::active_layer)
            .is_some_and(|l| l.kind().stores_voxels() && !l.volume.is_empty())
    }

    /// Tools may write into the active layer (hidden counts as writable).
    pub fn active_layer_editable(&self) -> bool {
        self.doc().is_some_and(|d| {
            matches!(
                d.editable_layer(d.active_layer),
                Ok(()) | Err(EditBlock::Hidden)
            )
        })
    }

    pub fn select_all(&mut self) {
        if let Some(d) = self.doc_mut() {
            d.select_all();
            let n = d.selection().map_or(0, Selection::len);
            self.status_message = Some(format!("Selected {n} voxels"));
        }
    }

    pub fn invert_selection(&mut self) {
        if let Some(d) = self.doc_mut() {
            d.invert_selection();
            let n = d.selection().map_or(0, Selection::len);
            self.status_message = Some(format!("Selection inverted: {n} voxels"));
        }
    }

    pub fn deselect(&mut self) {
        if self.doc_mut().is_some_and(Document::clear_selection) {
            self.status_message = Some("Selection cleared".to_owned());
        }
    }

    /// Deletes the selected voxels of the active layer (`Delete`).
    pub fn delete_selected(&mut self) {
        let Some(doc) = self.doc_mut() else { return };
        let removed = doc.delete_selected();
        self.status_message = Some(match doc.editable_layer(doc.active_layer) {
            Err(e) if removed == 0 && e != EditBlock::Hidden => e.message().to_owned(),
            _ => format!("Deleted {removed} selected voxels"),
        });
    }

    // --------------------------------------------------------- clipboard

    /// Copies the selection (or the whole active layer) to the clipboard.
    pub fn copy(&mut self) -> bool {
        let clip = self.doc().and_then(Clipboard::copy);
        self.status_message = Some(match &clip {
            Some(c) => format!("Copied {} voxels", c.cells.len()),
            None => "Nothing to copy".to_owned(),
        });
        let copied = clip.is_some();
        if copied {
            self.clipboard = clip;
        }
        copied
    }

    /// Copies, then deletes the selection (or the whole active layer), as
    /// one undo step.
    pub fn cut(&mut self) -> bool {
        let Some(doc) = self.doc_mut() else {
            return false;
        };
        let block = doc.editable_layer(doc.active_layer).err();
        let clip = doc.cut();
        self.status_message = Some(match (&clip, block) {
            (Some(c), _) => format!("Cut {} voxels", c.cells.len()),
            (None, Some(e)) if e != EditBlock::Hidden => e.message().to_owned(),
            (None, _) => "Nothing to cut".to_owned(),
        });
        let cut = clip.is_some();
        if cut {
            self.clipboard = clip;
        }
        cut
    }

    /// Starts a floating paste on the active layer that follows the cursor,
    /// starting with its bottom center at `at` (the hovered cell) or, off
    /// the model, where it was copied from.
    pub fn paste(&mut self, at: Option<IVec3>) -> bool {
        self.start_paste(at, true)
    }

    /// Floating paste at the position it was copied from; it stays there
    /// until committed.
    pub fn paste_in_place(&mut self) -> bool {
        self.start_paste(None, false)
    }

    fn start_paste(&mut self, at: Option<IVec3>, follow: bool) -> bool {
        let Some(clip) = self.clipboard.clone() else {
            self.status_message = Some("The clipboard is empty".to_owned());
            return false;
        };
        let origin = at.map_or(clip.origin, |cell| clip.origin_at(cell));
        let Some(doc) = self.doc_mut() else {
            return false;
        };
        let result = doc.begin_paste(&clip, origin, follow);
        self.status_message = Some(match result {
            Ok(()) => format!(
                "Pasting {} voxels: click or Enter to place, Esc to cancel",
                clip.cells.len()
            ),
            Err(e) => e.message().to_owned(),
        });
        result.is_ok()
    }

    /// Pastes in place into a new layer (one undo step).
    pub fn paste_to_new_layer(&mut self) -> bool {
        let Some(clip) = self.clipboard.clone() else {
            self.status_message = Some("The clipboard is empty".to_owned());
            return false;
        };
        let Some(doc) = self.doc_mut() else {
            return false;
        };
        doc.paste_to_new_layer(&clip);
        self.status_message = Some(format!(
            "Pasted {} voxels into a new layer",
            clip.cells.len()
        ));
        true
    }

    /// Places the floating paste of the active document.
    pub fn commit_paste(&mut self) {
        if let Some(doc) = self.doc_mut().filter(|d| d.floating_paste().is_some()) {
            let n = doc.commit_paste();
            self.status_message = Some(format!("Pasted {n} voxels"));
        }
    }

    pub fn cancel_paste(&mut self) {
        if let Some(doc) = self.doc_mut().filter(|d| d.floating_paste().is_some()) {
            doc.cancel_paste();
            self.status_message = Some("Paste cancelled".to_owned());
        }
    }

    // -------------------------------------------------------- transforms

    /// Axis the camera faces most (the default axis of rotate and flip).
    pub fn facing_axis(&self) -> usize {
        rizxel_engine::raycast::dominant_axis(self.camera.eye_direction())
    }

    /// Rotates the selection (or active layer) a quarter turn about `axis`:
    /// `turns` is counterclockwise seen from `+axis`.
    pub fn rotate(&mut self, axis: usize, turns: i32) {
        let label = format!("Rotate {}° about {}", turns * 90, AXIS_NAMES[axis]);
        self.run_transform(&label, |d| d.transform(Transform::Rotate { axis, turns }));
    }

    /// Rotates about the axis the camera faces, counterclockwise on screen
    /// (`clockwise` reverses it): `Ctrl+R` / `Ctrl+Shift+R`.
    pub fn rotate_facing(&mut self, clockwise: bool) {
        let axis = self.facing_axis();
        // Seen from the -axis side, a screen-counterclockwise turn is negative.
        let toward_plus = self.camera.eye_direction()[axis] >= 0.0;
        let turns = if toward_plus != clockwise { 1 } else { -1 };
        self.rotate(axis, turns);
    }

    pub fn flip(&mut self, axis: usize) {
        let label = format!("Flip {}", AXIS_NAMES[axis]);
        self.run_transform(&label, |d| d.transform(Transform::Flip { axis }));
    }

    /// Copies the `-axis` half onto the `+axis` half (Mirror ▸).
    pub fn mirror(&mut self, axis: usize) {
        let label = format!("Mirror {}", AXIS_NAMES[axis]);
        self.run_transform(&label, |d| d.mirror(axis));
    }

    fn run_transform(
        &mut self,
        label: &str,
        f: impl FnOnce(&mut Document) -> Result<usize, TransformError>,
    ) {
        let Some(doc) = self.doc_mut() else { return };
        self.status_message = Some(match f(doc) {
            Ok(n) => format!("{label}: {n} voxels changed"),
            Err(e) => format!("{label}: {}", e.message()),
        });
    }

    // ------------------------------------------------------------- slice

    /// Slice Mode on/off (`2D` button, `Space`) for the active document:
    /// over a voxel (`hover`) it slices there, otherwise it restores the
    /// last slice. Never switches tools.
    pub fn toggle_slice(&mut self, hover: Option<&RayHit>) {
        let Some(doc) = self.doc_mut() else { return };
        let bounds = doc.data.bounds;
        let on = doc.plane.toggle_slice(hover, bounds);
        self.status_message = Some(match (on, self.slice_counter()) {
            (true, Some((axis, n, count))) => {
                format!("Slice Mode: {} slice {n} / {count}", AXIS_NAMES[axis])
            }
            _ => "Slice Mode off".to_owned(),
        });
    }

    /// Shows or hides the work plane (`/`). Hiding it ends Slice Mode, and
    /// showing it again resumes that slice.
    pub fn toggle_plane_visible(&mut self) {
        let Some(doc) = self.doc_mut() else { return };
        let bounds = doc.data.bounds;
        let shown = doc.plane.toggle_visible(bounds);
        self.status_message = Some(match (shown, self.slice_counter()) {
            (true, Some((axis, n, count))) => {
                format!(
                    "Work plane shown, Slice Mode: {} slice {n} / {count}",
                    AXIS_NAMES[axis]
                )
            }
            (true, None) => "Work plane shown".to_owned(),
            (false, _) => "Work plane hidden".to_owned(),
        });
    }

    /// Moves the slice of the active document (no undo step).
    pub fn step_slice(&mut self, step: SliceStep) {
        let Some(doc) = self.doc_mut().filter(|d| d.plane.slice) else {
            return;
        };
        let bounds = doc.data.bounds;
        let p = &mut doc.plane;
        match step {
            SliceStep::Next => p.step_slice(1, bounds),
            SliceStep::Previous => p.step_slice(-1, bounds),
            SliceStep::First => p.set_slice_layer(0, bounds),
            SliceStep::Last => p.set_slice_layer(p.slice_count(bounds) - 1, bounds),
        }
    }

    /// `(axis, 1-based slice number, slice count)` while slicing.
    pub fn slice_counter(&self) -> Option<(usize, i32, i32)> {
        let doc = self.doc().filter(|d| d.plane.slice)?;
        let bounds = doc.data.bounds;
        let p = doc.plane.clamped(bounds);
        Some((p.axis, p.slice_layer(bounds) + 1, p.slice_count(bounds)))
    }

    // ------------------------------------------------------------ camera

    /// Snaps the camera to a view cube target, switching to orthographic
    /// for faces and corners when *Auto Ortho* is on.
    pub fn snap_view(&mut self, target: ViewTarget) {
        self.camera.snap_view(target, self.view.auto_ortho);
    }

    /// *Frame Selection* (`.`): fits the selection of the active layer, or
    /// the visible voxels without one, or the bounds of an empty model.
    pub fn frame_selection(&mut self) {
        let Some(doc) = self.doc() else {
            return;
        };
        let cells = doc
            .selection()
            .and_then(Selection::bounds)
            .or_else(|| doc.occupied_bounds());
        match cells {
            Some(cells) => self.frame_cells(cells),
            None => self.frame_work_area(),
        }
    }

    /// *Frame Work Area* (`Home`): fits the model bounds.
    pub fn frame_work_area(&mut self) {
        if let Some(bounds) = self.doc().map(|d| d.data.bounds) {
            self.frame_cells((IVec3::ZERO, bounds.0.as_ivec3()));
        }
    }

    /// Fits the voxel box `(min, max exclusive)` of the active document.
    fn frame_cells(&mut self, (lo, hi): (IVec3, IVec3)) {
        let Some(bounds) = self.doc().map(|d| d.data.bounds) else {
            return;
        };
        let (bmin, _) = world_bounds(bounds);
        self.camera.frame_box(
            orbit_base(bounds),
            bmin + lo.as_vec3(),
            bmin + hi.as_vec3(),
            self.viewport_aspect,
        );
    }

    // ------------------------------------------------------------ status

    /// Hint text for the status bar.
    pub fn hint(&self) -> String {
        format!(
            "{} Tool: {}",
            self.tool.label(),
            self.tool
                .hint_with(self.doc().is_some_and(|d| d.plane.slice))
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn starts_with_one_untitled_doc() {
        let s = AppState::default();
        assert_eq!(s.documents.len(), 1);
        assert_eq!(s.doc().unwrap().title(), "untitled_1.rzxl");
        assert!(s
            .hint()
            .starts_with("Plane Tool: Click a face to place the work plane"));
    }

    #[test]
    fn slice_commands_step_and_count_from_one() {
        let mut s = AppState {
            tool: Tool::Brush,
            ..AppState::default()
        };
        assert_eq!(s.slice_counter(), None);
        s.step_slice(SliceStep::Next);
        assert_eq!(s.slice_counter(), None, "no-op outside Slice Mode");
        s.toggle_slice(None);
        let n = s.doc().unwrap().data.bounds.y() as i32;
        assert_eq!(s.slice_counter(), Some((1, 1, n)));
        assert_eq!(s.tool, Tool::Brush, "the tool is kept");
        s.step_slice(SliceStep::Last);
        assert_eq!(s.slice_counter(), Some((1, n, n)));
        // The +Y side is hidden (camera above): Next goes down.
        s.step_slice(SliceStep::Next);
        assert_eq!(s.slice_counter(), Some((1, n - 1, n)));
        s.step_slice(SliceStep::Previous);
        s.step_slice(SliceStep::First);
        assert_eq!(s.slice_counter(), Some((1, 1, n)));
        s.toggle_slice(None);
        assert_eq!(s.slice_counter(), None);
        assert_eq!(s.status_message.as_deref(), Some("Slice Mode off"));

        // `/` while slicing hides the plane and ends Slice Mode; `/` again
        // resumes it on the same slice.
        s.toggle_slice(None);
        s.step_slice(SliceStep::Next);
        let before = s.slice_counter();
        s.toggle_plane_visible();
        assert_eq!(s.slice_counter(), None);
        s.toggle_plane_visible();
        assert_eq!(s.slice_counter(), before);
        assert!(s.doc().unwrap().plane.visible);
    }

    #[test]
    fn closing_tabs_keeps_valid_active_index() {
        let mut s = AppState::default();
        s.new_document();
        s.new_document();
        assert_eq!(s.active_doc, 2);
        s.close_document(2);
        assert_eq!(s.active_doc, 1);
        s.close_document(0);
        assert_eq!(s.active_doc, 0);
        s.close_document(0);
        assert_eq!(s.documents.len(), 1, "always keeps one document");
    }

    fn dirty(s: &mut AppState) {
        s.doc_mut().unwrap().add_layer();
    }

    #[test]
    fn clean_documents_close_without_a_prompt() {
        let mut s = AppState::default();
        s.new_document();
        assert!(s.close_document_checked(1));
        assert_eq!(s.documents.len(), 1);
        assert!(s.request_exit());
        assert!(s.closing().is_none());
    }

    #[test]
    fn cancel_keeps_everything_open() {
        let mut s = AppState::default();
        dirty(&mut s);
        s.new_document();
        assert!(!s.close_document_checked(0));
        assert_eq!(s.active_doc, 0, "the prompt shows its document");
        assert_eq!(
            s.close_prompt().map(Document::id),
            Some(s.documents[0].id())
        );
        assert_eq!(s.answer_close(CloseChoice::Cancel), CloseStep::Abort);
        assert_eq!(s.documents.len(), 2);
        assert!(s.close_prompt().is_none());

        assert!(!s.request_exit());
        assert_eq!(s.answer_close(CloseChoice::Cancel), CloseStep::Abort);
        assert_eq!(s.documents.len(), 2);
        assert!(s.documents[0].is_dirty());
    }

    #[test]
    fn dont_save_on_a_tab_closes_it() {
        let mut s = AppState::default();
        s.new_document();
        dirty(&mut s);
        let id = s.doc().unwrap().id();
        assert!(!s.close_document_checked(1));
        assert_eq!(s.answer_close(CloseChoice::DontSave), CloseStep::Close(id));
        assert_eq!(s.documents.len(), 1);
    }

    #[test]
    fn exit_asks_about_each_dirty_document() {
        let mut s = AppState::default();
        dirty(&mut s);
        s.new_document();
        s.new_document();
        dirty(&mut s);
        let (a, c) = (s.documents[0].id(), s.documents[2].id());
        assert!(!s.request_exit());
        assert_eq!(s.close_prompt().map(Document::id), Some(a));
        assert_eq!(s.answer_close(CloseChoice::DontSave), CloseStep::Ask(c));
        assert_eq!(s.active_doc, 2);
        assert_eq!(s.answer_close(CloseChoice::DontSave), CloseStep::Quit);
        assert!(s.closing().is_none());
    }

    #[test]
    fn save_during_exit_continues_only_when_saved() {
        let path = std::env::temp_dir().join(format!("rizxel_close_{}.rzxl", std::process::id()));
        let mut s = AppState::default();
        dirty(&mut s);
        s.new_document();
        dirty(&mut s);
        let second = s.documents[1].id();

        assert!(!s.request_exit());
        assert!(matches!(
            s.answer_close(CloseChoice::Save),
            CloseStep::Save(_)
        ));
        // Save As cancelled: the untitled document stays dirty, exit aborts.
        assert_eq!(s.close_after_save(), CloseStep::Abort);
        assert!(s.closing().is_none());

        assert!(!s.request_exit());
        assert!(matches!(
            s.answer_close(CloseChoice::Save),
            CloseStep::Save(_)
        ));
        s.save_active_as(&path).unwrap();
        assert_eq!(s.close_after_save(), CloseStep::Ask(second));
        assert_eq!(s.answer_close(CloseChoice::DontSave), CloseStep::Quit);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn save_and_open_roundtrip() {
        let path = std::env::temp_dir().join(format!("rizxel_app_{}.rzxl", std::process::id()));
        let mut s = AppState::default();
        s.doc_mut().unwrap().add_layer();
        s.save_active_as(&path).unwrap();
        assert!(!s.doc().unwrap().is_dirty());

        let mut s2 = AppState::default();
        s2.open(&path).unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(s2.documents.len(), 1, "pristine tab replaced");
        assert_eq!(s2.doc().unwrap().data.layers.len(), 2);
    }

    #[test]
    fn opens_and_saves_go_to_recent_files() {
        let dir = std::env::temp_dir().join(format!("rizxel_paths_{}", std::process::id()));
        let paths = AppPaths::with_root(dir.join("data"));
        let file = dir.join("a.rzxl");
        std::fs::create_dir_all(&dir).unwrap();
        let mut s = AppState::with_paths(Some(paths.clone()));
        s.save_active_as(&file).unwrap();
        assert_eq!(s.recent.paths().len(), 1);
        // A fresh start reads the list back.
        let mut s2 = AppState::with_paths(Some(paths));
        assert!(s2.recent.paths()[0].ends_with("a.rzxl"));
        s2.open(&file).unwrap();
        assert_eq!(s2.recent.paths().len(), 1, "no duplicate");
        std::fs::remove_file(&file).unwrap();
        s2.prune_recent();
        assert!(s2.recent.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    fn temp_paths(tag: &str) -> (std::path::PathBuf, AppPaths) {
        let dir = std::env::temp_dir().join(format!("rizxel_{tag}_{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        let paths = AppPaths::with_root(&dir);
        (dir, paths)
    }

    fn file_len(path: &Path) -> u64 {
        std::fs::metadata(path).map_or(0, |m| m.len())
    }

    #[test]
    fn autosave_waits_for_time_or_edits() {
        let (dir, paths) = temp_paths("autosave_timing");
        let mut s = AppState::with_paths(Some(paths.clone()));
        let t0 = Instant::now();
        s.autosave_tick(t0);
        dirty(&mut s);
        s.autosave_tick(t0);
        s.flush_autosave();
        assert!(s.recoverable().is_empty(), "the clock only just started");
        // Before the interval nothing is written; after it, once.
        s.autosave_tick(t0 + AUTOSAVE_INTERVAL / 2);
        s.flush_autosave();
        assert!(s.recoverable().is_empty());
        let t1 = t0 + AUTOSAVE_INTERVAL;
        s.autosave_tick(t1);
        s.flush_autosave();
        let entries = s.recoverable();
        assert_eq!(entries.len(), 1);
        let file = entries[0].rzxl(&paths);
        let first = file_len(&file);

        // Never during an open transaction.
        let doc = s.doc_mut().unwrap();
        doc.begin_edit();
        doc.edit_voxels(
            0,
            &[VoxelChange {
                pos: IVec3::ZERO,
                old: 0,
                new: 3,
            }],
        );
        s.autosave_tick(t1 + AUTOSAVE_INTERVAL * 2);
        s.flush_autosave();
        assert_eq!(file_len(&file), first, "skipped mid-edit");
        s.doc_mut().unwrap().cancel_edit();

        // 50 edits write again before the interval is over.
        let t2 = t1 + AUTOSAVE_INTERVAL * 2;
        s.autosave_tick(t2);
        s.flush_autosave();
        let second = file_len(&file);
        for i in 0..AUTOSAVE_EDITS as i32 {
            s.doc_mut()
                .unwrap()
                .set_voxels(0, [(IVec3::new(i % 30, i / 30, 1), 2)], false);
        }
        s.autosave_tick(t2 + Duration::from_secs(1));
        s.flush_autosave();
        assert_ne!(file_len(&file), second);

        s.clean_exit();
        assert!(s.recoverable().is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_crash_leaves_a_restorable_copy() {
        let (dir, paths) = temp_paths("autosave_crash");
        let mut s = AppState::with_paths(Some(paths.clone()));
        let t0 = Instant::now();
        s.autosave_tick(t0);
        s.doc_mut().unwrap().set_voxels(
            0,
            [(IVec3::new(1, 2, 3), 5), (IVec3::new(4, 0, 0), 7)],
            false,
        );
        s.autosave_tick(t0);
        s.autosave_tick(t0 + AUTOSAVE_INTERVAL);
        s.flush_autosave();
        let data = s.doc().unwrap().data.clone();
        drop(s); // "crash": no clean exit

        let mut fresh = AppState::with_paths(Some(paths));
        let entries = fresh.recoverable();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].meta.title, "untitled_1.rzxl");
        fresh.restore(&entries[0]).unwrap();
        assert_eq!(fresh.documents.len(), 1, "pristine tab replaced");
        let doc = fresh.doc().unwrap();
        assert!(doc.is_dirty());
        assert_eq!(doc.name, "untitled_1");
        assert_eq!(doc.data, data);
        // The file stays until the restored document is saved or closed.
        assert_eq!(fresh.recoverable().len(), 1);
        fresh.close_document(0);
        assert!(fresh.recoverable().is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn saving_removes_the_recovery_copy() {
        let (dir, paths) = temp_paths("autosave_save");
        std::fs::create_dir_all(&dir).unwrap();
        let saved = dir.join("saved.rzxl");
        let mut s = AppState::with_paths(Some(paths));
        let t0 = Instant::now();
        s.autosave_tick(t0);
        dirty(&mut s);
        s.autosave_tick(t0);
        s.autosave_tick(t0 + AUTOSAVE_INTERVAL);
        s.flush_autosave();
        assert_eq!(s.recoverable().len(), 1);
        s.save_active_as(&saved).unwrap();
        assert!(s.recoverable().is_empty());

        // Discard deletes an entry without opening it.
        dirty(&mut s);
        let t1 = t0 + AUTOSAVE_INTERVAL * 2;
        s.autosave_tick(t1);
        s.autosave_tick(t1 + AUTOSAVE_INTERVAL);
        s.flush_autosave();
        let entries = s.recoverable();
        assert_eq!(entries[0].meta.original.as_deref(), Some(saved.as_path()));
        s.discard_recovery(&entries[0]);
        assert!(s.recoverable().is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn camera_pitch_is_clamped() {
        let mut c = OrbitCamera::default();
        c.orbit(400.0, 200.0);
        assert_eq!(c.pitch, 90.0);
        assert!((c.yaw - 85.0).abs() < 1e-4);
        c.snap(ViewTarget::Face(ViewFace::Left));
        assert_eq!((c.yaw, c.pitch), (270.0, 0.0));
    }

    #[test]
    fn snapping_switches_to_ortho_until_orbiting() {
        let mut c = OrbitCamera::default();
        c.snap_view(ViewTarget::Edge(ViewEdge::FrontTop), true);
        assert!(!c.is_ortho(), "edges keep the projection");
        c.snap_view(ViewTarget::Face(ViewFace::Front), false);
        assert!(!c.is_ortho(), "auto ortho off");
        c.snap_view(ViewTarget::Face(ViewFace::Front), true);
        assert!(c.is_ortho() && c.auto_ortho);
        c.zoom(0.5);
        assert!(c.is_ortho(), "zoom keeps ortho");
        c.orbit(5.0, 0.0);
        assert!(!c.is_ortho(), "orbiting leaves an automatic ortho view");

        // A manual toggle sticks while orbiting.
        c.toggle_projection();
        c.orbit(5.0, 5.0);
        assert!(c.is_ortho());
        c.toggle_projection();
        assert!(!c.is_ortho());
    }

    #[test]
    fn corners_are_isometric() {
        let mut c = OrbitCamera::default();
        c.snap_view(ViewTarget::Corner(ViewCorner::FrontTopRight), true);
        assert!(c.is_ortho());
        // An isometric view sees all three axes at the same angle.
        let d = c.eye_direction();
        assert!(
            (d.x - d.y).abs() < 1e-4 && (d.y - d.z).abs() < 1e-4,
            "{d:?}"
        );
    }

    #[test]
    fn ortho_projection_keeps_the_view_height() {
        let mut c = OrbitCamera::default();
        let target = Vec3::new(0.0, 4.0, 0.0);
        let persp = c.view_proj(target, 1.5);
        c.toggle_projection();
        let ortho = c.view_proj(target, 1.5);
        // A point half a view height above the target sits on the top edge
        // of both projections.
        let up = c.view_height() * 0.5;
        let view_up = {
            let fwd = -c.eye_direction();
            fwd.cross(Vec3::Y).normalize().cross(fwd)
        };
        let p = target + view_up * up;
        assert!((persp.project_point3(p).y - 1.0).abs() < 1e-3);
        assert!((ortho.project_point3(p).y - 1.0).abs() < 1e-3);
        // Ortho depth stays inside 0..1 for the whole model range.
        for d in [-500.0, 0.0, 500.0] {
            let z = ortho.project_point3(target + c.eye_direction() * d).z;
            assert!((0.0..=1.0).contains(&z), "{d}: {z}");
        }
    }

    #[test]
    fn frame_commands_fit_the_box() {
        let mut s = AppState {
            viewport_aspect: 1.0,
            ..AppState::default()
        };
        let bounds = s.doc().unwrap().data.bounds;
        s.frame_work_area();
        let (lo, hi) = world_bounds(bounds);
        assert_eq!(orbit_base(bounds) + s.camera.pan, (lo + hi) * 0.5);
        let whole = s.camera.distance;
        // A single voxel frames much closer, centred on that voxel.
        s.doc_mut()
            .unwrap()
            .set_voxels(0, [(IVec3::new(2, 3, 4), 5)], false);
        s.frame_selection();
        assert!(s.camera.distance < whole * 0.5);
        let target = orbit_base(bounds) + s.camera.pan;
        assert_eq!(target, lo + Vec3::new(2.5, 3.5, 4.5));
        // A selection wins over the visible voxels.
        s.doc_mut()
            .unwrap()
            .set_voxels(0, [(IVec3::new(10, 0, 0), 5)], false);
        s.doc_mut()
            .unwrap()
            .select([IVec3::new(10, 0, 0)], rizxel_engine::MaskMode::Set);
        s.frame_selection();
        assert_eq!(
            orbit_base(bounds) + s.camera.pan,
            lo + Vec3::new(10.5, 0.5, 0.5)
        );
    }

    #[test]
    fn camera_pan_moves_in_view_plane() {
        // Front view: right = +X, up = +Y.
        let mut c = OrbitCamera::default();
        c.snap(ViewTarget::Face(ViewFace::Front));
        c.pan_screen(0.5, 0.0, 90f32.to_radians());
        // Dragging right moves the target left by half the viewport span.
        assert!((c.pan.x + c.distance).abs() < 1e-3, "{:?}", c.pan);
        assert!(c.pan.y.abs() < 1e-4 && c.pan.z.abs() < 1e-4);

        c.reset();
        c.snap(ViewTarget::Face(ViewFace::Front));
        c.pan_screen(0.0, 0.5, 90f32.to_radians());
        assert!((c.pan.y - c.distance).abs() < 1e-3, "{:?}", c.pan);
    }
}

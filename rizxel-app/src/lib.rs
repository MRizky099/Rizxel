//! # rizxel-app (L3)
//!
//! Headless application state. The UI layer reads this state and mutates it
//! only through the methods below, so all behavior is testable without a window.

pub mod document;
pub mod history;
pub mod interchange;
pub mod selection;

use std::path::Path;

use glam::camera::rh::{proj::directx::perspective, view::look_at_mat4};
use glam::{IVec3, Mat4, Vec3};
use rizxel_engine::{RayHit, Tool, ToolOptions, VoxelOp};
use rizxel_io::RzxlError;

pub use document::{DocData, Document, EditBlock, Layer, LayerProps, SliceOutcome};
pub use history::{ClippedVoxel, Edit, History, VoxelChange};
pub use interchange::FileFormat;
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

/// Orbit camera parameters (degrees). Driven by the placeholder viewport in
/// Phase 2 and reused by the wgpu viewport in Phase 3.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OrbitCamera {
    /// Rotation around the vertical axis.
    pub yaw: f32,
    /// Elevation, clamped to `-90..=90`.
    pub pitch: f32,
    pub distance: f32,
    /// Pan offset added to the document's default orbit target (world units).
    pub pan: Vec3,
}

impl OrbitCamera {
    pub const MIN_DISTANCE: f32 = 2.0;
    pub const MAX_DISTANCE: f32 = 1000.0;
    /// Vertical field of view (radians).
    pub const FOV_Y: f32 = 45.0 * std::f32::consts::PI / 180.0;
    pub const NEAR: f32 = 0.1;
    pub const FAR: f32 = 4000.0;

    /// Rotates by a delta (degrees), clamping pitch.
    pub fn orbit(&mut self, d_yaw: f32, d_pitch: f32) {
        self.yaw = (self.yaw + d_yaw).rem_euclid(360.0);
        self.pitch = (self.pitch + d_pitch).clamp(-90.0, 90.0);
    }

    pub fn zoom(&mut self, factor: f32) {
        self.distance = (self.distance * factor).clamp(Self::MIN_DISTANCE, Self::MAX_DISTANCE);
    }

    /// Snaps the camera so its view direction points at `target`.
    /// Faces keep the level horizon; edges tilt 45 deg; corners use a
    /// three-quarter view (yaw/pitch diagonals).
    pub fn snap(&mut self, target: ViewTarget) {
        let (yaw, pitch) = target.angles();
        self.yaw = yaw;
        self.pitch = pitch;
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

    /// Eye position orbiting `target`.
    pub fn eye(&self, target: Vec3) -> Vec3 {
        target + self.eye_direction() * self.distance
    }

    /// World -> view matrix (right-handed, +Y up).
    pub fn view_matrix(&self, target: Vec3) -> Mat4 {
        look_at_mat4(self.eye(target), target, Vec3::Y)
    }

    /// View -> clip matrix using the wgpu/DirectX convention (depth in `[0, 1]`).
    pub fn projection_matrix(aspect: f32) -> Mat4 {
        perspective(Self::FOV_Y, aspect.max(0.01), Self::NEAR, Self::FAR)
    }

    /// Combined world -> clip matrix for a viewport of the given aspect ratio.
    pub fn view_proj(&self, target: Vec3, aspect: f32) -> Mat4 {
        Self::projection_matrix(aspect) * self.view_matrix(target)
    }

    /// Restores the default orientation, zoom and pan.
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
        }
    }
}

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
            ViewCorner::FrontTopRight => (45.0, 35.0),
            ViewCorner::FrontTopLeft => (315.0, 35.0),
            ViewCorner::FrontBottomRight => (45.0, -35.0),
            ViewCorner::FrontBottomLeft => (315.0, -35.0),
            ViewCorner::BackTopRight => (135.0, 35.0),
            ViewCorner::BackTopLeft => (225.0, 35.0),
            ViewCorner::BackBottomRight => (135.0, -35.0),
            ViewCorner::BackBottomLeft => (225.0, -35.0),
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
    /// Voxel under the cursor (set by the viewport; `None` when off-model).
    pub cursor_voxel: Option<IVec3>,
    /// Transient message shown in the status bar (e.g. save errors).
    pub status_message: Option<String>,
    /// Live feedback of the active tool (hovered slice plane, pending
    /// change count while dragging). Set by the viewport every frame.
    pub tool_feedback: Option<String>,
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
            cursor_voxel: None,
            status_message: None,
            tool_feedback: None,
            untitled_counter: 0,
        };
        state.new_document();
        state
    }
}

impl AppState {
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

    /// Closes a tab. Keeps at least one document open.
    pub fn close_document(&mut self, index: usize) {
        if index >= self.documents.len() {
            return;
        }
        self.documents.remove(index);
        if self.documents.is_empty() {
            self.new_document();
        } else if self.active_doc >= index && self.active_doc > 0 {
            self.active_doc -= 1;
        }
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
        Ok(())
    }

    /// Saves the active document to `path`.
    pub fn save_active_as(&mut self, path: &Path) -> Result<(), RzxlError> {
        match self.doc_mut() {
            Some(doc) => doc.save_to(path),
            None => Ok(()),
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
    fn camera_pitch_is_clamped() {
        let mut c = OrbitCamera::default();
        c.orbit(400.0, 200.0);
        assert_eq!(c.pitch, 90.0);
        assert!((c.yaw - 85.0).abs() < 1e-4);
        c.snap(ViewTarget::Face(ViewFace::Left));
        assert_eq!((c.yaw, c.pitch), (270.0, 0.0));
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

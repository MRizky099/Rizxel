//! # rizxel-app (L3)
//!
//! Headless application state. The UI layer reads this state and mutates it
//! only through the methods below, so all behavior is testable without a window.

pub mod document;
pub mod history;

use std::path::Path;

use glam::{IVec3, Vec3};
use rizxel_engine::{Tool, ToolOptions, VoxelOp};
use rizxel_io::RzxlError;

pub use document::{DocData, Document, Layer};
pub use history::{Edit, History};
/// Re-exported so the UI layer does not need to depend on `rizxel-io` directly.
pub use rizxel_io::RZXL_EXTENSION;

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

    /// Rotates by a delta (degrees), clamping pitch.
    pub fn orbit(&mut self, d_yaw: f32, d_pitch: f32) {
        self.yaw = (self.yaw + d_yaw).rem_euclid(360.0);
        self.pitch = (self.pitch + d_pitch).clamp(-90.0, 90.0);
    }

    pub fn zoom(&mut self, factor: f32) {
        self.distance = (self.distance * factor).clamp(Self::MIN_DISTANCE, Self::MAX_DISTANCE);
    }

    pub fn snap(&mut self, view: ViewFace) {
        let (yaw, pitch) = view.angles();
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

/// Faces of the view cube that snap the camera.
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
    pub selected_voxels: usize,
    /// Transient message shown in the status bar (e.g. save errors).
    pub status_message: Option<String>,
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
            selected_voxels: 0,
            status_message: None,
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
        self.documents.push(doc);
        self.active_doc = self.documents.len() - 1;
    }

    pub fn activate(&mut self, index: usize) {
        if index < self.documents.len() {
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

    // ------------------------------------------------------------ status

    /// Hint text for the status bar.
    pub fn hint(&self) -> String {
        format!("{} Tool: {}", self.tool.label(), self.tool.hint())
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
        assert_eq!(
            s.hint(),
            "Plane Tool: Click and drag to create a flat plane of voxels."
        );
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
        c.snap(ViewFace::Left);
        assert_eq!((c.yaw, c.pitch), (270.0, 0.0));
    }

    #[test]
    fn camera_pan_moves_in_view_plane() {
        // Front view: right = +X, up = +Y.
        let mut c = OrbitCamera::default();
        c.snap(ViewFace::Front);
        c.pan_screen(0.5, 0.0, 90f32.to_radians());
        // Dragging right moves the target left by half the viewport span.
        assert!((c.pan.x + c.distance).abs() < 1e-3, "{:?}", c.pan);
        assert!(c.pan.y.abs() < 1e-4 && c.pan.z.abs() < 1e-4);

        c.reset();
        c.snap(ViewFace::Front);
        c.pan_screen(0.0, 0.5, 90f32.to_radians());
        assert!((c.pan.y - c.distance).abs() < 1e-3, "{:?}", c.pan);
    }
}

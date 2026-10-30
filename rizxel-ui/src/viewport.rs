//! Center 3D workspace.
//!
//! Phase 3: the scene (infinity grid, model bounds, hovered cell) is rendered
//! natively with wgpu through an `egui_wgpu` paint callback (see
//! [`crate::render`]). The view cube and XYZ axis gizmo remain egui overlays.
//! Picking runs on the CPU with the same matrices as the GPU.
//!
//! Phase 4: the visible layers are greedy-meshed once per document revision
//! ([`MeshCache`]) and drawn as opaque voxels.
//!
//! Phase 5a: voxel picking and the editing tools run in
//! [`crate::tool_input`]; their hover highlights and ghosts are drawn with
//! the same fill/line paths as the rest of the overlays.
//!
//! Slice Mode: the side between the slice and the camera is hidden every
//! frame ([`rizxel_engine::WorkPlane::face_camera`]), and a slice slider runs
//! along the right edge (Qubicle's Slice Slider).
//!
//! Controls: left drag = active tool, wheel = zoom, right drag = orbit,
//! middle drag = pan, middle double-click = reset camera.

use std::collections::HashMap;
use std::sync::Arc;

use eframe::egui_wgpu;
use egui::{Align2, Color32, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2};
use glam::{IVec3, Mat3, Mat4, Vec3};
use rizxel_app::{
    AppState, Document, OrbitCamera, ViewCorner, ViewEdge, ViewFace, ViewMode, ViewTarget,
};
use rizxel_math::{BoundSize, ImageData, VoxelBox, VoxelShape};

use crate::render::pathtrace::{document_params, PtInputs, PtScene, PtTables, SkyCache};
use crate::render::{
    GpuMesh, LineInstance, PtFrame, RefPixels, ReferenceDraw, RenderLook, SceneFrame,
    ViewportCallback,
};
use crate::theme;
use crate::tool_input::{self, ToolInput};

/// Viewport meshes of one document in world space.
#[derive(Clone, Default)]
pub struct DocMeshes {
    /// Every fully opaque layer.
    pub opaque: Arc<GpuMesh>,
    /// Translucent (or ghosted) layers in layer order.
    pub transparent: Vec<LayerMesh>,
}

/// One translucent layer.
#[derive(Clone)]
pub struct LayerMesh {
    pub mesh: Arc<GpuMesh>,
    pub alpha: f32,
    /// World-space center of the layer's voxels (depth sorting).
    pub center: Vec3,
}

/// What a document's meshes were built for: revision, slice clip, ghosts
/// and (with ghosts) the active layer.
type BuildKey = (u64, Option<VoxelBox>, bool, Option<usize>);

/// A document's last build, with the source mesh behind each GPU mesh.
struct CacheEntry {
    key: BuildKey,
    /// [`Document::view_generation`] the meshes were taken at.
    generation: u64,
    meshes: DocMeshes,
    sources: Vec<(Arc<rizxel_app::Mesh>, Arc<GpuMesh>)>,
}

/// Viewport meshes per open document, rebuilt only when the document's
/// revision changes (edits, undo/redo, load, visibility, opacity, palette,
/// bounds), its slice clip moves or ghosts change. Switching tabs therefore
/// never re-meshes, and parts the document did not re-mesh (e.g. after an
/// opacity change) keep their GPU mesh. Large updates mesh in the
/// background ([`Document::view_meshes_background`]); every call takes in
/// the tiles finished since the last one.
#[derive(Default)]
pub struct MeshCache {
    entries: HashMap<u64, CacheEntry>,
    /// Source of unique [`GpuMesh::key`]s. The renderer re-uploads only
    /// when the key changes, so every new mesh needs a fresh one: a moved
    /// slice plane changes the mesh without changing the revision.
    next_build: u64,
    /// Decoded reference images by [`ImageData::key`]. Holding the
    /// `ImageData` keeps its bytes alive, so a key is never reused for
    /// another image while it is cached.
    images: HashMap<usize, (ImageData, Arc<RefPixels>)>,
    /// Render mode's path-traced preview.
    pub preview: PtPreview,
    /// Render mode's raster shape mesh: built from these mesh keys and
    /// shape options.
    shaped: Option<(ShapedKey, Option<Arc<GpuMesh>>)>,
}

impl MeshCache {
    /// Meshes of `doc`'s visible (slice-clipped) layers in world space.
    pub fn get(&mut self, doc: &Document, ghosts: bool) -> DocMeshes {
        let key = (
            doc.revision(),
            doc.view_clip(),
            ghosts,
            ghosts.then_some(doc.active_layer),
        );
        // Same build: only background tiles that finished can change it,
        // or another caller meshing the document (it may have redone tiles
        // the pool was still working on).
        let view = match self.entries.get(&doc.id()) {
            Some(entry) if entry.key == key => match doc.poll_view_meshes() {
                Some(view) => view,
                None if entry.generation == doc.view_generation() => return entry.meshes.clone(),
                None => doc.view_meshes_background(ghosts),
            },
            _ => doc.view_meshes_background(ghosts),
        };
        let generation = doc.view_generation();
        let old = self.entries.remove(&doc.id());
        let (bmin, _) = bounds_box(doc.data.bounds);
        let mut sources = Vec::new();
        let mut gpu = |mesh: &Arc<rizxel_app::Mesh>| {
            let reused = old.as_ref().and_then(|e| {
                e.sources
                    .iter()
                    .find(|(m, _)| Arc::ptr_eq(m, mesh))
                    .map(|(_, g)| g.clone())
            });
            let g = reused.unwrap_or_else(|| {
                self.next_build += 1;
                Arc::new(GpuMesh::from_mesh((doc.id(), self.next_build), mesh, bmin))
            });
            sources.push((mesh.clone(), g.clone()));
            g
        };
        let meshes = DocMeshes {
            opaque: gpu(&view.opaque),
            transparent: view
                .transparent
                .iter()
                .map(|t| LayerMesh {
                    mesh: gpu(&t.mesh),
                    alpha: t.alpha,
                    center: t.center + bmin,
                })
                .collect(),
        };
        log::debug!(
            "meshed {} rev {}: {} opaque triangles, {} translucent layers",
            doc.title(),
            doc.revision(),
            meshes.opaque.triangle_count(),
            meshes.transparent.len()
        );
        self.entries.insert(
            doc.id(),
            CacheEntry {
                key,
                generation,
                meshes: meshes.clone(),
                sources,
            },
        );
        meshes
    }

    /// Drops meshes of documents that are no longer open, and decoded
    /// images no open document uses.
    pub fn retain_open(&mut self, documents: &[Document]) {
        self.entries
            .retain(|id, _| documents.iter().any(|d| d.id() == *id));
        self.images.retain(|key, _| {
            documents
                .iter()
                .any(|d| d.data.references.iter().any(|r| r.image.key() == *key))
        });
    }

    /// Decoded pixels of a reference image (decoded once per image).
    fn pixels(&mut self, image: &ImageData) -> Option<Arc<RefPixels>> {
        if let Some((_, pixels)) = self.images.get(&image.key()) {
            return Some(pixels.clone());
        }
        match rizxel_app::decode_reference(image) {
            Ok(rgba) => {
                let pixels = Arc::new(RefPixels {
                    width: rgba.width,
                    height: rgba.height,
                    pixels: rgba.pixels,
                });
                self.images
                    .insert(image.key(), (image.clone(), pixels.clone()));
                Some(pixels)
            }
            Err(e) => {
                log::warn!("reference image: {e}");
                None
            }
        }
    }

    /// `doc`'s visible reference images in world space, in draw order.
    /// Slice Mode filters apply.
    pub fn references(&mut self, doc: &Document, bmin: Vec3) -> Vec<ReferenceDraw> {
        let slicing = doc.plane.slice;
        doc.data
            .references
            .iter()
            .filter(|r| r.visible && r.opacity > 0.0 && r.slice.shows(slicing))
            .filter_map(|r| {
                Some(ReferenceDraw {
                    key: r.image.key(),
                    pixels: self.pixels(&r.image)?,
                    corners: r.corners().map(|c| c + bmin),
                    opacity: r.opacity,
                    on_top: r.depth == rizxel_math::DepthMode::OnTop,
                })
            })
            .collect()
    }
}

/// Degrees of rotation per dragged pixel.
const ORBIT_SPEED: f32 = 0.4;
/// What the preview's samples were traced for: a change restarts them.
type PreviewKey = (u64, u64, OrbitCamera, [u32; 2], Option<VoxelBox>);

/// Render mode's path-traced preview: the scene on the CPU (rebuilt only
/// when the meshes or the layer-to-material slots change) and the samples
/// accumulated for the current view.
#[derive(Default)]
pub struct PtPreview {
    /// Document id and mesh upload keys the scene was built from.
    scene_key: Option<(u64, Vec<(u64, u64)>)>,
    inputs: Option<PtInputs>,
    sky: SkyCache,
    key: Option<PreviewKey>,
    samples: u32,
    last_camera: Option<OrbitCamera>,
    /// Half resolution until then (the camera moved).
    moving_until: Option<std::time::Instant>,
}

/// How long the preview stays at half resolution after the camera moves.
const MOVING_HALF_RES: std::time::Duration = std::time::Duration::from_millis(250);

impl PtPreview {
    /// The preview frame for `doc` in a viewport of `size_px` pixels:
    /// adds a sample unless paused or done. Updates `view.samples_done`.
    pub fn frame(
        &mut self,
        doc: &Document,
        meshes: &DocMeshes,
        camera: &OrbitCamera,
        size_px: [u32; 2],
        view: &mut rizxel_app::RenderView,
    ) -> PtFrame {
        let now = std::time::Instant::now();
        if self.last_camera.is_some_and(|c| c != *camera) {
            self.moving_until = Some(now + MOVING_HALF_RES);
        }
        self.last_camera = Some(*camera);
        let moving = self.moving_until.is_some_and(|t| now < t);
        let scale = view.scale * if moving { 0.5 } else { 1.0 };
        let size = size_px.map(|v| ((v as f32 * scale).round() as u32).max(1));

        let tables = PtTables::new(doc);
        let mut keys = vec![meshes.opaque.key];
        keys.extend(meshes.transparent.iter().map(|t| t.mesh.key));
        let scene_key = (doc.id(), keys);
        let sky = self.sky.get(&doc.data.render);
        let rebuild = self.scene_key.as_ref() != Some(&scene_key)
            || self
                .inputs
                .as_ref()
                .is_none_or(|i| !i.tables.same_slots(&tables));
        let inputs = match self.inputs.take() {
            Some(mut i) if !rebuild => {
                if *i.tables != tables {
                    i.tables = Arc::new(tables);
                }
                i.sky = sky.clone();
                i
            }
            _ => {
                self.scene_key = Some(scene_key);
                PtInputs {
                    scene: Arc::new(PtScene::from_document(doc, &tables)),
                    tables: Arc::new(tables),
                    sky: sky.clone(),
                }
            }
        };
        self.inputs = Some(inputs.clone());

        let key = (doc.id(), doc.revision(), *camera, size, doc.view_clip());
        if self.key.as_ref() != Some(&key) {
            self.key = Some(key);
            self.samples = 0;
        }
        let target = doc.data.render.sample.samples;
        let count = if view.paused || self.samples >= target {
            0
        } else {
            1
        };
        let first = self.samples;
        self.samples += count;
        // The filter costs about a sample: after the first 16 samples the
        // image is refreshed every 4th one (and at the last).
        let resolve =
            count > 0 && (self.samples <= 16 || self.samples % 4 == 0 || self.samples >= target);
        view.samples_done = self.samples;
        let params = document_params(
            doc,
            camera,
            size,
            false,
            sky.as_ref().map(|(_, i)| i.as_ref()),
        );
        PtFrame {
            inputs,
            params,
            first,
            count,
            resolve,
            nearest: doc.data.render.sample.pixelated,
        }
    }

    /// The preview still adds samples (keep repainting).
    pub fn busy(&self, doc: &Document, view: &rizxel_app::RenderView) -> bool {
        !view.paused && self.samples < doc.data.render.sample.samples
            || self
                .moving_until
                .is_some_and(|t| std::time::Instant::now() < t)
    }
}

/// Zoom exponent per scrolled point.
const ZOOM_SPEED: f32 = 0.002;
/// Width of the yellow bounds wireframe, in logical points.
const BOUNDS_WIDTH: f32 = 1.5;
/// Width of the bounds wireframe while slicing, in logical points.
const SLICE_BOUNDS_WIDTH: f32 = 4.0;

/// Camera matrices for the current frame.
pub struct Projection {
    view_proj: Mat4,
    rect: Rect,
    eye: Vec3,
    /// Unit view direction.
    forward: Vec3,
    /// World units covered by the viewport height in orthographic mode;
    /// `None` in perspective.
    ortho_height: Option<f32>,
    /// Vertical field of view of the perspective camera (radians).
    fov_y: f32,
}

impl Projection {
    pub fn new(camera: &OrbitCamera, target: Vec3, rect: Rect) -> Self {
        Self::with_fov(camera, target, rect, OrbitCamera::FOV_Y)
    }

    /// Camera with a perspective field of view of `fov_y` radians (Render
    /// mode's lens setting).
    pub fn with_fov(camera: &OrbitCamera, target: Vec3, rect: Rect, fov_y: f32) -> Self {
        let aspect = rect.width() / rect.height().max(1.0);
        Self {
            fov_y,
            view_proj: camera.view_proj_fov(target, aspect, fov_y),
            rect,
            eye: camera.eye(target),
            forward: -camera.eye_direction(),
            ortho_height: camera.is_ortho().then(|| camera.view_height()),
        }
    }

    /// Camera position in world space (far back along the line of sight in
    /// orthographic mode).
    pub fn eye(&self) -> Vec3 {
        self.eye
    }

    /// World -> clip matrix.
    pub fn view_proj(&self) -> Mat4 {
        self.view_proj
    }

    /// Unit view direction.
    pub fn forward(&self) -> Vec3 {
        self.forward
    }

    /// Orthographic view height in world units (`None` in perspective).
    pub fn ortho_height(&self) -> Option<f32> {
        self.ortho_height
    }

    /// On-screen size (logical points) of one world unit at `world`.
    pub fn points_per_unit(&self, world: Vec3) -> f32 {
        let span = self.ortho_height.unwrap_or_else(|| {
            let depth = (world - self.eye).dot(self.forward).max(OrbitCamera::NEAR);
            2.0 * depth * (self.fov_y * 0.5).tan()
        });
        self.rect.height() / span
    }

    /// World-space mouse ray `(origin, unit direction)` through `screen`.
    /// Perspective rays start at the eye; orthographic rays are parallel
    /// and start on the near plane.
    pub fn world_ray(&self, screen: Pos2) -> (Vec3, Vec3) {
        let ndc = glam::Vec2::new(
            (screen.x - self.rect.center().x) / (self.rect.width() * 0.5),
            -(screen.y - self.rect.center().y) / (self.rect.height() * 0.5),
        );
        let inv = self.view_proj.inverse();
        if self.ortho_height.is_some() {
            return (inv.project_point3(ndc.extend(0.0)), self.forward);
        }
        let far = inv.project_point3(ndc.extend(1.0));
        (self.eye, (far - self.eye).normalize_or_zero())
    }

    /// Screen position of a world point (`None` behind the camera).
    pub fn to_screen(&self, world: Vec3) -> Option<Pos2> {
        let clip = self.view_proj * world.extend(1.0);
        if clip.w <= 1e-6 {
            return None;
        }
        let ndc = clip.truncate() / clip.w;
        Some(Pos2::new(
            self.rect.center().x + ndc.x * self.rect.width() * 0.5,
            self.rect.center().y - ndc.y * self.rect.height() * 0.5,
        ))
    }

    /// Intersects the mouse ray with the ground plane `y = 0`.
    #[cfg(test)]
    fn ground_hit(&self, screen: Pos2) -> Option<Vec3> {
        let (eye, dir) = self.world_ray(screen);
        if dir.y.abs() < 1e-6 {
            return None;
        }
        let t = -eye.y / dir.y;
        (t > 0.0).then(|| eye + dir * t)
    }
}

/// Field of view of the viewport: the Render settings' lens in Render mode.
pub fn view_fov(state: &AppState) -> f32 {
    match (state.view_mode, state.doc()) {
        (ViewMode::Render, Some(doc)) => doc.data.render.camera.fov.to_radians(),
        _ => OrbitCamera::FOV_Y,
    }
}

/// Camera rotation only (world -> view), used by the gizmos.
fn view_rotation(camera: &OrbitCamera) -> Mat3 {
    let unit = OrbitCamera {
        distance: 1.0,
        ..*camera
    };
    Mat3::from_mat4(unit.view_matrix(Vec3::ZERO))
}

/// `Alt` is held with the Alt + mouse camera controls and no tool drag is
/// in progress: the mouse moves the camera instead of the tools.
fn alt_navigation(ui: &Ui, state: &AppState, tools: &ToolInput) -> bool {
    state.settings.viewport.camera_controls == rizxel_app::CameraControls::AltMouse
        && ui.input(|i| i.modifiers.alt)
        && !tools.is_dragging()
}

/// World-space min/max of the model bounds, centered on the origin in XZ.
fn bounds_box(bounds: BoundSize) -> (Vec3, Vec3) {
    rizxel_app::world_bounds(bounds)
}

pub fn show(
    ui: &mut Ui,
    state: &mut AppState,
    target_format: Option<egui_wgpu::wgpu::TextureFormat>,
    meshes: &mut MeshCache,
    tools: &mut ToolInput,
) {
    let rect = ui.max_rect();
    let response = ui.allocate_rect(rect, Sense::click_and_drag());
    let painter = ui.painter_at(rect);

    let bounds = state.doc().map(|d| d.data.bounds).unwrap_or_default();
    let (bmin, bmax) = bounds_box(bounds);
    meshes.retain_open(&state.documents);

    // --- Camera input ------------------------------------------------------
    // Right drag: orbit. Middle drag: pan. Wheel: zoom. Left is reserved for
    // tools, except with the Alt + mouse controls (*Settings ▸ Viewport*):
    // Alt + left orbits, Alt + right zooms, and the tools get no input.
    let alt_nav = alt_navigation(ui, state, tools);
    let alt_orbit = alt_nav && response.dragged_by(egui::PointerButton::Primary);
    if response.dragged_by(egui::PointerButton::Secondary) && !alt_nav || alt_orbit {
        let d = response.drag_delta();
        state.camera.orbit(-d.x * ORBIT_SPEED, d.y * ORBIT_SPEED);
    }
    if alt_nav && response.dragged_by(egui::PointerButton::Secondary) {
        let d = response.drag_delta();
        state.camera.zoom((d.y * 4.0 * ZOOM_SPEED).exp());
    }
    if response.dragged_by(egui::PointerButton::Middle) {
        let d = response.drag_delta() / rect.height().max(1.0);
        state.camera.pan_screen(d.x, d.y, OrbitCamera::FOV_Y);
    }
    if response.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta().y);
        if scroll != 0.0 {
            state.camera.zoom((-scroll * ZOOM_SPEED).exp());
        }
    }
    if response.double_clicked_by(egui::PointerButton::Middle) {
        state.camera.reset();
    }
    state.viewport_aspect = rect.width() / rect.height().max(1.0);
    let target = rizxel_app::orbit_base(bounds) + state.camera.pan;
    let proj = Projection::with_fov(&state.camera, target, rect, view_fov(state));

    // --- Slice Mode: hide the camera's side, slider on the right edge -------
    if let Some(doc) = state.doc_mut() {
        doc.plane.face_camera(proj.eye() - bmin, bounds);
    }
    let slider = slice_slider(ui, state, rect);

    let ghosts = state.view.ghosts;
    let voxels = state.doc().map(|d| meshes.get(d, ghosts));
    // Box around the visible voxels, in world space, when it is smaller than
    // the bounds (shows how much of the editable volume the model uses).
    let occupied = state
        .doc()
        .and_then(Document::occupied_bounds)
        .filter(|&(lo, hi)| lo != IVec3::ZERO || hi != bounds.0.as_ivec3())
        .map(|(lo, hi)| (bmin + lo.as_vec3(), bmin + hi.as_vec3()));

    // --- Tools (picking, drag state machine, previews) ----------------------
    let cube_rect = Rect::from_min_size(
        Pos2::new(rect.right() - 100.0, rect.top() + 16.0),
        Vec2::splat(84.0),
    );
    let projection_rect = projection_button_rect(cube_rect);
    let toggles_rect = view_toggles_rect(rect);
    let ppp = ui.ctx().pixels_per_point();
    let blocked: Vec<Rect> = [cube_rect, projection_rect, toggles_rect]
        .into_iter()
        .chain(slider.map(|(r, _)| r))
        .collect();
    let render_mode = state.view_mode == ViewMode::Render;
    let preview = if render_mode {
        render_pick(ui, &response, &proj, bmin, &blocked, state);
        tool_input::Preview::default()
    } else if alt_nav {
        tool_input::Preview::default()
    } else {
        tools.update(
            &tool_input::Frame {
                ui,
                response: &response,
                proj: &proj,
                bmin,
                blocked: &blocked,
                pixels_per_point: ppp,
            },
            state,
        )
    };
    // Tool edits may have changed the document this frame.
    let voxels = state.doc().map(|d| meshes.get(d, ghosts)).or(voxels);
    // Keep frames coming while background tiles arrive.
    let meshing = state.doc().is_some_and(Document::view_meshing);
    if meshing {
        ui.ctx().request_repaint();
    }

    // --- Scene (wgpu) -------------------------------------------------------
    let mut frame = scene_frame(state, &proj, target, bmin, bmax, ppp);
    frame.fills.extend(preview.fills);
    frame.lines.extend(preview.lines);
    if let Some((lo, hi)) = occupied.filter(|_| state.view.model_bounds) {
        let color = theme::BOUND.gamma_multiply(0.45);
        frame.lines.extend(
            bounds_edges(lo, hi)
                .into_iter()
                .map(|(a, b)| LineInstance::new(a, b, color, ppp)),
        );
    }
    if let Some(meshes) = &voxels {
        frame.translucent = sorted_back_to_front(&proj, &meshes.transparent);
        frame.voxels = Some(meshes.opaque.clone());
    }
    // Render mode's raster view draws the voxel shape (view only).
    let raster_view = !(state.render_view.path_traced && state.render_view.path_tracing);
    if let (true, Some(doc), Some(m)) = (
        render_mode && raster_view,
        state.documents.get(state.active_doc),
        &voxels,
    ) {
        if doc.data.render.display.shape != VoxelShape::Cube || doc.data.render.display.cell < 1.0 {
            match meshes.shaped(doc, m, bmin) {
                Some(shaped) => {
                    frame.voxels = Some(shaped);
                    frame.translucent.clear();
                }
                None => state.tool_feedback = Some(SHAPE_FALLBACK.to_owned()),
            }
        }
    }
    if let Some(doc) = state.doc().filter(|_| !render_mode) {
        frame.references = meshes.references(doc, bmin);
    }
    let size_px = [
        (rect.width() * ppp).round().max(1.0) as u32,
        (rect.height() * ppp).round().max(1.0) as u32,
    ];
    // Render mode: the path-traced preview replaces the raster scene.
    let traced = render_mode
        && state.render_view.path_traced
        && state.render_view.path_tracing
        && target_format.is_some();
    if traced {
        if let (Some(doc), Some(m)) = (state.documents.get(state.active_doc), &voxels) {
            let mut view = state.render_view;
            frame.pt = Some(
                meshes
                    .preview
                    .frame(doc, m, &state.camera, size_px, &mut view),
            );
            if meshes.preview.busy(doc, &view) {
                ui.ctx().request_repaint();
            }
            state.render_view = view;
        }
    }
    match target_format {
        Some(format) => {
            painter.add(egui_wgpu::Callback::new_paint_callback(
                rect,
                ViewportCallback {
                    frame,
                    target_format: format,
                    size_px,
                },
            ));
        }
        // No wgpu device (should not happen with the wgpu renderer).
        None => {
            painter.rect_filled(rect, 0.0, theme::WORKSPACE);
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                "wgpu renderer unavailable",
                theme::regular(13.0),
                theme::TEXT,
            );
        }
    }

    // --- Overlays -----------------------------------------------------------
    if let Some((track, active)) = slider {
        paint_slice_slider(&painter, state, track, active);
    }
    if let Some(target) = view_cube(ui, &state.camera, cube_rect) {
        state.snap_view(target);
    }
    if projection_button(ui, &state.camera, projection_rect) {
        state.camera.toggle_projection();
    }
    view_toggles(ui, &mut state.view, toggles_rect);

    if meshing {
        painter.text(
            Pos2::new(rect.left() + 12.0, rect.bottom() - 12.0),
            Align2::LEFT_BOTTOM,
            "Meshing...",
            theme::regular(12.0),
            theme::TEXT_DIM,
        );
    }

    let axis_center = Pos2::new(rect.right() - 56.0, rect.bottom() - 56.0);
    paint_axis_gizmo(&painter, &state.camera, axis_center);
}

/// Voxels above which the raster view keeps cubes for the voxel shape (the
/// path tracer draws any number).
pub const MAX_SHAPED_VOXELS: usize = 1_000_000;
const SHAPE_FALLBACK: &str = "Shape preview: too many voxels, the raster view shows cubes";

/// What a raster shape mesh was built from: document, mesh keys, options.
type ShapedKey = (u64, Vec<(u64, u64)>, rizxel_app::ShapeMeshOptions);

fn shape_options(
    doc: &Document,
    quality: rizxel_app::ShapeQuality,
) -> rizxel_app::ShapeMeshOptions {
    let d = &doc.data.render.display;
    rizxel_app::ShapeMeshOptions {
        shape: d.shape,
        cell: d.cell,
        bevel: d.bevel,
        quality,
        studs: true,
    }
}

/// The *Image* output's shape mesh (`None` for full-size cubes or too many
/// voxels).
fn shaped_output(doc: &Document, bmin: Vec3) -> Option<Arc<GpuMesh>> {
    let d = &doc.data.render.display;
    if (d.shape == VoxelShape::Cube && d.cell >= 1.0)
        || doc.visible_voxel_count() > MAX_SHAPED_VOXELS
    {
        return None;
    }
    let mesh = doc.shaped_mesh(&shape_options(doc, rizxel_app::ShapeQuality::Medium));
    let key = OUTPUT_BUILD.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    Some(Arc::new(GpuMesh::from_mesh((doc.id(), key), &mesh, bmin)))
}

impl MeshCache {
    /// Low-quality shape mesh of `doc` for the raster view, rebuilt only
    /// when the meshes or the shape options change. `None` above
    /// [`MAX_SHAPED_VOXELS`].
    fn shaped(&mut self, doc: &Document, meshes: &DocMeshes, bmin: Vec3) -> Option<Arc<GpuMesh>> {
        let opts = shape_options(doc, rizxel_app::ShapeQuality::Low);
        let mut keys = vec![meshes.opaque.key];
        keys.extend(meshes.transparent.iter().map(|t| t.mesh.key));
        let key = (doc.id(), keys, opts);
        if let Some((k, mesh)) = &self.shaped {
            if *k == key {
                return mesh.clone();
            }
        }
        let mesh = (doc.visible_voxel_count() <= MAX_SHAPED_VOXELS).then(|| {
            self.next_build += 1;
            Arc::new(GpuMesh::from_mesh(
                (doc.id(), self.next_build),
                &doc.shaped_mesh(&opts),
                bmin,
            ))
        });
        self.shaped = Some((key, mesh.clone()));
        mesh
    }
}

/// Source of upload keys for output meshes (never shared with the viewport's).
static OUTPUT_BUILD: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1 << 62);

/// Camera matrices of a `size`-pixel output image seen through `camera`,
/// with the Render settings' lens. Same framing as the viewport for the
/// same camera and aspect ratio.
pub fn output_projection(doc: &Document, camera: &OrbitCamera, size: [u32; 2]) -> Projection {
    let target = rizxel_app::orbit_base(doc.data.bounds) + camera.pan;
    let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(size[0] as f32, size[1] as f32));
    Projection::with_fov(
        camera,
        target,
        rect,
        doc.data.render.camera.fov.to_radians(),
    )
}

/// Scene of the *Image* output: `doc`'s Render-mode raster view through
/// `camera` at `size` pixels, without grid lines, bounds or references.
/// The meshes are built here (not taken from the viewport cache), so the
/// image never shows tiles still meshing in the background.
pub fn output_frame(doc: &Document, camera: &OrbitCamera, size: [u32; 2], ao: bool) -> SceneFrame {
    let bounds = doc.data.bounds;
    let (bmin, _) = bounds_box(bounds);
    let proj = output_projection(doc, camera, size);
    let target = rizxel_app::orbit_base(bounds) + camera.pan;
    let (look, background) = RenderLook::from_settings(&doc.data.render);
    let view = doc.output_meshes(false);
    let gpu = |mesh: &rizxel_app::Mesh| {
        let key = OUTPUT_BUILD.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Arc::new(GpuMesh::from_mesh((doc.id(), key), mesh, bmin))
    };
    let transparent: Vec<LayerMesh> = view
        .transparent
        .iter()
        .map(|t| LayerMesh {
            mesh: gpu(&t.mesh),
            alpha: t.alpha,
            center: t.center + bmin,
        })
        .collect();
    SceneFrame {
        view_proj: proj.view_proj,
        eye: proj.eye,
        target,
        forward: proj.forward,
        ortho: camera.is_ortho(),
        distance: camera.distance,
        grid_origin: glam::Vec2::new(bmin.x.fract(), bmin.z.fract()),
        render_mode: true,
        floor_grid: false,
        voxel_grid: false,
        ao,
        background,
        grid_color: theme::GRID_LINE,
        axis_x: theme::AXIS_X,
        axis_z: theme::AXIS_Z,
        lines: Vec::new(),
        fills: Vec::new(),
        voxels: Some(match shaped_output(doc, bmin) {
            Some(mesh) => mesh,
            None => gpu(&view.opaque),
        }),
        translucent: if doc.data.render.display.shape == VoxelShape::Cube {
            sorted_back_to_front(&proj, &transparent)
        } else {
            Vec::new()
        },
        materials: doc.data.materials.clone(),
        references: Vec::new(),
        look,
        screen_y: [1.0, 0.0],
        pt: None,
    }
}

/// Render mode has no editing tools: a click on the model sets the depth
/// of field focus there (when the lens has an aperture) and `Alt`+click
/// makes the clicked voxel's layer active, so its material opens in the
/// Render panel.
fn render_pick(
    ui: &Ui,
    response: &egui::Response,
    proj: &Projection,
    bmin: Vec3,
    blocked: &[Rect],
    state: &mut AppState,
) {
    if !response.clicked_by(egui::PointerButton::Primary) {
        return;
    }
    let Some(pos) = response.interact_pointer_pos() else {
        return;
    };
    if blocked.iter().any(|r| r.contains(pos)) {
        return;
    }
    let (origin, dir) = proj.world_ray(pos);
    let ray = rizxel_engine::Ray::new(origin - bmin, dir);
    let Some(doc) = state.doc_mut() else { return };
    let area = VoxelBox::from_bounds(doc.data.bounds);
    let hit = doc.with_visible_geometry(|vols| {
        rizxel_engine::raycast::raycast_grid(&ray, &area, |p| {
            vols.iter().any(|v| v.get(p) != rizxel_math::EMPTY_VOXEL)
        })
    });
    let Some(hit) = hit else { return };
    if ui.input(|i| i.modifiers.alt) {
        if let Some(layer) = doc.layer_at(hit.voxel) {
            doc.active_layer = layer;
            let name = doc.data.layers[layer].name.clone();
            state.status_message = Some(format!("Layer \"{name}\" is active"));
        }
        return;
    }
    if doc.data.render.camera.aperture > 0.0 {
        // Distance along the view direction, which is what the lens uses.
        let world = origin + dir * hit.t;
        let mut s = doc.data.render.clone();
        s.camera.focus_distance = (world - proj.eye()).dot(proj.forward).max(0.1);
        doc.labeled("Focus", |d| d.set_render(s, false));
    }
}

/// Translucent layers ordered farthest first by the depth of their box
/// center along the view direction. No per-triangle sort: voxel layers
/// rarely interpenetrate.
fn sorted_back_to_front(proj: &Projection, layers: &[LayerMesh]) -> Vec<(Arc<GpuMesh>, f32)> {
    let depth = |l: &LayerMesh| (l.center - proj.eye).dot(proj.forward);
    let mut sorted: Vec<&LayerMesh> = layers.iter().collect();
    sorted.sort_by(|a, b| depth(b).total_cmp(&depth(a)));
    sorted
        .into_iter()
        .map(|l| (l.mesh.clone(), l.alpha))
        .collect()
}

/// Width of the slice slider, in logical points.
const SLIDER_WIDTH: f32 = 12.0;

/// Track of the slice slider inside the viewport `rect`: along the right
/// edge, between the view cube and the axis gizmo.
fn slider_track(rect: Rect) -> Rect {
    let right = rect.right() - 8.0;
    Rect::from_min_max(
        Pos2::new(right - SLIDER_WIDTH, rect.top() + 132.0),
        Pos2::new(right, (rect.bottom() - 104.0).max(rect.top() + 176.0)),
    )
}

/// Slider position `t` (0 at the top, 1 at the bottom) of `layer`. Qubicle's
/// direction: up on the slider is up (Y) or back (X, Z) on screen, so the
/// bottom end is the first Y slice and the top end the first X or Z slice.
fn slider_t(axis: usize, layer: i32, count: i32) -> f32 {
    let count = count.max(1) as f32;
    let from_top = if axis == 1 {
        count - 1.0 - layer as f32
    } else {
        layer as f32
    };
    from_top / count
}

/// Layer under slider position `t` (inverse of [`slider_t`]).
fn slider_layer(axis: usize, t: f32, count: i32) -> i32 {
    let i = ((t * count as f32).floor() as i32).clamp(0, count.max(1) - 1);
    if axis == 1 {
        count - 1 - i
    } else {
        i
    }
}

/// Qubicle's Slice Slider: a thin vertical slider whose thumb is one slice
/// tall, shown only while slicing. Click or drag sets the slice (no undo
/// step). Returns the track it owns and whether it is hovered or dragged;
/// [`paint_slice_slider`] draws it over the scene.
fn slice_slider(ui: &mut Ui, state: &mut AppState, rect: Rect) -> Option<(Rect, bool)> {
    let (axis, _, count) = state.slice_counter()?;
    let track = slider_track(rect);
    // Driven from the raw pointer: the viewport's own drag-sensing response
    // covers the track, and tool presses inside it are blocked.
    let id = ui.id().with("slice_slider");
    let (pressed, down, pos) = ui.input(|i| {
        (
            i.pointer.button_pressed(egui::PointerButton::Primary),
            i.pointer.primary_down(),
            i.pointer.latest_pos(),
        )
    });
    let over = pos.is_some_and(|p| track.expand(2.0).contains(p));
    let mut dragging = ui.ctx().data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    if pressed && over {
        dragging = true;
    }
    if !down {
        dragging = false;
    }
    ui.ctx().data_mut(|d| d.insert_temp(id, dragging));
    if let (true, Some(pos)) = (dragging, pos) {
        let t = (pos.y - track.top()) / track.height().max(1.0);
        let layer = slider_layer(axis, t, count);
        if let Some(doc) = state.doc_mut() {
            let bounds = doc.data.bounds;
            doc.plane.set_slice_layer(layer, bounds);
        }
    }
    let active = over || dragging;
    if active {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
    Some((track, active))
}

/// Draws the slice slider (`track` from [`slice_slider`]).
fn paint_slice_slider(painter: &egui::Painter, state: &AppState, track: Rect, active: bool) {
    let Some((axis, n, count)) = state.slice_counter() else {
        return;
    };
    let layer = n - 1;
    painter.rect_filled(track, 3.0, Color32::from_black_alpha(90));
    let thumb_h = (track.height() / count.max(1) as f32).max(6.0);
    let t = slider_t(axis, layer, count);
    let top = (track.top() + t * track.height()).min(track.bottom() - thumb_h);
    let thumb = Rect::from_min_size(
        Pos2::new(track.left(), top),
        Vec2::new(SLIDER_WIDTH, thumb_h),
    );
    let color = if active {
        theme::TEXT_STRONG
    } else {
        Color32::from_gray(200)
    };
    painter.rect_filled(thumb, 3.0, color);
    painter.rect_stroke(
        thumb,
        3.0,
        Stroke::new(1.0, tool_input::axis_color(axis)),
        egui::StrokeKind::Inside,
    );
    if active {
        painter.text(
            Pos2::new(track.left() - 6.0, thumb.center().y),
            Align2::RIGHT_CENTER,
            format!("{} {} / {count}", ["X", "Y", "Z"][axis], layer + 1),
            theme::semibold(11.0),
            theme::TEXT_STRONG,
        );
    }
}

/// Builds the GPU scene description for this frame. While slicing, the
/// bounds are drawn one voxel thick (Slice panel look).
fn scene_frame(
    state: &AppState,
    proj: &Projection,
    target: Vec3,
    bmin: Vec3,
    bmax: Vec3,
    pixels_per_point: f32,
) -> SceneFrame {
    let camera = &state.camera;
    let view = state.view;
    let (look, background) = match (state.view_mode, state.doc()) {
        (ViewMode::Render, Some(doc)) => {
            let (mut look, bg) = RenderLook::from_settings(&doc.data.render);
            // The viewport never shows the transparency; output does.
            look.transparent = false;
            (look, bg)
        }
        _ => {
            let bg = state.settings.viewport.background;
            let bg = bg.map_or(theme::WORKSPACE, |[r, g, b]| {
                egui::Color32::from_rgb(r, g, b)
            });
            (RenderLook::default(), bg)
        }
    };
    let bold = state.doc().is_some_and(|d| d.plane.slice);
    let bound_color = if bold {
        theme::BOUND
    } else {
        theme::BOUND.gamma_multiply(0.9)
    };
    let width = if bold {
        SLICE_BOUNDS_WIDTH
    } else {
        BOUNDS_WIDTH
    } * pixels_per_point;
    let lines: Vec<LineInstance> = if view.model_bounds {
        bounds_edges(bmin, bmax)
            .into_iter()
            .map(|(a, b)| LineInstance::new(a, b, bound_color, width))
            .collect()
    } else {
        Vec::new()
    };

    SceneFrame {
        view_proj: proj.view_proj,
        eye: proj.eye,
        target,
        forward: proj.forward,
        ortho: camera.is_ortho(),
        distance: camera.distance,
        // Align grid lines with the voxel cells of the (possibly odd-sized) bounds.
        grid_origin: glam::Vec2::new(bmin.x.fract(), bmin.z.fract()),
        render_mode: state.view_mode == ViewMode::Render,
        look,
        screen_y: [1.0, 0.0],
        pt: None,
        floor_grid: view.floor_grid,
        voxel_grid: view.voxel_grid,
        ao: view.ao(state.view_mode),
        background,
        grid_color: theme::GRID_LINE,
        axis_x: theme::AXIS_X,
        axis_z: theme::AXIS_Z,
        lines,
        fills: Vec::new(),
        voxels: None,
        translucent: Vec::new(),
        materials: state
            .doc()
            .map(|d| d.data.materials.clone())
            .unwrap_or_default(),
        references: Vec::new(),
    }
}

/// The 12 edges of the model bounds box.
fn bounds_edges(min: Vec3, max: Vec3) -> [(Vec3, Vec3); 12] {
    let corner = |i: u8| {
        Vec3::new(
            if i & 1 == 0 { min.x } else { max.x },
            if i & 2 == 0 { min.y } else { max.y },
            if i & 4 == 0 { min.z } else { max.z },
        )
    };
    const EDGES: [(u8, u8); 12] = [
        (0, 1),
        (2, 3),
        (4, 5),
        (6, 7), // X
        (0, 2),
        (1, 3),
        (4, 6),
        (5, 7), // Y
        (0, 4),
        (1, 5),
        (2, 6),
        (3, 7), // Z
    ];
    EDGES.map(|(a, b)| (corner(a), corner(b)))
}

/// Corner of the view cube, indexed by bits: 1 = X, 2 = Y, 4 = Z (positive).
const CUBE_CORNERS: [Vec3; 8] = [
    Vec3::new(-1.0, -1.0, -1.0),
    Vec3::new(1.0, -1.0, -1.0),
    Vec3::new(-1.0, 1.0, -1.0),
    Vec3::new(1.0, 1.0, -1.0),
    Vec3::new(-1.0, -1.0, 1.0),
    Vec3::new(1.0, -1.0, 1.0),
    Vec3::new(-1.0, 1.0, 1.0),
    Vec3::new(1.0, 1.0, 1.0),
];

/// `(face, outward normal, corner indices in CCW order, corner-neighbouring faces)`.
const CUBE_FACES: [(ViewFace, Vec3, [u8; 4]); 6] = [
    (ViewFace::Front, Vec3::Z, [4, 5, 7, 6]),
    (ViewFace::Back, Vec3::NEG_Z, [1, 0, 2, 3]),
    (ViewFace::Right, Vec3::X, [5, 1, 3, 7]),
    (ViewFace::Left, Vec3::NEG_X, [0, 4, 6, 2]),
    (ViewFace::Top, Vec3::Y, [6, 7, 3, 2]),
    (ViewFace::Bottom, Vec3::NEG_Y, [0, 1, 5, 4]),
];

/// `(edge, corner indices)`.
const CUBE_EDGES: [(ViewEdge, [u8; 2]); 12] = [
    (ViewEdge::FrontTop, [6, 7]),
    (ViewEdge::FrontBottom, [4, 5]),
    (ViewEdge::FrontRight, [5, 7]),
    (ViewEdge::FrontLeft, [4, 6]),
    (ViewEdge::BackTop, [2, 3]),
    (ViewEdge::BackBottom, [0, 1]),
    (ViewEdge::BackRight, [1, 3]),
    (ViewEdge::BackLeft, [0, 2]),
    (ViewEdge::TopRight, [3, 7]),
    (ViewEdge::TopLeft, [2, 6]),
    (ViewEdge::BottomRight, [1, 5]),
    (ViewEdge::BottomLeft, [0, 4]),
];

/// `(corner, index into [`CUBE_CORNERS`])`.
const CUBE_CORNER_IDS: [(ViewCorner, usize); 8] = [
    (ViewCorner::BackBottomLeft, 0),
    (ViewCorner::BackBottomRight, 1),
    (ViewCorner::BackTopLeft, 2),
    (ViewCorner::BackTopRight, 3),
    (ViewCorner::FrontBottomLeft, 4),
    (ViewCorner::FrontBottomRight, 5),
    (ViewCorner::FrontTopLeft, 6),
    (ViewCorner::FrontTopRight, 7),
];

/// Radius of the hover dot painted on a corner, in logical points.
const CORNER_DOT_RADIUS: f32 = 5.0;

/// Looks up the cube edge joining two corners (order-independent).
fn edge_between(a: u8, b: u8) -> ViewEdge {
    CUBE_EDGES
        .iter()
        .find(|&&(_, idx)| idx == [a, b] || idx == [b, a])
        .map(|&(e, _)| e)
        .expect("corner pair is a cube edge")
}

/// Looks up the view corner id for a cube corner index.
fn corner_id(i: u8) -> ViewCorner {
    CUBE_CORNER_IDS
        .iter()
        .find(|&&(_, j)| j == i as usize)
        .map(|&(c, _)| c)
        .expect("valid corner index")
}

/// Bilinear interpolation inside a quad ordered `[q0, q1, q2, q3]`.
fn quad_at(quad: [Pos2; 4], u: f32, v: f32) -> Pos2 {
    let w = [(1.0 - u) * (1.0 - v), u * (1.0 - v), u * v, (1.0 - u) * v];
    let mut p = Pos2::ZERO;
    for (i, wi) in w.iter().enumerate() {
        p.x += quad[i].x * wi;
        p.y += quad[i].y * wi;
    }
    p
}

/// Inverse bilinear mapping of `p` into quad parameter space. The
/// orthographic projection of a square face is a parallelogram, so the
/// affine solve is exact.
fn quad_uv(p: Pos2, quad: [Pos2; 4]) -> (f32, f32) {
    let [p00, p10, _, p01] = quad;
    let ex = p10 - p00;
    let ey = p01 - p00;
    let det = ex.x * ey.y - ex.y * ey.x;
    if det.abs() < 1e-6 {
        return (0.5, 0.5);
    }
    let d = p - p00;
    let u = (d.x * ey.y - d.y * ey.x) / det;
    let v = (ex.x * d.y - ex.y * d.x) / det;
    (u, v)
}

/// True when `p` is inside the convex quad.
fn point_in_convex(p: Pos2, quad: &[Pos2; 4]) -> bool {
    let mut sign = 0.0f32;
    for i in 0..4 {
        let a = quad[i];
        let b = quad[(i + 1) % 4];
        let cross = (b - a).x * (p - a).y - (b - a).y * (p - a).x;
        if cross.abs() < f32::EPSILON {
            continue;
        }
        if sign == 0.0 {
            sign = cross.signum();
        } else if cross.signum() != sign {
            return false;
        }
    }
    true
}

/// Splits one visible face into a 3x3 grid of clickable cells (Goxel style):
/// the center is the face, side strips are its 4 edges and corner cells are
/// its 4 corners.
fn face_cells(face: ViewFace, idx: [u8; 4], quad: [Pos2; 4]) -> [(ViewTarget, [Pos2; 4]); 9] {
    let mut cells = std::array::from_fn(|_| (ViewTarget::Face(face), [Pos2::ZERO; 4]));
    for j in 0..3usize {
        for i in 0..3usize {
            let (u0, u1) = (i as f32 / 3.0, (i + 1) as f32 / 3.0);
            let (v0, v1) = (j as f32 / 3.0, (j + 1) as f32 / 3.0);
            let cell = [
                quad_at(quad, u0, v0),
                quad_at(quad, u1, v0),
                quad_at(quad, u1, v1),
                quad_at(quad, u0, v1),
            ];
            let target = match (i, j) {
                (1, 1) => ViewTarget::Face(face),
                // Corner cells map to the quad corner they wrap.
                (0, 0) => ViewTarget::Corner(corner_id(idx[0])),
                (2, 0) => ViewTarget::Corner(corner_id(idx[1])),
                (2, 2) => ViewTarget::Corner(corner_id(idx[2])),
                (0, 2) => ViewTarget::Corner(corner_id(idx[3])),
                // Side strips map to the two quad corners along that side.
                (_, 0) => ViewTarget::Edge(edge_between(idx[0], idx[1])),
                (2, _) => ViewTarget::Edge(edge_between(idx[1], idx[2])),
                (_, 2) => ViewTarget::Edge(edge_between(idx[2], idx[3])),
                _ => ViewTarget::Edge(edge_between(idx[3], idx[0])),
            };
            cells[j * 3 + i] = (target, cell);
        }
    }
    cells
}

/// True when `target` lies on the face with quad corner indices `face_idx`
/// (it is the face itself, or one of its edges or corners).
fn target_touches_face(target: Option<ViewTarget>, face: ViewFace, face_idx: [u8; 4]) -> bool {
    match target {
        Some(ViewTarget::Face(f)) => f == face,
        Some(ViewTarget::Edge(e)) => CUBE_EDGES
            .iter()
            .any(|&(ve, idx)| ve == e && idx.iter().all(|&c| face_idx.contains(&c))),
        Some(ViewTarget::Corner(c)) => CUBE_CORNER_IDS
            .iter()
            .any(|&(vc, i)| vc == c && face_idx.iter().any(|&f| f as usize == i)),
        None => false,
    }
}

/// Solid, clickable view cube in the style of Goxel's. Each visible face is
/// divided into a 3x3 grid of hit regions: the center picks the face, a side
/// strip picks one of its edges and a corner cell picks one of its corners,
/// so edge and corner targets have large, reliable hit zones. Hovering
/// highlights every face that meets the target in yellow. Clicking snaps the
/// camera to the target's preset view.
fn view_cube(ui: &mut Ui, camera: &OrbitCamera, rect: Rect) -> Option<ViewTarget> {
    let rot = view_rotation(camera);
    let center = rect.center();
    let scale = rect.width() * 0.3;

    // Orthographic projection of a unit-cube vertex (view space y is up).
    let to_screen = |v: Vec3| {
        let p = rot * v;
        (
            Pos2::new(center.x + p.x * scale, center.y - p.y * scale),
            p.z,
        )
    };
    let verts: [Pos2; 8] = CUBE_CORNERS.map(|c| to_screen(c).0);

    // Visible faces, sorted back to front so painter order matches depth.
    let mut visible: Vec<(ViewFace, Vec3, [u8; 4], f32)> = CUBE_FACES
        .iter()
        .filter_map(|&(face, n, idx)| {
            let depth = (rot * n).z;
            (depth > 1e-3).then_some((face, n, idx, depth))
        })
        .collect();
    visible.sort_by(|a, b| a.3.total_cmp(&b.3));

    let response = ui.interact(rect, ui.id().with("view_cube"), Sense::click());

    // Hit-test from the topmost face down. Inside a face the 3x3 cells do
    // not overlap, so the first matching cell wins.
    let hovered: Option<ViewTarget> = response.hover_pos().and_then(|p| {
        visible.iter().rev().find_map(|&(face, _, idx, _)| {
            let quad = idx.map(|i| verts[i as usize]);
            if !point_in_convex(p, &quad) {
                return None;
            }
            let (u, v) = quad_uv(p, quad);
            let i = (u * 3.0).clamp(0.0, 2.999) as usize;
            let j = (v * 3.0).clamp(0.0, 2.999) as usize;
            Some(face_cells(face, idx, quad)[j * 3 + i].0)
        })
    });
    if hovered.is_some() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    let painter = ui.painter();
    for &(face, n, idx, depth) in &visible {
        let quad = idx.map(|i| verts[i as usize]);
        let highlighted = target_touches_face(hovered, face, idx);
        // Solid, opaque fill; camera-facing faces are brighter.
        let shade = 0.7 + 0.3 * depth;
        let base = Color32::from_rgb(0x52, 0x52, 0x5b);
        let lit = Color32::from_rgb(0x8a, 0x8a, 0x94);
        let fill = if highlighted {
            blend(base, theme::BOUND, 0.85)
        } else {
            blend(base, lit, shade)
        };
        let stroke = if highlighted {
            theme::BOUND
        } else {
            theme::BORDER
        };
        painter.add(Shape::convex_polygon(
            quad.to_vec(),
            fill,
            Stroke::new(1.0, stroke),
        ));
        if depth > 0.35 {
            let label_pos = to_screen(n).0;
            let text = if highlighted {
                theme::TEXT_STRONG
            } else {
                Color32::from_rgb(0xd4, 0xd4, 0xd8)
            };
            painter.text(
                label_pos,
                Align2::CENTER_CENTER,
                face.label(),
                theme::semibold(11.0),
                text.gamma_multiply(depth.clamp(0.5, 1.0)),
            );
        }
    }
    // Hover indicator painted on top of the faces.
    match hovered {
        Some(ViewTarget::Edge(e)) => {
            if let Some(&(_, idx)) = CUBE_EDGES.iter().find(|&&(ve, _)| ve == e) {
                painter.add(Shape::line_segment(
                    [verts[idx[0] as usize], verts[idx[1] as usize]],
                    Stroke::new(4.0, theme::TEXT_STRONG),
                ));
            }
        }
        Some(ViewTarget::Corner(c)) => {
            if let Some(&(_, i)) = CUBE_CORNER_IDS.iter().find(|&&(vc, _)| vc == c) {
                painter.add(Shape::circle_filled(
                    verts[i],
                    CORNER_DOT_RADIUS,
                    theme::TEXT_STRONG,
                ));
            }
        }
        _ => {}
    }

    response.clicked().then_some(hovered).flatten()
}

/// Perspective / orthographic toggle under the view cube.
fn projection_button_rect(cube_rect: Rect) -> Rect {
    Rect::from_center_size(
        Pos2::new(cube_rect.center().x, cube_rect.bottom() + 12.0),
        Vec2::new(64.0, 18.0),
    )
}

/// Draws the projection toggle; true when clicked.
fn projection_button(ui: &mut Ui, camera: &OrbitCamera, rect: Rect) -> bool {
    let response = ui
        .interact(rect, ui.id().with("projection_toggle"), Sense::click())
        .on_hover_text("Toggle perspective / orthographic (Numpad 5)");
    let hovered = response.hovered();
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let painter = ui.painter();
    let fill = if hovered {
        Color32::from_rgb(0x62, 0x62, 0x6b)
    } else {
        Color32::from_rgb(0x52, 0x52, 0x5b)
    };
    painter.rect_filled(rect, 4.0, fill);
    let label = if camera.is_ortho() { "Ortho" } else { "Persp" };
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        theme::semibold(11.0),
        if camera.is_ortho() {
            theme::BOUND
        } else {
            Color32::from_rgb(0xd4, 0xd4, 0xd8)
        },
    );
    response.clicked()
}

/// Size of one view toggle button, in logical points.
const TOGGLE_SIZE: f32 = 26.0;

/// Row of view toggle buttons in the top-left corner of the viewport.
fn view_toggles_rect(rect: Rect) -> Rect {
    Rect::from_min_size(
        rect.left_top() + Vec2::splat(10.0),
        Vec2::new(TOGGLE_SIZE * 3.0 + 4.0, TOGGLE_SIZE),
    )
}

/// Floor grid, voxel grid and model bounds buttons (same as *View ▸ Show*).
fn view_toggles(ui: &mut Ui, view: &mut rizxel_app::ViewOptions, rect: Rect) {
    use egui_phosphor::regular as icon;
    let toggles: [(&mut bool, &str, &str); 3] = [
        (&mut view.floor_grid, icon::GRID_FOUR, "Floor Grid (G)"),
        (&mut view.voxel_grid, icon::HASH, "Voxel Grid (Shift+G)"),
        (
            &mut view.model_bounds,
            icon::BOUNDING_BOX,
            "Model Bounds (Shift+B)",
        ),
    ];
    for (i, (on, glyph, tip)) in toggles.into_iter().enumerate() {
        let r = Rect::from_min_size(
            rect.min + Vec2::new(i as f32 * (TOGGLE_SIZE + 2.0), 0.0),
            Vec2::splat(TOGGLE_SIZE),
        );
        let response = ui
            .interact(r, ui.id().with(("view_toggle", i)), Sense::click())
            .on_hover_text(tip);
        if response.clicked() {
            *on = !*on;
        }
        let fill = match (*on, response.hovered()) {
            (true, _) => Color32::from_rgb(0x62, 0x62, 0x6b),
            (false, true) => Color32::from_rgb(0x4a, 0x4a, 0x52),
            (false, false) => Color32::from_black_alpha(90),
        };
        let painter = ui.painter();
        painter.rect_filled(r, 4.0, fill);
        painter.text(
            r.center(),
            Align2::CENTER_CENTER,
            glyph,
            theme::icon(15.0),
            if *on {
                theme::TEXT_STRONG
            } else {
                theme::TEXT_DIM
            },
        );
    }
}

/// Linear blend from `a` toward `b` by `t` in 0..=1.
fn blend(a: Color32, b: Color32, t: f32) -> Color32 {
    let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t) as u8;
    Color32::from_rgb(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()))
}

/// XYZ axis indicator in the bottom-right corner.
fn paint_axis_gizmo(painter: &egui::Painter, camera: &OrbitCamera, center: Pos2) {
    let rot = view_rotation(camera);
    let len = 30.0;

    let mut axes = [
        ("X", Vec3::X, theme::AXIS_X),
        ("Y", Vec3::Y, theme::AXIS_Y),
        ("Z", Vec3::Z, theme::AXIS_Z),
    ]
    .map(|(label, dir, color)| {
        let p = rot * dir;
        (label, Vec2::new(p.x, -p.y), p.z, color)
    });
    // Draw axes pointing away from the camera first.
    axes.sort_by(|a, b| a.2.total_cmp(&b.2));

    for (label, dir, depth, color) in axes {
        let color = if depth < -0.2 {
            color.gamma_multiply(0.6)
        } else {
            color
        };
        let tip = center + dir * len;
        painter.line_segment([center, tip], Stroke::new(2.0, color));
        painter.text(
            center + dir * (len + 9.0),
            Align2::CENTER_CENTER,
            label,
            theme::semibold(10.0),
            color,
        );
    }
    painter.circle_filled(center, 2.5, Color32::WHITE);
}

#[cfg(test)]
mod tests {
    use super::*;
    use rizxel_app::LayerProps;

    #[test]
    fn front_view_looks_down_negative_z() {
        let mut cam = OrbitCamera::default();
        cam.snap(ViewTarget::Face(ViewFace::Front));
        let rot = view_rotation(&cam);
        // The +Z face normal points straight at the camera.
        assert!(((rot * Vec3::Z).z - 1.0).abs() < 1e-4);
    }

    #[test]
    fn ground_pick_hits_center() {
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));
        let mut cam = OrbitCamera::default();
        cam.snap(ViewTarget::Face(ViewFace::Top));
        let proj = Projection::new(&cam, Vec3::ZERO, rect);
        let hit = proj.ground_hit(rect.center()).unwrap();
        assert!(hit.length() < 0.5, "hit = {hit:?}");
    }

    fn test_rect() -> Rect {
        Rect::from_min_size(Pos2::new(10.0, 20.0), Vec2::new(800.0, 600.0))
    }

    #[test]
    fn screen_round_trip_in_both_projections() {
        let rect = test_rect();
        for ortho in [false, true] {
            let mut cam = OrbitCamera::default();
            if ortho {
                cam.toggle_projection();
            }
            let proj = Projection::new(&cam, Vec3::new(0.0, 5.0, 0.0), rect);
            for world in [Vec3::new(3.0, 2.0, -4.0), Vec3::new(-10.0, 12.0, 7.5)] {
                let screen = proj.to_screen(world).unwrap();
                let (origin, dir) = proj.world_ray(screen);
                // The ray through the point's pixel passes through the point.
                let t = (world - origin).dot(dir);
                let closest = origin + dir * t;
                assert!(
                    (closest - world).length() < 1e-2,
                    "ortho {ortho}: {closest:?} vs {world:?}"
                );
            }
        }
    }

    #[test]
    fn ortho_rays_are_parallel_and_keep_the_framing() {
        let rect = test_rect();
        let mut cam = OrbitCamera::default();
        cam.snap(ViewTarget::Face(ViewFace::Front));
        let target = Vec3::new(0.0, 4.0, 0.0);
        let persp = Projection::new(&cam, target, rect);
        cam.toggle_projection();
        let ortho = Projection::new(&cam, target, rect);
        let (_, a) = ortho.world_ray(rect.left_top());
        let (_, b) = ortho.world_ray(rect.right_bottom());
        assert!((a - b).length() < 1e-5 && (a + Vec3::Z).length() < 1e-5);
        // At the orbit target both projections show the same scale.
        let (p, o) = (persp.points_per_unit(target), ortho.points_per_unit(target));
        assert!((p - o).abs() < 1e-3, "{p} vs {o}");
        // Ortho scale does not depend on depth.
        assert_eq!(o, ortho.points_per_unit(target + Vec3::Z * 50.0));
    }

    #[test]
    fn ortho_and_perspective_pick_the_same_voxel_in_a_face_view() {
        // A voxel column seen from the front: the centre pixel of voxel
        // (5, 3, *) picks the front-most voxel in both projections.
        use rizxel_engine::raycast::{raycast_volume, Ray};
        let mut doc = Document::new("t");
        doc.set_voxels(0, (0..6).map(|z| (IVec3::new(5, 3, z), 9)), false);
        let bounds = doc.data.bounds;
        let (bmin, _) = bounds_box(bounds);
        let rect = test_rect();
        let mut cam = OrbitCamera::default();
        cam.snap(ViewTarget::Face(ViewFace::Front));
        let target = rizxel_app::orbit_base(bounds);
        let mut picks = Vec::new();
        for ortho in [false, true] {
            if ortho {
                cam.toggle_projection();
            }
            let proj = Projection::new(&cam, target, rect);
            let screen = proj.to_screen(bmin + Vec3::new(5.5, 3.5, 6.0)).unwrap();
            let (origin, dir) = proj.world_ray(screen);
            let hit = raycast_volume(&Ray::new(origin - bmin, dir), &doc.data.layers[0].volume);
            picks.push(hit.map(|h| (h.voxel, h.normal)));
        }
        assert_eq!(picks[0], Some((IVec3::new(5, 3, 5), IVec3::Z)));
        assert_eq!(picks[0], picks[1]);
    }

    #[test]
    fn model_bounds_toggle_hides_the_wireframe() {
        let mut state = AppState::default();
        let rect = test_rect();
        let bounds = state.doc().unwrap().data.bounds;
        let (bmin, bmax) = bounds_box(bounds);
        let proj = Projection::new(&state.camera, Vec3::ZERO, rect);
        let frame = scene_frame(&state, &proj, Vec3::ZERO, bmin, bmax, 1.0);
        assert_eq!(frame.lines.len(), 12);
        assert!(frame.floor_grid && !frame.voxel_grid);
        // Also while slicing (the bold slice frame).
        state.doc_mut().unwrap().plane.slice = true;
        state.view.model_bounds = false;
        state.view.floor_grid = false;
        let frame = scene_frame(&state, &proj, Vec3::ZERO, bmin, bmax, 1.0);
        assert!(frame.lines.is_empty() && !frame.floor_grid);
    }

    #[test]
    fn mesh_cache_rebuilds_only_on_new_revision() {
        let mut doc = Document::new("t");
        let mut cache = MeshCache::default();
        let a = cache.get(&doc, false).opaque;
        assert!(a.indices.is_empty());
        assert!(
            Arc::ptr_eq(&a, &cache.get(&doc, false).opaque),
            "same revision is cached"
        );

        doc.set_voxels(0, [(IVec3::ZERO, 5)], false);
        let b = cache.get(&doc, false).opaque;
        assert_eq!(b.triangle_count(), 12);
        // Voxel (0,0,0) sits at the bounds' min corner in world space.
        let (bmin, _) = bounds_box(doc.data.bounds);
        let min_x = b.vertices.iter().map(|v| v.pos[0]).fold(f32::MAX, f32::min);
        assert_eq!(min_x, bmin.x);

        cache.retain_open(&[]);
        assert!(cache.entries.is_empty());
    }

    #[test]
    fn moving_the_slice_plane_gives_a_new_upload_key() {
        // Four voxels along X; slide the plane through them, then untick
        // Slice. Every step that changes the mesh must give it a fresh key
        // (the GPU re-uploads on key change only) and the right extent; the
        // plane at x = 4 shows the whole model, so that mesh keeps its key.
        let mut doc = Document::new("t");
        doc.set_voxels(0, (0..4).map(|x| (IVec3::new(x, 0, 0), 5)), false);
        let mut cache = MeshCache::default();
        let mut keys = vec![cache.get(&doc, false).opaque.key];
        doc.plane.axis = 0;
        doc.plane.slice = true;
        doc.plane.coord = 4;
        assert_eq!(
            cache.get(&doc, false).opaque.key,
            keys[0],
            "same mesh, same key"
        );
        let (bmin, _) = bounds_box(doc.data.bounds);
        for coord in (1..=3).rev() {
            doc.plane.coord = coord;
            let mesh = cache.get(&doc, false).opaque;
            let max_x = mesh
                .vertices
                .iter()
                .map(|v| v.pos[0])
                .fold(f32::MIN, f32::max);
            // The slice layer and the far side below it stay visible.
            let expected = coord as f32;
            assert_eq!(max_x, bmin.x + expected, "plane at x = {coord}");
            keys.push(mesh.key);
        }
        doc.plane.slice = false;
        let full = cache.get(&doc, false).opaque;
        let max_x = full
            .vertices
            .iter()
            .map(|v| v.pos[0])
            .fold(f32::MIN, f32::max);
        assert_eq!(max_x, bmin.x + 4.0, "unticking Slice restores the model");
        keys.push(full.key);
        let unique: std::collections::HashSet<_> = keys.iter().collect();
        assert_eq!(unique.len(), keys.len());
    }

    #[test]
    fn references_are_placed_in_world_space_and_filtered() {
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(4, 2, image::Rgba([9, 8, 7, 255]))
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let image = ImageData::new(png.into_inner(), 4, 2);
        let mut doc = Document::new("t");
        doc.set_bounds(BoundSize::new(8, 4, 6), false);
        doc.add_reference("a", image.clone(), rizxel_math::RefPlane::Xy);
        doc.add_reference("b", image, rizxel_math::RefPlane::Xz);
        doc.update_reference(1, false, |r| {
            r.depth = rizxel_math::DepthMode::OnTop;
            r.slice = rizxel_math::SliceFilter::SliceOnly;
        });
        let (bmin, _) = bounds_box(doc.data.bounds);
        let mut cache = MeshCache::default();
        let draws = cache.references(&doc, bmin);
        assert_eq!(draws.len(), 1, "the slice-only one is hidden");
        // The XY image fills the z = 0 face: top-left at (min.x, 4, min.z).
        assert_eq!(draws[0].corners[0], bmin + Vec3::new(0.0, 4.0, 0.0));
        assert_eq!(draws[0].corners[2], bmin + Vec3::new(8.0, 0.0, 0.0));
        assert_eq!((draws[0].pixels.width, draws[0].pixels.height), (4, 2));
        assert!(!draws[0].on_top);
        // Both share the decoded pixels.
        doc.plane.slice = true;
        let draws = cache.references(&doc, bmin);
        assert_eq!(draws.len(), 2);
        assert!(Arc::ptr_eq(&draws[0].pixels, &draws[1].pixels));
        assert!(draws[1].on_top);
        doc.toggle_all_references();
        assert!(cache.references(&doc, bmin).is_empty());
        // Closing the document frees the decoded image.
        cache.retain_open(&[]);
        assert!(cache.images.is_empty());
    }

    #[test]
    fn opacity_changes_keep_the_uploaded_meshes() {
        // Two layers; making the top one translucent moves it into its own
        // mesh. Further opacity changes only change the alpha.
        let mut doc = Document::new("t");
        doc.set_voxels(0, [(IVec3::ZERO, 5)], false);
        doc.add_layer();
        doc.set_voxels(1, [(IVec3::new(3, 0, 0), 6)], false);
        let mut cache = MeshCache::default();
        let all = cache.get(&doc, false);
        assert_eq!(all.opaque.triangle_count(), 24);
        assert!(all.transparent.is_empty());

        let set_opacity = |doc: &mut Document, opacity: f32| {
            let props = LayerProps {
                opacity,
                ..doc.data.layers[1].props.clone()
            };
            assert!(doc.set_layer_props(1, props, false));
        };
        set_opacity(&mut doc, 0.5);
        let half = cache.get(&doc, false);
        assert_eq!(half.opaque.triangle_count(), 12);
        assert_eq!(half.transparent.len(), 1);
        assert_eq!(half.transparent[0].alpha, 0.5);
        assert_eq!(half.transparent[0].mesh.triangle_count(), 12);

        set_opacity(&mut doc, 0.3);
        let less = cache.get(&doc, false);
        assert_eq!(less.transparent[0].alpha, 0.3);
        assert!(Arc::ptr_eq(&less.opaque, &half.opaque), "no re-mesh");
        assert!(Arc::ptr_eq(
            &less.transparent[0].mesh,
            &half.transparent[0].mesh
        ));

        // Fully transparent layers are not drawn at all.
        set_opacity(&mut doc, 0.0);
        assert!(cache.get(&doc, false).transparent.is_empty());
    }

    #[test]
    fn ghosts_dim_every_layer_but_the_active_one() {
        let mut doc = Document::new("t");
        doc.set_voxels(0, [(IVec3::ZERO, 5)], false);
        doc.add_layer();
        doc.set_voxels(1, [(IVec3::new(3, 0, 0), 6)], false);
        doc.active_layer = 1;
        let mut cache = MeshCache::default();
        let ghosted = cache.get(&doc, true);
        assert_eq!(ghosted.opaque.triangle_count(), 12);
        assert_eq!(ghosted.transparent.len(), 1);
        assert_eq!(ghosted.transparent[0].alpha, rizxel_app::GHOST_ALPHA);
        // Switching the active layer swaps them without an edit.
        doc.active_layer = 0;
        let swapped = cache.get(&doc, true);
        let (bmin, _) = bounds_box(doc.data.bounds);
        assert_eq!(
            swapped.transparent[0].center,
            bmin + Vec3::new(3.5, 0.5, 0.5)
        );
        assert!(cache.get(&doc, false).transparent.is_empty());
    }

    #[test]
    fn translucent_layers_sort_back_to_front() {
        let rect = test_rect();
        let mut cam = OrbitCamera::default();
        cam.snap(ViewTarget::Face(ViewFace::Front));
        let proj = Projection::new(&cam, Vec3::ZERO, rect);
        let layer = |z: f32, key: u64| LayerMesh {
            mesh: Arc::new(GpuMesh {
                key: (0, key),
                ..GpuMesh::default()
            }),
            alpha: 0.5,
            center: Vec3::new(0.0, 0.0, z),
        };
        // The camera looks down -Z: lower z is farther away.
        let sorted = sorted_back_to_front(&proj, &[layer(5.0, 1), layer(-5.0, 2), layer(0.0, 3)]);
        let keys: Vec<u64> = sorted.iter().map(|(m, _)| m.key.1).collect();
        assert_eq!(keys, [2, 3, 1]);
    }

    #[test]
    fn slider_runs_like_qubicle() {
        // Y: bottom = first slice; X and Z: top = first slice.
        assert_eq!(slider_layer(1, 0.99, 16), 0);
        assert_eq!(slider_layer(1, 0.0, 16), 15);
        assert_eq!(slider_layer(2, 0.0, 9), 0);
        assert_eq!(slider_layer(0, 1.0, 9), 8);
        for axis in 0..3 {
            for layer in 0..9 {
                let t = slider_t(axis, layer, 9) + 0.5 / 9.0;
                assert_eq!(slider_layer(axis, t, 9), layer);
            }
        }
    }

    #[test]
    fn point_in_convex_square() {
        let q = [
            Pos2::new(0.0, 0.0),
            Pos2::new(1.0, 0.0),
            Pos2::new(1.0, 1.0),
            Pos2::new(0.0, 1.0),
        ];
        assert!(point_in_convex(Pos2::new(0.5, 0.5), &q));
        assert!(!point_in_convex(Pos2::new(1.5, 0.5), &q));
    }

    /// A 32³ model in all 8 tiles of 16³, more than meshes synchronously.
    fn eight_tile_doc() -> Document {
        let mut doc = Document::new("t");
        doc.set_bounds(BoundSize::new(32, 32, 32), false);
        let cubes = (0..8).flat_map(|t: i32| {
            let o = IVec3::new(t & 1, (t >> 1) & 1, (t >> 2) & 1) * 16 + IVec3::splat(4);
            (0..27).map(move |i| (o + IVec3::new(i % 3, i / 3 % 3, i / 9), 5))
        });
        doc.set_voxels(0, cubes, false);
        doc
    }

    #[test]
    fn output_frame_does_not_drop_background_viewport_tiles() {
        // A freshly opened model meshes in the background; a thumbnail or
        // Image output rendered meanwhile must not leave the viewport empty.
        let doc = eight_tile_doc();
        let mut cache = MeshCache::default();
        assert!(cache.get(&doc, false).opaque.indices.is_empty());
        assert!(doc.view_meshing(), "the first mesh goes to the pool");
        let frame = output_frame(&doc, &OrbitCamera::default(), [64, 64], false);
        assert!(!frame.voxels.unwrap().indices.is_empty());
        let start = std::time::Instant::now();
        while doc.view_meshing() && start.elapsed() < std::time::Duration::from_secs(10) {
            cache.get(&doc, false);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        // Late tiles from a drained request would have had time to land.
        std::thread::sleep(std::time::Duration::from_millis(300));
        let meshes = cache.get(&doc, false);
        assert!(
            !meshes.opaque.indices.is_empty(),
            "the viewport shows the model once meshing is done"
        );
    }
}

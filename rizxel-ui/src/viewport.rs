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
use rizxel_math::{BoundSize, VoxelBox};

use crate::render::{GpuMesh, LineInstance, SceneFrame, ViewportCallback};
use crate::theme;
use crate::tool_input::{self, ToolInput};

/// Viewport meshes per open document, rebuilt only when the document's
/// revision changes (edits, undo/redo, load, visibility, palette, bounds)
/// or its slice clip moves. Switching tabs therefore never re-meshes.
#[derive(Default)]
pub struct MeshCache {
    /// Per document: `(revision, slice clip, mesh)` the mesh was built for.
    entries: HashMap<u64, (u64, Option<VoxelBox>, Arc<GpuMesh>)>,
    /// Source of unique [`GpuMesh::key`]s. The renderer re-uploads only
    /// when the key changes, so every rebuild needs a fresh one: a moved
    /// slice plane changes the mesh without changing the revision.
    next_build: u64,
}

impl MeshCache {
    /// Mesh of `doc`'s visible (slice-clipped) layers in world space.
    pub fn get(&mut self, doc: &Document) -> Arc<GpuMesh> {
        let revision = doc.revision();
        let clip = doc.view_clip();
        match self.entries.get(&doc.id()) {
            Some((r, c, m)) if *r == revision && *c == clip => m.clone(),
            _ => {
                self.next_build += 1;
                let key = (doc.id(), self.next_build);
                let (bmin, _) = bounds_box(doc.data.bounds);
                let mesh = Arc::new(GpuMesh::from_mesh(key, &doc.view_mesh(), bmin));
                log::debug!(
                    "meshed {} rev {revision}: {} triangles",
                    doc.title(),
                    mesh.triangle_count()
                );
                self.entries
                    .insert(doc.id(), (revision, clip, mesh.clone()));
                mesh
            }
        }
    }

    /// Drops meshes of documents that are no longer open.
    pub fn retain_open(&mut self, documents: &[Document]) {
        self.entries
            .retain(|id, _| documents.iter().any(|d| d.id() == *id));
    }
}

/// Degrees of rotation per dragged pixel.
const ORBIT_SPEED: f32 = 0.4;
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
}

impl Projection {
    fn new(camera: &OrbitCamera, target: Vec3, rect: Rect) -> Self {
        let aspect = rect.width() / rect.height().max(1.0);
        Self {
            view_proj: camera.view_proj(target, aspect),
            rect,
            eye: camera.eye(target),
            forward: -camera.eye_direction(),
        }
    }

    /// Camera position in world space.
    pub fn eye(&self) -> Vec3 {
        self.eye
    }

    /// On-screen size (logical points) of one world unit at `world`.
    pub fn points_per_unit(&self, world: Vec3) -> f32 {
        let depth = (world - self.eye).dot(self.forward).max(OrbitCamera::NEAR);
        self.rect.height() / (2.0 * depth * (OrbitCamera::FOV_Y * 0.5).tan())
    }

    /// World-space mouse ray `(origin, unit direction)` through `screen`.
    pub fn world_ray(&self, screen: Pos2) -> (Vec3, Vec3) {
        let ndc = glam::Vec2::new(
            (screen.x - self.rect.center().x) / (self.rect.width() * 0.5),
            -(screen.y - self.rect.center().y) / (self.rect.height() * 0.5),
        );
        let inv = self.view_proj.inverse();
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

/// Camera rotation only (world -> view), used by the gizmos.
fn view_rotation(camera: &OrbitCamera) -> Mat3 {
    let unit = OrbitCamera {
        distance: 1.0,
        ..*camera
    };
    Mat3::from_mat4(unit.view_matrix(Vec3::ZERO))
}

/// World-space min/max of the model bounds, centered on the origin in XZ.
fn bounds_box(bounds: BoundSize) -> (Vec3, Vec3) {
    let size = bounds.0.as_vec3();
    let half = Vec3::new(size.x * 0.5, 0.0, size.z * 0.5);
    (-half, Vec3::new(half.x, size.y, half.z))
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
    // Right drag: orbit. Middle drag: pan. Wheel: zoom. Left is reserved for tools.
    if response.dragged_by(egui::PointerButton::Secondary) {
        let d = response.drag_delta();
        state.camera.orbit(-d.x * ORBIT_SPEED, d.y * ORBIT_SPEED);
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
    let target = Vec3::new(0.0, bmax.y * 0.4, 0.0) + state.camera.pan;
    let proj = Projection::new(&state.camera, target, rect);

    // --- Slice Mode: hide the camera's side, slider on the right edge -------
    if let Some(doc) = state.doc_mut() {
        doc.plane.face_camera(proj.eye() - bmin, bounds);
    }
    let slider = slice_slider(ui, state, rect);

    let voxels = state.doc().map(|d| meshes.get(d));
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
    let ppp = ui.ctx().pixels_per_point();
    let blocked: Vec<Rect> = std::iter::once(cube_rect)
        .chain(slider.map(|(r, _)| r))
        .collect();
    let preview = tools.update(
        &tool_input::Frame {
            ui,
            response: &response,
            proj: &proj,
            bmin,
            blocked: &blocked,
            pixels_per_point: ppp,
        },
        state,
    );
    // Tool edits may have changed the document this frame.
    let voxels = state.doc().map(|d| meshes.get(d)).or(voxels);

    // --- Scene (wgpu) -------------------------------------------------------
    // While slicing, the bounds are drawn one voxel thick (Slice panel look).
    let bold = state.doc().is_some_and(|d| d.plane.slice);
    let mut frame = scene_frame(&state.camera, &proj, state.view_mode, bmin, bmax, ppp, bold);
    frame.fills.extend(preview.fills);
    frame.lines.extend(preview.lines);
    if let Some((lo, hi)) = occupied {
        let color = theme::BOUND.gamma_multiply(0.45);
        frame.lines.extend(
            bounds_edges(lo, hi)
                .into_iter()
                .map(|(a, b)| LineInstance::new(a, b, color, ppp)),
        );
    }
    frame.voxels = voxels;
    match target_format {
        Some(format) => {
            let size_px = [
                (rect.width() * ppp).round().max(1.0) as u32,
                (rect.height() * ppp).round().max(1.0) as u32,
            ];
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
        state.camera.snap(target);
    }

    let axis_center = Pos2::new(rect.right() - 56.0, rect.bottom() - 56.0);
    paint_axis_gizmo(&painter, &state.camera, axis_center);
}

/// Width of the slice slider, in logical points.
const SLIDER_WIDTH: f32 = 12.0;

/// Track of the slice slider inside the viewport `rect`: along the right
/// edge, between the view cube and the axis gizmo.
fn slider_track(rect: Rect) -> Rect {
    let right = rect.right() - 8.0;
    Rect::from_min_max(
        Pos2::new(right - SLIDER_WIDTH, rect.top() + 116.0),
        Pos2::new(right, (rect.bottom() - 104.0).max(rect.top() + 160.0)),
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

/// Builds the GPU scene description for this frame.
fn scene_frame(
    camera: &OrbitCamera,
    proj: &Projection,
    view_mode: ViewMode,
    bmin: Vec3,
    bmax: Vec3,
    pixels_per_point: f32,
    bold: bool,
) -> SceneFrame {
    let bound_color = if bold {
        theme::BOUND
    } else {
        theme::BOUND.gamma_multiply(0.9)
    };
    let lines: Vec<LineInstance> = bounds_edges(bmin, bmax)
        .into_iter()
        .map(|(a, b)| {
            let width = if bold {
                SLICE_BOUNDS_WIDTH
            } else {
                BOUNDS_WIDTH
            } * pixels_per_point;
            LineInstance::new(a, b, bound_color, width)
        })
        .collect();

    SceneFrame {
        view_proj: proj.view_proj,
        eye: proj.eye,
        distance: camera.distance,
        // Align grid lines with the voxel cells of the (possibly odd-sized) bounds.
        grid_origin: glam::Vec2::new(bmin.x.fract(), bmin.z.fract()),
        render_mode: view_mode == ViewMode::Render,
        background: theme::WORKSPACE,
        grid_color: theme::GRID_LINE,
        axis_x: theme::AXIS_X,
        axis_z: theme::AXIS_Z,
        lines,
        fills: Vec::new(),
        voxels: None,
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

    #[test]
    fn mesh_cache_rebuilds_only_on_new_revision() {
        let mut doc = Document::new("t");
        let mut cache = MeshCache::default();
        let a = cache.get(&doc);
        assert!(a.indices.is_empty());
        assert!(Arc::ptr_eq(&a, &cache.get(&doc)), "same revision is cached");

        doc.set_voxels(0, [(IVec3::ZERO, 5)], false);
        let b = cache.get(&doc);
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
        // Slice. Every step must produce a mesh with a fresh key (the GPU
        // re-uploads on key change only) and the right extent.
        let mut doc = Document::new("t");
        doc.set_voxels(0, (0..4).map(|x| (IVec3::new(x, 0, 0), 5)), false);
        let mut cache = MeshCache::default();
        let mut keys = vec![cache.get(&doc).key];
        doc.plane.axis = 0;
        doc.plane.slice = true;
        let (bmin, _) = bounds_box(doc.data.bounds);
        for coord in (1..=4).rev() {
            doc.plane.coord = coord;
            let mesh = cache.get(&doc);
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
        let full = cache.get(&doc);
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
}

//! Center 3D workspace.
//!
//! Phase 2 placeholder rendered entirely with the egui painter (CPU projection):
//! infinity grid, yellow model bounds, view cube and XYZ axis gizmo. The orbit
//! camera lives in [`AppState::camera`] so the Phase 3 wgpu renderer can take
//! over without touching the UI state.

use egui::{Align2, Color32, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2};
use glam::camera::rh::{proj::directx::perspective, view::look_at_mat4};
use glam::{IVec3, Mat3, Mat4, Vec3, Vec4Swizzles};
use rizxel_app::{AppState, OrbitCamera, ViewFace, ViewMode};
use rizxel_math::BoundSize;

use crate::theme;

/// Half-extent of the drawn ground grid, in voxels.
const GRID_HALF_EXTENT: i32 = 96;
/// Sub-segments per grid line (needed for the radial fade).
const GRID_SEGMENTS: i32 = 24;
const FOV_Y: f32 = 45.0_f32.to_radians();
const NEAR: f32 = 0.1;
const FAR: f32 = 4000.0;
/// Degrees of rotation per dragged pixel.
const ORBIT_SPEED: f32 = 0.4;
/// Zoom exponent per scrolled point.
const ZOOM_SPEED: f32 = 0.002;

/// Camera matrices for the current frame.
struct Projection {
    view_proj: Mat4,
    rect: Rect,
    eye: Vec3,
}

impl Projection {
    fn new(camera: &OrbitCamera, target: Vec3, rect: Rect) -> Self {
        let eye = target + orbit_offset(camera) * camera.distance;
        let view = look_at_mat4(eye, target, Vec3::Y);
        let aspect = (rect.width() / rect.height().max(1.0)).max(0.01);
        // DirectX/wgpu convention: right-handed, depth in [0, 1].
        let proj = perspective(FOV_Y, aspect, NEAR, FAR);
        Self {
            view_proj: proj * view,
            rect,
            eye,
        }
    }

    /// Projects a world point to screen space; `None` if behind the camera.
    fn project(&self, p: Vec3) -> Option<Pos2> {
        let clip = self.view_proj * p.extend(1.0);
        (clip.w > NEAR).then(|| self.ndc_to_screen(clip.xy() / clip.w))
    }

    fn ndc_to_screen(&self, ndc: glam::Vec2) -> Pos2 {
        Pos2::new(
            self.rect.center().x + ndc.x * self.rect.width() * 0.5,
            self.rect.center().y - ndc.y * self.rect.height() * 0.5,
        )
    }

    /// Projects a segment, clipping it against the near plane.
    fn project_segment(&self, a: Vec3, b: Vec3) -> Option<[Pos2; 2]> {
        let ca = self.view_proj * a.extend(1.0);
        let cb = self.view_proj * b.extend(1.0);
        let (ca, cb) = match (ca.w > NEAR, cb.w > NEAR) {
            (true, true) => (ca, cb),
            (false, false) => return None,
            // Interpolate to the point where w == NEAR.
            (true, false) => (ca, ca + (cb - ca) * ((ca.w - NEAR) / (ca.w - cb.w))),
            (false, true) => (cb + (ca - cb) * ((cb.w - NEAR) / (cb.w - ca.w)), cb),
        };
        Some([
            self.ndc_to_screen(ca.xy() / ca.w),
            self.ndc_to_screen(cb.xy() / cb.w),
        ])
    }

    /// Intersects the mouse ray with the ground plane `y = 0`.
    fn ground_hit(&self, screen: Pos2) -> Option<Vec3> {
        let ndc = glam::Vec2::new(
            (screen.x - self.rect.center().x) / (self.rect.width() * 0.5),
            -(screen.y - self.rect.center().y) / (self.rect.height() * 0.5),
        );
        let inv = self.view_proj.inverse();
        let far = inv.project_point3(ndc.extend(1.0));
        let dir = (far - self.eye).normalize_or_zero();
        if dir.y.abs() < 1e-6 {
            return None;
        }
        let t = -self.eye.y / dir.y;
        (t > 0.0).then(|| self.eye + dir * t)
    }
}

/// Unit vector from target to eye for the given yaw/pitch.
fn orbit_offset(camera: &OrbitCamera) -> Vec3 {
    camera.eye_direction()
}

/// Camera rotation only (world -> view), used by the gizmos.
fn view_rotation(camera: &OrbitCamera) -> Mat3 {
    Mat3::from_mat4(look_at_mat4(orbit_offset(camera), Vec3::ZERO, Vec3::Y))
}

/// World-space min/max of the model bounds, centered on the origin in XZ.
fn bounds_box(bounds: BoundSize) -> (Vec3, Vec3) {
    let size = bounds.0.as_vec3();
    let half = Vec3::new(size.x * 0.5, 0.0, size.z * 0.5);
    (-half, Vec3::new(half.x, size.y, half.z))
}

pub fn show(ui: &mut Ui, state: &mut AppState) {
    let rect = ui.max_rect();
    let response = ui.allocate_rect(rect, Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, theme::WORKSPACE);

    let bounds = state.doc().map(|d| d.data.bounds).unwrap_or_default();
    let (bmin, bmax) = bounds_box(bounds);
    // --- Camera input ------------------------------------------------------
    // Right drag: orbit. Middle drag: pan. Wheel: zoom. Left is reserved for tools.
    if response.dragged_by(egui::PointerButton::Secondary) {
        let d = response.drag_delta();
        state.camera.orbit(-d.x * ORBIT_SPEED, d.y * ORBIT_SPEED);
    }
    if response.dragged_by(egui::PointerButton::Middle) {
        let d = response.drag_delta() / rect.height().max(1.0);
        state.camera.pan_screen(d.x, d.y, FOV_Y);
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

    // --- Cursor voxel (ground plane pick) -----------------------------------
    state.cursor_voxel = response
        .hover_pos()
        .and_then(|p| proj.ground_hit(p))
        .and_then(|hit| {
            let v = IVec3::new(
                (hit.x - bmin.x).floor() as i32,
                0,
                (hit.z - bmin.z).floor() as i32,
            );
            let inside = v.x >= 0 && v.z >= 0 && v.x < bounds.x() as i32 && v.z < bounds.z() as i32;
            inside.then_some(v)
        });

    // --- Scene --------------------------------------------------------------
    paint_grid(&painter, &proj);
    paint_bounds(&painter, &proj, bmin, bmax);
    if let Some(v) = state.cursor_voxel {
        paint_hover_cell(
            &painter,
            &proj,
            bmin + Vec3::new(v.x as f32, 0.0, v.z as f32),
        );
    }

    if state.view_mode == ViewMode::Render {
        painter.text(
            rect.center_top() + Vec2::new(0.0, 16.0),
            Align2::CENTER_TOP,
            "Render preview is available from Phase 3",
            theme::regular(12.0),
            theme::TEXT_STRONG.gamma_multiply(0.7),
        );
    }

    // --- Overlays -----------------------------------------------------------
    let cube_rect = Rect::from_min_size(
        Pos2::new(rect.right() - 100.0, rect.top() + 16.0),
        Vec2::splat(84.0),
    );
    if let Some(face) = view_cube(ui, &state.camera, cube_rect) {
        state.camera.snap(face);
    }

    let axis_center = Pos2::new(rect.right() - 56.0, rect.bottom() - 56.0);
    paint_axis_gizmo(&painter, &state.camera, axis_center);
}

/// Ground grid at `y = 0`, fading radially to create the "infinity" look.
fn paint_grid(painter: &egui::Painter, proj: &Projection) {
    let ext = GRID_HALF_EXTENT as f32;
    let step = (2.0 * ext) / GRID_SEGMENTS as f32;

    for i in -GRID_HALF_EXTENT..=GRID_HALF_EXTENT {
        let c = i as f32;
        // Every 8th line is slightly stronger, like a major grid.
        let base_alpha = if i % 8 == 0 { 1.0 } else { 0.55 };

        for s in 0..GRID_SEGMENTS {
            let t0 = -ext + s as f32 * step;
            let t1 = t0 + step;
            let mid = (t0 + t1) * 0.5;
            // Radial fade measured at the segment midpoint.
            let fade = |x: f32, z: f32| {
                let r = (x * x + z * z).sqrt() / ext;
                (1.0 - r).clamp(0.0, 1.0).powf(1.5)
            };

            for (a, b, f) in [
                (Vec3::new(c, 0.0, t0), Vec3::new(c, 0.0, t1), fade(c, mid)),
                (Vec3::new(t0, 0.0, c), Vec3::new(t1, 0.0, c), fade(mid, c)),
            ] {
                if f <= 0.01 {
                    continue;
                }
                if let Some(seg) = proj.project_segment(a, b) {
                    let color = theme::GRID_LINE.gamma_multiply(f * base_alpha);
                    painter.line_segment(seg, Stroke::new(1.0, color));
                }
            }
        }
    }
}

/// Yellow wireframe of the editable model volume.
fn paint_bounds(painter: &egui::Painter, proj: &Projection, min: Vec3, max: Vec3) {
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
    let stroke = Stroke::new(1.5, theme::BOUND.gamma_multiply(0.9));
    for (a, b) in EDGES {
        if let Some(seg) = proj.project_segment(corner(a), corner(b)) {
            painter.line_segment(seg, stroke);
        }
    }
}

/// Highlights the ground cell under the cursor.
fn paint_hover_cell(painter: &egui::Painter, proj: &Projection, origin: Vec3) {
    let pts: Option<Vec<Pos2>> = [Vec3::ZERO, Vec3::X, Vec3::X + Vec3::Z, Vec3::Z]
        .iter()
        .map(|o| proj.project(origin + *o))
        .collect();
    if let Some(pts) = pts {
        painter.add(Shape::convex_polygon(
            pts,
            theme::ACCENT_LIGHT.gamma_multiply(0.35),
            Stroke::new(1.0, theme::ACCENT_LIGHT),
        ));
    }
}

/// Clickable view cube. Returns the face that was clicked, if any.
fn view_cube(ui: &mut Ui, camera: &OrbitCamera, rect: Rect) -> Option<ViewFace> {
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

    // (face, normal, tangent u, tangent v)
    let faces = [
        (ViewFace::Front, Vec3::Z, Vec3::X, Vec3::Y),
        (ViewFace::Back, Vec3::NEG_Z, Vec3::NEG_X, Vec3::Y),
        (ViewFace::Right, Vec3::X, Vec3::NEG_Z, Vec3::Y),
        (ViewFace::Left, Vec3::NEG_X, Vec3::Z, Vec3::Y),
        (ViewFace::Top, Vec3::Y, Vec3::X, Vec3::NEG_Z),
        (ViewFace::Bottom, Vec3::NEG_Y, Vec3::X, Vec3::Z),
    ];

    // Keep only camera-facing faces, sorted back to front.
    let mut visible: Vec<_> = faces
        .iter()
        .filter_map(|&(face, n, u, v)| {
            let depth = (rot * n).z;
            (depth > 1e-3).then(|| {
                let quad = [n - u - v, n + u - v, n + u + v, n - u + v].map(|c| to_screen(c).0);
                (face, quad, depth, to_screen(n).0)
            })
        })
        .collect();
    visible.sort_by(|a, b| a.2.total_cmp(&b.2));

    let response = ui.interact(rect, ui.id().with("view_cube"), Sense::click());
    let pointer = response.hover_pos();
    // Topmost face under the pointer (last in back-to-front order).
    let hovered = pointer.and_then(|p| {
        visible
            .iter()
            .rev()
            .find(|(_, quad, _, _)| point_in_convex(p, quad))
            .map(|(face, ..)| *face)
    });
    if hovered.is_some() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    let painter = ui.painter();
    for (face, quad, depth, label_pos) in &visible {
        let is_hovered = hovered == Some(*face);
        let (fill, stroke, text) = if is_hovered {
            (theme::ACCENT, theme::ACCENT_LIGHT, theme::TEXT_STRONG)
        } else {
            // Faces turned away from the camera are shaded darker.
            let shade = 0.75 + 0.25 * depth;
            (
                Color32::from_rgb(0x3f, 0x3f, 0x46).gamma_multiply(shade),
                Color32::from_rgb(0x52, 0x52, 0x5b),
                Color32::from_rgb(0xd4, 0xd4, 0xd8),
            )
        };
        painter.add(Shape::convex_polygon(
            quad.to_vec(),
            fill,
            Stroke::new(1.0, stroke),
        ));
        if *depth > 0.35 {
            painter.text(
                *label_pos,
                Align2::CENTER_CENTER,
                face.label(),
                theme::semibold(11.0),
                text.gamma_multiply(depth.clamp(0.5, 1.0)),
            );
        }
    }

    response.clicked().then_some(hovered).flatten()
}

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
        cam.snap(ViewFace::Front);
        let rot = view_rotation(&cam);
        // The +Z face normal points straight at the camera.
        assert!(((rot * Vec3::Z).z - 1.0).abs() < 1e-4);
    }

    #[test]
    fn ground_pick_hits_center() {
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));
        let mut cam = OrbitCamera::default();
        cam.snap(ViewFace::Top);
        let proj = Projection::new(&cam, Vec3::ZERO, rect);
        let hit = proj.ground_hit(rect.center()).unwrap();
        assert!(hit.length() < 0.5, "hit = {hit:?}");
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

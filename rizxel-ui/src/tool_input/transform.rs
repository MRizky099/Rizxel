//! Transform tool: a gizmo around the selection (or the active layer)
//! with three rotation rings (X red, Y green, Z blue), six face arrows for
//! one-axis scaling, eight corner cubes for uniform scaling and the box
//! itself for moving. The first change lifts the voxels into the
//! document's transform session; `Enter` (or a tool switch) applies it as
//! one undo step, `Esc` cancels it.
//!
//! Rotation snaps to 90° (`Shift`: 15°, `Ctrl`: 1°). Scaling snaps to
//! whole factors `n` or `1/n` (`Shift`: any whole-voxel size). Corner
//! cubes keep the aspect ratio unless `Alt` is held.

use egui::{Color32, Key, Modifiers, Pos2};
use glam::{IVec3, Vec2, Vec3};
use rizxel_app::{AppState, Document};
use rizxel_engine::raycast::raycast_face;
use rizxel_engine::tools::{box_face_hit, TransformParams, MAX_SCALE, MIN_SCALE};
use rizxel_engine::{Pivot, Tool};
use rizxel_math::VoxelBox;

use super::{arrow, axis_color, box_outline, ghost, quad, Frame, Preview, ToolInput};
use crate::render::LineInstance;
use crate::theme;

/// Ring radius, in logical points.
const RING_RADIUS: f32 = 70.0;
const RING_SEGMENTS: usize = 64;
/// Face arrow length, in logical points.
const ARROW_LENGTH: f32 = 36.0;
/// Corner cube edge, in logical points.
const CORNER_SIZE: f32 = 9.0;
/// Hover distance (logical points) for handles.
const GRAB: f32 = 9.0;
/// Out-of-bounds cells drawn at most.
const MAX_RED_CELLS: usize = 4096;

/// Part of the gizmo under the cursor.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Handle {
    Ring(usize),
    Face {
        axis: usize,
        positive: bool,
    },
    /// Corner `i` (bit 0: +X side, bit 1: +Y, bit 2: +Z).
    Corner(usize),
    Body {
        axis: usize,
        point: Vec3,
    },
}

/// A gizmo drag in progress.
pub(super) struct TransformDrag {
    handle: Handle,
    start: Pos2,
    params: TransformParams,
    /// Box at press time.
    area: VoxelBox,
    /// Size of the untransformed voxels.
    source_size: IVec3,
}

/// The box the gizmo surrounds: the transformed voxels, or what the tool
/// would act on.
fn gizmo_box(doc: &Document) -> Option<VoxelBox> {
    match doc.transform_session() {
        Some(s) => s.result_box().or(Some(VoxelBox {
            min: s.min,
            max: s.min + s.size,
        })),
        None => doc.transform_target_box(),
    }
}

fn center(area: &VoxelBox) -> Vec3 {
    (area.min + area.max).as_vec3() * 0.5
}

fn corner(area: &VoxelBox, i: usize) -> Vec3 {
    let (lo, hi) = (area.min.as_vec3(), area.max.as_vec3());
    Vec3::new(
        if i & 1 == 0 { lo.x } else { hi.x },
        if i & 2 == 0 { lo.y } else { hi.y },
        if i & 4 == 0 { lo.z } else { hi.z },
    )
}

fn face_center(area: &VoxelBox, axis: usize, positive: bool) -> Vec3 {
    let mut c = center(area);
    c[axis] = if positive { area.max } else { area.min }[axis] as f32;
    c
}

fn unit(axis: usize, sign: f32) -> Vec3 {
    let mut d = Vec3::ZERO;
    d[axis] = sign;
    d
}

fn world_per_point(f: &Frame, p: Vec3) -> f32 {
    1.0 / f.proj.points_per_unit(p + f.bmin).max(1e-3)
}

/// The two axes a ring about `axis` spans (right-handed).
fn ring_axes(axis: usize) -> (usize, usize) {
    match axis {
        0 => (1, 2),
        1 => (2, 0),
        _ => (0, 1),
    }
}

fn ring_point(c: Vec3, axis: usize, radius: f32, angle: f32) -> Vec3 {
    let (u, v) = ring_axes(axis);
    c + unit(u, radius * angle.cos()) + unit(v, radius * angle.sin())
}

fn to_vec2(p: Pos2) -> Vec2 {
    Vec2::new(p.x, p.y)
}

fn segment_distance(f: &Frame, cursor: Pos2, a: Vec3, b: Vec3) -> Option<f32> {
    let (sa, sb) = (to_vec2(f.screen(a)?), to_vec2(f.screen(b)?));
    let c = to_vec2(cursor);
    let ab = sb - sa;
    let t = ((c - sa).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
    Some((sa + ab * t).distance(c))
}

/// Gizmo part under `pos`: arrows, corners, rings, then the box.
fn handle_at(f: &Frame, area: &VoxelBox, pos: Pos2) -> Option<Handle> {
    let mut best: Option<(f32, Handle)> = None;
    for axis in 0..3 {
        for positive in [false, true] {
            let base = face_center(area, axis, positive);
            let len = ARROW_LENGTH * world_per_point(f, base);
            let tip = base + unit(axis, if positive { len } else { -len });
            offer(
                &mut best,
                segment_distance(f, pos, base, tip),
                Handle::Face { axis, positive },
            );
        }
    }
    for i in 0..8 {
        let d = f.screen(corner(area, i)).map(|s| s.distance(pos));
        offer(&mut best, d, Handle::Corner(i));
    }
    if let Some(found) = best {
        return Some(found.1);
    }
    let c = center(area);
    let radius = RING_RADIUS * world_per_point(f, c);
    for axis in 0..3 {
        for k in 0..RING_SEGMENTS {
            let a0 = k as f32 / RING_SEGMENTS as f32 * std::f32::consts::TAU;
            let a1 = (k + 1) as f32 / RING_SEGMENTS as f32 * std::f32::consts::TAU;
            let d = segment_distance(
                f,
                pos,
                ring_point(c, axis, radius, a0),
                ring_point(c, axis, radius, a1),
            );
            offer(&mut best, d, Handle::Ring(axis));
        }
    }
    if let Some(found) = best {
        return Some(found.1);
    }
    let ray = f.ray(pos);
    box_face_hit(area, ray.origin, ray.dir).map(|(axis, _, t)| Handle::Body {
        axis,
        point: ray.at(t),
    })
}

/// Keeps the nearest handle within [`GRAB`] points.
fn offer(best: &mut Option<(f32, Handle)>, d: Option<f32>, h: Handle) {
    if let Some(d) = d.filter(|&d| d <= GRAB) {
        if best.is_none_or(|(bd, _)| d < bd) {
            *best = Some((d, h));
        }
    }
}

/// Snaps a scale factor to `n` or `1/n`.
fn snap_scale(s: f32) -> f32 {
    if s >= 1.0 {
        s.round().max(1.0)
    } else {
        1.0 / (1.0 / s.max(MIN_SCALE)).round()
    }
}

/// Scale factor turning `from` voxels into `to` voxels, snapped unless
/// `free`.
fn scale_for(to: f32, from: i32, free: bool) -> f32 {
    let s = (to.round().max(1.0) / from.max(1) as f32).clamp(MIN_SCALE, MAX_SCALE);
    if free {
        s
    } else {
        snap_scale(s)
    }
}

/// Pivot the session uses (from the tool options).
fn pivot_of(state: &AppState) -> (Pivot, IVec3) {
    let o = state.options.transform;
    (o.pivot, o.pivot_cell)
}

/// Applies the open transform; a refusal (voxels outside the bounds)
/// keeps it open unless `or_cancel`.
pub(super) fn apply(state: &mut AppState, or_cancel: bool) {
    let clip = state.options.transform.clip_to_bounds;
    let Some(doc) = state.doc_mut() else { return };
    if doc.transform_session().is_none() {
        return;
    }
    let message = match doc.apply_transform(clip) {
        Ok(n) => format!("Transform: {n} voxels changed"),
        Err(e) if or_cancel => {
            doc.cancel_transform();
            format!("Transform cancelled: {}", e.message())
        }
        Err(_) => {
            let n = doc.transform_session().map_or(0, |s| s.out_of_bounds.len());
            format!(
                "Transform: {n} voxels would leave the bounds. Move them back or turn on Clip to bounds"
            )
        }
    };
    state.status_message = Some(message);
}

/// Opens a session (when none is open) and sets its parameters.
pub(super) fn set_params(state: &mut AppState, params: TransformParams) {
    let (pivot, cell) = pivot_of(state);
    let Some(doc) = state.doc_mut() else { return };
    if doc.transform_session().is_none() {
        if let Err(e) = doc.begin_transform(pivot, cell) {
            state.status_message = Some(format!("Transform: {}", e.message()));
            return;
        }
    }
    doc.set_transform(params, pivot, cell);
}

impl ToolInput {
    /// Applies the open transform when its tool, layer or document is left.
    pub(super) fn sync_transform(&mut self, state: &mut AppState) {
        let open = state
            .doc()
            .and_then(Document::transform_session)
            .map(|s| s.layer);
        let Some(layer) = open else {
            self.transform = None;
            return;
        };
        let active = state.doc().map(|d| d.active_layer);
        if state.tool != Tool::Transform || active != Some(layer) {
            self.transform = None;
            apply(state, true);
        }
    }

    /// `Enter` applies and `Esc` cancels an open transform.
    pub(super) fn transform_keys(&mut self, f: &Frame, state: &mut AppState) {
        if state.doc().is_none_or(|d| d.transform_session().is_none()) {
            return;
        }
        let (enter, esc) = f.ui.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::Enter),
                i.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        if esc {
            self.transform = None;
            if let Some(doc) = state.doc_mut() {
                doc.cancel_transform();
            }
            state.status_message = Some("Transform cancelled".to_owned());
        } else if enter {
            self.transform = None;
            apply(state, false);
        }
    }

    /// Transform tool input for this frame: gizmo hover, presses, drags
    /// and drawing.
    pub(super) fn transform_frame(
        &mut self,
        f: &Frame,
        state: &mut AppState,
        pressed: bool,
        mods: Modifiers,
        out: &mut Preview,
    ) {
        let Some(area) = state.doc().and_then(gizmo_box) else {
            state.tool_feedback =
                Some("Transform: select voxels, or pick a layer that has some".to_owned());
            return;
        };
        let (down, latest) =
            f.ui.input(|i| (i.pointer.primary_down(), i.pointer.latest_pos()));
        let hover = f.response.hover_pos();
        let hovered = match (&self.transform, hover) {
            (None, Some(p)) => handle_at(f, &area, p),
            _ => None,
        };

        if pressed {
            if let (Some(pos), Some(handle)) = (hover, hovered) {
                let (pivot, cell) = pivot_of(state);
                let begun = state.doc_mut().map(|d| d.begin_transform(pivot, cell));
                match begun {
                    Some(Ok(())) => {
                        let s = state.doc().and_then(Document::transform_session);
                        if let Some(s) = s {
                            self.transform = Some(TransformDrag {
                                handle,
                                start: pos,
                                params: s.params,
                                area,
                                source_size: s.size,
                            });
                        }
                    }
                    Some(Err(e)) => {
                        state.status_message = Some(format!("Transform: {}", e.message()));
                    }
                    None => {}
                }
            } else if hover.is_some() && state.options.transform.pivot == Pivot::Cursor {
                // Off the gizmo, with the Cursor pivot: a click picks it.
                if let Some(cell) = state.cursor_voxel {
                    state.options.transform.pivot_cell = cell;
                    let params = state
                        .doc()
                        .and_then(Document::transform_session)
                        .map(|s| s.params);
                    if let Some(params) = params {
                        set_params(state, params);
                    }
                }
            }
        }

        if let (Some(drag), Some(pos)) = (&self.transform, latest) {
            if down {
                if let Some(params) = drag_params(f, drag, pos, mods) {
                    set_params(state, params);
                }
            }
        }
        if !down {
            self.transform = None;
        }

        let grabbed = self.transform.as_ref().map(|d| d.handle);
        if let Some(doc) = state.doc() {
            draw(f, doc, &area, grabbed.or(hovered), out);
            state.tool_feedback = Some(feedback(doc, grabbed.or(hovered)));
        }
        match grabbed.or(hovered) {
            Some(Handle::Body { .. }) => f.ui.ctx().set_cursor_icon(egui::CursorIcon::Move),
            Some(_) => f.ui.ctx().set_cursor_icon(egui::CursorIcon::Grab),
            None => {}
        }
    }
}

/// Parameters for the pointer at `pos` during `drag`.
fn drag_params(
    f: &Frame,
    drag: &TransformDrag,
    pos: Pos2,
    mods: Modifiers,
) -> Option<TransformParams> {
    let mut p = drag.params;
    let delta = to_vec2(pos) - to_vec2(drag.start);
    let area = &drag.area;
    let src = drag.source_size;
    match drag.handle {
        Handle::Ring(axis) => {
            let c = f.screen(center(area))?;
            let angle = |q: Pos2| (q.y - c.y).atan2(q.x - c.x);
            let mut d = (angle(pos) - angle(drag.start)).to_degrees();
            // Screen y points down: counterclockwise on screen is a
            // negative angle there. Seen from -axis the turn reverses.
            let eye = f.proj.eye() - f.bmin;
            let facing = (eye - center(area))[axis] >= 0.0;
            d = if facing { -d } else { d };
            let step = if mods.command {
                1.0
            } else if mods.shift {
                15.0
            } else {
                90.0
            };
            let r = ((drag.params.rotate[axis] + d) / step).round() * step;
            p.rotate[axis] = (r + 180.0).rem_euclid(360.0) - 180.0;
        }
        Handle::Face { axis, positive } => {
            let base = face_center(area, axis, positive);
            let s = f.axis_screen(base)[axis];
            if s.length() <= 0.5 {
                return None;
            }
            let amount = delta.dot(s) / s.length_squared();
            let outward = if positive { amount } else { -amount };
            let size = area.size()[axis] as f32 + outward;
            p.scale[axis] = scale_for(size, src[axis], mods.shift);
        }
        Handle::Corner(i) => {
            let c = to_vec2(f.screen(center(area))?);
            let from = to_vec2(drag.start) - c;
            if !mods.alt {
                let ratio = (to_vec2(pos) - c).length() / from.length().max(1.0);
                for a in 0..3 {
                    let size = area.size()[a] as f32 * ratio;
                    p.scale[a] = scale_for(size, src[a], mods.shift);
                }
            } else {
                // Each axis follows the pointer on its own.
                let k = corner(area, i);
                for a in 0..3 {
                    let s = f.axis_screen(k)[a];
                    if s.length() <= 0.5 {
                        continue;
                    }
                    let amount = delta.dot(s) / s.length_squared();
                    let positive = i & (1 << a) != 0;
                    let outward = if positive { amount } else { -amount };
                    let size = area.size()[a] as f32 + outward;
                    p.scale[a] = scale_for(size, src[a], mods.shift);
                }
            }
        }
        Handle::Body { axis, point } => {
            let shift = if mods.alt {
                let s = f.axis_screen(point)[axis];
                if s.length() <= 0.5 {
                    return None;
                }
                unit(axis, (delta.dot(s) / s.length_squared()).round())
            } else {
                let hit = raycast_face(&f.ray(pos), axis, point[axis])?;
                let mut d = (hit - point).round();
                d[axis] = 0.0;
                d
            };
            p.offset = drag.params.offset + shift.as_ivec3();
        }
    }
    Some(p)
}

/// Status text: the transform and what the hovered handle does.
fn feedback(doc: &Document, handle: Option<Handle>) -> String {
    let what = match handle {
        Some(Handle::Ring(a)) => format!(
            "Drag to rotate about {} (Shift 15°, Ctrl 1°). ",
            ["X", "Y", "Z"][a]
        ),
        Some(Handle::Face { axis, .. }) => format!(
            "Drag to scale along {} (Shift: free). ",
            ["X", "Y", "Z"][axis]
        ),
        Some(Handle::Corner(_)) => "Drag to scale (Alt: per axis, Shift: free). ".to_owned(),
        Some(Handle::Body { .. }) => "Drag to move (Alt: along the face normal). ".to_owned(),
        None => String::new(),
    };
    match doc.transform_session() {
        Some(s) => {
            let p = s.params;
            let mut text = format!(
                "{what}Rotate ({:.0}°, {:.0}°, {:.0}°), scale ({:.0}%, {:.0}%, {:.0}%), move ({}, {}, {}): {} voxels. Enter applies, Esc cancels",
                p.rotate.x,
                p.rotate.y,
                p.rotate.z,
                p.scale.x * 100.0,
                p.scale.y * 100.0,
                p.scale.z * 100.0,
                p.offset.x,
                p.offset.y,
                p.offset.z,
                s.voxel_count()
            );
            if !s.out_of_bounds.is_empty() {
                text.push_str(&format!(
                    ". {} voxels outside the bounds",
                    s.out_of_bounds.len()
                ));
            }
            text
        }
        None => format!("{what}Transform the selection, or the whole layer without one"),
    }
}

/// Box, rings, arrows, corner cubes, the pivot and the cells outside the
/// bounds.
fn draw(f: &Frame, doc: &Document, area: &VoxelBox, active: Option<Handle>, out: &mut Preview) {
    let ppp = f.pixels_per_point;
    let bmin = f.bmin;
    let (lo, hi) = (area.min.as_vec3(), area.max.as_vec3());
    let box_color = theme::ACCENT_LIGHT;
    box_outline(lo + bmin, hi + bmin, box_color, 2.0 * ppp, out);
    if let Some(Handle::Body { axis, point }) = active {
        let positive = (point[axis] - hi[axis]).abs() < (point[axis] - lo[axis]).abs();
        let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
        let at = if positive { hi[axis] } else { lo[axis] };
        let mut q = [lo; 4];
        for (i, c) in q.iter_mut().enumerate() {
            c[axis] = at;
            c[u] = if i == 1 || i == 2 { hi[u] } else { lo[u] };
            c[v] = if i >= 2 { hi[v] } else { lo[v] };
        }
        quad(out, q.map(|c| c + bmin), box_color.gamma_multiply(0.18));
    }
    let highlight = |c: Color32, on: bool| {
        if on {
            c.lerp_to_gamma(Color32::WHITE, 0.45)
        } else {
            c
        }
    };
    // Rings.
    let c = center(area);
    let radius = RING_RADIUS * world_per_point(f, c);
    for axis in 0..3 {
        let color = highlight(axis_color(axis), active == Some(Handle::Ring(axis)));
        let width = if active == Some(Handle::Ring(axis)) {
            3.0
        } else {
            2.0
        };
        for k in 0..RING_SEGMENTS {
            let a0 = k as f32 / RING_SEGMENTS as f32 * std::f32::consts::TAU;
            let a1 = (k + 1) as f32 / RING_SEGMENTS as f32 * std::f32::consts::TAU;
            out.lines.push(LineInstance::new(
                ring_point(c, axis, radius, a0) + bmin,
                ring_point(c, axis, radius, a1) + bmin,
                color,
                width * ppp,
            ));
        }
    }
    // Face arrows.
    for axis in 0..3 {
        for positive in [false, true] {
            let base = face_center(area, axis, positive);
            let scale = world_per_point(f, base);
            let on = active == Some(Handle::Face { axis, positive });
            arrow(
                out,
                base + bmin,
                unit(axis, if positive { 1.0 } else { -1.0 }),
                ARROW_LENGTH * scale,
                scale * 0.6,
                highlight(axis_color(axis), on),
                f.proj.eye(),
            );
        }
    }
    // Corner cubes.
    for i in 0..8 {
        let k = corner(area, i);
        let half = CORNER_SIZE * 0.5 * world_per_point(f, k);
        let on = active == Some(Handle::Corner(i));
        let color = highlight(Color32::from_gray(225), on);
        cube(out, k + bmin, half, color, f.proj.eye());
    }
    // Pivot.
    if let Some(s) = doc.transform_session() {
        let p = s.pivot_point() + bmin;
        let r = 6.0 * world_per_point(f, s.pivot_point());
        for axis in 0..3 {
            out.lines.push(LineInstance::new(
                p - unit(axis, r),
                p + unit(axis, r),
                Color32::WHITE,
                1.5 * ppp,
            ));
        }
        // Voxels outside the bounds, in red.
        if !s.out_of_bounds.is_empty() && s.out_of_bounds.len() <= MAX_RED_CELLS {
            ghost(
                bmin,
                &s.out_of_bounds,
                theme::DANGER.gamma_multiply(0.6),
                out,
            );
        }
    }
}

/// Small shaded cube centered on `c` (camera-facing faces only).
fn cube(out: &mut Preview, c: Vec3, half: f32, color: Color32, eye: Vec3) {
    for axis in 0..3 {
        for sign in [-1.0f32, 1.0] {
            let n = unit(axis, sign);
            let face = c + n * half;
            if n.dot(eye - face) <= 0.0 {
                continue;
            }
            let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
            let q = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
                .map(|(a, b)| face + unit(u, a * half) + unit(v, b * half));
            let shade = [0.8, 1.0, 0.9][axis];
            quad(out, q, color.gamma_multiply(shade));
        }
    }
}

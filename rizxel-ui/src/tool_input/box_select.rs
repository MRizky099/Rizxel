//! Selection tool, *Box* mode: the open box's handles, their drags, the
//! `Enter` / `Esc` keys and the box drawing. The box itself (and its live
//! preview) lives in the document ([`Document::box_select`]).

use egui::{Color32, Key, Modifiers, Pos2};
use glam::{IVec3, Vec2, Vec3};
use rizxel_app::{AppState, Document};
use rizxel_engine::raycast::raycast_face;
use rizxel_engine::tools::{box_face_hit, move_box, resize_face};
use rizxel_engine::{Ray, SelectionMode, Tool, VoxelOp};
use rizxel_math::{BoundSize, VoxelBox, VoxelVolume};

use super::{
    arrow, axis_color, box_outline, mask_mode, quad, Drag, DragKind, Frame, Preview, ToolInput,
};
use crate::theme;

/// Length of the box's face arrows, in logical points.
const BOX_ARROW_LENGTH: f32 = 40.0;
/// The face arrows are thinner than the plane's.
const BOX_ARROW_SCALE: f32 = 0.7;
/// Hover distance (logical points) at which a face arrow can be grabbed.
const BOX_GRAB: f32 = 9.0;

/// Part of the box under the cursor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum BoxGrab {
    /// Arrow of the face on `axis` (`positive` side): drags that face.
    Face { axis: usize, positive: bool },
    /// The box surface at `point` (voxel space) on that face: moves the box
    /// in the face's plane (`Alt`: along its normal).
    Body {
        axis: usize,
        positive: bool,
        point: Vec3,
    },
}

/// Whether the Selection tool is in Box mode.
pub(super) fn box_mode(state: &AppState) -> bool {
    state.tool == Tool::Selection && state.options.selection.mode == SelectionMode::Box
}

/// The box with handles: the open one (`true`), else the last applied one.
fn handle_box(doc: &Document) -> Option<(VoxelBox, bool)> {
    match (doc.box_select(), doc.last_box()) {
        (Some(b), _) => Some((b.area, true)),
        (None, Some(last)) => Some((last.area, false)),
        _ => None,
    }
}

fn face_center(area: &VoxelBox, axis: usize, positive: bool) -> Vec3 {
    let mut c = (area.min + area.max).as_vec3() * 0.5;
    c[axis] = if positive { area.max } else { area.min }[axis] as f32;
    c
}

fn face_dir(axis: usize, positive: bool) -> Vec3 {
    let mut d = Vec3::ZERO;
    d[axis] = if positive { 1.0 } else { -1.0 };
    d
}

/// World units per logical point at the voxel-space point `p`.
fn world_per_point(f: &Frame, p: Vec3) -> f32 {
    1.0 / f.proj.points_per_unit(p + f.bmin).max(1e-3)
}

/// Screen distance from `cursor` to the segment `a`–`b` (voxel space).
fn segment_distance(f: &Frame, cursor: Pos2, a: Vec3, b: Vec3) -> Option<f32> {
    let (sa, sb) = (f.screen(a)?, f.screen(b)?);
    let (sa, sb) = (Vec2::new(sa.x, sa.y), Vec2::new(sb.x, sb.y));
    let c = Vec2::new(cursor.x, cursor.y);
    let ab = sb - sa;
    let t = ((c - sa).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
    Some((sa + ab * t).distance(c))
}

/// Face arrow or box surface under `pos`, arrows first.
pub(super) fn grab_at(f: &Frame, doc: &Document, pos: Pos2) -> Option<BoxGrab> {
    let (area, _) = handle_box(doc)?;
    let mut best: Option<(f32, BoxGrab)> = None;
    for axis in 0..3 {
        for positive in [false, true] {
            let base = face_center(&area, axis, positive);
            let len = BOX_ARROW_LENGTH * world_per_point(f, base);
            let tip = base + face_dir(axis, positive) * len;
            let Some(d) = segment_distance(f, pos, base, tip) else {
                continue;
            };
            if d <= BOX_GRAB && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, BoxGrab::Face { axis, positive }));
            }
        }
    }
    if let Some((_, grab)) = best {
        return Some(grab);
    }
    let ray = f.ray(pos);
    box_face_hit(&area, ray.origin, ray.dir).map(|(axis, positive, t)| BoxGrab::Body {
        axis,
        positive,
        point: ray.at(t),
    })
}

/// Blue box with its six face arrows (axis colors); the last applied box
/// is drawn dimmed. `hovered` is highlighted.
pub(super) fn overlay(f: &Frame, doc: &Document, hovered: Option<BoxGrab>, out: &mut Preview) {
    let Some((area, open)) = handle_box(doc) else {
        return;
    };
    let ppp = f.pixels_per_point;
    let (color, width) = if open {
        (theme::SELECTION_BOX, 2.5)
    } else {
        (theme::SELECTION_BOX.gamma_multiply(0.5), 1.5)
    };
    let (lo, hi) = (area.min.as_vec3(), area.max.as_vec3());
    box_outline(lo + f.bmin, hi + f.bmin, color, width * ppp, out);
    if let Some(BoxGrab::Body { axis, positive, .. }) = hovered {
        // The hovered face, which a drag moves the box across.
        let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
        let mut q = [lo; 4];
        let at = if positive { hi[axis] } else { lo[axis] };
        for (i, c) in q.iter_mut().enumerate() {
            c[axis] = at;
            c[u] = if i == 1 || i == 2 { hi[u] } else { lo[u] };
            c[v] = if i >= 2 { hi[v] } else { lo[v] };
        }
        quad(out, q.map(|c| c + f.bmin), color.gamma_multiply(0.18));
    }
    for axis in 0..3 {
        for positive in [false, true] {
            let base = face_center(&area, axis, positive);
            let scale = world_per_point(f, base);
            let mut c = axis_color(axis);
            if hovered == Some(BoxGrab::Face { axis, positive }) {
                c = c.lerp_to_gamma(Color32::WHITE, 0.45);
            } else if !open {
                c = c.gamma_multiply(0.6);
            }
            arrow(
                out,
                base + f.bmin,
                face_dir(axis, positive),
                BOX_ARROW_LENGTH * scale,
                scale * BOX_ARROW_SCALE,
                c,
                f.proj.eye(),
            );
        }
    }
}

/// `Enter` applies the open box (combined by the modifiers held), `Esc`
/// cancels it. Outside a drag only.
pub(super) fn keys(f: &Frame, state: &mut AppState, mods: Modifiers) {
    if state.doc().is_none_or(|d| d.box_select().is_none()) {
        return;
    }
    let (enter, esc) = f.ui.input_mut(|i| {
        (
            i.consume_key(Modifiers::NONE, Key::Enter)
                || i.consume_key(Modifiers::COMMAND, Key::Enter),
            i.consume_key(Modifiers::NONE, Key::Escape),
        )
    });
    let mode = mask_mode(mods, state.options.selection.mask);
    if esc {
        if let Some(doc) = state.doc_mut() {
            doc.cancel_box_select();
        }
        state.status_message = Some("Box selection cancelled".to_owned());
    } else if enter {
        apply(state, mode);
    }
}

/// Applies the open box as the selection (one undo step).
pub(super) fn apply(state: &mut AppState, mode: rizxel_engine::MaskMode) {
    let Some(doc) = state.doc_mut() else { return };
    if doc.box_select().is_none() {
        return;
    }
    doc.set_box_mode(mode);
    doc.apply_box_select();
    state.status_message = Some(format!("Selected: {} voxels", state.selected_voxels()));
}

/// Status bar text for the open box.
pub(super) fn feedback(doc: &Document) -> Option<String> {
    let b = doc.box_select()?;
    let (p, s) = (b.area.min, b.area.size());
    let found = if b.stale {
        "updates on release".to_owned()
    } else {
        format!("{} voxels", b.found())
    };
    Some(format!(
        "Box ({}): at ({}, {}, {}), {}×{}×{}, {found}. Drag the arrows or faces, Enter applies, Esc cancels",
        b.mode.label(),
        p.x,
        p.y,
        p.z,
        s.x,
        s.y,
        s.z
    ))
}

impl ToolInput {
    /// Starts dragging a face arrow or the surface of the box (re-opening
    /// the last applied box when its handle is grabbed).
    pub(super) fn begin_box_drag(
        &mut self,
        state: &mut AppState,
        pos: Pos2,
        grab: BoxGrab,
        mods: Modifiers,
    ) {
        let mode = mask_mode(mods, state.options.selection.mask);
        let Some(doc) = state.doc_mut() else { return };
        if doc.box_select().is_none() && !doc.reopen_last_box(mode) {
            return;
        }
        let Some(area) = doc.box_select().map(|b| b.area) else {
            return;
        };
        let kind = match grab {
            BoxGrab::Face { axis, positive } => DragKind::BoxFace {
                axis,
                positive,
                start: area,
                pivot: face_center(&area, axis, positive),
            },
            BoxGrab::Body { axis, point, .. } => DragKind::BoxMove {
                axis,
                start: area,
                from: point,
                along_normal: mods.alt,
            },
        };
        self.drag = Some(Drag {
            doc_id: doc.id(),
            tool: Tool::Selection,
            layer: doc.active_layer,
            start: pos,
            snapshot: VoxelVolume::new(BoundSize::new(1, 1, 1)),
            plane: doc.plane,
            op: VoxelOp::Paint,
            kind,
        });
    }
}

/// Face arrow drag: the face follows the pointer along its axis.
pub(super) fn drag_face(
    f: &Frame,
    state: &mut AppState,
    delta: Vec2,
    (axis, positive, start, pivot): (usize, bool, VoxelBox, Vec3),
) {
    let s = f.axis_screen(pivot)[axis];
    if s.length() <= 0.5 {
        return;
    }
    let amount = (delta.dot(s) / s.length_squared()).round() as i32;
    let outward = if positive { amount } else { -amount };
    let Some(doc) = state.doc_mut() else { return };
    let area = resize_face(start, axis, positive, outward, &doc.box_limit());
    doc.set_box_area(area, false);
}

/// Surface drag: the box slides in the grabbed face's plane, or with `Alt`
/// along its normal.
pub(super) fn drag_body(
    f: &Frame,
    state: &mut AppState,
    ray: &Ray,
    delta: Vec2,
    (axis, start, from, along_normal): (usize, VoxelBox, Vec3, bool),
) {
    let offset = if along_normal {
        let s = f.axis_screen(from)[axis];
        if s.length() <= 0.5 {
            return;
        }
        let mut o = IVec3::ZERO;
        o[axis] = (delta.dot(s) / s.length_squared()).round() as i32;
        o
    } else {
        let Some(p) = raycast_face(ray, axis, from[axis]) else {
            return;
        };
        let mut o = (p - from).round().as_ivec3();
        o[axis] = 0;
        o
    };
    let Some(doc) = state.doc_mut() else { return };
    let area = move_box(start, offset, &doc.box_limit());
    doc.set_box_area(area, false);
}

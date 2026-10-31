//! Line tool: lines, polylines and curves drawn with the Brush's settings.
//!
//! *Line*: drag from start to end. *Polyline*: every click adds a point;
//! `Enter`, a double-click or a right-click finishes. *Curve*: drag start
//! to end, then drag once to bend it (quadratic) and once more for a cubic;
//! a click elsewhere or `Enter` applies it. While a polyline or curve is
//! open its points are handles that can be dragged; `Esc` cancels. The
//! path is previewed as ghost voxels and written as one undo step.

use egui::{Color32, Key, Modifiers, Pos2};
use glam::{IVec3, Vec3};
use rizxel_app::{AppState, Document};
use rizxel_engine::tools::{
    bezier_at, bezier_path, brush_center, connect, line_cells_stamped, line_path, line_stroke,
    polyline_path,
};
use rizxel_engine::{BrushOptions, LineMode, RayHit, Tool, ToolContext, VoxelOp};
use rizxel_math::VoxelBox;

use super::{
    box_outline, cell_on_plane, ghost, ghost_cells, is_additive, pick_doc, write, Frame, Preview,
    ToolInput,
};
use crate::theme;

/// Screen distance (logical points) at which a point handle is grabbed.
const HANDLE_GRAB: f32 = 10.0;
/// Pointer travel (points) that turns a press into a drag.
const DRAG_START: f32 = 3.0;
/// Largest distance (voxels) of a curve piece from its chord.
const CURVE_FLATNESS: f32 = 0.35;

/// Path and brush settings a ghost was stamped for.
type GhostKey = (Vec<IVec3>, BrushOptions);

/// A point of the open line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Handle {
    /// Anchor `i` (start, end, or a polyline corner).
    Point(usize),
    /// Bend point `i` of a curve.
    Control(usize),
}

/// The line being drawn (owned by [`ToolInput`] across frames).
pub(super) struct LineSession {
    doc_id: u64,
    layer: usize,
    mode: LineMode,
    op: VoxelOp,
    /// Start and end (Line, Curve) or the clicked corners (Polyline).
    points: Vec<IVec3>,
    /// Curve bend points, at most two.
    controls: Vec<IVec3>,
    /// Axis of the first point's face: picks off the model land on its
    /// plane.
    axis: usize,
    /// Cell under the pointer (the polyline's next segment).
    hover: Option<IVec3>,
    /// Handle being dragged.
    grabbed: Option<Handle>,
    /// Press that may still become a drag (curves: bend, or click to
    /// apply).
    pending: Option<Pos2>,
    /// Ghost cache: path and brush settings, and the stamped cells.
    ghost: Option<(GhostKey, Vec<IVec3>)>,
}

impl LineSession {
    fn point(&self, h: Handle) -> IVec3 {
        match h {
            Handle::Point(i) => self.points[i],
            Handle::Control(i) => self.controls[i],
        }
    }

    fn set(&mut self, h: Handle, cell: IVec3) {
        match h {
            Handle::Point(i) => self.points[i] = cell,
            Handle::Control(i) => self.controls[i] = cell,
        }
    }

    fn handles(&self) -> Vec<Handle> {
        (0..self.points.len())
            .map(Handle::Point)
            .chain((0..self.controls.len()).map(Handle::Control))
            .collect()
    }

    /// Cells of the path; `preview` adds the polyline's segment to the
    /// pointer.
    fn path(&self, preview: bool, brush: &BrushOptions) -> Vec<IVec3> {
        let raw = match self.mode {
            LineMode::Line => line_path(self.points[0], self.points[self.points.len() - 1]),
            LineMode::Polyline => {
                let mut pts = self.points.clone();
                if let (true, Some(h)) = (preview, self.hover) {
                    pts.push(h);
                }
                polyline_path(&pts)
            }
            LineMode::Curve => bezier_path(&self.curve_points(), CURVE_FLATNESS),
        };
        connect(raw, brush.connect)
    }

    fn curve_points(&self) -> Vec<IVec3> {
        let mut pts = vec![self.points[0]];
        pts.extend(&self.controls);
        pts.push(self.points[self.points.len() - 1]);
        pts
    }

    fn label(&self) -> &'static str {
        self.mode.label()
    }
}

/// Cell a line point goes to: the brush center of the face under the
/// pointer, else the cell on the first point's plane.
fn point_at(
    f: &Frame,
    doc: &Document,
    pos: Pos2,
    s: Option<&LineSession>,
    op: VoxelOp,
) -> Option<IVec3> {
    let ray = f.ray(pos);
    if let Some(hit) = pick_doc(doc, &ray, true, is_additive(op)) {
        return Some(brush_center(&hit, op));
    }
    let s = s?;
    cell_on_plane(&ray, s.points[0], s.axis, doc.data.bounds)
}

/// Handle within [`HANDLE_GRAB`] points of `pos`, nearest first.
fn handle_at(f: &Frame, s: &LineSession, pos: Pos2) -> Option<Handle> {
    s.handles()
        .into_iter()
        .filter_map(|h| {
            let p = f.screen(s.point(h).as_vec3() + Vec3::splat(0.5))?;
            let d = p.distance(pos);
            (d <= HANDLE_GRAB).then_some((d, h))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, h)| h)
}

impl ToolInput {
    /// `Enter` finishes and `Esc` cancels an open polyline or curve
    /// (before the selection keys see them).
    pub(super) fn line_keys(&mut self, f: &Frame, state: &mut AppState) {
        if self.line.is_none() {
            return;
        }
        let (enter, esc) = f.ui.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::Enter),
                i.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        if esc {
            self.line = None;
            state.status_message = Some("Line cancelled".to_owned());
        } else if enter {
            self.finish_line(state);
        }
    }

    /// Ends the open line when its document, layer or tool changed (a
    /// tool switch applies it).
    pub(super) fn sync_line(&mut self, state: &mut AppState) {
        let Some(s) = &self.line else { return };
        let same_doc = state.doc().is_some_and(|d| d.id() == s.doc_id);
        let same_layer = state.doc().is_some_and(|d| d.active_layer == s.layer);
        if !same_doc || !same_layer {
            self.line = None;
        } else if state.tool != Tool::Line {
            self.finish_line(state);
        }
    }

    /// Line tool input for this frame: presses, handle drags and the
    /// preview. `pressed` is a left press inside the viewport.
    pub(super) fn line_frame(
        &mut self,
        f: &Frame,
        state: &mut AppState,
        pressed: bool,
        mods: Modifiers,
        out: &mut Preview,
    ) {
        let (down, latest) =
            f.ui.input(|i| (i.pointer.primary_down(), i.pointer.latest_pos()));
        let hover = f.response.hover_pos();
        let op = if mods.shift {
            VoxelOp::Subtract
        } else {
            state.op
        };
        let mode = state.options.line.mode;

        // Right-click or double-click finishes a polyline.
        if self
            .line
            .as_ref()
            .is_some_and(|s| s.mode == LineMode::Polyline)
            && (f.response.secondary_clicked() || f.response.double_clicked())
        {
            self.finish_line(state);
            return;
        }

        if pressed {
            if let Some(pos) = hover {
                self.line_press(f, state, pos, op, mode);
            }
        }

        // Drag of a handle (or the first segment) follows the pointer.
        if let (Some(pos), Some(doc)) = (latest, state.doc()) {
            let session = self.line.as_ref();
            let cell = session.and_then(|s| point_at(f, doc, pos, Some(s), s.op));
            if let (Some(s), Some(cell)) = (self.line.as_mut(), cell) {
                if let Some(start) = s.pending.filter(|_| down) {
                    if start.distance(pos) > DRAG_START {
                        s.pending = None;
                        // A drag on a curve bends it.
                        if s.controls.len() < 2 {
                            s.controls.push(cell);
                        }
                        s.grabbed = Some(Handle::Control(s.controls.len() - 1));
                    }
                }
                if let Some(h) = s.grabbed.filter(|_| down) {
                    s.set(h, cell);
                }
                if hover.is_some() {
                    s.hover = Some(cell);
                }
            }
        }

        if !down {
            let released = self.line.as_ref().map(|s| (s.grabbed, s.pending));
            if let Some((grabbed, pending)) = released {
                if let Some(s) = &mut self.line {
                    s.grabbed = None;
                    s.pending = None;
                }
                match (mode_of(&self.line), grabbed, pending) {
                    // A Line is done when its drag ends.
                    (Some(LineMode::Line), Some(_), _) => self.finish_line(state),
                    // A click (no drag) on an open curve applies it.
                    (Some(LineMode::Curve), None, Some(_)) => self.finish_line(state),
                    _ => {}
                }
            }
        }

        self.line_preview(f, state, out);
    }

    fn line_press(
        &mut self,
        f: &Frame,
        state: &mut AppState,
        pos: Pos2,
        op: VoxelOp,
        mode: LineMode,
    ) {
        let Some(doc) = state.doc() else { return };
        if let Some(s) = &mut self.line {
            if let Some(h) = handle_at(f, s, pos) {
                s.grabbed = Some(h);
                return;
            }
            match s.mode {
                LineMode::Polyline => {
                    if let Some(cell) = point_at(f, doc, pos, Some(s), s.op) {
                        if s.points.last() != Some(&cell) {
                            s.points.push(cell);
                        }
                    }
                }
                LineMode::Curve => s.pending = Some(pos),
                LineMode::Line => {}
            }
            return;
        }
        let layer = doc.active_layer;
        if let Err(e) = doc.editable_layer(layer) {
            state.status_message = Some(e.message().to_owned());
            return;
        }
        let ray = f.ray(pos);
        let Some(hit) = pick_doc(doc, &ray, true, is_additive(op)) else {
            return;
        };
        let start = brush_center(&hit, op);
        let axis = face_axis(doc, &hit);
        let (points, grabbed) = match mode {
            LineMode::Polyline => (vec![start], None),
            LineMode::Line | LineMode::Curve => (vec![start, start], Some(Handle::Point(1))),
        };
        self.line = Some(LineSession {
            doc_id: doc.id(),
            layer,
            mode,
            op,
            points,
            controls: Vec::new(),
            axis,
            hover: Some(start),
            grabbed,
            pending: None,
            ghost: None,
        });
    }

    /// Writes the open line as one undo step.
    pub(super) fn finish_line(&mut self, state: &mut AppState) {
        let Some(s) = self.line.take() else { return };
        let path = s.path(false, &state.options.brush);
        if path.is_empty() {
            return;
        }
        let Some(doc) = state.doc_mut() else { return };
        doc.begin_edit();
        write(state, s.layer, s.op, |ctx| line_stroke(ctx, &path));
        let Some(doc) = state.doc_mut() else { return };
        let changed = doc.pending_changes();
        if doc.labeled(s.label(), Document::commit_edit) {
            state.status_message = Some(format!("{}: {changed} voxels changed", s.label()));
        }
    }

    /// Ghost voxels of the path, point handles and the curve's control
    /// lines.
    fn line_preview(&mut self, f: &Frame, state: &mut AppState, out: &mut Preview) {
        let Some(doc) = state.doc() else { return };
        let options = state.options;
        let Some(s) = &mut self.line else { return };
        let path = s.path(true, &options.brush);
        let key = (path.clone(), options.brush);
        if s.ghost.as_ref().is_none_or(|(k, _)| *k != key) {
            let volume = doc.data.layers.get(s.layer).map(|l| &l.volume);
            let cells = volume.map_or_else(Vec::new, |volume| {
                let ctx = ToolContext {
                    volume,
                    bounds: doc.data.bounds,
                    color: doc.selected_color as rizxel_math::Voxel,
                    op: s.op,
                    options: &options,
                    hit: None,
                };
                line_cells_stamped(&ctx, &path)
            });
            s.ghost = Some((key, cells));
        }
        let clip = doc
            .edit_clip()
            .unwrap_or(VoxelBox::from_bounds(doc.data.bounds));
        if let Some((_, cells)) = &s.ghost {
            let color = match s.op {
                VoxelOp::Subtract => theme::p().danger.gamma_multiply(0.45),
                _ => {
                    let c = doc.selected_rgba();
                    Color32::from_rgb(c.r, c.g, c.b).gamma_multiply(0.55)
                }
            };
            ghost(f.bmin, &ghost_cells(cells.clone(), &clip), color, out);
        }
        let ppp = f.pixels_per_point;
        let cell_box = |p: IVec3, color: Color32, out: &mut Preview| {
            let lo = p.as_vec3() + f.bmin;
            box_outline(lo, lo + Vec3::ONE, color, 2.0 * ppp, out);
        };
        for &p in &s.points {
            cell_box(p, theme::ACCENT_LIGHT, out);
        }
        if s.mode == LineMode::Curve {
            for &c in &s.controls {
                cell_box(c, theme::PAINT, out);
            }
            // Control polygon and the exact curve as a thin line.
            let pts = s.curve_points();
            let center = |p: IVec3| p.as_vec3() + Vec3::splat(0.5) + f.bmin;
            if !s.controls.is_empty() {
                for w in pts.windows(2) {
                    out.lines.push(crate::render::LineInstance::new(
                        center(w[0]),
                        center(w[1]),
                        theme::PAINT.gamma_multiply(0.5),
                        ppp,
                    ));
                }
            }
            const STEPS: usize = 48;
            let at = |t: f32| bezier_at(&pts, t) + Vec3::splat(0.5) + f.bmin;
            for i in 0..STEPS {
                let (a, b) = (i as f32 / STEPS as f32, (i + 1) as f32 / STEPS as f32);
                out.lines.push(crate::render::LineInstance::new(
                    at(a),
                    at(b),
                    theme::ACCENT_LIGHT,
                    1.5 * ppp,
                ));
            }
        }
        state.tool_feedback = Some(match s.mode {
            LineMode::Line => format!("Line: {} cells", path.len()),
            LineMode::Polyline => format!(
                "Polyline: {} points, {} cells. Click to add, Enter or right-click ends, Esc cancels",
                s.points.len(),
                path.len()
            ),
            LineMode::Curve if s.grabbed == Some(Handle::Point(1)) && s.controls.is_empty() => {
                format!("Curve: {} cells. Release, then drag to bend", path.len())
            }
            LineMode::Curve => format!(
                "Curve: {} of 2 bends, {} cells. Drag to bend, click or Enter applies, Esc cancels",
                s.controls.len(),
                path.len()
            ),
        });
    }
}

fn mode_of(s: &Option<LineSession>) -> Option<LineMode> {
    s.as_ref().map(|s| s.mode)
}

/// Axis whose plane the line stays on when the pointer leaves the model:
/// the slice axis while slicing, else the pressed face's.
fn face_axis(doc: &Document, hit: &RayHit) -> usize {
    if doc.plane.slice {
        doc.plane.axis.min(2)
    } else {
        hit.axis()
    }
}

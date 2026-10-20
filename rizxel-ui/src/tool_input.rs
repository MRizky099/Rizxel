//! Viewport tool input (Phase 5a): voxel picking, the left-drag state
//! machine and the hover/ghost previews.
//!
//! `Idle -> Pressed -> Dragging -> Released`: a left press inside the
//! viewport opens an edit transaction on the active document, every frame
//! recomputes the tool result from the pointer and writes it tentatively,
//! release commits it as one undo step and `Esc` (or losing window focus)
//! cancels it. Picking during a drag uses a snapshot of the visible model
//! taken at press time, so a stroke never climbs onto the voxels it just
//! added.
//!
//! Modifiers are read at press time: `Shift` erases/subtracts, `Alt` picks a
//! color with any tool, `Ctrl` copies the picked color into the selected
//! swatch.
//!
//! The Plane tool never writes voxels (Goxel / Qubicle behaviour): a click
//! places the document's [`WorkPlane`] on the clicked face. While the plane
//! is visible every drawing tool also picks on it, so voxels can be drawn
//! in empty space. With Slice on, the side beyond the plane is hidden, edits
//! are confined to the visible side, and dragging with the Plane tool moves
//! the plane along its normal.

use std::collections::HashSet;

use egui::{Color32, Key, Modifiers, Pos2, Rect, Response, Ui};
use glam::{IVec3, Vec2, Vec3};
use rizxel_app::{AppState, Document, VoxelChange};
use rizxel_engine::raycast::{dominant_axis, raycast_floor, raycast_grid};
use rizxel_engine::tools::{
    brush_center, brush_positions, brush_stroke, drag_axis, extrude_changes, extrude_region,
    move_changes, pick_voxel, resize_bounds,
};
use rizxel_engine::{DragMode, Ray, RayHit, Tool, ToolContext, VoxelOp, WorkPlane};
use rizxel_math::{BoundSize, Voxel, VoxelBox, VoxelVolume, EMPTY_VOXEL};

use crate::render::{FillVertex, LineInstance};
use crate::theme;
use crate::viewport::Projection;

/// Ghost previews with more cells than this fall back to the face highlight.
const MAX_GHOST_CELLS: usize = 4096;
/// Lift of highlight quads off the voxel surface, against z-fighting.
const SURFACE_EPS: f32 = 0.01;

/// Overlay geometry produced by the active tool, in world space.
#[derive(Default)]
pub struct Preview {
    pub fills: Vec<FillVertex>,
    pub lines: Vec<LineInstance>,
}

/// Persistent tool-input state (owned by the app across frames).
#[derive(Default)]
pub struct ToolInput {
    drag: Option<Drag>,
    /// Extrude hover region cache: `(doc id, revision, voxel, normal)`.
    region_cache: Option<(RegionKey, Vec<IVec3>)>,
}

/// `(document id, revision, hit voxel, hit normal)`.
type RegionKey = (u64, u64, IVec3, IVec3);

/// One left drag in progress.
struct Drag {
    doc_id: u64,
    tool: Tool,
    layer: usize,
    start: Pos2,
    /// Visible model at press time; the pick target while dragging.
    snapshot: VoxelVolume,
    /// Work plane at press time (picked together with the snapshot).
    plane: WorkPlane,
    op: VoxelOp,
    kind: DragKind,
}

enum DragKind {
    Brush {
        last: IVec3,
    },
    /// Plane tool with Slice on: moves the plane along its normal.
    PlaneMove {
        pivot: Vec3,
        /// Point of the plane edge that was grabbed (voxel space), where the
        /// arrow hint follows the plane; `None` for a drag started elsewhere.
        grab: Option<Vec3>,
        start_coord: i32,
        moved: bool,
        /// Face under the press; a click without drag moves the plane there.
        click: Option<RayHit>,
    },
    Move {
        /// Voxel-space point whose screen axes measure the drag.
        pivot: Vec3,
        offset: IVec3,
    },
    Resize {
        pivot: Vec3,
        start: BoundSize,
        pushed: bool,
    },
    Extrude {
        region: Vec<IVec3>,
        normal: IVec3,
        /// Volume the region was found in (colors of the extruded faces).
        colors: VoxelVolume,
        pivot: Vec3,
        base: i32,
        depth: i32,
    },
}

/// Everything the tool code needs about the current frame.
pub struct Frame<'a> {
    pub ui: &'a Ui,
    pub response: &'a Response,
    pub proj: &'a Projection,
    /// World position of voxel `(0, 0, 0)`'s minimum corner.
    pub bmin: Vec3,
    /// Screen area owned by overlays (view cube); presses there are ignored.
    pub blocked: Rect,
    pub pixels_per_point: f32,
}

impl Frame<'_> {
    /// Mouse ray in voxel space.
    fn ray(&self, screen: Pos2) -> Ray {
        let (origin, dir) = self.proj.world_ray(screen);
        Ray::new(origin - self.bmin, dir)
    }

    /// Screen position of a voxel-space point.
    fn screen(&self, p: Vec3) -> Option<Pos2> {
        self.proj.to_screen(p + self.bmin)
    }

    /// Screen motion of one voxel along `+X`, `+Y`, `+Z` at `pivot`.
    fn axis_screen(&self, pivot: Vec3) -> [Vec2; 3] {
        let base = self.screen(pivot);
        [Vec3::X, Vec3::Y, Vec3::Z].map(|a| match (base, self.screen(pivot + a)) {
            (Some(b), Some(t)) => Vec2::new(t.x - b.x, t.y - b.y),
            _ => Vec2::ZERO,
        })
    }
}

/// Shared pick: the nearest of the first solid cell inside the visible
/// region and the work plane (when active; while slicing, the slice slab,
/// reachable from either side), then the floor. So while slicing, drawing
/// tools edit visible voxels on both sides of the plane as well as the
/// slice layer itself.
fn pick_with(
    ray: &Ray,
    bounds: BoundSize,
    plane: &WorkPlane,
    use_plane: bool,
    additive: bool,
    solid: impl FnMut(IVec3) -> bool,
) -> Option<RayHit> {
    let clip = plane.clip_box(bounds);
    let area = clip.unwrap_or(VoxelBox::from_bounds(bounds));
    let voxel = raycast_grid(ray, &area, solid);
    let on_plane = use_plane
        .then(|| plane.pick(ray, bounds, additive))
        .flatten();
    let best = match (voxel, on_plane) {
        (Some(v), Some(p)) => Some(if p.t <= v.t { p } else { v }),
        (v, p) => v.or(p),
    };
    best.or_else(|| {
        raycast_floor(ray, bounds).filter(|h| clip.is_none_or(|c| c.contains(h.adjacent())))
    })
}

/// Picks against the visible (and slice-clipped) geometry of `doc`.
fn pick_doc(doc: &Document, ray: &Ray, use_plane: bool, additive: bool) -> Option<RayHit> {
    let bounds = doc.data.bounds;
    doc.with_visible_geometry(|vols| {
        pick_with(ray, bounds, &doc.plane, use_plane, additive, |p| {
            vols.iter().any(|v| v.get(p) != EMPTY_VOXEL)
        })
    })
}

fn pick_drag(drag: &Drag, ray: &Ray) -> Option<RayHit> {
    let additive = matches!(drag.op, VoxelOp::Add | VoxelOp::Join);
    let volume = &drag.snapshot;
    pick_with(ray, volume.bounds(), &drag.plane, true, additive, |p| {
        volume.get(p) != EMPTY_VOXEL
    })
}

/// Whether a tool snaps onto the work plane.
fn draws_on_plane(tool: Tool) -> bool {
    matches!(tool, Tool::Brush | Tool::Cursor)
}

fn is_additive(op: VoxelOp) -> bool {
    matches!(op, VoxelOp::Add | VoxelOp::Join)
}

/// True for a hit on a real voxel (not the virtual floor row).
fn is_voxel(hit: &RayHit) -> bool {
    hit.voxel.y >= 0
}

const AXIS_NAMES: [&str; 3] = ["X", "Y", "Z"];

impl ToolInput {
    /// Runs the active tool for this frame and returns its overlays.
    pub fn update(&mut self, f: &Frame, state: &mut AppState) -> Preview {
        let mut preview = Preview::default();
        state.tool_feedback = None;
        let Some(doc_id) = state.doc().map(Document::id) else {
            self.drag = None;
            return preview;
        };

        // A drag survives only on its own document and while its
        // transaction is open (undo or a tab switch may have closed it).
        if let Some(drag) = &self.drag {
            let alive = drag.doc_id == doc_id
                && (matches!(
                    drag.kind,
                    DragKind::Resize { .. } | DragKind::PlaneMove { .. }
                ) || state.doc().is_some_and(Document::is_editing));
            if !alive {
                self.drag = None;
            }
        }

        let (pressed, released, down, focused, mods, latest) = f.ui.input(|i| {
            (
                i.pointer.button_pressed(egui::PointerButton::Primary),
                i.pointer.button_released(egui::PointerButton::Primary),
                i.pointer.primary_down(),
                i.focused,
                i.modifiers,
                i.pointer.latest_pos(),
            )
        });

        if self.drag.is_some() {
            let esc =
                f.ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape));
            if esc || !focused {
                self.cancel(state);
                state.status_message = Some("Edit cancelled".to_owned());
            } else if let Some(pos) = latest {
                self.drag_frame(f, state, pos);
            }
            if released || !down {
                self.finish(state);
            }
        }

        let hover = f.response.hover_pos();
        let tool = if mods.alt {
            Tool::ColorPicker
        } else {
            state.tool
        };
        let op = if mods.shift {
            VoxelOp::Subtract
        } else {
            state.op
        };
        let hit = match (&self.drag, hover) {
            (Some(d), Some(p)) => pick_drag(d, &f.ray(p)),
            (None, Some(p)) => state
                .doc()
                .and_then(|d| pick_doc(d, &f.ray(p), draws_on_plane(tool), is_additive(op))),
            _ => None,
        };
        state.cursor_voxel = hit.map(|h| if is_voxel(&h) { h.voxel } else { h.adjacent() });

        // Plane edge under the cursor (Plane tool): grab handle for moving.
        let edge = match (&self.drag, hover, state.doc()) {
            (None, Some(p), Some(doc)) if tool == Tool::Plane => plane_edge_at(f, doc, p),
            _ => None,
        };

        let press = pressed
            && self.drag.is_none()
            && f.response.hovered()
            && hover.is_some_and(|p| !f.blocked.contains(p));
        if press {
            if let Some(p) = hover {
                self.press(f, state, p, hit, edge, mods);
            }
        }

        if let Some(drag) = &self.drag {
            let pending = state.doc().map_or(0, Document::pending_changes);
            state.tool_feedback = Some(match drag.kind {
                DragKind::Resize { .. } => {
                    let b = state.doc().map(|d| d.data.bounds).unwrap_or_default();
                    format!("Resize: bounds {}×{}×{}", b.x(), b.y(), b.z())
                }
                DragKind::Move { offset, .. } => format!(
                    "Move: offset ({}, {}, {}), {pending} voxels changed",
                    offset.x, offset.y, offset.z
                ),
                DragKind::Extrude { depth, .. } => {
                    format!("Extrude: depth {depth}, {pending} voxels changed")
                }
                DragKind::PlaneMove { .. } => state.doc().map_or_else(String::new, |d| {
                    format!("Slice plane: {}", plane_label(&d.plane, d.data.bounds))
                }),
                _ => format!("{}: {pending} voxels changed", drag.tool.label()),
            });
        } else if edge.is_some() {
            state.tool_feedback = Some("Drag the plane edge to move the plane".to_owned());
        } else if let Some(hit) = hit {
            self.hover_preview(f, state, &hit, mods, &mut preview);
        }

        // The arrow is a hint: it appears where the plane edge is hovered and
        // follows the grabbed point while dragging.
        let arrow_at = match &self.drag {
            Some(Drag {
                kind: DragKind::PlaneMove { grab, .. },
                ..
            }) => {
                f.ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                *grab
            }
            Some(_) => None,
            None => {
                if edge.is_some() {
                    f.ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                }
                edge
            }
        };
        if let Some(doc) = state.doc() {
            work_plane_overlay(f, doc, arrow_at, &mut preview);
        }
        preview
    }

    // ---------------------------------------------------------------- press

    fn press(
        &mut self,
        f: &Frame,
        state: &mut AppState,
        pos: Pos2,
        hit: Option<RayHit>,
        edge: Option<Vec3>,
        mods: Modifiers,
    ) {
        let tool = if mods.alt {
            Tool::ColorPicker
        } else {
            state.tool
        };
        match tool {
            Tool::Cursor => {}
            Tool::ColorPicker => pick_color(f, state, pos, mods.command),
            Tool::Plane => {
                let slicing = state.doc().is_some_and(|d| d.plane.slice);
                if edge.is_some() || slicing {
                    self.begin_plane_move(state, pos, hit, edge);
                } else {
                    place_plane(state, hit);
                }
            }
            Tool::Brush | Tool::Move | Tool::Extrude => {
                self.begin_drag(f, state, tool, pos, hit, mods)
            }
            _ => {
                state.status_message =
                    Some(format!("The {} tool arrives in Phase 5b", tool.label()));
            }
        }
    }

    fn begin_drag(
        &mut self,
        f: &Frame,
        state: &mut AppState,
        tool: Tool,
        pos: Pos2,
        hit: Option<RayHit>,
        mods: Modifiers,
    ) {
        let options = state.options;
        let op = if mods.shift {
            VoxelOp::Subtract
        } else {
            state.op
        };
        let resize = tool == Tool::Move && options.moving.drag_mode == DragMode::Resize;
        let Some(doc) = state.doc_mut() else {
            return;
        };
        let layer = doc.active_layer;
        if !resize {
            if let Err(e) = doc.editable_layer(layer) {
                state.status_message = Some(e.message().to_owned());
                return;
            }
        }
        let bounds = doc.data.bounds;
        let center = bounds.0.as_vec3() * 0.5;
        let kind = match tool {
            Tool::Brush => {
                let Some(hit) = hit else { return };
                DragKind::Brush {
                    last: brush_center(&hit, op),
                }
            }
            Tool::Move if resize => DragKind::Resize {
                pivot: hit.map_or(center, |h| h.voxel.as_vec3() + Vec3::splat(0.5)),
                start: bounds,
                pushed: false,
            },
            Tool::Move => DragKind::Move {
                pivot: hit.map_or(center, |h| h.voxel.as_vec3() + Vec3::splat(0.5)),
                offset: IVec3::ZERO,
            },
            Tool::Extrude => {
                let Some(hit) = hit.filter(is_voxel) else {
                    return;
                };
                let colors = if options.extrude.confine_to_layer {
                    doc.data.layers[layer].volume.clone()
                } else {
                    doc.merged_visible_volume()
                };
                let region = extrude_region(&colors, &hit, &options.extrude, &doc.data.palette);
                if region.is_empty() {
                    state.status_message = Some("Nothing to extrude here".to_owned());
                    return;
                }
                let sign = if mods.shift { -1 } else { 1 };
                DragKind::Extrude {
                    region,
                    normal: hit.normal,
                    colors,
                    pivot: hit.adjacent().as_vec3() + Vec3::splat(0.5),
                    base: sign * options.extrude.depth.max(1) as i32,
                    depth: 0,
                }
            }
            _ => return,
        };
        if !resize {
            doc.begin_edit();
        }
        let snapshot = match doc.view_clip() {
            Some(clip) => doc.merged_visible_volume().cropped(&clip),
            None => doc.merged_visible_volume(),
        };
        self.drag = Some(Drag {
            doc_id: doc.id(),
            tool,
            layer,
            start: pos,
            snapshot,
            plane: doc.plane,
            op,
            kind,
        });
        // Apply the press itself (first stamp, 1×1 plane, base extrude).
        if let Some(DragKind::Brush { last }) = self.drag.as_ref().map(|d| &d.kind) {
            let last = *last;
            write(state, layer, op, |ctx| brush_stroke(ctx, None, last));
        }
        self.drag_frame(f, state, pos);
    }

    /// Starts dragging the plane along its normal: from a grabbed edge, or
    /// from anywhere while slicing.
    fn begin_plane_move(
        &mut self,
        state: &mut AppState,
        pos: Pos2,
        hit: Option<RayHit>,
        grab: Option<Vec3>,
    ) {
        let Some(doc) = state.doc_mut() else { return };
        let bounds = doc.data.bounds;
        doc.plane = doc.plane.clamped(bounds);
        let p = doc.plane;
        let pivot = grab.unwrap_or_else(|| {
            let mut c = bounds.0.as_vec3() * 0.5;
            c[p.axis] = p.coord as f32;
            c
        });
        self.drag = Some(Drag {
            doc_id: doc.id(),
            tool: Tool::Plane,
            layer: doc.active_layer,
            start: pos,
            snapshot: VoxelVolume::new(BoundSize::new(1, 1, 1)),
            plane: p,
            op: VoxelOp::Add,
            kind: DragKind::PlaneMove {
                pivot,
                grab,
                start_coord: p.coord,
                moved: false,
                // A click on the edge is not a "jump to this face" click.
                click: hit.filter(is_voxel).filter(|_| grab.is_none()),
            },
        });
    }

    // ----------------------------------------------------------------- drag

    /// Recomputes the drag result for the pointer at `pos`.
    fn drag_frame(&mut self, f: &Frame, state: &mut AppState, pos: Pos2) {
        let Some(drag) = &mut self.drag else {
            return;
        };
        let delta = Vec2::new(pos.x - drag.start.x, pos.y - drag.start.y);
        let ray = f.ray(pos);
        let (layer, op) = (drag.layer, drag.op);
        match &mut drag.kind {
            DragKind::Brush { last } => {
                let Some(hit) = pick_with(
                    &ray,
                    drag.snapshot.bounds(),
                    &drag.plane,
                    true,
                    is_additive(op),
                    |p| drag.snapshot.get(p) != EMPTY_VOXEL,
                ) else {
                    return;
                };
                let center = brush_center(&hit, op);
                if center == *last {
                    return;
                }
                let from = std::mem::replace(last, center);
                write(state, layer, op, |ctx| {
                    brush_stroke(ctx, Some(from), center)
                });
            }
            DragKind::PlaneMove {
                pivot,
                grab,
                start_coord,
                moved,
                ..
            } => {
                *moved |= delta.length() > 3.0;
                let axis = drag.plane.axis;
                let s = f.axis_screen(*pivot)[axis];
                if s.length() <= 0.5 {
                    return;
                }
                let amount = (delta.dot(s) / s.length_squared()).round() as i32;
                let Some(doc) = state.doc_mut() else { return };
                doc.plane.coord = *start_coord + amount;
                doc.plane = doc.plane.clamped(doc.data.bounds);
                if let Some(g) = grab {
                    g[axis] = doc.plane.coord as f32;
                }
            }
            DragKind::Move { pivot, offset } => {
                let new = match drag_axis(delta, f.axis_screen(*pivot)) {
                    Some((axis, amount)) => {
                        let mut o = IVec3::ZERO;
                        o[axis] = amount;
                        o
                    }
                    None => IVec3::ZERO,
                };
                if new == *offset {
                    return;
                }
                *offset = new;
                rewrite(state, layer, op, |ctx| move_changes(ctx.volume, new));
            }
            DragKind::Resize {
                pivot,
                start,
                pushed,
            } => {
                let Some((axis, amount)) = drag_axis(delta, f.axis_screen(*pivot)) else {
                    return;
                };
                let new = resize_bounds(*start, axis, amount);
                let Some(doc) = state.doc_mut() else { return };
                if new != doc.data.bounds {
                    doc.set_bounds(new, *pushed);
                    *pushed = true;
                }
            }
            DragKind::Extrude {
                region,
                normal,
                colors,
                pivot,
                base,
                depth,
            } => {
                let axis = dominant_axis(normal.as_vec3());
                let mut s = f.axis_screen(*pivot)[axis];
                if normal[axis] < 0 {
                    s = -s;
                }
                let amount = if s.length() > 0.5 {
                    (delta.dot(s) / s.length_squared()).round() as i32
                } else {
                    0
                };
                let mut new = *base + amount;
                // Never collapse to a no-op: crossing zero jumps to the
                // other direction.
                if new == 0 {
                    new = if amount > 0 { 1 } else { -1 };
                }
                if new == *depth {
                    return;
                }
                *depth = new;
                let normal = *normal;
                rewrite(state, layer, op, |ctx| {
                    extrude_changes(ctx.volume, colors, region, normal, new)
                });
            }
        }
    }

    fn finish(&mut self, state: &mut AppState) {
        let Some(drag) = self.drag.take() else { return };
        if let DragKind::PlaneMove {
            moved: false,
            click: Some(hit),
            ..
        } = drag.kind
        {
            // A click without drag moves the slice plane onto that face.
            if let Some(doc) = state.doc_mut() {
                doc.plane.set_from_hit(&hit);
            }
            return;
        }
        if matches!(
            drag.kind,
            DragKind::Resize { .. } | DragKind::PlaneMove { .. }
        ) {
            return;
        }
        if let Some(doc) = state.doc_mut() {
            let changed = doc.pending_changes();
            if doc.commit_edit() {
                state.status_message =
                    Some(format!("{}: {changed} voxels changed", drag.tool.label()));
            }
        }
    }

    fn cancel(&mut self, state: &mut AppState) {
        let Some(drag) = self.drag.take() else { return };
        let Some(doc) = state.doc_mut() else { return };
        match drag.kind {
            DragKind::Resize { pushed: true, .. } => {
                doc.undo();
            }
            DragKind::Resize { .. } => {}
            DragKind::PlaneMove { start_coord, .. } => doc.plane.coord = start_coord,
            _ => doc.cancel_edit(),
        }
    }

    // ------------------------------------------------------------- previews

    fn hover_preview(
        &mut self,
        f: &Frame,
        state: &mut AppState,
        hit: &RayHit,
        mods: Modifiers,
        out: &mut Preview,
    ) {
        let Some(doc) = state.doc() else { return };
        let ppp = f.pixels_per_point;
        let bounds = doc.data.bounds;
        let clip = doc.view_clip().unwrap_or(VoxelBox::from_bounds(bounds));
        let tool = if mods.alt {
            Tool::ColorPicker
        } else {
            state.tool
        };
        let op = if mods.shift {
            VoxelOp::Subtract
        } else {
            state.op
        };
        let paint = color32(doc.selected_rgba());
        let ghost_color = match op {
            VoxelOp::Subtract => theme::DANGER.gamma_multiply(0.45),
            _ => paint.gamma_multiply(0.55),
        };
        match tool {
            Tool::Brush => {
                let cells = ghost_cells(
                    brush_positions(brush_center(hit, op), &state.options.brush, bounds),
                    &clip,
                );
                ghost(f.bmin, &cells, ghost_color, out);
                face_highlight(f.bmin, hit, ppp, out);
            }
            Tool::Plane if doc.plane.slice => {
                state.tool_feedback = Some(format!(
                    "Slice plane: {} — drag to move it, click a face to jump there",
                    plane_label(&doc.plane, doc.data.bounds)
                ));
            }
            Tool::Plane => {
                face_highlight(f.bmin, hit, ppp, out);
                let mut p = doc.plane;
                p.set_from_hit(hit);
                state.tool_feedback = Some(format!(
                    "Click to place the plane at {}",
                    plane_label(&p, bounds)
                ));
            }
            Tool::Extrude if is_voxel(hit) => {
                let key = (doc.id(), doc.revision(), hit.voxel, hit.normal);
                if self.region_cache.as_ref().is_none_or(|(k, _)| *k != key) {
                    let opts = &state.options.extrude;
                    let source;
                    let volume = if opts.confine_to_layer {
                        doc.active_layer().map(|l| &l.volume)
                    } else {
                        source = doc.merged_visible_volume();
                        Some(&source)
                    };
                    let region = volume
                        .map(|v| extrude_region(v, hit, opts, &doc.data.palette))
                        .unwrap_or_default();
                    self.region_cache = Some((key, region));
                }
                if let Some((_, region)) = &self.region_cache {
                    if region.len() <= MAX_GHOST_CELLS {
                        for &v in region {
                            let h = RayHit { voxel: v, ..*hit };
                            face_fill(f.bmin, &h, theme::ACCENT_LIGHT.gamma_multiply(0.4), out);
                        }
                    }
                }
                face_highlight(f.bmin, hit, ppp, out);
            }
            Tool::Move => {}
            _ => face_highlight(f.bmin, hit, ppp, out),
        }
    }
}

/// Runs `make` against the drag layer and writes its changes on top of
/// what the drag already wrote (Brush).
fn write(
    state: &mut AppState,
    layer: usize,
    op: VoxelOp,
    make: impl FnOnce(&ToolContext) -> Vec<VoxelChange>,
) {
    let options = state.options;
    let Some(doc) = state.doc_mut() else { return };
    let Some(volume) = doc.data.layers.get(layer).map(|l| &l.volume) else {
        return;
    };
    let mut changes = make(&ToolContext {
        volume,
        bounds: doc.data.bounds,
        color: doc.selected_color as Voxel,
        op,
        options: &options,
        hit: None,
    });
    // While slicing, edits stay in the visible part (hidden voxels are
    // never touched); overlaps follow the operation, e.g. Add overwrites.
    if let Some(clip) = doc.view_clip() {
        changes.retain(|c| clip.contains(c.pos));
    }
    doc.edit_voxels(layer, &changes);
}

/// Like [`write`] but reverts the drag's previous result first (tools that
/// recompute from the anchor).
fn rewrite(
    state: &mut AppState,
    layer: usize,
    op: VoxelOp,
    make: impl FnOnce(&ToolContext) -> Vec<VoxelChange>,
) {
    if let Some(doc) = state.doc_mut() {
        doc.revert_pending();
    }
    write(state, layer, op, make);
}

// ------------------------------------------------------------------ clicks

/// Color Picker (or `Alt`+click): selects the palette index under the
/// cursor; with `Ctrl` copies its color into the selected swatch instead.
fn pick_color(f: &Frame, state: &mut AppState, pos: Pos2, copy: bool) {
    let all = state.options.color_picker.sample_all_layers;
    let ray = f.ray(pos);
    let Some(doc) = state.doc_mut() else { return };
    let picked = if all {
        pick_doc(doc, &ray, false, false)
            .filter(is_voxel)
            .and_then(|hit| doc.with_visible_geometry(|vols| pick_voxel(vols, hit.voxel)))
    } else {
        doc.active_layer().and_then(|l| {
            let bounds = l.volume.bounds();
            raycast_grid(&ray, &VoxelBox::from_bounds(bounds), |p| {
                l.volume.get(p) != EMPTY_VOXEL
            })
            .map(|hit| l.volume.get(hit.voxel))
        })
    };
    let Some(index) = picked else {
        state.status_message = Some("No voxel to pick here".to_owned());
        return;
    };
    let index = index as usize;
    state.status_message = Some(if copy {
        let target = doc.selected_color;
        match doc.data.palette.get(index) {
            Some(rgb) => {
                doc.set_palette_color(target, rgb, false);
                format!("Copied {} into swatch {target}", rgb.to_hex_string())
            }
            None => "Picked color is outside the palette".to_owned(),
        }
    } else if doc.select_color(index) {
        format!("Picked color {index}")
    } else {
        "Picked color is outside the palette".to_owned()
    });
}

/// Plane tool without Slice: places the work plane on the clicked face
/// (or the floor) and shows it.
fn place_plane(state: &mut AppState, hit: Option<RayHit>) {
    let Some(doc) = state.doc_mut() else { return };
    let Some(hit) = hit else { return };
    doc.plane.set_from_hit(&hit);
    doc.plane.visible = true;
    state.status_message = Some(format!(
        "Work plane at {}",
        plane_label(&doc.plane, doc.data.bounds)
    ));
}

/// `X = 4` style label of the plane position.
pub fn plane_label(p: &WorkPlane, bounds: BoundSize) -> String {
    let axis = AXIS_NAMES[p.axis.min(2)];
    if p.slice {
        format!("{axis} layer {}", p.slice_layer(bounds))
    } else {
        format!("{axis} = {}", p.coord)
    }
}

// ---------------------------------------------------------------- drawing

fn color32(c: rizxel_math::Rgba8) -> Color32 {
    Color32::from_rgb(c.r, c.g, c.b)
}

/// Keeps ghost previews cheap: clipped to the editable area and capped.
fn ghost_cells(cells: Vec<IVec3>, area: &VoxelBox) -> Vec<IVec3> {
    if cells.len() > MAX_GHOST_CELLS {
        return Vec::new();
    }
    cells.into_iter().filter(|p| area.contains(*p)).collect()
}

/// Corners of the face of `voxel` facing `normal`, slightly lifted.
fn face_corners(bmin: Vec3, voxel: IVec3, normal: IVec3) -> [Vec3; 4] {
    let axis = dominant_axis(normal.as_vec3());
    let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
    let mut origin = voxel.as_vec3();
    origin[axis] += normal[axis].max(0) as f32 + normal[axis] as f32 * SURFACE_EPS;
    let (mut du, mut dv) = (Vec3::ZERO, Vec3::ZERO);
    du[u] = 1.0;
    dv[v] = 1.0;
    [origin, origin + du, origin + du + dv, origin + dv].map(|p| p + bmin)
}

fn quad(out: &mut Preview, q: [Vec3; 4], color: Color32) {
    out.fills
        .extend([0, 1, 2, 0, 2, 3].map(|i| FillVertex::new(q[i], color)));
}

fn face_fill(bmin: Vec3, hit: &RayHit, color: Color32, out: &mut Preview) {
    quad(out, face_corners(bmin, hit.voxel, hit.normal), color);
}

/// Hovered face: translucent accent quad plus outline.
fn face_highlight(bmin: Vec3, hit: &RayHit, ppp: f32, out: &mut Preview) {
    let q = face_corners(bmin, hit.voxel, hit.normal);
    quad(out, q, theme::ACCENT_LIGHT.gamma_multiply(0.35));
    for i in 0..4 {
        out.lines.push(LineInstance::new(
            q[i],
            q[(i + 1) % 4],
            theme::ACCENT_LIGHT,
            ppp,
        ));
    }
}

/// Translucent cells; faces shared by two ghost cells are skipped so the
/// ghost reads as one solid.
fn ghost(bmin: Vec3, cells: &[IVec3], color: Color32, out: &mut Preview) {
    let set: HashSet<IVec3> = cells.iter().copied().collect();
    const NORMALS: [IVec3; 6] = [
        IVec3::X,
        IVec3::NEG_X,
        IVec3::Y,
        IVec3::NEG_Y,
        IVec3::Z,
        IVec3::NEG_Z,
    ];
    for &c in cells {
        for n in NORMALS {
            if !set.contains(&(c + n)) {
                quad(out, face_corners(bmin, c, n), color);
            }
        }
    }
}

/// Axis color of a plane normal (X red, Y green, Z blue).
pub fn axis_color(axis: usize) -> Color32 {
    [theme::AXIS_X, theme::AXIS_Y, theme::AXIS_Z][axis.min(2)]
}

/// Corners of the work plane rectangle (voxel space), at its coordinate.
fn plane_corners(p: &WorkPlane, bounds: BoundSize, at: f32) -> [Vec3; 4] {
    let size = bounds.0.as_vec3();
    let axis = p.axis.min(2);
    let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
    let corner = |b: f32, c: f32| {
        let mut q = Vec3::ZERO;
        q[axis] = at;
        q[u] = b;
        q[v] = c;
        q
    };
    [
        corner(0.0, 0.0),
        corner(size[u], 0.0),
        corner(size[u], size[v]),
        corner(0.0, size[v]),
    ]
}

/// Point (voxel space) of the active plane's outline within
/// [`EDGE_GRAB`] points of the cursor, nearest first.
fn plane_edge_at(f: &Frame, doc: &Document, pos: Pos2) -> Option<Vec3> {
    let bounds = doc.data.bounds;
    let p = doc.plane.clamped(bounds);
    if !p.active() {
        return None;
    }
    let q = plane_corners(&p, bounds, p.coord as f32);
    let cursor = Vec2::new(pos.x, pos.y);
    let mut best: Option<(f32, Vec3)> = None;
    for i in 0..4 {
        let (a, b) = (q[i], q[(i + 1) % 4]);
        let (Some(sa), Some(sb)) = (f.screen(a), f.screen(b)) else {
            continue;
        };
        let (sa, sb) = (Vec2::new(sa.x, sa.y), Vec2::new(sb.x, sb.y));
        let ab = sb - sa;
        let t = ((cursor - sa).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
        let d = (sa + ab * t).distance(cursor);
        if d <= EDGE_GRAB && best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, a.lerp(b, t)));
        }
    }
    best.map(|(_, point)| point)
}

/// Work plane drawing (`docs/Slice panel feature.png`): one translucent
/// quad over the bounds' cross-section with an outline in the axis color
/// (grey and thicker while slicing). The gizmo arrow is drawn only at
/// `arrow_at` (a hovered or grabbed edge point, voxel space).
fn work_plane_overlay(f: &Frame, doc: &Document, arrow_at: Option<Vec3>, out: &mut Preview) {
    let bounds = doc.data.bounds;
    let p = doc.plane.clamped(bounds);
    if !p.active() {
        return;
    }
    let axis = p.axis;
    let color = axis_color(axis);
    // While slicing, lift toward the hidden side so the quad never fights
    // the voxel faces at the plane.
    let lift = if p.slice {
        p.hidden_sign() as f32 * 0.02
    } else {
        0.0
    };
    let q = plane_corners(&p, bounds, p.coord as f32 + lift).map(|c| c + f.bmin);
    let (fill, width) = if p.slice {
        (
            Color32::from_rgba_unmultiplied(170, 170, 170, 70),
            PLANE_SLICE_WIDTH,
        )
    } else {
        (color.gamma_multiply(0.12), PLANE_WIDTH)
    };
    quad(out, q, fill);
    let ppp = f.pixels_per_point;
    for i in 0..4 {
        out.lines
            .push(LineInstance::new(q[i], q[(i + 1) % 4], color, width * ppp));
    }
    if let Some(point) = arrow_at {
        // Gizmo arrow (`docs/gizmo arrow.jpg`) pointing where a positive
        // drag moves the plane.
        let mut base = point;
        base[axis] = p.coord as f32;
        let base = base + f.bmin;
        let mut dir = Vec3::ZERO;
        dir[axis] = p.hidden_sign() as f32;
        let scale = 1.0 / f.proj.points_per_unit(base).max(1e-3);
        arrow(
            out,
            base,
            dir,
            ARROW_LENGTH * scale,
            scale,
            color,
            f.proj.eye(),
        );
    }
}

/// Hover distance (logical points) at which the plane edge can be grabbed.
const EDGE_GRAB: f32 = 10.0;
/// Outline width of the visible work plane, in logical points.
const PLANE_WIDTH: f32 = 1.5;
/// Outline width of the slice plane, in logical points.
const PLANE_SLICE_WIDTH: f32 = 3.0;
/// Arrow gizmo size, in logical points (constant on screen).
const ARROW_LENGTH: f32 = 72.0;
const ARROW_SHAFT_RADIUS: f32 = 3.0;
const ARROW_HEAD_RADIUS: f32 = 8.0;
const ARROW_HEAD_LENGTH: f32 = 20.0;
/// Segments around the shaft and cone.
const ARROW_SEGMENTS: usize = 20;

/// Solid shaded arrow: a cylinder shaft and a cone head along `dir`, built
/// from fill triangles. `scale` converts logical points to world units at
/// `base`, so the arrow keeps a constant on-screen size. Fills do not write
/// depth, so back faces are culled here and the two parts are emitted far
/// to near.
fn arrow(
    out: &mut Preview,
    base: Vec3,
    dir: Vec3,
    len: f32,
    scale: f32,
    color: Color32,
    eye: Vec3,
) {
    let d = dir.normalize_or(Vec3::Y);
    let e1 = d.any_orthonormal_vector();
    let e2 = d.cross(e1);
    let ring = |i: usize| {
        let a = i as f32 / ARROW_SEGMENTS as f32 * std::f32::consts::TAU;
        e1 * a.cos() + e2 * a.sin()
    };
    let light = Vec3::new(0.4, 0.8, 0.45).normalize();
    let shade = |n: Vec3| {
        let k = 0.45 + 0.55 * n.dot(light).max(0.0);
        let c = |v: u8| (v as f32 * k).round().clamp(0.0, 255.0) as u8;
        Color32::from_rgb(c(color.r()), c(color.g()), c(color.b()))
    };
    let mut tri = |verts: [(Vec3, Vec3); 3]| {
        let center = (verts[0].0 + verts[1].0 + verts[2].0) / 3.0;
        let face = (verts[1].0 - verts[0].0).cross(verts[2].0 - verts[0].0);
        if face.dot(eye - center) <= 0.0 {
            return;
        }
        for (p, n) in verts {
            out.fills.push(FillVertex::new(p, shade(n)));
        }
    };

    let r_shaft = ARROW_SHAFT_RADIUS * scale;
    let r_head = ARROW_HEAD_RADIUS * scale;
    let head_len = (ARROW_HEAD_LENGTH * scale).min(len * 0.6);
    let neck = base + d * (len - head_len);
    let tip = base + d * len;

    let shaft = |tri: &mut dyn FnMut([(Vec3, Vec3); 3])| {
        for i in 0..ARROW_SEGMENTS {
            let (r0, r1) = (ring(i), ring(i + 1));
            let (a0, a1) = (base + r0 * r_shaft, base + r1 * r_shaft);
            let (b0, b1) = (neck + r0 * r_shaft, neck + r1 * r_shaft);
            tri([(a0, r0), (a1, r1), (b1, r1)]);
            tri([(a0, r0), (b1, r1), (b0, r0)]);
            tri([(base, -d), (a1, -d), (a0, -d)]);
        }
    };
    let head = |tri: &mut dyn FnMut([(Vec3, Vec3); 3])| {
        // Cone side normals lean toward the tip.
        let slope = r_head / head_len.max(1e-6);
        for i in 0..ARROW_SEGMENTS {
            let (r0, r1) = (ring(i), ring(i + 1));
            let (c0, c1) = (neck + r0 * r_head, neck + r1 * r_head);
            let n0 = (r0 + d * slope).normalize();
            let n1 = (r1 + d * slope).normalize();
            let nt = (ring(i) + ring(i + 1)).normalize_or(r0) + d * slope;
            tri([(c0, n0), (c1, n1), (tip, nt.normalize())]);
            tri([(neck, -d), (c1, -d), (c0, -d)]);
        }
    };
    if d.dot(eye - base) >= 0.0 {
        shaft(&mut tri);
        head(&mut tri);
    } else {
        head(&mut tri);
        shaft(&mut tri);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face_corners_sit_on_the_hit_face() {
        let q = face_corners(Vec3::ZERO, IVec3::new(2, 3, 4), IVec3::Y);
        assert!(q.iter().all(|p| (p.y - (4.0 + SURFACE_EPS)).abs() < 1e-6));
        let q = face_corners(Vec3::ZERO, IVec3::new(2, 3, 4), IVec3::NEG_X);
        assert!(q.iter().all(|p| (p.x - (2.0 - SURFACE_EPS)).abs() < 1e-6));
    }

    #[test]
    fn arrow_culls_back_faces() {
        let mut out = Preview::default();
        let eye = Vec3::new(0.0, 0.0, 100.0);
        arrow(&mut out, Vec3::ZERO, Vec3::X, 10.0, 0.1, Color32::RED, eye);
        let n = out.fills.len();
        assert!(n > 0 && n % 3 == 0);
        // About half of the closed surface faces the camera.
        let total = ARROW_SEGMENTS * 5 * 3;
        assert!(n < total, "{n} of {total}");
        assert!(out
            .fills
            .iter()
            .all(|v| v.pos[0] >= -1e-4 && v.pos[0] <= 10.0 + 1e-4));
    }

    #[test]
    fn slicing_edits_the_slab_and_visible_voxels_on_both_sides() {
        let bounds = BoundSize::new(8, 8, 8);
        let mut plane = WorkPlane {
            axis: 0,
            slice: true,
            ..WorkPlane::default()
        };
        plane.set_slice_layer(2, bounds);
        // Slice layer 2 plus layer 3 beyond it stay visible.
        assert_eq!(plane.clip_box(bounds).unwrap().max.x, 4);
        let from_right = Ray::new(Vec3::new(20.0, 3.5, 3.5), Vec3::NEG_X);
        let from_left = Ray::new(Vec3::new(-20.0, 3.5, 3.5), Vec3::X);
        let target = |hit: RayHit, additive: bool| {
            if additive {
                hit.adjacent()
            } else {
                hit.voxel
            }
        };

        // Empty row: the slab is reachable from both sides.
        for ray in [from_right, from_left] {
            for additive in [true, false] {
                let hit = pick_with(&ray, bounds, &plane, true, additive, |_| false).unwrap();
                assert_eq!(target(hit, additive), IVec3::new(2, 3, 3));
            }
        }

        // Voxels at x = 0 (behind the plane) and x = 3 (one layer beyond):
        // both are visible and editable from their side.
        let mut solid = VoxelVolume::new(bounds);
        for x in [0, 3, 6] {
            solid.set(IVec3::new(x, 3, 3), 1);
        }
        let is_solid = |p: IVec3| solid.get(p) != EMPTY_VOXEL;
        let hit = pick_with(&from_right, bounds, &plane, true, false, is_solid).unwrap();
        assert_eq!(hit.voxel, IVec3::new(3, 3, 3), "x = 6 is hidden");
        let hit = pick_with(&from_left, bounds, &plane, true, false, is_solid).unwrap();
        assert_eq!(hit.voxel, IVec3::new(0, 3, 3));
        let hit = pick_with(&from_left, bounds, &plane, true, true, is_solid).unwrap();
        assert_eq!(
            hit.adjacent(),
            IVec3::new(-1, 3, 3),
            "outside: filtered later"
        );
    }

    #[test]
    fn ghost_skips_shared_faces() {
        let mut out = Preview::default();
        ghost(
            Vec3::ZERO,
            &[IVec3::ZERO, IVec3::X],
            Color32::WHITE,
            &mut out,
        );
        assert_eq!(out.fills.len(), 10 * 6, "two cubes share one face pair");
        let area = VoxelBox::from_bounds(BoundSize::default());
        assert!(ghost_cells(vec![IVec3::ZERO; MAX_GHOST_CELLS + 1], &area).is_empty());
    }
}

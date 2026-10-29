//! Viewport tool input: voxel picking, the left-drag state machine, the
//! hover/ghost previews and the selection outline.
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
//! swatch. With the selection tools `Shift` adds to the selection and `Ctrl`
//! subtracts from it (overriding the option bar's mask mode).
//!
//! Selections (Phase 5b) belong to the active layer. Move and Extrude act on
//! the selection when there is one and carry it along in the same undo
//! step; Fill stays inside it. `Esc` deselects, `Delete` removes the
//! selected voxels. In Mask Mode (`Shift+M`) the selection is protected
//! instead: every tool edits the whole layer around it.
//!
//! The Plane tool never writes voxels (Goxel / Qubicle behaviour): a click
//! places the document's [`WorkPlane`] on the clicked face. While the plane
//! is visible every drawing tool also picks on it, so voxels can be drawn
//! in empty space.
//!
//! Slice Mode (Qubicle 3.1.2) is global: `Space` toggles it from any tool,
//! slicing at the hovered voxel (axis = hovered face normal) or restoring
//! the last slice. While slicing only the slice layer can be picked and
//! edited, by every tool; dragging the thick bounds frame (any tool) or
//! anywhere with the Plane tool moves the slice along its normal. Slice
//! moves are view state, never undo steps.
//!
//! The Selection tool's *Box* mode (Phase 10c, [`box_select`]) leaves the
//! dragged box open: its face arrows and faces move it, `Enter` (or a
//! click outside it, or switching tools) applies it, `Esc` cancels it.
//! The Line tool ([`line`]) keeps its own open line across clicks, and the
//! Transform tool ([`transform`]) its gizmo drags.

mod box_select;
mod line;
mod transform;

use std::collections::HashSet;

use egui::{Color32, Key, Modifiers, Pos2, Rect, Response, Ui};
use glam::{IVec3, Vec2, Vec3};
use rizxel_app::{AppState, Document, Selection, VoxelChange};
use rizxel_engine::raycast::{dominant_axis, raycast_face, raycast_floor, raycast_grid};
use rizxel_engine::tools::{
    apply_op, brush_center, brush_positions, brush_stroke, connect, drag_axis, extrude_changes,
    extrude_region, extruded_cells, fill_cells, fuzzy_select, line_cells, line_stroke, mirror,
    move_changes, move_selection_changes, pick_voxel, resize_bounds, select_box, selection_box,
    selection_region, shape_box, shape_cells_toward, MAX_FLOOD_CELLS,
};
use rizxel_engine::{
    Connectivity, DragMode, ExtrudeOptions, FillMode, MaskMode, Ray, RayHit, Symmetry, Tool,
    ToolContext, VoxelOp, WorkPlane,
};
use rizxel_math::{BoundSize, Voxel, VoxelBox, VoxelVolume, EMPTY_VOXEL};

use crate::render::{FillVertex, LineInstance};
use crate::theme;
use crate::viewport::Projection;

/// Ghost previews with more cells than this fall back to the face highlight.
const MAX_GHOST_CELLS: usize = 4096;
/// Selection outlines with more faces than this draw only the selection's box.
const MAX_SELECTION_FACES: usize = 20_000;
/// Lift of highlight quads off the voxel surface, against z-fighting.
const SURFACE_EPS: f32 = 0.01;

/// Overlay geometry produced by the active tool, in world space.
#[derive(Default, Clone)]
pub struct Preview {
    pub fills: Vec<FillVertex>,
    pub lines: Vec<LineInstance>,
}

/// Persistent tool-input state (owned by the app across frames).
#[derive(Default)]
pub struct ToolInput {
    drag: Option<Drag>,
    /// Extrude hover region cache.
    region_cache: Option<(RegionKey, Vec<IVec3>)>,
    /// Selection outline cache: geometry and whether it fell back to the box.
    selection_cache: Option<(SelectionKey, Preview, bool)>,
    /// Pick target while a floating paste follows the cursor: the model
    /// without the paste, keyed by `(document id, edit count)`.
    paste_pick: Option<((u64, u64), VoxelVolume)>,
    /// Open line, polyline or curve of the Line tool.
    line: Option<line::LineSession>,
    /// Last brush stamp `(document id, layer, center)`: `Ctrl`+click draws
    /// a line from it.
    last_stamp: Option<(u64, usize, IVec3)>,
    /// Transform gizmo drag in progress.
    transform: Option<transform::TransformDrag>,
}

/// `(document id, revision, hit voxel, hit normal, options, selection
/// revision)`: any change invalidates the hover region, nothing else.
type RegionKey = (u64, u64, IVec3, IVec3, ExtrudeOptions, u64);

/// `(document id, layer, selection revision, size, bounds origin, pixels
/// per point)`.
type SelectionKey = (u64, usize, u64, usize, [f32; 3], f32);

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
        /// Stamp centers so far, when the stroke is redrawn as one
        /// connected path (size 1 with Pixel perfect or Solid).
        path: Option<Vec<IVec3>>,
    },
    /// Plane tool with Slice on: moves the plane along its normal.
    PlaneMove {
        pivot: Vec3,
        /// Point of the plane edge that was grabbed (voxel space), where the
        /// arrow hint follows the plane; `None` for a drag started elsewhere.
        grab: Option<Vec3>,
        /// Slice layer (while slicing) or plane coordinate at press time.
        start: i32,
        moved: bool,
        /// Face under the press; a click without drag moves the plane there.
        click: Option<RayHit>,
    },
    Move {
        /// Voxel-space point whose screen axes measure the drag.
        pivot: Vec3,
        offset: IVec3,
        /// Offset under the pointer; differs from `offset` while a move of
        /// the selection is blocked by other voxels.
        target: IVec3,
        /// Selection being moved (otherwise the whole layer moves).
        selected: Option<Selection>,
    },
    /// Selection tool: box between the pressed voxel and the hovered one.
    Select {
        anchor: IVec3,
        /// Axis of the pressed face; off the model the drag projects onto it.
        axis: usize,
        current: IVec3,
        mode: MaskMode,
        moved: bool,
        /// Box mode: the drag sizes the document's open box.
        boxed: bool,
    },
    /// Box mode: a face arrow being dragged along its axis.
    BoxFace {
        axis: usize,
        positive: bool,
        start: VoxelBox,
        /// Face center (voxel space) whose screen axis measures the drag.
        pivot: Vec3,
    },
    /// Box mode: the box dragged by a face, in its plane or (`Alt`) along
    /// its normal.
    BoxMove {
        axis: usize,
        start: VoxelBox,
        /// Grabbed point on the face (voxel space).
        from: Vec3,
        along_normal: bool,
    },
    /// Shape tool: shape between the anchor and the hovered cell.
    Shape {
        anchor: IVec3,
        /// Direction the shape grows in from the anchor's plane.
        grow: IVec3,
        area: Option<VoxelBox>,
        /// The last area exceeded the cell cap and was not written.
        too_large: bool,
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
        /// Selection being extruded; it grows with the new cells.
        selection: Option<Selection>,
    },
}

/// Everything the tool code needs about the current frame.
pub struct Frame<'a> {
    pub ui: &'a Ui,
    pub response: &'a Response,
    pub proj: &'a Projection,
    /// World position of voxel `(0, 0, 0)`'s minimum corner.
    pub bmin: Vec3,
    /// Screen areas owned by overlays (view cube, slice slider); presses
    /// there are ignored.
    pub blocked: &'a [Rect],
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

/// Shared pick: the nearest of the first solid cell inside the editable
/// region and the work plane (when active; while slicing, the slice slab,
/// reachable from either side), then the floor. While slicing the editable
/// region is the slice layer alone, so voxels off the slice are never
/// picked (Qubicle Slice Mode).
fn pick_with(
    ray: &Ray,
    bounds: BoundSize,
    plane: &WorkPlane,
    use_plane: bool,
    additive: bool,
    solid: impl FnMut(IVec3) -> bool,
) -> Option<RayHit> {
    let clip = plane.slice_box(bounds);
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

/// Plane tool pick: the first visible voxel, on or off the slice (a click
/// jumps the slice there).
fn pick_view(doc: &Document, ray: &Ray) -> Option<RayHit> {
    let area = doc
        .view_clip()
        .unwrap_or(VoxelBox::from_bounds(doc.data.bounds));
    doc.with_visible_geometry(|vols| {
        raycast_grid(ray, &area, |p| vols.iter().any(|v| v.get(p) != EMPTY_VOXEL))
    })
    .or_else(|| raycast_floor(ray, doc.data.bounds).filter(|h| area.contains(h.adjacent())))
}

/// Picks against the visible geometry of `doc` (the slice layer only while
/// slicing).
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
    matches!(tool, Tool::Brush | Tool::Line | Tool::Cursor | Tool::Shape)
}

/// Mask mode of a selection click: `Shift` adds, `Ctrl` subtracts,
/// otherwise the option bar's mode.
pub fn mask_mode(mods: Modifiers, option: MaskMode) -> MaskMode {
    if mods.shift {
        MaskMode::Add
    } else if mods.command {
        MaskMode::Subtract
    } else {
        option
    }
}

/// Pickable model of `doc` (the slice layer while slicing): the pick
/// target during a drag.
fn pick_snapshot(doc: &Document) -> VoxelVolume {
    match doc.edit_clip() {
        Some(clip) => doc.merged_visible_volume().cropped(&clip),
        None => doc.merged_visible_volume(),
    }
}

/// Cell under `ray` on the plane through the centers of the cells at
/// `anchor[axis]`, clamped into `bounds`.
fn cell_on_plane(ray: &Ray, anchor: IVec3, axis: usize, bounds: BoundSize) -> Option<IVec3> {
    let point = raycast_face(ray, axis, anchor[axis] as f32 + 0.5)?;
    let mut cell = point.floor().as_ivec3();
    cell[axis] = anchor[axis];
    Some(cell.clamp(IVec3::ZERO, bounds.0.as_ivec3() - IVec3::ONE))
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
    /// A tool drag is in progress.
    pub fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// Runs the active tool for this frame and returns its overlays.
    pub fn update(&mut self, f: &Frame, state: &mut AppState) -> Preview {
        let mut preview = Preview::default();
        state.tool_feedback = None;
        let Some(doc_id) = state.doc().map(Document::id) else {
            self.drag = None;
            return preview;
        };

        // A floating paste takes the viewport input until it is placed.
        if state.doc().is_some_and(|d| d.floating_paste().is_some()) {
            self.drag = None;
            self.paste_frame(f, state, &mut preview);
            return preview;
        }
        self.paste_pick = None;

        // Leaving the Selection tool (or its Box mode) applies the open box.
        if !box_select::box_mode(state) && has_open_box(state) {
            box_select::apply(state, MaskMode::Set);
        }
        if let Some(doc) = state.doc_mut() {
            doc.sync_box_select();
        }
        self.sync_line(state);
        self.sync_transform(state);

        // A drag survives only on its own document and while its
        // transaction is open (undo or a tab switch may have closed it).
        if let Some(drag) = &self.drag {
            let boxed = matches!(
                drag.kind,
                DragKind::BoxFace { .. } | DragKind::BoxMove { .. }
            );
            let alive = drag.doc_id == doc_id
                && (matches!(
                    drag.kind,
                    DragKind::Resize { .. } | DragKind::PlaneMove { .. } | DragKind::Select { .. }
                ) || (boxed && has_open_box(state))
                    || state.doc().is_some_and(Document::is_editing));
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
        } else if !f.ui.ctx().egui_wants_keyboard_input() {
            self.transform_keys(f, state);
            self.line_keys(f, state);
            box_select::keys(f, state, mods);
            selection_keys(f, state);
        }
        // The open box previews the mode of the modifiers held now.
        if self.drag.is_none() && box_select::box_mode(state) && !mods.alt {
            let mode = mask_mode(mods, state.options.selection.mask);
            if let Some(doc) = state.doc_mut() {
                doc.set_box_mode(mode);
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
        let pick = |state: &AppState| match (&self.drag, hover) {
            (Some(d), Some(p)) => pick_drag(d, &f.ray(p)),
            (None, Some(p)) if tool == Tool::Plane => {
                state.doc().and_then(|d| pick_view(d, &f.ray(p)))
            }
            (None, Some(p)) => state
                .doc()
                .and_then(|d| pick_doc(d, &f.ray(p), draws_on_plane(tool), is_additive(op))),
            _ => None,
        };
        let mut hit = pick(state);

        // Space (keymap): Slice Mode on at the hovered voxel (or the last
        // slice), or off again. Works with every tool and keeps it.
        let space = self.drag.is_none()
            && !f.ui.ctx().egui_wants_keyboard_input()
            && f.ui.input_mut(|i| {
                crate::keys::consume(i, &state.settings.keymap, rizxel_app::ActionId::SliceMode)
            });
        if space {
            state.toggle_slice(hit.filter(is_voxel).as_ref());
            // The pickable region changed.
            hit = pick(state);
        }
        state.cursor_voxel = hit.map(|h| if is_voxel(&h) { h.voxel } else { h.adjacent() });

        // Box mode: a face arrow or the surface of the box under the cursor.
        let box_grab = match (&self.drag, hover, state.doc()) {
            (None, Some(p), Some(doc)) if box_select::box_mode(state) => {
                box_select::grab_at(f, doc, p)
            }
            _ => None,
        };
        // Plane edge or (while slicing, any tool) bounds frame under the
        // cursor: grab handle for moving the plane.
        let edge = match (&self.drag, hover, state.doc()) {
            (None, Some(p), Some(doc))
                if box_grab.is_none() && (tool == Tool::Plane || doc.plane.slice) =>
            {
                plane_edge_at(f, doc, p)
            }
            _ => None,
        };

        let press = pressed
            && self.drag.is_none()
            && f.response.hovered()
            && hover.is_some_and(|p| !f.blocked.iter().any(|r| r.contains(p)));
        // The Line tool handles its own presses and drags (the plane edge
        // still moves the plane).
        let line_tool = state.tool == Tool::Line && !mods.alt && self.drag.is_none();
        if line_tool {
            self.line_frame(f, state, press && edge.is_none(), mods, &mut preview);
        }
        // So does the Transform tool (`Alt` is one of its drag modifiers).
        let transform_tool = state.tool == Tool::Transform && self.drag.is_none();
        if transform_tool {
            self.transform_frame(f, state, press && edge.is_none(), mods, &mut preview);
        }
        let own_input = line_tool || transform_tool;
        if press && (!own_input || edge.is_some()) {
            if let Some(p) = hover {
                match box_grab {
                    Some(grab) => self.begin_box_drag(state, p, grab, mods),
                    None => self.press(f, state, p, hit, edge, mods),
                }
            }
        }

        if let Some(drag) = &self.drag {
            let pending = state.doc().map_or(0, Document::pending_changes);
            state.tool_feedback = Some(match drag.kind {
                DragKind::Resize { .. } => {
                    let b = state.doc().map(|d| d.data.bounds).unwrap_or_default();
                    format!("Resize: bounds {}×{}×{}", b.x(), b.y(), b.z())
                }
                DragKind::Move { offset, target, .. } if offset != target => format!(
                    "Move: blocked by other voxels at ({}, {}, {}) (enable Overwrite)",
                    target.x, target.y, target.z
                ),
                DragKind::Move { offset, .. } => format!(
                    "Move: offset ({}, {}, {}), {pending} voxels changed",
                    offset.x, offset.y, offset.z
                ),
                DragKind::Select { boxed: true, .. }
                | DragKind::BoxFace { .. }
                | DragKind::BoxMove { .. } => state
                    .doc()
                    .and_then(box_select::feedback)
                    .unwrap_or_default(),
                DragKind::Select {
                    anchor,
                    current,
                    mode,
                    ..
                } => {
                    let s = selection_box(anchor, current).size();
                    format!("Select ({}): box {}×{}×{}", mode.label(), s.x, s.y, s.z)
                }
                DragKind::Shape {
                    too_large: true, ..
                } => format!("Shape: too large (over {MAX_FLOOD_CELLS} cells)"),
                DragKind::Shape { area, .. } => {
                    let s = area.map_or(IVec3::ONE, |a| a.size());
                    format!("Shape: {}×{}×{}, {pending} voxels changed", s.x, s.y, s.z)
                }
                DragKind::Extrude { depth, .. } => {
                    format!("Extrude: depth {depth}, {pending} voxels changed")
                }
                DragKind::PlaneMove { .. } => state.doc().map_or_else(String::new, |d| {
                    let what = if d.plane.slice { "Slice" } else { "Plane" };
                    format!("{what} {}", plane_label(&d.plane, d.data.bounds))
                }),
                _ => format!("{}: {pending} voxels changed", drag.tool.label()),
            });
        } else if edge.is_some() {
            let slicing = state.doc().is_some_and(|d| d.plane.slice);
            state.tool_feedback = Some(if slicing {
                "Drag the frame to move the slice".to_owned()
            } else {
                "Drag the plane edge to move the plane".to_owned()
            });
        } else if let Some(grab) = box_grab {
            f.ui.ctx().set_cursor_icon(match grab {
                box_select::BoxGrab::Face { .. } => egui::CursorIcon::Grab,
                box_select::BoxGrab::Body { .. } => egui::CursorIcon::Move,
            });
            state.tool_feedback = Some(
                state
                    .doc()
                    .and_then(box_select::feedback)
                    .unwrap_or_else(|| "Drag an arrow or a face to re-open the box".to_owned()),
            );
        } else if let Some(hit) =
            hit.filter(|_| !transform_tool && !(line_tool && self.line.is_some()))
        {
            self.hover_preview(f, state, &hit, mods, &mut preview);
        }
        // An open box reports itself wherever the pointer is.
        if self.drag.is_none() && box_grab.is_none() && box_select::box_mode(state) {
            if let Some(text) = state.doc().and_then(box_select::feedback) {
                state.tool_feedback = Some(text);
            }
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
        self.selection_overlay(f, state, &mut preview);
        if let Some(doc) = state.doc().filter(|_| box_select::box_mode(state)) {
            box_select::overlay(f, doc, box_grab, &mut preview);
        }
        if let Some(drag) = &self.drag {
            drag_marquee(f, drag, &mut preview);
        }
        preview
    }

    // ---------------------------------------------------------------- paste

    /// Floating paste: follows the cursor (Paste) or stays put (Paste in
    /// Place); a click or `Enter` places it, `Esc` cancels it.
    fn paste_frame(&mut self, f: &Frame, state: &mut AppState, out: &mut Preview) {
        let (pressed, focused) = f.ui.input(|i| {
            (
                i.pointer.button_pressed(egui::PointerButton::Primary),
                i.focused,
            )
        });
        let keys = !f.ui.ctx().egui_wants_keyboard_input();
        let (enter, esc) = f.ui.input_mut(|i| {
            (
                keys && i.consume_key(Modifiers::NONE, Key::Enter),
                keys && i.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        if esc {
            state.cancel_paste();
            return;
        }
        let hover = f.response.hover_pos();
        let Some(doc) = state.doc_mut() else { return };
        let key = (doc.id(), doc.edit_count());
        if self.paste_pick.as_ref().is_none_or(|(k, _)| *k != key) {
            let snapshot = doc.without_paste(pick_snapshot);
            self.paste_pick = Some((key, snapshot));
        }
        let follows = doc.floating_paste().is_some_and(|p| p.follows_cursor);
        if let (true, Some(pos), Some((_, snap))) = (follows && focused, hover, &self.paste_pick) {
            let plane = doc.plane;
            let hit = pick_with(&f.ray(pos), snap.bounds(), &plane, true, true, |p| {
                snap.get(p) != EMPTY_VOXEL
            });
            let origin = hit.and_then(|h| doc.floating_paste().map(|p| p.origin_at(h.adjacent())));
            if let Some(origin) = origin {
                doc.move_paste(origin);
            }
        }
        if let Some(p) = doc.floating_paste() {
            let lo = p.origin.as_vec3() + f.bmin;
            box_outline(
                lo,
                lo + p.size.as_vec3(),
                theme::ACCENT_LIGHT,
                2.0 * f.pixels_per_point,
                out,
            );
            state.tool_feedback = Some(format!(
                "Paste at ({}, {}, {}): click or Enter to place, Esc to cancel",
                p.origin.x, p.origin.y, p.origin.z
            ));
        }
        let click = pressed
            && f.response.hovered()
            && hover.is_some_and(|p| !f.blocked.iter().any(|r| r.contains(p)));
        if enter || click {
            state.commit_paste();
        }
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
        if edge.is_some() {
            self.begin_plane_move(state, pos, hit, edge);
            return;
        }
        match tool {
            // The Line tool takes its presses in `line_frame`.
            // The Line and Transform tools take their presses in their
            // own frame functions.
            Tool::Cursor | Tool::Line | Tool::Transform => {}
            Tool::ColorPicker => pick_color(f, state, pos, mods.command),
            Tool::Plane => {
                let slicing = state.doc().is_some_and(|d| d.plane.slice);
                if slicing {
                    self.begin_plane_move(state, pos, hit, edge);
                } else {
                    place_plane(state, hit);
                }
            }
            Tool::Brush | Tool::Move | Tool::Extrude | Tool::Shape => {
                self.begin_drag(f, state, tool, pos, hit, mods)
            }
            Tool::Selection if box_select::box_mode(state) && has_open_box(state) => {
                // A click outside the open box applies it.
                let mode = mask_mode(mods, state.options.selection.mask);
                box_select::apply(state, mode);
            }
            Tool::Selection => self.begin_select(state, pos, hit, mods),
            Tool::FuzzySelect => fuzzy_click(state, hit, mods),
            Tool::Fill => fill_click(state, hit),
        }
    }

    /// Selection tool press: starts a box at the pressed voxel (in Box
    /// mode also on the floor, and the box stays open after the drag). A
    /// press off the model in Set mode deselects.
    fn begin_select(
        &mut self,
        state: &mut AppState,
        pos: Pos2,
        hit: Option<RayHit>,
        mods: Modifiers,
    ) {
        let mode = mask_mode(mods, state.options.selection.mask);
        let boxed = box_select::box_mode(state);
        let all_layers = state.options.selection.all_layers;
        let Some(doc) = state.doc_mut() else { return };
        // Box mode starts on the floor cell too.
        let hit = hit.filter(|h| boxed || is_voxel(h)).map(|h| {
            if is_voxel(&h) {
                h
            } else {
                RayHit {
                    voxel: h.adjacent(),
                    ..h
                }
            }
        });
        let Some(hit) = hit else {
            if mode == MaskMode::Set && doc.clear_selection() {
                state.status_message = Some("Selection cleared".to_owned());
            }
            return;
        };
        if boxed {
            doc.begin_box_select(selection_box(hit.voxel, hit.voxel), all_layers, mode);
        }
        // While slicing, the box is drawn on the slice.
        let axis = if doc.plane.slice {
            doc.plane.axis.min(2)
        } else {
            hit.axis()
        };
        self.drag = Some(Drag {
            doc_id: doc.id(),
            tool: Tool::Selection,
            layer: doc.active_layer,
            start: pos,
            snapshot: pick_snapshot(doc),
            plane: doc.plane,
            // Non-additive: picks report the voxel itself.
            op: VoxelOp::Paint,
            kind: DragKind::Select {
                anchor: hit.voxel,
                axis,
                current: hit.voxel,
                mode,
                moved: false,
                boxed,
            },
        });
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
        // Ctrl+click draws a line from the last stamp on this layer.
        let line_from = self
            .last_stamp
            .filter(|&(id, l, _)| mods.command && id == doc.id() && l == layer)
            .map(|(_, _, c)| c);
        let kind = match tool {
            Tool::Brush => {
                let Some(hit) = hit else { return };
                let center = brush_center(&hit, op);
                let brush = &options.brush;
                let connected = brush.single_voxel() && brush.connect != Connectivity::Diagonal;
                let start = match line_from {
                    Some(from) => line_cells(from, center),
                    None => vec![center],
                };
                DragKind::Brush {
                    last: center,
                    path: connected.then_some(start),
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
                target: IVec3::ZERO,
                selected: doc.region_selection().cloned(),
            },
            Tool::Shape => {
                let Some(hit) = hit else { return };
                let (anchor, mut grow) = if is_additive(op) {
                    (hit.adjacent(), hit.normal)
                } else {
                    (hit.voxel, -hit.normal)
                };
                // While slicing, the shape is drawn on the slice.
                if doc.plane.slice {
                    let axis = doc.plane.axis.min(2);
                    if grow[axis] == 0 {
                        grow = IVec3::ZERO;
                        grow[axis] = 1;
                    }
                }
                DragKind::Shape {
                    anchor,
                    grow,
                    area: None,
                    too_large: false,
                }
            }
            Tool::Extrude => {
                let Some(hit) = hit.filter(is_voxel) else {
                    return;
                };
                let colors = if options.extrude.confine_to_layer {
                    doc.data.layers[layer].volume.clone()
                } else {
                    doc.merged_visible_volume()
                };
                let selection = doc.region_selection().cloned();
                let region = match &selection {
                    Some(sel) => selection_region(&colors, sel.cells(), hit.normal),
                    None => extrude_region(&colors, &hit, &options.extrude, &doc.data.palette),
                };
                if region.is_empty() {
                    state.status_message = Some(match selection {
                        Some(_) => "The selection has no exposed face this way".to_owned(),
                        None => "Nothing to extrude here".to_owned(),
                    });
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
                    selection,
                }
            }
            _ => return,
        };
        if !resize {
            doc.begin_edit();
        }
        let snapshot = pick_snapshot(doc);
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
        // Apply the press itself (first stamp or the Ctrl+click line, 1×1
        // plane, base extrude).
        if let Some(DragKind::Brush { last, path }) = self.drag.as_ref().map(|d| &d.kind) {
            let last = *last;
            let connect_mode = options.brush.connect;
            match (path, line_from) {
                (Some(path), _) => {
                    let p = connect(path.clone(), connect_mode);
                    write(state, layer, op, |ctx| line_stroke(ctx, &p));
                }
                (None, Some(from)) => {
                    write(state, layer, op, |ctx| brush_stroke(ctx, Some(from), last));
                }
                (None, None) => write(state, layer, op, |ctx| brush_stroke(ctx, None, last)),
            }
        }
        self.drag_frame(f, state, pos);
    }

    /// Starts dragging the plane along its normal: from a grabbed edge (or
    /// bounds frame while slicing), or from anywhere with the Plane tool
    /// while slicing.
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
            c[p.axis] = p.sheet_coord(bounds) as f32;
            c
        });
        // Slices move by layer (the hidden side may flip on the way).
        let start = if p.slice {
            p.slice_layer(bounds)
        } else {
            p.coord
        };
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
                start,
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
        let bounds = drag.snapshot.bounds();
        match &mut drag.kind {
            DragKind::Select {
                anchor,
                axis,
                current,
                moved,
                boxed,
                ..
            } => {
                *moved |= delta.length() > 3.0;
                let on_model = pick_with(&ray, bounds, &drag.plane, true, false, |p| {
                    drag.snapshot.get(p) != EMPTY_VOXEL
                })
                .filter(is_voxel)
                .map(|h| h.voxel);
                if let Some(cell) = on_model.or_else(|| cell_on_plane(&ray, *anchor, *axis, bounds))
                {
                    *current = cell;
                }
                if *boxed {
                    if let Some(doc) = state.doc_mut() {
                        doc.set_box_area(selection_box(*anchor, *current), false);
                    }
                }
            }
            DragKind::BoxFace {
                axis,
                positive,
                start,
                pivot,
            } => box_select::drag_face(f, state, delta, (*axis, *positive, *start, *pivot)),
            DragKind::BoxMove {
                axis,
                start,
                from,
                along_normal,
            } => {
                box_select::drag_body(f, state, &ray, delta, (*axis, *start, *from, *along_normal))
            }
            DragKind::Shape {
                anchor,
                grow,
                area,
                too_large,
            } => {
                let axis = dominant_axis(grow.as_vec3());
                let Some(cell) = cell_on_plane(&ray, *anchor, axis, bounds) else {
                    return;
                };
                let planar = state.options.shape.planar;
                let new = shape_box(*anchor, cell, *grow, planar);
                if *area == Some(new) {
                    return;
                }
                *area = Some(new);
                let cells = new.size().as_i64vec3().element_product();
                *too_large = cells > MAX_FLOOD_CELLS as i64;
                if *too_large {
                    if let Some(doc) = state.doc_mut() {
                        doc.revert_pending();
                    }
                    return;
                }
                let up = *grow;
                rewrite(state, layer, op, |ctx| {
                    let cells = shape_cells_toward(&new, &ctx.options.shape, ctx.bounds, up);
                    apply_op(ctx.volume, cells, ctx.op, ctx.color)
                });
            }
            DragKind::Brush { last, path } => {
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
                match path {
                    // The whole stroke is redrawn so corners can go.
                    Some(path) => {
                        path.extend(line_cells(from, center).into_iter().skip(1));
                        let p = connect(path.clone(), state.options.brush.connect);
                        rewrite(state, layer, op, |ctx| line_stroke(ctx, &p));
                    }
                    None => write(state, layer, op, |ctx| {
                        brush_stroke(ctx, Some(from), center)
                    }),
                }
            }
            DragKind::PlaneMove {
                pivot,
                grab,
                start,
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
                let bounds = doc.data.bounds;
                if doc.plane.slice {
                    doc.plane.set_slice_layer(*start + amount, bounds);
                } else {
                    doc.plane.coord = *start + amount;
                    doc.plane = doc.plane.clamped(bounds);
                }
                if let Some(g) = grab {
                    g[axis] = doc.plane.sheet_coord(bounds) as f32;
                }
            }
            DragKind::Move {
                pivot,
                offset,
                target,
                selected,
            } => {
                let new = match drag_axis(delta, f.axis_screen(*pivot)) {
                    Some((axis, amount)) => {
                        let mut o = IVec3::ZERO;
                        o[axis] = amount;
                        o
                    }
                    None => IVec3::ZERO,
                };
                if new == *target {
                    return;
                }
                *target = new;
                let Some(sel) = selected else {
                    *offset = new;
                    rewrite(state, layer, op, |ctx| move_changes(ctx.volume, new));
                    return;
                };
                // A blocked selection move stays at the last free offset.
                let overwrite = state.options.moving.overwrite;
                let last = *offset;
                let mut applied = last;
                rewrite(state, layer, op, |ctx| {
                    match move_selection_changes(ctx.volume, sel.cells(), new, overwrite) {
                        Some(changes) => {
                            applied = new;
                            changes
                        }
                        None => move_selection_changes(ctx.volume, sel.cells(), last, true)
                            .unwrap_or_default(),
                    }
                });
                *offset = applied;
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
                ..
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
        if let DragKind::Brush { last, .. } = drag.kind {
            self.last_stamp = Some((drag.doc_id, drag.layer, last));
        }
        if let DragKind::PlaneMove {
            moved: false,
            click: Some(hit),
            ..
        } = drag.kind
        {
            // A click without drag slices at that voxel.
            if let Some(doc) = state.doc_mut() {
                let bounds = doc.data.bounds;
                doc.plane.slice_at(&hit, bounds);
            }
            return;
        }
        if matches!(
            drag.kind,
            DragKind::Select { boxed: true, .. }
                | DragKind::BoxFace { .. }
                | DragKind::BoxMove { .. }
        ) {
            // The box stays open; a large one updates now.
            if let Some(doc) = state.doc_mut() {
                if doc.box_select().is_some_and(|b| b.stale) {
                    doc.refresh_box_select();
                }
            }
            return;
        }
        if let DragKind::Select {
            anchor,
            current,
            mode,
            moved,
            ..
        } = drag.kind
        {
            let all = state.options.selection.all_layers;
            let end = if moved { current } else { anchor };
            apply_box_selection(state, selection_box(anchor, end), mode, !moved, all);
            return;
        }
        if matches!(
            drag.kind,
            DragKind::Resize { .. } | DragKind::PlaneMove { .. }
        ) {
            return;
        }
        let Some(doc) = state.doc_mut() else { return };
        let bounds = doc.data.bounds;
        // Move and Extrude take the selection along in the same undo step.
        let carried = match &drag.kind {
            DragKind::Move {
                selected: Some(sel),
                offset,
                ..
            } => Some(sel.translated(*offset, bounds)),
            DragKind::Extrude {
                selection: Some(sel),
                region,
                normal,
                depth,
                ..
            } => {
                let cells = extruded_cells(region, *normal, *depth);
                Some(if *depth > 0 {
                    let mut grown = sel.clone();
                    grown.extend(cells);
                    grown.translated(IVec3::ZERO, bounds)
                } else {
                    let carved: Selection = cells.into_iter().collect();
                    sel.combine(&carved, MaskMode::Subtract)
                })
            }
            _ => None,
        };
        let changed = doc.pending_changes();
        let done = doc.labeled(drag.tool.label(), |doc| match carried {
            Some(sel) => doc.commit_edit_with_selection(drag.layer, sel),
            None => doc.commit_edit(),
        });
        if done {
            state.status_message = Some(format!("{}: {changed} voxels changed", drag.tool.label()));
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
            DragKind::PlaneMove { start, .. } => {
                let bounds = doc.data.bounds;
                if drag.plane.slice {
                    doc.plane.set_slice_layer(start, bounds);
                } else {
                    doc.plane.coord = start;
                }
            }
            DragKind::Select { boxed: true, .. } => {
                doc.cancel_box_select();
            }
            DragKind::BoxFace { start, .. } | DragKind::BoxMove { start, .. } => {
                doc.set_box_area(start, true);
            }
            DragKind::Select { .. } => {}
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
        let clip = doc.edit_clip().unwrap_or(VoxelBox::from_bounds(bounds));
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
            Tool::Brush | Tool::Line => {
                let cells = ghost_cells(
                    brush_positions(brush_center(hit, op), &state.options.brush, bounds),
                    &clip,
                );
                ghost(f.bmin, &cells, ghost_color, out);
                face_highlight(f.bmin, hit, ppp, out);
            }
            Tool::Plane if doc.plane.slice => {
                face_highlight(f.bmin, hit, ppp, out);
                state.tool_feedback = Some(format!(
                    "Slice {} — drag to move it, click a voxel to slice there",
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
                let opts = &state.options.extrude;
                let selection = doc.region_selection();
                let key = (
                    doc.id(),
                    doc.revision(),
                    hit.voxel,
                    hit.normal,
                    *opts,
                    selection.map_or(0, Selection::revision),
                );
                if self.region_cache.as_ref().is_none_or(|(k, _)| *k != key) {
                    let source;
                    let volume = if opts.confine_to_layer {
                        doc.active_layer().map(|l| &l.volume)
                    } else {
                        source = doc.merged_visible_volume();
                        Some(&source)
                    };
                    let region = volume
                        .map(|v| match selection {
                            Some(sel) => selection_region(v, sel.cells(), hit.normal),
                            None => extrude_region(v, hit, opts, &doc.data.palette),
                        })
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
            Tool::Shape => {
                let anchor = if is_additive(op) {
                    hit.adjacent()
                } else {
                    hit.voxel
                };
                if clip.contains(anchor) {
                    ghost(f.bmin, &[anchor], ghost_color, out);
                }
                face_highlight(f.bmin, hit, ppp, out);
            }
            Tool::Fill if state.options.fill.mode == FillMode::Empty => {
                // The empty cell the flood starts from.
                let c = hit.adjacent().as_vec3() + f.bmin;
                box_outline(c, c + Vec3::ONE, paint, 1.5 * ppp, out);
                face_highlight(f.bmin, hit, ppp, out);
            }
            Tool::Move => {}
            _ => face_highlight(f.bmin, hit, ppp, out),
        }
    }

    /// Outline of the active layer's selection (translucent faces plus an
    /// edge outline), shifted by the offset of a selection move in
    /// progress. Large selections draw only their box.
    fn selection_overlay(&mut self, f: &Frame, state: &mut AppState, out: &mut Preview) {
        let Some(doc) = state.doc() else { return };
        // An open box shows the selection it would make.
        let Some(sel) = doc.shown_selection().filter(|s| !s.is_empty()) else {
            return;
        };
        let ppp = f.pixels_per_point;
        let key = (
            doc.id(),
            doc.active_layer,
            sel.revision(),
            sel.len(),
            f.bmin.to_array(),
            ppp,
        );
        if self
            .selection_cache
            .as_ref()
            .is_none_or(|(k, ..)| *k != key)
        {
            let mut geo = Preview::default();
            let faces = sel.boundary_faces();
            let boxed = faces.len() > MAX_SELECTION_FACES;
            if boxed {
                if let Some((lo, hi)) = sel.bounds() {
                    let (lo, hi) = (lo.as_vec3() + f.bmin, hi.as_vec3() + f.bmin);
                    box_outline(lo, hi, theme::ACCENT_LIGHT, 2.0 * ppp, &mut geo);
                }
            } else {
                selection_faces(f.bmin, sel, &faces, ppp, &mut geo);
            }
            self.selection_cache = Some((key, geo, boxed));
        }
        let Some((_, geo, boxed)) = &self.selection_cache else {
            return;
        };
        let shift = match &self.drag {
            Some(Drag {
                kind:
                    DragKind::Move {
                        selected: Some(_),
                        offset,
                        ..
                    },
                ..
            }) => offset.as_vec3(),
            _ => Vec3::ZERO,
        };
        let moved = |p: [f32; 3]| (Vec3::from_array(p) + shift).to_array();
        out.fills.extend(geo.fills.iter().map(|v| FillVertex {
            pos: moved(v.pos),
            ..*v
        }));
        out.lines.extend(geo.lines.iter().map(|l| LineInstance {
            a: moved(l.a),
            b: moved(l.b),
            ..*l
        }));
        if *boxed && state.tool_feedback.is_none() {
            state.tool_feedback = Some(format!(
                "Selection: {} voxels (too large to outline, showing its box)",
                sel.len()
            ));
        }
    }
}

/// Option bar: opens the transform when needed and sets its parameters.
pub fn set_transform_params(state: &mut AppState, params: rizxel_engine::tools::TransformParams) {
    transform::set_params(state, params);
}

/// Option bar *Apply* of the open transform.
pub fn apply_transform(state: &mut AppState) {
    transform::apply(state, false);
}

/// True when the active document has an open box selection.
fn has_open_box(state: &AppState) -> bool {
    state.doc().is_some_and(|d| d.box_select().is_some())
}

/// `Esc` deselects and `Delete`/`Backspace` (keymap) deletes the selected
/// voxels of the active layer (outside a drag).
fn selection_keys(f: &Frame, state: &mut AppState) {
    if state.selected_voxels() == 0 {
        return;
    }
    let keymap = &state.settings.keymap;
    let (esc, del) = f.ui.input_mut(|i| {
        (
            i.consume_key(Modifiers::NONE, Key::Escape),
            crate::keys::consume(i, keymap, rizxel_app::ActionId::Delete),
        )
    });
    if del {
        state.delete_selected();
    } else if esc {
        state.deselect();
    }
}

/// Selection tool release: selects the voxels inside `area` (the active
/// layer's, or every visible layer's), clipped to the slice layer while
/// slicing, combined by `mode`. A click on an already selected voxel in Set mode deselects it.
fn apply_box_selection(
    state: &mut AppState,
    area: VoxelBox,
    mode: MaskMode,
    click: bool,
    all_layers: bool,
) {
    let Some(doc) = state.doc_mut() else { return };
    let mut cells = if all_layers {
        doc.with_visible_geometry(|vols| select_box(vols, &area))
    } else {
        doc.active_layer()
            .map(|l| select_box(&[&l.volume], &area))
            .unwrap_or_default()
    };
    if let Some(clip) = doc.edit_clip() {
        cells.retain(|p| clip.contains(*p));
    }
    let toggle_off = click
        && mode == MaskMode::Set
        && cells.len() == 1
        && doc.selection().is_some_and(|s| s.contains(cells[0]));
    let mode = if toggle_off { MaskMode::Subtract } else { mode };
    doc.select(cells, mode);
    state.status_message = Some(format!("Selected: {} voxels", state.selected_voxels()));
}

/// Fuzzy Select click: selects the voxels connected to the clicked one
/// with a similar color. A click off the model in Set mode deselects.
fn fuzzy_click(state: &mut AppState, hit: Option<RayHit>, mods: Modifiers) {
    let opts = state.options.fuzzy_select;
    let mode = mask_mode(mods, opts.mask);
    let Some(doc) = state.doc_mut() else { return };
    let Some(hit) = hit.filter(is_voxel) else {
        if mode == MaskMode::Set && doc.clear_selection() {
            state.status_message = Some("Selection cleared".to_owned());
        }
        return;
    };
    // While slicing the search runs on the slice layer alone, so neither
    // a connected nor a global match reaches other slices (Qubicle's Magic
    // Wand).
    let clip = doc.edit_clip();
    let source = if opts.all_layers {
        Some(doc.merged_visible_volume())
    } else {
        doc.active_layer().map(|l| l.volume.clone())
    };
    let result = match source.map(|v| match clip {
        Some(c) => v.cropped(&c),
        None => v,
    }) {
        Some(v) => fuzzy_select(&v, hit.voxel, &doc.data.palette, &opts),
        None => Ok(Vec::new()),
    };
    let cells = match result {
        Ok(cells) => cells,
        Err(capped) => {
            state.status_message = Some(capped.message());
            return;
        }
    };
    if cells.is_empty() && mode != MaskMode::Set {
        state.status_message = Some("No voxel of the active layer here".to_owned());
        return;
    }
    doc.select(cells, mode);
    state.status_message = Some(format!("Selected: {} voxels", state.selected_voxels()));
}

/// Fill click: floods from the clicked face (inside the selection, when
/// there is one) and writes the active color into the active layer as one
/// undo step.
fn fill_click(state: &mut AppState, hit: Option<RayHit>) {
    let opts = state.options.fill;
    let Some(doc) = state.doc_mut() else { return };
    let layer = doc.active_layer;
    if let Err(e) = doc.editable_layer(layer) {
        state.status_message = Some(e.message().to_owned());
        return;
    }
    let Some(hit) = hit else { return };
    // While slicing the flood stays on the slice layer.
    let limit = doc
        .edit_clip()
        .unwrap_or(VoxelBox::from_bounds(doc.data.bounds));
    let allowed = doc.region_selection().map(|s| s.cells().clone());
    let merged;
    let source = if opts.current_layer_only {
        &doc.data.layers[layer].volume
    } else {
        merged = doc.merged_visible_volume();
        &merged
    };
    let color = doc.selected_color as Voxel;
    let changes = match fill_cells(
        source,
        &hit,
        &doc.data.palette,
        &opts,
        &limit,
        allowed.as_ref(),
    ) {
        Ok(cells) => apply_op(&doc.data.layers[layer].volume, cells, VoxelOp::Add, color),
        Err(e) => {
            state.status_message = Some(e.message());
            return;
        }
    };
    let n = doc.labeled("Fill", |doc| doc.edit_voxels(layer, &changes));
    state.status_message = Some(format!("Fill: {n} voxels changed"));
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
    let symmetry = match state.tool {
        Tool::Brush | Tool::Line => options.brush.symmetry,
        Tool::Shape => options.shape.symmetry,
        _ => Symmetry::default(),
    };
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
    // While slicing, edits stay on the slice layer; overlaps follow the
    // operation, e.g. Add overwrites. A mirror axis along the slice normal
    // still writes its copy onto the mirrored slice (as Qubicle does).
    if let Some(slab) = doc.edit_clip() {
        let mirrored = mirrored_slab(&doc.plane, slab, doc.data.bounds, symmetry);
        changes.retain(|c| slab.contains(c.pos) || mirrored.is_some_and(|m| m.contains(c.pos)));
    }
    doc.edit_voxels(layer, &changes);
}

/// Mirror image of the slice slab when `symmetry` mirrors along the slice
/// normal.
fn mirrored_slab(
    plane: &WorkPlane,
    slab: VoxelBox,
    bounds: BoundSize,
    symmetry: Symmetry,
) -> Option<VoxelBox> {
    let axis = plane.axis.min(2);
    if ![symmetry.x, symmetry.y, symmetry.z][axis] {
        return None;
    }
    let mut m = slab;
    m.min[axis] = mirror(slab.min, bounds, axis)[axis];
    m.max[axis] = m.min[axis] + 1;
    Some(m)
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
    // Placing a plane is not a request to resume an earlier slice.
    doc.plane.resume_slice = false;
    doc.plane.visible = true;
    state.status_message = Some(format!(
        "Work plane at {}",
        plane_label(&doc.plane, doc.data.bounds)
    ));
}

/// `X = 4` style label of the plane position; while slicing `X 3 / 16`
/// (1-based, like Qubicle's slice counter).
pub fn plane_label(p: &WorkPlane, bounds: BoundSize) -> String {
    let axis = AXIS_NAMES[p.axis.min(2)];
    if p.slice {
        format!(
            "{axis} {} / {}",
            p.slice_layer(bounds) + 1,
            p.slice_count(bounds)
        )
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

/// The 12 edges of the box `lo..hi` (world space).
fn box_outline(lo: Vec3, hi: Vec3, color: Color32, width: f32, out: &mut Preview) {
    let c = |x: bool, y: bool, z: bool| {
        Vec3::new(
            if x { hi.x } else { lo.x },
            if y { hi.y } else { lo.y },
            if z { hi.z } else { lo.z },
        )
    };
    for a in [false, true] {
        for b in [false, true] {
            out.lines.extend([
                LineInstance::new(c(false, a, b), c(true, a, b), color, width),
                LineInstance::new(c(a, false, b), c(a, true, b), color, width),
                LineInstance::new(c(a, b, false), c(a, b, true), color, width),
            ]);
        }
    }
}

/// Marquee of a Selection or Shape drag: the 3D box being dragged out.
fn drag_marquee(f: &Frame, drag: &Drag, out: &mut Preview) {
    let area = match drag.kind {
        DragKind::Select {
            anchor,
            current,
            boxed: false,
            ..
        } => selection_box(anchor, current),
        DragKind::Shape {
            area: Some(area), ..
        } => area,
        _ => return,
    };
    let (lo, hi) = (area.min.as_vec3() + f.bmin, area.max.as_vec3() + f.bmin);
    box_outline(lo, hi, theme::ACCENT_LIGHT, 1.5 * f.pixels_per_point, out);
}

/// Selection faces: a translucent accent quad per boundary face and an
/// outline along the edges where the outline's surface ends or bends.
fn selection_faces(
    bmin: Vec3,
    sel: &Selection,
    faces: &[(IVec3, IVec3)],
    ppp: f32,
    out: &mut Preview,
) {
    let fill = theme::ACCENT_LIGHT.gamma_multiply(0.22);
    for &(cell, n) in faces {
        let q = face_corners(bmin, cell, n);
        quad(out, q, fill);
        let axis = dominant_axis(n.as_vec3());
        let (mut du, mut dv) = (IVec3::ZERO, IVec3::ZERO);
        du[(axis + 1) % 3] = 1;
        dv[(axis + 2) % 3] = 1;
        // Edge i runs from q[i] to q[i + 1]; `across` is the neighbour on
        // the other side of it within the same plane.
        for (i, across) in [-dv, du, dv, -du].into_iter().enumerate() {
            let next = cell + across;
            let continues = sel.contains(next) && !sel.contains(next + n);
            if !continues {
                out.lines.push(LineInstance::new(
                    q[i],
                    q[(i + 1) % 4],
                    theme::ACCENT_LIGHT,
                    ppp,
                ));
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

/// Point (voxel space) of the active plane's outline, or while slicing of
/// the bounds frame (Qubicle's Slice Selector), within [`EDGE_GRAB`] points
/// of the cursor, nearest first.
fn plane_edge_at(f: &Frame, doc: &Document, pos: Pos2) -> Option<Vec3> {
    let bounds = doc.data.bounds;
    let p = doc.plane.clamped(bounds);
    if !p.active() {
        return None;
    }
    let q = plane_corners(&p, bounds, p.sheet_coord(bounds) as f32);
    let mut edges: Vec<(Vec3, Vec3)> = (0..4).map(|i| (q[i], q[(i + 1) % 4])).collect();
    if p.slice {
        let size = bounds.0.as_vec3();
        let corner = |i: usize| {
            Vec3::new(
                if i & 1 == 0 { 0.0 } else { size.x },
                if i & 2 == 0 { 0.0 } else { size.y },
                if i & 4 == 0 { 0.0 } else { size.z },
            )
        };
        for i in 0..8 {
            for bit in [1, 2, 4] {
                if i & bit == 0 {
                    edges.push((corner(i), corner(i | bit)));
                }
            }
        }
    }
    let cursor = Vec2::new(pos.x, pos.y);
    let mut best: Option<(f32, Vec3)> = None;
    for (a, b) in edges {
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

/// Work plane drawing: one translucent
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
    // While slicing, the sheet sits on the slice layer's far face, lifted
    // toward the camera so it never fights the far side's voxel faces.
    let lift = if p.slice {
        p.hidden_sign() as f32 * 0.02
    } else {
        0.0
    };
    let sheet = p.sheet_coord(bounds) as f32;
    let q = plane_corners(&p, bounds, sheet + lift).map(|c| c + f.bmin);
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
        // Gizmo arrow pointing where a positive
        // drag moves the plane.
        let mut base = point;
        base[axis] = sheet;
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
    fn slicing_picks_the_slice_layer_only() {
        let bounds = BoundSize::new(8, 8, 8);
        let mut plane = WorkPlane {
            axis: 0,
            slice: true,
            ..WorkPlane::default()
        };
        plane.set_slice_layer(2, bounds);
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

        // Voxels at x = 0 (far side) and x = 3, 6 (off the slice) are
        // never picked: the ray goes through to the slice cell.
        let mut solid = VoxelVolume::new(bounds);
        for x in [0, 3, 6] {
            solid.set(IVec3::new(x, 3, 3), 1);
        }
        let is_solid = |p: IVec3| solid.get(p) != EMPTY_VOXEL;
        for ray in [from_right, from_left] {
            let hit = pick_with(&ray, bounds, &plane, true, false, is_solid).unwrap();
            assert_eq!(hit.voxel, IVec3::new(2, 3, 3));
            let hit = pick_with(&ray, bounds, &plane, false, false, is_solid);
            assert_eq!(hit, None, "nothing pickable off the slice");
        }

        // A slice voxel is hit on its face; a side face adds within the
        // slice.
        solid.set(IVec3::new(2, 3, 3), 1);
        let is_solid = |p: IVec3| solid.get(p) != EMPTY_VOXEL;
        let hit = pick_with(&from_right, bounds, &plane, true, false, is_solid).unwrap();
        assert_eq!((hit.voxel, hit.normal), (IVec3::new(2, 3, 3), IVec3::X));
        let down = Ray::new(Vec3::new(2.5, 20.0, 3.5), Vec3::NEG_Y);
        let hit = pick_with(&down, bounds, &plane, true, true, is_solid).unwrap();
        assert_eq!(hit.adjacent(), IVec3::new(2, 4, 3));
    }

    #[test]
    fn normal_axis_mirror_reaches_the_mirrored_slice() {
        let bounds = BoundSize::new(17, 8, 9);
        let mut plane = WorkPlane {
            axis: 2,
            slice: true,
            ..WorkPlane::default()
        };
        plane.set_slice_layer(8, bounds);
        let slab = plane.slice_box(bounds).unwrap();
        let z = Symmetry {
            z: true,
            ..Symmetry::default()
        };
        let m = mirrored_slab(&plane, slab, bounds, z).unwrap();
        assert_eq!((m.min.z, m.max.z), (0, 1), "z = 8 mirrors onto z = 0");
        let x = Symmetry {
            x: true,
            ..Symmetry::default()
        };
        assert_eq!(
            mirrored_slab(&plane, slab, bounds, x),
            None,
            "in-plane axis"
        );
    }

    #[test]
    fn selection_outline_skips_inner_edges() {
        // Two selected cells side by side on the floor: 10 boundary faces;
        // the shared edge on each long side is not outlined.
        let sel: Selection = [IVec3::ZERO, IVec3::X].into_iter().collect();
        let faces = sel.boundary_faces();
        assert_eq!(faces.len(), 10);
        let mut out = Preview::default();
        selection_faces(Vec3::ZERO, &sel, &faces, 1.0, &mut out);
        assert_eq!(out.fills.len(), 10 * 6);
        // The 2×1×1 box has 16 edge segments, each drawn by both faces
        // meeting there; the edges between the two cells are skipped.
        assert_eq!(out.lines.len(), 32);
    }

    #[test]
    fn mask_modifiers_override_the_option() {
        let shift = Modifiers::SHIFT;
        assert_eq!(mask_mode(shift, MaskMode::Set), MaskMode::Add);
        assert_eq!(
            mask_mode(Modifiers::COMMAND, MaskMode::Add),
            MaskMode::Subtract
        );
        assert_eq!(mask_mode(Modifiers::NONE, MaskMode::Add), MaskMode::Add);
    }

    #[test]
    fn plane_projection_keeps_the_anchor_layer() {
        let b = BoundSize::new(8, 8, 8);
        let down = Ray::new(Vec3::new(5.5, 20.0, 2.5), Vec3::NEG_Y);
        assert_eq!(
            cell_on_plane(&down, IVec3::new(1, 3, 1), 1, b),
            Some(IVec3::new(5, 3, 2))
        );
        let far = Ray::new(Vec3::new(50.5, 20.0, 2.5), Vec3::NEG_Y);
        assert_eq!(
            cell_on_plane(&far, IVec3::new(1, 3, 1), 1, b),
            Some(IVec3::new(7, 3, 2)),
            "clamped into the bounds"
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

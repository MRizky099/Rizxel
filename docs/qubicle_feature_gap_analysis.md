# Rizxel — Qubicle Feature Gap Analysis

**Date:** 2026-10-23 (fourth revision: Slice Mode verified hands-on in Qubicle 3.1.2)
**Scope:** Compare the current Rizxel implementation with the Qubicle 3 voxel-editor
feature set, with special attention to Qubicle's **2D / Slice Mode**.

## Source and methodology

### Qubicle documentation

This revision is based on the **full text of all 98 Qubicle 3 documentation pages** at
<https://www.getqubicle.com/qubicle-docs/>, plus the screenshots embedded in them.

> **How the docs were read.** Opening an article URL directly (for example
> `/qubicle-docs/docs/tools/slice_mode`) returns an HTTP 404 or the Minddesk archive splash
> screen ("Qubicle is no longer in active development"). The articles still exist, though.
> The site is a client-side VitePress app, so every article renders once it is reached
> through the sidebar of the docs landing page. The previous revision of this document said
> the article bodies were unreadable. That was wrong, and the behavior notes below now come
> from the article text rather than being inferred from command names.

Qubicle's documentation sections mirror its main menu:

| Qubicle 3 menu area | Documented commands |
|---|---|
| User Interface | Start Screen, Work Area, Quick Access Bar, Header Bar, Toolbar, Color Map, Property Panel, Status Bar, Shortcuts |
| Objects | Voxel, Voxel Matrix, Voxel Compound, Voxel Model |
| File | New (Model / Matrix / Compound), Import, Import Heightmap, Export Mesh, Export Cubes, Export Slices, QB, QBT, QEF specs |
| Edit | Cut/Copy/Paste (+ Paste OS Clipboard), Duplicate Special, Edit Isolated / In Place, Preferences |
| Create | Standard Primitives (Box, Sphere, Cylinder, Cone, Pyramid, Torus), Terrain |
| Transform | Align, Distribute, Translate, Rotate, Integer Scale, Scale, Rotate 90°, Flip, Mirror |
| Modify ▸ Color | Apply Colors, Color Adjustments |
| Modify ▸ Work Area | Resize, Optimize, Extend (1 / 5), Divide |
| Modify ▸ Compound | Create, Break, Parent/Unparent, Crop, Merge |
| Modify ▸ Core | Hollow, Advanced Hollow, Fill Hollows, Extract Core, Remove Hull |
| Modify ▸ Boolean | Union, Difference, Intersection |
| Modify ▸ Other | Split Off, Linear Snapshot, Linear Projection |
| Modify ▸ Pivot | Position, Center, Align Pivot |
| Select | All, Deselect, Invert, Previous/Next Object, By Type, By Status |
| Voxelize | Create Voxelizer, Load Mesh, Scale Mesh, Rotate Mesh, Convert To Matrix |
| Tools | Draw, Area, Auxiliary, Standard Primitive, Extrude, **Slice Mode**, Tool Options |
| Colors | Open/Save QSM, Import, Export, Reset, Clear, Pack, Remove Duplicates, Create Gradient, Auto Name, Clear Names |
| Camera | Frame / Frame Work Area, Home, Perpendicular View, Isometric View, Properties |
| View | Work Area Settings, Render Settings |

### Qubicle application (Qubicle 3.1.2 demo)

Slice Mode was studied hands-on in the **Qubicle 3.1.2 demo** (Windows, desktop control),
using the built-in *File ▸ Test Files ▸ Deer* sample (one 17 × 26 × 9 matrix). The sample
was closed with **No** at "Save changes", so no Qubicle file was written. Results are in
[Qubicle Slice Mode — observed in Qubicle 3.1.2](#qubicle-slice-mode--observed-in-qubicle-312).
Screenshots of the key states are in `docs/images/qubicle_slice_mode/` and are cited by
file name (for example `03_space_over_voxel_z_006.jpg`).

Limits of the hands-on pass: the camera was moved only with the Cam Cube, the *Camera* menu
and the wheel (no free right-drag orbit), the Box primitive and Extrude were only tried with
a single drag (they did not produce a result that way, see below), and tool shortcuts were
not remapped.

### Rizxel

Rizxel's side of the comparison was checked against the current source tree:

- Tools: `Cursor (V)`, `Brush (B)`, `Shape (S)`, `Plane (P)`, `Move (M)`,
  `ColorPicker (C)`, `Selection (R)`, `FuzzySelect (W)`, `Extrude (E)`, `Fill (F)`
  (`rizxel-engine/src/tools/mod.rs`).
- Voxel operations: `Add`, `Subtract`, `Join`, `Paint`. Brush modifiers: `Shift` erases,
  `Alt` picks a color, `Ctrl` copies the picked color into the selected swatch.
- Tool options: brush shape and size, surface paint, antialiasing, X/Y/Z `Symmetry`,
  selection `MaskMode::{Set, Add, Subtract}` (how a new selection combines with the old
  one), fuzzy threshold, fill modes (`rizxel-engine/src/tools/options.rs`).
- Work plane / slice: `WorkPlane { axis, coord, visible, slice, flip }`, `slice_layer`,
  `set_slice_layer`, `clip_box`, destructive `slice_layers` Cut with optional cap
  (`rizxel-engine/src/tools/work_plane.rs`, `slice.rs`).
- Formats: `.rzxl`, `.vox`, `.gox`, `.obj`, `.mtl`, `.stl`.
- Document: layers and groups, opacity/material/crop properties, undo history, selection
  per layer, duplicate layer.
- View mode: `ViewMode::{Model, Render}`; perspective camera with view-cube snapping.

## Qubicle Slice Mode — documented behavior

### What the documentation says

From *Tools ▸ Slice Mode*, *User Interface ▸ Toolbar* and *Tools ▸ Draw Tools*:

1. **Purpose.** "Use the Slice Mode to work on a 2D slice of a Voxel Matrix."
2. **Only inside the Matrix Editor.** You first open the Matrix Editor (double-click a
   matrix). Slice Mode applies to the one matrix being edited, not to the whole model.
3. **It is a global tool option, not a separate view.** The Toolbar page lists Slice Mode
   with the other *global tool options*: tool mirroring, masking and stroke tracing. It
   changes how every tool behaves and stays on while you switch tools. *(Confirmed in the
   app. It is turned off, and its slice reset, whenever the Matrix Editor is left.)*
4. **Toggle.** Click the **`2D`** button on the toolbar, or press **`[Space]`**.
5. **Jump to a voxel's slice.** If the mouse is over a voxel when `[Space]` is pressed,
   Slice Mode opens on *that voxel's* slice. One key both enters the mode and picks the
   slice.
6. **Two navigation controls:**
   - **Slice Slider:** a vertical slider along the right edge of the Work Area.
   - **Slice Selector:** an in-viewport handle at the slice position. *(In the app it is the
     whole thick frame drawn around the matrix bounding box while Slice Mode is on; the
     light notches only mark where the slice plane meets the frame. Dragging anywhere on the
     frame moves the slice.)*
7. **Slice counter.** The active slice number is shown on the toolbar as a three-digit
   counter next to the `2D` button (for example `003`). *(In the app it is **1-based**:
   voxel layer 0 shows as `001`. The counter sits in the left Tools panel, not in a top
   toolbar.)*
8. **Tool semantics change.** The Pencil (`[B]`) "paints voxels in 3D mode. In 2D Slice
   Mode, it draws new voxels." In 3D the Pencil only recolors existing voxels; adding
   voxels needs the Attach tool (`[A]`). In Slice Mode every cell of the slice is a target,
   so the Pencil fills empty cells directly, as in a pixel-art editor.

### What the official screenshot shows

- **The view stays 3D.** The camera keeps its perspective, three-quarter view, and the Cam
  Cube is still visible and usable. Slice Mode does **not** switch to a flat orthographic
  canvas. (An exact square-on view comes from *Camera ▸ Perpendicular View*, which is a
  separate, orthographic, 90° camera command.)
- A **translucent sheet** fills the matrix bounding box at the active slice.
- The model is shown solid on one side of the slice. On the other side, voxels are only
  seen faintly through the sheet. *(Observed: the half between the slice and the camera is
  **hidden completely**, not ghosted. The far half is drawn normally and only looks faint
  where it is seen through the translucent sheet. The hidden half follows the camera.)*
- The slice axis matches the side of the model the camera faces. *(Observed: this is a
  coincidence of the screenshot. The axis comes from the face under the mouse when `Space`
  is pressed, or is kept from the last slice. Rotating the camera never changes it.)*

### Related Qubicle behaviors that matter in Slice Mode

| Feature | Documented behavior |
|---|---|
| Line drawing | Draw tools: click voxel A, `Shift`+click voxel B draws a line. A dedicated Line tool chains clicks; `Esc` finishes. |
| Rectangle drawing | `Ctrl`+drag with Pencil/Eraser/Attach/Paint draws a rectangle. A dedicated Rectangle tool does the same, with `Shift` to add and `Ctrl` to erase. |
| Color pick | `Alt`+click a voxel with any draw or area tool. |
| Tool Mirroring | Repeats any tool action across the selected axis. |
| Mask Mode | **Locks selected voxels** so draw actions leave them alone. Extrude also skips selected voxels when Mask Mode is on. *(Confirmed in Slice Mode.)* |
| Trace Stroke | *Corrected:* the app's own help text is "Enable Trace Stroke to fill gaps of freehand strokes". It joins the sampled points of a fast freehand stroke into a continuous line. It does **not** fill the area enclosed by the stroke (earlier revisions called it a lasso fill). |
| Paint Bucket / Magic Wand | `Contiguous` and `Tolerance` options. Paint Bucket drag recolors everything inside a rectangle. |
| Ghosts | *View ▸ Ghosts* draws the model's other objects as semi-transparent ghosts while in the Matrix Editor. *Edit Isolated* draws them behind the active object, *Edit In Place* in front. |
| Export Slices | *File ▸ Export ▸ Slices* writes one PNG per slice, with a direction and pixels-per-voxel scale. |

## Qubicle Slice Mode — observed in Qubicle 3.1.2

### Controls and layout

- **`2D` button and counter.** Both sit in the left **Tools** panel, below the tool grid and
  above the `X` `Y` `Z` mirror buttons: a small `2D` button followed by a three-digit
  seven-segment counter (unlit segments stay faintly visible, so `000` shows when off).
  Outside the Matrix Editor both are greyed out. Tooltip: "Slice Mode"; status bar:
  "Activate Slice Mode to view and modify 2D slices of a Matrix." The counter has no
  tooltip and cannot be typed into. (`01_slice_on_y_001.jpg`)
- **Slice Slider.** A thin vertical scrollbar-style slider along the right edge of the
  viewport, next to the Info panel. Its thumb is sized to one slice. It appears only while
  Slice Mode is on. Dragging the thumb moves the slice; the mouse wheel over it zooms the
  camera instead. (`02_slider_y_010.jpg`)
- **Slice Selector.** While Slice Mode is on, the matrix bounding box is drawn with a thick
  grey frame, and the far bounding-box walls get a voxel grid. Two light notches on the
  frame mark where the slice plane meets it. Dragging **anywhere on the frame** (not only
  on a notch) moves the slice, with any tool active. (`02_slider_y_010.jpg`)
- **Slice sheet.** The slice is filled with a light, translucent sheet across the whole
  matrix cross-section. The slice voxels are drawn solid on top of it.
- **Tools ▸ Slice submenu** (not in the docs): **Slice Mode** (`Space`), **Next**,
  **Previous**, **First**, **Last**. Next/Previous/First/Last have no default shortcut; the
  status bar text is, for example, "Switch to the last slice." Like every menu entry they
  can be bound through Qubicle's hover-and-press remapping. (`15_tools_slice_menu.jpg`)
- **Hover feedback.** On the slice, the cursor cell is outlined and the Info panel's
  *Cursor* field shows its `x : y : z`. Over off-slice voxels the *Cursor* field stays
  empty, which signals that they cannot be picked.

### Rendering of the two sides

- The half of the matrix between the slice and the **camera is hidden completely**. It is
  not ghosted.
- The slice layer is drawn solid. The far half is drawn with normal shading; where it lies
  behind the sheet it looks faint because it is seen through the sheet, and where it
  projects outside the sheet it looks fully solid.
- The hidden half **follows the camera**. With an X slice at `004` seen from the default
  (top-front-right) view, the head side was hidden and the body was visible; after
  *Camera ▸ Isometric View ▸ Top Back Left* the body was hidden and the head side showed
  through the sheet. (`04_x_004_home_view.jpg`, `05_x_004_opposite_view.jpg`)

### Draw tools in Slice Mode compared with 3D

| Tool | 3D | Slice Mode |
|---|---|---|
| Pencil (`B`) | Recolors the clicked voxel; does not add voxels | Recolors slice voxels **and adds voxels on empty slice cells**; off-slice voxels are not affected (`06_pencil_adds_on_slice.jpg`) |
| Attach (`A`) | Adds a voxel on the clicked face | Adds voxels on **empty slice cells** like the Pencil; clicking a slice voxel does nothing, because the neighbour along the slice normal is off the slice |
| Eraser (`E`) | Removes the clicked voxel | Removes slice voxels only; off-slice voxels are not affected |
| Paint | Recolors voxels | Recolors slice voxels only; status hint "Color voxels. Alternatively in a line with Shift and a rectangle with Ctrl." |
| Pencil `Shift`+click | Line | Line within the slice |
| Line tool | Chained clicks, `Esc` ends | Same; the pending segment is previewed as wireframe voxels on the slice (`07_line_tool_preview.jpg`) |
| Rectangle tool (`R`) | Drag a rectangle | Drag on the slice draws a filled one-voxel-thick rectangle; a "Width: n Height: n" readout follows the cursor. Slice-aligned crosshair guide lines span the slice while hovering |
| Paint Bucket (`G`) | Contiguous / Tolerance | With Contiguous on, the fill stays on the slice: the grey body was recolored on slice `006` only, and the grey front layers were unchanged in 3D (`14_paint_bucket_slice_only_3d_check.jpg`) |
| Magic Wand (`W`) | Contiguous / Tolerance | Selects on the slice only, **even with Contiguous off**: selecting teal returned an `11 × 4 × 1` selection with the three slice voxels, leaving teal voxels on other slices unselected (`10_magic_wand_slice_only.jpg`) |
| Box / Sphere / … primitives, Extrude | Multi-step (base → height → `Enter`) | A single drag showed the slice crosshair but produced nothing. Not conclusive: the multi-step primitive workflow was not completed in this pass |

### Tool Mirroring, Mask Mode and Trace Stroke in Slice Mode

- **Mirroring along an axis that lies in the slice** (X or Y on a Z slice): the hover shows
  two cursor outlines and the mirrored voxel lands **on the same slice**, mirrored about the
  matrix centre (`x = 13` → `x = 3` in a 17-wide matrix). (`08_mirror_x_same_slice.jpg`)
- **Mirroring along the slice normal** (Z on a Z slice): only one cursor outline shows, but
  the mirrored voxel is still written, **on the mirrored slice** (`z = 8` → `z = 0`), which
  is off the active slice. (`09_mirror_z_lands_on_slice_001.jpg`)
- **Mask Mode** works on the slice: with a selection and Mask on, the Pencil left the
  selected slice voxel unchanged and recolored an unselected one.
  (`11_mask_mode_protects.jpg`) The on/off state of the Mask and Trace buttons is hard to
  read from their shading.
- **Trace Stroke** on: a fast freehand stroke on the slice becomes a continuous outline.
  Off: the same stroke leaves only the sampled voxels with gaps between them. The enclosed
  area is **not** filled either way. (`12_freehand_trace_off.jpg`, `13_freehand_trace_on.jpg`)

### Answers to the verification checklist

1. **`Space` over a voxel vs. over empty space.** Over a voxel, the slice axis is the
   normal of the **hovered face** and the slice is that voxel's layer: hovering the front
   face of voxel `11 : 10 : 5` gave a Z slice `006`; hovering a side face of `3 : 10 : 4`
   gave an X slice `004`. Over empty space, `Space` restores the **last** axis and slice.
   `Space` while in Slice Mode turns it off. The first `2D` click after opening the Matrix
   Editor starts at Y, slice `001` (the bottom layer). (`03_space_over_voxel_z_006.jpg`)
2. **Camera rotation.** The axis **does not follow the camera**. Switching to the RIGHT
   perpendicular view and to the opposite isometric view kept the Z slice.
3. **Hidden side.** The side between the slice and the camera is hidden completely; the
   far side is drawn normally, seen through the translucent sheet. It **flips when the
   camera moves to the other side**. (`04_…`, `05_…`)
4. **Editing off the slice.** Pencil, Eraser, Attach, Paint and Paint Bucket did **not**
   change voxels off the active slice, even where those voxels are visible and solid. Off-
   slice voxels are not pickable at all. The only off-slice write seen was a mirror copy
   along the slice normal (item 8).
5. **Stepping.** Arrow keys, `Page Up/Down` and the wheel (plain, `Ctrl`, `Shift`, `Alt`,
   and over the slider) do **not** step the slice; the wheel always zooms. Stepping is done
   with the Slice Slider, the frame drag, or *Tools ▸ Slice ▸ Next / Previous / First /
   Last* (no default keys). On a Z slice seen from the default view, *Next* went from
   `006` to `005`, i.e. away from the camera.
6. **Slice Selector.** Drag the thick bounding-box frame anywhere; the notches are only
   markers. A frame drag moves the slice along its normal with the mouse direction.
7. **Slider ends and counter base.** The counter is **1-based** (layer 0 = `001`; the
   9-deep Z axis runs `001`–`009`). The slider's direction depends on the axis: for Y the
   bottom end is `001`; for Z and X the **top** end is `001`. In the default view this
   matches "up on the slider = up/back on screen".
8. **Mirroring.** Mirror axes inside the slice plane mirror within the same slice; the
   mirror axis along the slice normal writes the copy onto the mirrored slice.
9. **Persistence.** Slice Mode and the slice survive **tool switches** (Paint, Pencil,
   Attach, Line, Rectangle, Bucket, Wand, Extrude). The `2D` button restores the last axis
   and slice. **Leaving the Matrix Editor** (a double-click in the viewport, as the
   status bar's "Double-click to exit the Matrix editor." says, or `Ctrl+Z` as in item 10) turns Slice Mode off; re-entering starts
   with it off and the next `2D` click starts at Y `001` again. Two quick clicks with a draw
   tool count as that double-click, which makes it easy to leave the editor by accident.
10. **Undo.** Slice moves are **not** undo steps. `Ctrl+Z` undoes the last voxel edit and
    **moves the slice back to the slice where that edit was made** (an edit on `009`,
    then moving to `001`, then `Ctrl+Z` → slice `009`, edit gone; `Shift+Ctrl+Z` redid it
    on `009`). Entering the Matrix Editor is itself an undo step: with no edit since
    entering, `Ctrl+Z` left the Matrix Editor (and Slice Mode); redo re-entered it with
    Slice Mode off.

## Rizxel slice workflow vs. Qubicle Slice Mode

Earlier revisions said Qubicle's 2D icon opens a dedicated orthographic 2D canvas, and
recommended `ViewMode::Slice2D`. The documentation, the screenshot and the app all
contradict that. **Qubicle's Slice Mode is a 3D-view editing mode that isolates one
slice.** That is the same idea as Rizxel's `WorkPlane` with **Slice** on. The hands-on
pass showed two differences that mattered more than earlier revisions assumed: in Qubicle
**only the slice layer can be edited**, by every tool, and the hidden side is always **the
camera's side**.

**Update (2026-10-23, after implementation):** Rizxel now implements Qubicle-style Slice
Mode on top of `WorkPlane`: a global `2D` toggle and `Space`, slice-layer-only picking
and editing, the camera's side hidden every frame, a viewport slice slider, frame drag
with any tool, *View ▸ Slice* Next / Previous / First / Last, a 1-based counter, and the
slice restored on undo and redo. The table below describes Rizxel after that change.

Ratings: **High** blocks the Qubicle slice workflow, **Medium** makes it noticeably
slower or different, **Low** is polish. *Covered* means Rizxel already behaves the same.

| Aspect | Qubicle 3.1.2 (observed) | Rizxel today (source) | Gap |
|---|---|---|---|
| How to enter | `2D` button in the Tools panel or `Space`, from any tool; *Tools ▸ Slice ▸ Slice Mode* | `2D` button below the tool grid (`panels/tools.rs` `slice_toggle`), `Space` with any tool (`tool_input.rs`), *View ▸ Slice ▸ Slice Mode* (`top_bar.rs` `slice_menu`); the Plane tool's **Slice** checkbox uses the same toggle | Covered. |
| Global vs. tool-local | Global; kept across tool switches; turned off when leaving the Matrix Editor | Global per document (`doc.plane.slice`); kept across tool switches; every control works from any tool (`AppState::toggle_slice`, `step_slice`) | Covered. Rizxel has no Matrix Editor to leave; switching documents keeps each one's slice. |
| Jump to hovered voxel's slice | `Space` over a voxel: axis = hovered face normal, slice = that voxel's layer; `Space` over empty space restores the last slice; `Space` again turns it off | `WorkPlane::toggle_slice`: over a voxel `slice_at` (axis = hit face, layer = `hit.voxel[axis]`); otherwise `last_slice`, Y layer 0 the first time; `Space` again turns it off and remembers the slice. The `2D` button and menu always restore. With the Plane tool, a click on any visible voxel slices there | Covered. |
| Axis selection | Hovered face normal (or the last axis); **never follows the camera** | Axis of the clicked face (`set_from_hit`) or the X/Y/Z control in the options bar | Covered. The earlier "nearest facing axis" idea is not Qubicle behavior. |
| Which side is hidden | The side between slice and camera, hidden completely; flips when the camera crosses the slice | `WorkPlane::face_camera` runs every frame (`viewport.rs`) and hides the side containing the eye, keeping the slice layer; the **Flip** button was removed | Covered. |
| Rendering | Slice voxels solid; translucent sheet over the whole cross-section; far side drawn normally behind the sheet; no extra context layer; thick bounding-box frame | Slice layer and far side meshed (`clip_box`); the extra context layer was dropped (it would sit between camera and slice). Translucent grey sheet on the slice layer's far face (`sheet_coord`), so slice voxels draw in front of it; bounds drawn thick | Covered. No grid on the far bounding-box walls and no notches (Low). |
| What can be edited | **Only the slice layer.** Off-slice voxels are not even pickable (Info ▸ Cursor stays empty) | Only the slice layer: `pick_with` raycasts `WorkPlane::slice_box` and the drag snapshot is cropped to it, so off-slice voxels are not pickable (the cursor readout stays empty); `write` drops changes off the slice | Covered. |
| Fill and selections in Slice Mode | Paint Bucket and Magic Wand stay on the slice, Magic Wand even with Contiguous off | Fill floods inside `edit_clip()`; box selection is drawn on the slice and clipped to it; Fuzzy Select searches a copy cropped to the slice, so neither a connected nor a global match reaches other slices | Covered. |
| Pencil on empty cells | Pencil and Attach add voxels on empty slice cells; in 3D the Pencil only recolors | Brush with `Add` picks the slice slab, so it already draws on empty slice cells | Covered. |
| Paint-only on the slice | Paint tool | `Paint` op | Covered. |
| Erase on the slice | Eraser removes slice voxels only | Brush with `Subtract` (or `Shift`), restricted to the slice | Covered. |
| Line on the slice | Line tool (chained clicks, `Esc`) and Pencil `Shift`+click, previewed on the slice | Brush interpolates between drag samples (`line_cells`); no click-anchor line | **Medium.** No click-then-`Shift`+click or chained line. |
| Rectangle on the slice | Rectangle tool drag: one voxel thick, live "Width/Height" readout | `Shape` drawn on the slice (its grow axis is forced to the slice normal and the result clipped to the slice); size shown in the status feedback | Covered. |
| Slice navigation | Slice Slider at the viewport's right edge; drag the bounding-box frame with any tool | Slider on the viewport's right edge while slicing, thumb one slice tall (`viewport.rs` `slice_slider`); dragging any bounds edge moves the slice with any tool (`plane_edge_at`); the Plane tool also drags from anywhere; the options-bar Slice field remains | Covered. The wheel still zooms, as in Qubicle. |
| Keyboard and menu stepping | No default keys; arrows, `Page Up/Down` and the wheel do nothing to the slice. *Tools ▸ Slice ▸ Next / Previous / First / Last* (remappable) | *View ▸ Slice ▸ Next / Previous / First / Last* (`SliceStep`); Next goes away from the camera like Qubicle; no default keys | Covered. Remapping waits for a keymap (see the UI gaps). |
| Slice number display | 1-based three-digit counter beside the `2D` button (`001`) | `X 3/16`-style 1-based counter under the `2D` button in the axis color; the options bar, slider label, status and tool feedback also show `n / N` (`plane_label`, `slice_counter`) | Covered. |
| Slider direction | Y: bottom = `001`; X and Z: top = `001` | Same (`slider_t` / `slider_layer`) | Covered. |
| Undo and the slice | Slice moves are not undo steps; undoing an edit also moves the slice back to the edit's slice | Slice moves never touch the history; each undo step records the slice it was made on (`History::tag`), and undo / redo move the slice back there (`WorkPlane::restore_slice`), or make it the slice to restore when Slice Mode is off | Covered. |
| Mirroring in Slice Mode | In-plane mirror axes mirror on the same slice; the normal-axis mirror writes onto the mirrored slice; the mirror centre is the matrix centre | Brush and Shape symmetry: in-plane copies stay on the slice; a mirror axis along the slice normal may also write the mirrored slice (`mirrored_slab`) | Covered. |
| Mask Mode (lock selection) | Selected slice voxels are protected from the Pencil | Selection confines Fill and drives Move/Extrude; `MaskMode` only means Set/Add/Subtract for selection | **Medium.** No "protect selected voxels" option. |
| Trace Stroke | Joins the gaps of fast freehand strokes; does not fill the enclosed area | Brush strokes are always gap-free (`brush_stroke` stamps along `line_cells` between samples) | Covered. (Earlier revisions mistook it for a lasso fill.) |
| Destructive cut | Not part of Slice Mode (Boolean / Split Off / Resize are used instead) | `Cut` (`slice_layers`) with optional cap, still a separate explicit button in the Plane tool's options; it removes the currently hidden (camera) side. Turning Slice Mode off never deletes voxels | Rizxel extra, kept separate. |
| Exact square-on view | Cam Cube faces and *Camera ▸ Perpendicular View* give an orthographic 90° view, and Slice Mode works in it | View-cube snaps to a side, but the camera stays perspective | **Medium.** See the camera gaps below. |
| Slice export | *File ▸ Export ▸ Slices* (PNG per slice) | None | Low–Medium. Builds on `png_io.rs`. |

### Recommended Slice Mode behavior for Rizxel

*Status 2026-10-23:* items 1–7 and 9 are implemented; item 8 (orthographic
Perpendicular View) is still open, and the extra context layer of item 4 was dropped.

1. **Make Slice a global toggle.** Add a `2D` button to the tools panel or the view toggle
   area, bound to `Space`. It sets `doc.plane.slice` without switching tools. `Space`
   while slicing turns it off.
2. **Jump to the hovered voxel.** When `Space` is pressed over a voxel, take the axis from
   the hovered face normal and make the hovered voxel's layer the slice layer
   (`set_from_hit`, then `set_slice_layer(hit.voxel[axis])`). With no hover, reuse the last
   axis and layer; with none stored, start at Y, layer 0.
3. **Restrict every edit to the slice layer** while slicing. Brush, Shape, Fill, box
   selection and Fuzzy Select should clip to the slice slab instead of `view_clip()`, and
   `pick_with` should ignore off-slice voxels. Symmetry copies along the slice normal may
   still land on the mirrored slice, as in Qubicle.
4. **Hide the camera's side.** Derive `flip` from the camera direction every frame (hide
   the side the camera is on) instead of relying on the **Flip** button. Keep the far side
   visible behind the translucent quad; the extra context layer can stay as an option.
5. **Add a slice slider in the viewport** along the right edge, plus Next / Previous /
   First / Last commands (menu entries, optional keys), and let the bounds frame be dragged
   with any tool. Moving the slice changes only the view and pick state and never adds an
   undo step (`WorkPlane` is already outside the undo history).
6. **Show a slice counter** next to the toggle: axis and 1-based `n / N`.
7. **Restore the slice on undo and redo** to the slice the edit was made on.
8. **Add an orthographic Perpendicular View** (and a "snap to slice axis" action) for
   pixel-precise slice work. This is a camera feature shared with the rest of the editor,
   not a separate 2D renderer.
9. **Keep `Cut` explicit.** Leaving Slice Mode must never delete voxels.

## Missing or incomplete features in Rizxel (beyond Slice Mode)

Severity: **High** blocks a common production workflow. **Medium** is a meaningful
capability or compatibility limit. **Low** is polish or scale.

### Object model and hierarchy

Qubicle scenes are a tree: a **Model** root holding **Matrices** (voxel grids, up to
1024×512×1024, best kept at 128³ or less) and **Compounds** (Smart-Object-like groups with
their own work area). The Header Bar shows the breadcrumb path to the active node, and
double-clicking enters a node.

| Gap | Severity | Rizxel today | Recommendation |
|---|---|---|---|
| Per-object grids with their own size and position | Medium | One shared document grid; layers share bounds (limit 256³) | Per-layer bounds and offsets would allow Qubicle-style matrices and `.qb` round-trips. |
| Enter/leave object (Matrix Editor) and breadcrumb | Low | Active layer selection only | Optional; mainly needed once per-layer bounds exist. |
| Compound Create / Break / Parent / Unparent / Crop / Merge | Medium | Groups exist; no merge-to-one-layer or break | "Merge layers" and "Break into connected components" first. |
| Ghosts / Edit Isolated / Edit In Place | Medium | Hidden layers are hidden; no ghost display | Draw non-active layers semi-transparent (needs opacity rendering). |
| Select All / Invert / By Type / By Status / Previous–Next object | Medium | Box and fuzzy selection, `Esc` deselects | Add Select All, Invert, Grow/Shrink, and "select empty layers". |

### Drawing, area and auxiliary tools

| Qubicle tool or behavior | Severity | Rizxel today | Recommendation |
|---|---|---|---|
| Attach vs. Paint vs. Pencil split (`A` / Paint / `B`) | Low | One Brush with `Add` / `Subtract` / `Paint` ops | Covered by the ops; consider separate shortcuts. |
| Line tool and `Shift`+click line | Medium | Drag interpolation only | Add click-anchor and `Shift`+click line to Brush. |
| Rectangle via `Ctrl`+drag | Low | `Shape` tool | Covered. |
| Trace Stroke (joins gaps in fast freehand strokes; verified, not a lasso fill) | Low | Brush strokes are always interpolated between samples | Covered. A real lasso fill would be a new feature, not Qubicle parity. |
| Mask Mode (protect selection) | Medium | None | Add a global "protect selection" option that every write respects. |
| Paint Bucket rectangle drag | Low | Fill is click-based | Optional. |
| Paint Selection (freehand select) | Medium | Box and fuzzy only | Add paint/lasso selection. |
| Box Select with handles + `Enter` | Low | Box selection by drag | Add resize handles. |
| Resize tool (drag object bounds) | Low | `Move` with `DragMode::Resize` resizes document bounds | Mostly covered. |
| Primitive tools: Cube, Sphere, Cylinder, **Pyramid, Cone** | Low | `BrushShape` cube/sphere/cylinder | Add pyramid and cone shapes. |
| Primitive workflow: base → height → resize → `Enter` → arrow-key move → `Enter` | Low | One drag for the box | Optional editable handles before commit. |
| Extrude that skips masked voxels | Low | Extrude follows the selection | Comes with Mask Mode. |
| Tool Wheel (`Shift`+right mouse) | Low | Toolbar and shortcuts only | Optional radial menu. |
| Favorite colors on keys `0`–`9` | Low | None | Easy win for palette workflows. |

### Transform and modify commands

| Gap | Severity | Rizxel today | Recommendation |
|---|---|---|---|
| Cut / Copy / Paste (paste floats as "detached voxels" until `Enter`) | High | None | App-level voxel clipboard; paste as a floating selection and commit with `Enter`. |
| Paste OS clipboard image | Low | `png_io.rs` exists | Paste an image as a one-voxel-thick layer. |
| Duplicate Special (N copies with offset) | Low | Duplicate layer only | Add an array duplicate with count and offset. |
| Rotate 90° / Flip about the camera-facing axis | High | Symmetry affects placement only | Rotate 90° and Flip for a selection or layer, using the view-cube facing axis by default. |
| Mirror (copy across an axis, e.g. `X+`) | Medium | Live symmetry only | Add a one-shot mirror command. |
| Free Rotate (degrees) / Scale / Integer Scale | Medium | None | Integer Scale first: it keeps the shape exact. |
| Translate (relative or absolute) | Low | `Move` tool | Add numeric input. |
| Align / Distribute across objects | Low | None | Needs per-layer offsets. |
| Work area: Resize with anchors, Optimize (fit to content), Extend 1/5, Divide | Medium | `resize_bounds` via Move drag | Add Optimize/fit-to-content and anchored numeric resize. |
| Core: Hollow, Advanced Hollow (hull thickness), Fill Hollows, Extract Core, Remove Hull | Medium | None; `flood` exists in `quality.rs` | Fill Hollows and Hollow first (they cut exported polygon count). |
| Boolean Union / Difference (A minus last selected B) / Intersection | Medium | None | Difference, then Union and Intersection between layers. |
| Split Off (selection → new object) | Medium | None | "Selection to new layer". |
| Linear Snapshot (orthographic PNG of one side of a matrix) | Low | None | Pairs with Export Slices and rendered image output. |
| Linear Projection (project an image onto a side; Offset, Repeat, Pierce) | Medium | None | A powerful texturing tool; builds on `png_io.rs`. |
| Pivot (export-only pivot point) | Low | None | Needed once mesh export supports pivots. |
| Apply Colors (snap to palette) and Color Adjustments | Medium | Palette editing only | Add "remap to nearest palette color" and HSV/brightness adjustments. |

Note: an earlier revision described Linear Snapshot and Linear Projection as array tools.
They are image tools: one saves an orthographic image of a matrix, the other projects an
image onto one. Qubicle's array tool is Duplicate Special.

### Create and voxelize

| Gap | Severity | Rizxel today | Recommendation |
|---|---|---|---|
| Standard primitives as new objects (Box, Sphere, Cylinder, Cone, Pyramid, Torus, with name/size/color and a preview) | Low | `Shape` tool draws into the active layer | Optional "Create primitive layer" dialog. |
| Terrain generator (dimension, height ≤256, roughness, smoothing, gradient coloring, sun shadows) | Low | None | Low priority for an asset editor. |
| Heightmap import (heightmap + optional texture, max height) | Medium | None | Cheap with `png_io.rs`. |
| Voxelizer (load OBJ with MTL texture, scale and rotate with live re-voxelize, convert to matrix) | Medium | `rizxel-engine/src/voxelize.rs` has no UI | Expose it as a command with scale and rotate preview. |

### File and pipeline compatibility

| Gap | Severity | Rizxel today | Recommendation |
|---|---|---|---|
| `.qb` (multi-matrix, RLE, RGBA/BGRA, left/right-handed Z) | Medium | Not supported | Highest-value Qubicle format; the full spec is in the docs. |
| `.qbt` (tree of Model/Matrix/Compound nodes, zlib, color map, pivots) | Medium | Not supported | Second; maps to layers and groups. |
| `.qef` (ASCII) | Low | Not supported | Trivial to add with `.qb`. |
| `.qbcl` / `.qmo` project files | Low | Not supported | No public spec; skip. |
| Other imports: Voxlap VOX/KV6, Paint3D, BINVOX, MCEdit Schematic, 3D-Coat Rawvox, images | Low–Medium | `.vox` (MagicaVoxel), `.gox`, `.rzxl` | Schematic and BINVOX are the most useful. |
| Mesh export: FBX, Collada DAE | Low | OBJ/MTL, STL | Prefer glTF/GLB. |
| Mesh export options: None / Non-Manifold / Manifold optimization, per-face culling, pivot and position mode, voxel size, unit scale, texture atlas (texel size, edge bleed), one file per object, filename prefixes | Medium | Greedy meshing, no user-facing options | Add an export dialog with optimization mode, face culling, voxel size and atlas options. |
| Export Cubes (one cube per voxel) | Low | None | Niche (physics). |
| Export Slices (PNG per slice) | Low–Medium | None | Also serves Slice Mode users. |
| Color map files: QSM, QCM, QCP, ASE, CSV | Low | None | CSV and ASE import/export are cheap. |

### Palette (Color Map)

Qubicle: 256 colors in 4 pages of 64, foreground and background colors with swap,
`Shift`+click multi-select, `Ctrl`+click to move, Pack, Remove Duplicates, Create Gradient
between two selected colors, Auto Name and Clear Names, RGB and name tooltips.

| Gap | Severity | Recommendation |
|---|---|---|
| Foreground/background color pair with swap | Low | Useful with Paint Bucket and erase. |
| Pack, Remove Duplicates, Create Gradient | Low–Medium | Gradient is the most-requested of these. |
| Color names (Auto Name, Clear Names) | Low | Optional. |

### Camera, view and rendering

| Gap | Severity | Rizxel today | Recommendation |
|---|---|---|---|
| Orthographic Perpendicular View (incl. "Nearest") | **High** for slice work | View-cube snap, perspective only | Add an orthographic projection; also improves Slice Mode. |
| Isometric views (8 corners, "Nearest") | Medium | None | Presets on the view cube. |
| Frame selection / Frame Work Area, Home | Low | View-cube snapping; no frame command | Add Frame Selection, Frame Work Area and Home. |
| Camera Properties (numeric) | Low | None | Optional. |
| Classic `Alt`+mouse camera controls | Low | Right-drag orbit, middle pan, wheel zoom | Optional preference. |
| Work Area settings: Shading on/off, voxel Wireframe, Grid, Bounding Boxes, Ghosts, Camera/Directional light, Global Scale, Resolution Gate, custom colors | Medium | Some toggles | Add wireframe, unshaded (flat RGB) and ghosts. |
| Render to file: background gradient, watermark, preview | Medium | `ViewMode::Render` without file output | PNG render with transparent or gradient background. |
| Visible layer opacity and materials | High | Stored but not rendered | Needed for ghosts as well. |
| Ambient occlusion and shadows | Medium | Lambert only | Add voxel AO first. |

### Application shell

| Gap | Severity | Rizxel today | Recommendation |
|---|---|---|---|
| Start screen with recent files and thumbnails | Medium | None | Comes with recent-file history. |
| Quick Access Bar with Favorites tab | Low | Toolbar | Optional. |
| Remappable shortcuts (hover a menu entry and press a key) | Medium | Fixed shortcuts | Add a keymap in Preferences. |
| Preferences (nudge size, double-click to enter, refresh rate, popup position, start with empty matrix, swatch tooltips, export oversize warning, tool wheel slots) | Medium | Settings button is a stub | Add a preferences window. |
| Status bar action, error and warning logs | Low | Status text only | Optional log panel. |
| Autosave and crash recovery | High | None | Atomic autosave plus a recovery prompt. |

## Recommended implementation order

1. ~~**Global Slice toggle**~~ — done (`2D` button, `Space`, *View ▸ Slice*).
2. ~~**Slice-layer-only editing**~~ and automatic camera-side hiding — done.
3. ~~**Slice navigation**~~ — done (slider, frame drag, Next / Previous / First / Last,
   1-based counter, slice restored on undo and redo).
4. **Line drawing:** click-anchor and `Shift`+click lines, plus a chained Line mode.
5. **Orthographic Perpendicular View** (and isometric presets) on the view cube.
6. **Clipboard** (cut/copy/paste as floating voxels) and **Rotate 90° / Flip** about the
   facing axis.
7. **Mask Mode** (protect selection). Trace Stroke needs no work: Rizxel strokes are
   already gap-free.
8. **`.qb` import/export**, then `.qbt` and `.qef`.
9. Core ops (Fill Hollows, Hollow), Booleans and Split Off between layers.
10. Visible opacity and ghosts, mesh export options, Export Slices and PNG render.
11. Autosave and recovery, preferences and keymap, start screen with recent files.

This order delivers the most recognizable Qubicle workflow — fast `Space`-driven slice
editing — on top of machinery Rizxel already has, before the larger object-model work.

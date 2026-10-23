# Changelog

All notable changes to Rizxel are documented here. Format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

## [0.2.0] — 2026-10-23

First tagged release: everything from Phase 1 to Phase 7 (milestone M1). The
sections below keep the per-phase notes, newest first.

### Phase 7: CI (2026-10-23)

#### Added
- GitHub Actions workflow (`.github/workflows/ci.yml`): `cargo fmt --check`
  on Linux, then `clippy -D warnings` and `cargo test --workspace` on
  Windows and Linux. Runs on pushes to `main` and `ui-*` and on every pull
  request.

#### Changed
- `.tmp-craft/` is ignored, so a local `craft-ui` checkout can't be
  committed by accident.

### Fixes (2026-10-23)

#### Fixed
- The model can be seen from below the ground (B1). With the camera under
  the `y = 0` plane, the ground grid wrote its depth in front of the model,
  so only the bottom faces of the lowest voxel row showed. From below, the
  grid now only clears the background (`fs_grid`), and its lines are drawn
  after the voxels as a faded overlay without depth writes
  (`fs_grid_under`, `eye_below_ground`).

### Slice Mode (2026-10-23)

Qubicle-style global slice editing. Comparison:
`docs/qubicle_feature_gap_analysis.md`.

#### Added
- **Slice Mode** from any tool, without switching tools (`14d6d52`):
  - A `2D` button in the Tools panel with a 1-based slice counter
    (`Y 3/32`).
  - `Space` toggles it: over a voxel it slices at that voxel, otherwise it
    restores the last slice.
  - *View ▸ Slice* with Slice Mode, Next, Previous, First and Last.
  - A slice slider in the viewport, and dragging the bounds frame moves the
    slice with any tool.
- While slicing, only the slice layer can be picked and edited. Brush,
  Shape, Fill, box Selection and Fuzzy Select stay on it; a mirror axis
  along the slice normal still reaches the mirrored slice.
- `/` toggles the work plane (`70d9bbd`). Hiding it (the key, the Visible
  checkbox or *View ▸ Slice ▸ Show Work Plane*) while slicing turns Slice
  Mode off and remembers it; showing the plane again resumes the same
  slice. Turning Slice Mode on another way or placing a new plane drops the
  pending resume.

#### Changed
- The camera's side of the slice is hidden every frame, so the Flip button
  is gone. The context layer is dropped and the slice sheet sits on the
  slice's far face.
- Slice moves are not undo steps; undo and redo move the slice back to the
  edited slice.

### Import & Export: MagicaVoxel, Goxel, OBJ/MTL, STL (2026-10-20)

#### Added
- **File ▸ Import** and **File ▸ Export** submenus. **Open…** and the
  command line also accept `.vox`, `.gox`, `.obj` and `.stl`. Imported
  models open in a new tab marked unsaved. Spec:
  `docs/interchange_formats.md`.
- **MagicaVoxel `.vox`** (`rizxel-io/src/format_vox.rs`):
  - Import reads the 0.99 scene graph (translations, rotations, groups,
    hidden nodes and layers).
  - Repeated instances of a model become linked clones.
  - The palette maps one to one; files without an `RGBA` chunk use
    MagicaVoxel's default palette, rebuilt exactly.
  - Export writes one model per layer, with names and hidden flags.
- **Goxel `.gox`** (`format_gox.rs`):
  - Reads 64×64 PNG blocks and layer dicts.
  - Groups are inferred from `parent_id`; a group's own voxels move to a
    child layer.
  - Shapes, materials, opacity and lock are kept. Whole-voxel clones stay
    linked; other transforms are baked.
  - Export writes shapes, clones, groups and materials natively; crops are
    baked.
- **OBJ + MTL in MagicaVoxel's layout** (`format_obj.rs`, `format_mtl.rs`):
  - Writes the `.obj`, a `palette` `.mtl` and a 256×1 palette `.png`.
  - Header, sections, CRLF, `%g` numbers, six shared normals and one `vt`
    per color, matching `chr_knight.obj`.
  - The 0.1 scale, axis conversion and centering are MagicaVoxel's.
  - Vertices are shared and sorted per normal group, and the triangle split
    matches MagicaVoxel's.
  - Re-exporting `chr_knight.vox` covers exactly the same surface as
    MagicaVoxel's own OBJ, with fewer triangles (1012 vs 1124) and a
    pixel-identical palette.
- **OBJ import**:
  - Reads polygons, negative indices, vertex colors, `Kd` and `map_Kd`
    textures.
  - A 256×1 palette texture becomes the palette.
- **MTL import** replaces the active palette and materials as one undo step
  (new `Edit::SetPalette`). **MTL export** writes the library and palette
  texture.
- **STL** (`format_stl.rs`): binary export (Z-up, 1 voxel = 1 mm); binary
  and ASCII import.
- Engine helpers:
  - `greedy_quads`: whole-volume export faces.
  - `build_color_map`: median-cut reduction to 255 colors.
  - `voxelize`: grid detection makes voxel-editor meshes import exactly;
    other meshes use parity fill plus surface rasterization and
    nearest-face colors.
- `rizxel-app`:
  - `FileFormat`, `AppState::import`, `can_export` and `export_active`.
  - Document helpers `export_scene`, `replace_palette_and_materials` and
    `mark_unsaved`.
- `cargo run -p rizxel-app --example convert -- in.vox out.obj` converts
  between formats headless.
- Tests: round trips for every format, hostile inputs, and comparisons with
  the MagicaVoxel samples and your files when present (`chr_knight.vox`,
  `chr_knight.obj`, `coba_goxel.gox`).

#### Changed
- File ▸ Export OBJ now writes the MagicaVoxel-style OBJ + MTL + PNG
  instead of vertex-colored OBJ (`export_obj` remains for generic meshes).
- `rizxel-io` depends on `png` (already in the build via `image`).

### Phase 5b: Selection, Fill, Shape & Polish (2026-10-20)

#### Added
- Selection model (`rizxel-app/src/selection.rs`): `Selection` (sparse cell
  set with an incrementally cached AABB and a revision for render caches),
  `combine` with `MaskMode::{Set, Add, Subtract}`, `visible_shell`,
  `boundary_faces`, `translated`. Each `Layer` owns its selection; it is
  undoable through `Edit::SetSelection` and never saved to `.rzxl`.
- `Document::selection/set_selection/select/clear_selection/delete_selected`
  and `commit_edit_with_selection` (voxel changes and the new selection as
  one undo step); `History::commit_transaction_with`;
  `AppState::selected_voxels()`.
- Tool algorithms in `rizxel-engine/src/tools`:
  - `select.rs`: box selection (`selection_box`, `select_box`).
  - `fuzzy.rs`: magic-wand flood with an RGBA threshold, entire-layer mode.
  - `fill.rs`: Recolor (connected or whole layer, threshold) and Fill empty
    (flood bounded by the model's box), restricted to the selection when
    there is one; `FillError` reports capped or impossible fills.
  - `shape.rs`: cube/sphere/cylinder drag (`shape_box`, `shape_cells`) with
    antialiasing and symmetry (shared with Brush).
  - `moving.rs`: `move_selection_changes` (overwrite or refuse on collision).
  - `extrude.rs`: `selection_region`, `extruded_cells` (selection extrudes
    chain: the selection grows with each extrude).
  - `quality.rs`: guarded `flood` with `MAX_FLOOD_CELLS` (2M) and
    `color_distance` / `colors_match`.
- Viewport: Selection (box drag between the pressed and hovered voxel,
  click toggles one voxel), Fuzzy Select and Fill clicks, Shape drag with a
  live preview, selection-aware Move (the outline follows the drag) and
  Extrude. `Shift`/`Ctrl` add/subtract for the selection tools, `Esc`
  deselects, `Delete` deletes the selection. Selection outline: translucent
  accent faces plus edge outline, falling back to the selection's box above
  20k faces; marquee box for Selection/Shape drags; fill start cell
  highlight; Shape anchor ghost.
- Option bar: every tool's row is enabled. Selection (mask, all layers,
  Deselect/Delete), Fuzzy Select (+ all layers), Fill (mode, connected,
  current layer, threshold, color swatch), Move (+ Overwrite).
- Status bar: selected voxel count and the selection tools' mask mode
  (following the held modifiers). Layers panel: selection badge per layer.
- `TileMeshCache` / `mesh_tile` (`rizxel-engine/src/mesher.rs`):
  incremental re-meshing of changed tiles, used by `Document::view_mesh`.
  `rizxel-app/examples/mesh_bench.rs` times a drag on a 128³ / 51k-voxel
  model (release: ~21 ms → ~4 ms per frame).
- Tests: per-algorithm unit tests (box counts, mask combination, fuzzy
  threshold boundaries, closed room / open shell fills, shape volumes for
  1³–3³, selection move collisions, tile cache == full mesh) and
  `rizxel-app/tests/phase5b_tools.rs` (undoable selections, per-layer
  selections, selection move/extrude in one step, and a randomized tool
  sequence whose undo-all / redo-all restore grids and selections exactly).

#### Changed
- `FillOptions` gains `mode` and `connected`; `MoveOptions` gains
  `overwrite`; `SelectionOptions` and `FuzzySelectOptions` gain `all_layers`.
- `Tool::is_available` is gone (every tool works); `Tool::selects` marks the
  selection tools. Tool hints describe the new modifiers.
- The extrude hover cache is keyed on the extrude options and selection too.
- `AppState::selected_voxels` is a method (was a never-updated field).

### Phase 5a: Interaction & Core Tools (2026-10-20)

#### Added
- Voxel picking (`rizxel-engine/src/raycast.rs`): `RayHit { voxel, normal, t }`,
  Amanatides–Woo DDA `raycast_grid` / `raycast_volume` clipped against an
  integer box first, `raycast_face` for plane picks and `raycast_floor`
  (hits on the floor report the virtual row `y = -1`, so `hit.adjacent()`
  is the floor cell). Zero direction components and loop bounds are guarded.
- Tool algorithms as pure functions in `rizxel-engine/src/tools`, all
  returning `VoxelChange` lists (the type moved from `rizxel-app`, which
  re-exports it):
  - `brush.rs`: cube/sphere/cylinder stamps with per-axis size,
    deterministic dithered antialiasing, surface paint, and a gapless line
    walk between drag samples.
  - `work_plane.rs`: the Plane tool's `WorkPlane` (Goxel / Qubicle
    behaviour): axis, position, visible, slice, flip; `clip_box` for the
    slice view and `pick` so drawing tools snap onto the plane.
  - `slice.rs`: `slice_volume` / `slice_layers` for the destructive Cut.
    Cuts on voxel boundaries, keeps the `+`/`-` side, and the optional cap
    puts a lid in the current color on the first discarded layer
    (cross-section with enclosed holes filled).
  - `moving.rs`: layer move, bounds resize and `drag_axis` (screen drag to
    whole voxels along the best-aligned world axis).
  - `pick.rs`, `extrude.rs` (contiguous exposed-face region, optional color
    threshold, extrude out or carve in), `symmetry.rs`, `quality.rs`
    (`apply_op` and the bounds/no-op `filter_placeable`).
- `ToolContext`, `KeepSide`, `SliceRequest`, `WorkPlane`; `PlaneOptions` is
  now just `cap` (the plane itself is per-document view state,
  `Document::plane`); `ExtrudeOptions` gains `depth`.
- Undo transactions: `History::begin_transaction/record/revert_pending/
  commit_transaction/cancel_transaction` and `Document::begin_edit/
  edit_voxels/revert_pending/commit_edit/cancel_edit`. One drag is one undo
  step; cancel restores the grid exactly; an empty drag records nothing.
  Other edits, undo, redo and save commit an open transaction first; tab
  switches and opening files cancel it.
- `Document::slice` / `cut_hidden_side` (visible, unlocked voxel layers, one
  undo step), `view_clip`, `view_mesh` (slice-clipped, closed cut face),
  `editable_layer`, `merged_visible_volume`.
- Plane tool (reference: Goxel `tools/plane.c`, Qubicle slice mode): it never writes voxels. A click places the
  work plane on the clicked face and shows it; while visible, Brush (and the
  Cursor readout) snap onto it so voxels can be drawn in empty space. Slice
  works like Qubicle's 2D slice mode: it selects one slice layer that
  drawing tools pick from either side; they also edit the visible voxels on
  both sides of the plane (nearest hit wins), never the hidden ones, and
  edits merge into the model (latest edit wins). The view keeps
  the slice layer, the side behind it and one more layer beyond it; the
  rest is hidden (view only, Flip hides the other side, Cut deletes it).
  The option bar shows a
  Layer counter with -/+ while slicing. Hovering the plane's edge or corner
  shows the 3D arrow there as a drag hint; dragging it moves the plane
  (with or without Slice). Non-slice
  plane picks target the cell on the camera's side for add and erase alike.
  It also draws the yellow bounds with thick lines and the plane as a grey
  quad with a thick outline in the axis color (X red, Y green, Z blue) plus
  a shaded 3D arrow gizmo of constant screen size,
  shown only while an edge is hovered or dragged. With Slice on, dragging moves the plane along its normal and a
  click jumps it to a face. Option bar: Visible, Axis, Position, Slice,
  Flip, Reset, Cut (+ Cap).
- Viewport tool input (`rizxel-ui/src/tool_input.rs`): left-drag state
  machine for Brush, Move (Move/Resize), Extrude and the slice plane, plus
  Color Picker. `Shift` subtracts, `Alt` picks a color with any tool, `Ctrl`
  copies the picked color into the swatch, `Esc` or losing focus cancels.
  Drags pick against a snapshot taken at press time.
- Previews: hovered face highlight, translucent brush ghosts (capped at
  4096 cells), extrude region highlight, work plane overlay.
- Option bar: Plane rows (see above), extrude depth;
  Shape/Selection/Fuzzy Select/Fill rows are disabled until Phase 5b.
  Status bar shows live tool feedback (slice plane, pending change count).
- Tool shortcuts (plain letters, ignored while a text field has focus):
  V Cursor, B Brush, S Shape, P Plane, M Move, C Color Picker, R Selection,
  W Fuzzy Select, E Extrude, F Fill. Toolbar tooltips show them.
- Tests: raycast cases, brush counts per shape, plane rectangles, slice
  counts/axes/offset/cap/holes, move, extrude, symmetry, and
  `rizxel-app/tests/phase5a_tools.rs` (transactions, random undo/redo,
  multi-layer slice with hidden/locked layers).

#### Changed
- Tool hints describe the modifier keys; Plane has a separate hint while
  slicing (`Tool::hint_with`). Plane no longer uses Add/Subtract/Join/Paint.
- `VoxelOp::Join` adds into empty cells only (existing colors win);
  `Add` overwrites.

### `.rzxl` v3: Layer Data (2026-10-19)

#### Added
- `.rzxl` **version 3** (spec: `docs/rzxl_format.md`). Layers gain a stable
  `id`, `opacity`, `parent` (group), `material`, a non-destructive `crop`
  box, and a tagged `kind`: `voxels`, `group`, `clone` (linked clone of a
  voxel/shape layer by id, with an offset) or `shape` (cube, sphere or
  cylinder inscribed in a box). Projects gain a `materials` list (metallic,
  roughness, emission, tint) and a `generator` string.
- `rizxel-math/src/layer.rs`: shared `LayerId`, `LayerKind`, `ShapeKind`,
  `VoxelBox` and `Material` types. `VoxelVolume::translated` / `cropped`.
- `rizxel-engine/src/shapes.rs`: `rasterize_shape` for shape layers.
- `ProjectFile::validate`: unique non-zero ids, parents that exist and are
  groups (no cycles, depth ≤ 32), clone sources that exist and are
  voxel/shape layers, opacity/material ranges, valid crop/shape boxes,
  palette shape colors, no runs on non-voxel layers, plus caps of 1024
  layers, 256 materials and 256-character names. Runs on every read and
  every write, so a save can never produce a file that fails to load. New
  `RzxlError::Invalid` variant (malformed voxel runs report through it too).
- Document API: `add_group`, `add_linked_clone`, `add_shape_layer`,
  `set_layer_props` (one undo step, slider drags merge), `set_materials`,
  `can_delete_active_layer`, and `with_visible_geometry`, which evaluates
  shapes, clones, crops and group visibility once per revision. New
  `Edit::SetLayerProps` and `Edit::SetMaterials`.
- Tests: `rizxel-app/tests/layer_features.rs` plus format tests (v3 round
  trip, v2 migration, every validation rule). The sample model example now
  includes a group, a sphere shape layer and a cropped linked clone.

#### Changed
- Version 1/2 files load through migration (layers get ids `1..=n`). Files
  newer than version 3 are still rejected rather than partially loaded.
- Mesh, voxel count, occupied bounds and OBJ export use the evaluated
  geometry, so shapes, linked clones, crops and hidden groups show
  correctly. Opacity and materials are stored but not rendered yet.
- Layer operations keep links intact:
  - Deleting a group moves its children up one level.
  - A layer with linked clones cannot be deleted, and the delete button is
    disabled for it.
  - Merges bake shapes and clones, and are refused when a surviving layer
    still links to a merged one. "Delete hidden" is refused in the same case.
  - Only voxel layers accept voxel edits.

### Phase 4: Core Engine & Data (2026-10-19)

#### Added
- `rizxel-math`: `Voxel = u8` palette index (`EMPTY_VOXEL = 0`,
  `FIRST_COLOR_INDEX = 1`), `Palette::is_selectable`, and `volume.rs` with
  the Goxel-style tiled `VoxelVolume` (16³ tiles in a hash map keyed by tile
  origin, empty tiles never stored, shared static empty tile for misses,
  `occupied_bounds`, `tiles_in`, O(tiles) `resize`).
- `rizxel-engine`: `greedy_mesh` / `mesh_volume` — per-tile greedy meshing
  over 3 axes × 2 directions, faces culled against neighbours across tiles,
  out-of-bounds treated as empty (closed shell), per-color quad merging, CCW
  winding from outside, deterministic output (sorted tiles). Hidden layers
  are skipped; overlapping visible layers resolve to the later layer.
- `rizxel-io`: `.rzxl` **version 2** — each layer stores `voxels` as RLE
  `VoxelRun { start, len, value }` over the bounds in Z-X-Y order (`y`
  fastest). `runs_from_volume` / `volume_from_runs` validate runs (inside the
  bounds, sorted, non-overlapping, value ≠ 0). Version 1 files still load
  with empty layers; newer versions are rejected.
- `rizxel-app`: `Layer::volume`; `Edit::Voxels` (with `VoxelChange`) and
  per-layer coalescing so a drag is one undo step; `Document::set_voxels`,
  `select_color`, `visible_mesh`, `visible_voxel_count`, `occupied_bounds`,
  and `id()` / `revision()` for caching; `AppState::export_active_obj`.
- `rizxel-ui`: opaque voxel pipeline (`VoxelVertex`, dynamic vertex + index
  buffers, back-face culling, 4× MSAA, depth) with `vs_voxel` / `fs_voxel`:
  Render mode is Lambert key light + 0.35 ambient + half-Lambert fill, Model
  mode keeps palette colors with a light per-axis tint. Meshes are cached per
  document and rebuilt once per revision (`MeshCache`); the GPU upload happens
  only when the mesh key changes.
- File ▸ **Export OBJ…** is enabled when the visible layers hold voxels and
  exports only visible layers (vertex colors included).
- Status bar shows `Voxels: N` for the visible layers.
- `rizxel <file.rzxl>` opens a project at startup;
  `cargo run -p rizxel-app --example sample_model` writes a demo project.
- Phase 4 smoke test (`rizxel-app/tests/phase4_smoke.rs`): edit, undo, redo,
  save/load, mesh and OBJ export.

#### Changed
- Shrinking the bounds is now undoable with voxel content: `Edit::SetBounds`
  records the clipped voxels and restores them on undo (merged bound drags
  stay consistent on redo).
- Palette slot 0 is drawn as a struck-out "empty" swatch and can no longer be
  selected; loaded projects with `selected_color = 0` move to slot 1.
- The viewport draws a dimmer box around the occupied voxels (when it is
  smaller than the bounds) inside the yellow bounds wireframe.
- `Document::from_project` returns `Result` (malformed voxel runs fail the
  load instead of producing a broken document).
- Architecture plan: Phase 4 marked done; L0/L1 descriptions updated (u8
  voxels in tiles, in-house greedy mesher instead of `block_mesh`); local
  reference clones of CADCraft (`cadcraft`, with its current crate
  layering) and Goxel are listed.

### UI Optimization: View Cube, Zen Mode, Resizable Panels

#### Added
- Enhanced view cube with 26 clickable view targets in the Goxel style: the
  6 faces, the 12 edges (45° diagonal views) and the 8 corners
  (three-quarter views). Edges render as thick accent lines and corners as
  accent dots; corners and edges take hit-test priority over faces.
- `ViewTarget` (`Face`/`Edge`/`Corner`) with `ViewEdge` (12) and
  `ViewCorner` (8) enums and camera snap angles in `rizxel-app`.
- View menu lists all 26 targets grouped into Faces / Edges / Corners.
- Zen mode (`Ctrl+Shift+F` or View menu) hides the option bar, tools panel
  and sidebar, leaving the title bar, tabs, viewport (with view cube and
  axis gizmo) and status bar.

#### Changed
- Tools panel and sidebar are resizable: the tools panel can grow from 48 to
  200 pt and the sidebar from 200 to 400 pt via their separator lines; the
  sidebar no longer paints a duplicate border.

### Phase Plans: 4, 5a, 5b (2026-10-19)

#### Added
- `docs/plan/rizxel_phase4_implementation_plan.md` — Core Engine & Data: tiled
  `VoxelVolume` (Goxel-style 16³ hash-mapped tiles), greedy meshing, lit GPU
  voxel pipeline, `.rzxl` version 2 with RLE voxel runs, OBJ export enablement.
- `docs/plan/rizxel_phase5a_implementation_plan.md` — Interaction & Core Tools:
  DDA voxel raycast, Cursor/Brush/Plane/Move/Color Picker/Extrude, undo
  transactions (one drag = one step), hover and stamp previews. Includes the
  Plane **Slice** mode (§4.3a): axis-aligned cut with keep-side, offset and
  optional cap, previewed as a translucent cut plane, undoable as one step.
- `docs/plan/rizxel_phase5b_implementation_plan.md` — Selection, Fill, Shape &
  Polish: per-layer selection with Set/Add/Subtract mask modes, Selection,
  Fuzzy Select, Fill, Shape, selection-aware Move/Extrude, option-bar
  completion and the performance pass.

#### Changed
- `docs/plan/rizxel_native_architecture_plan.md` splits Phase 5 into 5a and 5b and
  links each phase heading to its implementation plan.

### Phase 3: WGPU Viewport (2026-10-19)

#### Added
- Native wgpu viewport renderer (`rizxel-ui/src/render/`) embedded in the
  central panel through `egui_wgpu::Callback`:
  - Offscreen 4× MSAA color + `Depth32Float` targets, resolved and blitted
    into egui's render pass; recreated on resize.
  - `scene.wgsl`: procedural infinity grid (`#aeaeae` minor/major lines,
    `fwidth` anti-aliasing, red X / blue Z axes, distance and grazing-angle
    fade, depth output), instanced thick screen-space lines (yellow model
    bounds, hover outline) and translucent fills (hovered ground cell).
  - Render mode toggle switches to a gradient background with a dimmer grid.
- `OrbitCamera` matrix helpers (`eye`, `view_matrix`, `projection_matrix`,
  `view_proj`, `FOV_Y`/`NEAR`/`FAR`) shared by the GPU and CPU picking.
- Tests: WGSL validation with naga, GPU struct layout sizes, grid fade
  clamping, color conversion.
- Dependencies: `bytemuck` (rizxel-ui), `naga` 30 (dev, WGSL validation).

#### Changed
- `viewport.rs` no longer paints the grid/bounds/hover cell with the egui
  painter; it builds a `SceneFrame` for the GPU. The view cube and axis gizmo
  remain egui overlays.
- `docs/rizxel_phase1-2_implementation_plan.md` renamed to
  `docs/plan/rizxel_phase1-3_implementation_plan.md` and extended with Phase 3.

### UI optimization (2026-10-19)

#### Added
- Per-tool option bar parameters after the Paint button, modeled on Goxel's
  tool panels (`rizxel-engine/src/tools/options.rs`, `ToolOptions`):
  - Cursor: Show names (On Hover / Always).
  - Brush: shape (Cube/Sphere/Cylinder), size X/Y/Z, Surface paint,
    Antialias, Symmetry X/Y/Z.
  - Shape: shape, Planar, Antialias, Symmetry X/Y/Z.
  - Plane: Slice, Relative/Absolute, Offset (unchanged).
  - Move: Drag mode (Move / Resize).
  - Color Picker: Sample all layers.
  - Selection: Mask (Set / Add / Sub).
  - Fuzzy Select: Entire layer, Use color + Threshold, Mask.
  - Extrude: Confine to layer, Use color + Threshold.
  - Fill: Current layer only, Threshold.
  Each tool keeps its own settings when switching.
- Option bar scrolls horizontally (mouse wheel + chevrons) when its content
  does not fit; the Model/Render toggle stays pinned right. The scroll strip
  is a shared widget (`widgets::hscroll_strip`) also used by the tab bar.
- Camera pan (`OrbitCamera::pan`, `pan_screen`, `reset`).

#### Changed
- Window opens windowed (not maximized) at 1080×640; minimum 800×480.
- Option bar Undo/Redo/Add/Subtract/Join/Paint use the pixel-art icons.
  Add/Subtract/Join/Paint are dimmed for tools they do not affect.
- Viewport controls: wheel = zoom, right drag = orbit, middle drag = pan,
  middle double-click = reset camera. Left button is reserved for tools.

### UI polish (2026-10-19)

#### Added
- Pixel-art tool icons: `docs/Pixel-Art Voxel Editor Icon Sheet-2.png` sliced
  into 17 transparent 128×128 PNGs in `assets/icons/` (tools, settings, undo,
  redo, add, subtract, join, paint). The toolbar uses the 10 tool icons and
  the settings icon, loaded as mipmapped textures (`rizxel-ui/src/icons.rs`).
- Application icon: window icon from `assets/Rizxel_icon_256x256.png`, plus a
  multi-size `assets/Rizxel_icon.ico` (16–256 px) embedded into the Windows
  executable through `rizxel-ui/build.rs` (`winresource`).
- Document tabs scroll horizontally when they overflow: mouse wheel scrolling,
  left/right chevron buttons, auto-scroll to the active/new tab, and a "+"
  button pinned outside the scroll area.

#### Changed
- Option bar height 40 → 48 px.
- Model/Render toggle sized to its text (`px-3 py-1`) instead of stretching to
  the option bar's full height.

### Phase 1–2: Workspace & UI Skeleton (2026-10-18)

Project restarted as a pure Rust application. The previous Tauri/React
prototype is discontinued.

#### Added

##### Workspace
- Cargo workspace with 5 layered crates (`ui → app → io → engine → math`),
  shared `[workspace.dependencies]`, `unsafe_code = "forbid"`, clippy `all`.
- `rust-toolchain.toml` (stable + rustfmt + clippy), `.gitignore`.
- Dependencies optimized (`opt-level = 2`) in dev builds for a responsive UI.

##### L0 `rizxel-math`
- `Rgba8` (Pod, hex parse/format, HSL conversion).
- `Palette` (max 256 colors). Default = 56 swatches generated exactly like the
  HTML prototype, with `#38BDF8` pre-selected.
- `BoundSize` (default 32³, clamped `1..=256`), `Aabb` (voxel/point containment).

##### L1 `rizxel-engine`
- `Tool` (10 tools with labels and status-bar hints), `VoxelOp`
  (Add/Subtract/Join/Paint), `PlaneOptions` (Slice, Relative/Absolute, Offset).
- `Mesh` container (+ `unit_cube` helper) for OBJ export and Phase 4 meshing.
- `Ray` with slab ray/AABB intersection (Phase 5 raycasting foundation).

##### L2 `rizxel-io`
- `.rzxl` v1 project format: `RZXL` magic, `u32` version, `u32` body length,
  zstd-compressed JSON (`ProjectFile`). Atomic save (temp file + rename),
  validation of magic, version, declared length and a 512 MiB body limit.
- OBJ exporter with vertex colors (`v x y z r g b`) and optional normals.

##### L3 `rizxel-app`
- `Document` (layers, palette, bounds, selection, dirty tracking via revisions).
- `History` undo/redo with reversible `Edit` commands and merging of continuous
  drags (bounds, palette color) into a single undo step; 512-step cap.
- `AppState` (tabs, active tool/op, plane options, view mode, orbit camera,
  cursor voxel, status message) with open/save/close logic.

##### L4 `rizxel-ui`
- Frameless `eframe` window (wgpu renderer), custom title bar (drag, double-click
  maximize, minimize/maximize/close), edge & corner resize zones, 1px border.
- Embedded Inter (Regular/SemiBold, OFL) and Phosphor icons.
- Dark theme matching `docs/rizxel_ui.html` color tokens.
- Panels: menu bar (File/Edit/View/Help), option bar, document tabs (dirty dot,
  middle-click close), left toolbar with blue active accent, right sidebar
  (Color Palette with editable selected color, Layers with eye/lock/add/delete/
  rename, yellow Bound Model with X/Y/Z), status bar (coords, selection, bounds,
  tool hint / messages).
- Placeholder viewport (egui painter): radial-fade infinity grid, yellow bounds
  wireframe, ground-cell hover pick feeding status coordinates, interactive view
  cube (hover highlight, click to snap), XYZ axis gizmo, orbit (drag) and zoom
  (scroll).
- Shortcuts: Ctrl+N/O/S, Ctrl+Shift+S, Ctrl+Z, Ctrl+Y / Ctrl+Shift+Z.
- About and Keyboard Shortcuts dialogs.

#### Notes
- "Export OBJ…" is visible but disabled until voxel meshing exists (Phase 4).
- Dependency pins: `eframe`/`egui` 0.36 (required by `egui-phosphor` 0.14),
  `glam` 0.34 (camera functions live in `glam::camera`).

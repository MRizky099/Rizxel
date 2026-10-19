# Changelog

All notable changes to Rizxel are documented here. Format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased] — `.rzxl` v3: Layer Data (2026-10-19)

### Added
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

### Changed
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

## [Unreleased] — Phase 4: Core Engine & Data (2026-10-19)

### Added
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

### Changed
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

## [Unreleased] — UI Optimization: View Cube, Zen Mode, Resizable Panels

### Added
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

### Changed
- Tools panel and sidebar are resizable: the tools panel can grow from 48 to
  200 pt and the sidebar from 200 to 400 pt via their separator lines; the
  sidebar no longer paints a duplicate border.

## [Unreleased] — Phase Plans: 4, 5a, 5b (2026-10-19)

### Added
- `docs/rizxel_phase4_implementation_plan.md` — Core Engine & Data: tiled
  `VoxelVolume` (Goxel-style 16³ hash-mapped tiles), greedy meshing, lit GPU
  voxel pipeline, `.rzxl` version 2 with RLE voxel runs, OBJ export enablement.
- `docs/rizxel_phase5a_implementation_plan.md` — Interaction & Core Tools:
  DDA voxel raycast, Cursor/Brush/Plane/Move/Color Picker/Extrude, undo
  transactions (one drag = one step), hover and stamp previews. Includes the
  Plane **Slice** mode (§4.3a): axis-aligned cut with keep-side, offset and
  optional cap, previewed as a translucent cut plane, undoable as one step
  (reference: `docs/Slice panel feature.png`).
- `docs/rizxel_phase5b_implementation_plan.md` — Selection, Fill, Shape &
  Polish: per-layer selection with Set/Add/Subtract mask modes, Selection,
  Fuzzy Select, Fill, Shape, selection-aware Move/Extrude, option-bar
  completion and the performance pass.

### Changed
- `docs/rizxel_native_architecture_plan.md` splits Phase 5 into 5a and 5b and
  links each phase heading to its implementation plan.

## [Unreleased] — Phase 3: WGPU Viewport (2026-10-19)

### Added
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

### Changed
- `viewport.rs` no longer paints the grid/bounds/hover cell with the egui
  painter; it builds a `SceneFrame` for the GPU. The view cube and axis gizmo
  remain egui overlays.
- `docs/rizxel_phase1-2_implementation_plan.md` renamed to
  `docs/rizxel_phase1-3_implementation_plan.md` and extended with Phase 3.

## UI optimization (2026-10-19)

### Added
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

### Changed
- Window opens windowed (not maximized) at 1080×640; minimum 800×480.
- Option bar Undo/Redo/Add/Subtract/Join/Paint use the pixel-art icons.
  Add/Subtract/Join/Paint are dimmed for tools they do not affect.
- Viewport controls: wheel = zoom, right drag = orbit, middle drag = pan,
  middle double-click = reset camera. Left button is reserved for tools.

## UI polish (2026-10-19)

### Added
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

### Changed
- Option bar height 40 → 48 px.
- Model/Render toggle sized to its text (`px-3 py-1`) instead of stretching to
  the option bar's full height.

## Phase 1–2: Workspace & UI Skeleton (2026-10-18)

Project restarted as a pure Rust application. The previous Tauri/React
prototype is discontinued.

### Added

#### Workspace
- Cargo workspace with 5 layered crates (`ui → app → io → engine → math`),
  shared `[workspace.dependencies]`, `unsafe_code = "forbid"`, clippy `all`.
- `rust-toolchain.toml` (stable + rustfmt + clippy), `.gitignore`.
- Dependencies optimized (`opt-level = 2`) in dev builds for a responsive UI.

#### L0 `rizxel-math`
- `Rgba8` (Pod, hex parse/format, HSL conversion).
- `Palette` (max 256 colors). Default = 56 swatches generated exactly like the
  HTML prototype, with `#38BDF8` pre-selected.
- `BoundSize` (default 32³, clamped `1..=256`), `Aabb` (voxel/point containment).

#### L1 `rizxel-engine`
- `Tool` (10 tools with labels and status-bar hints), `VoxelOp`
  (Add/Subtract/Join/Paint), `PlaneOptions` (Slice, Relative/Absolute, Offset).
- `Mesh` container (+ `unit_cube` helper) for OBJ export and Phase 4 meshing.
- `Ray` with slab ray/AABB intersection (Phase 5 raycasting foundation).

#### L2 `rizxel-io`
- `.rzxl` v1 project format: `RZXL` magic, `u32` version, `u32` body length,
  zstd-compressed JSON (`ProjectFile`). Atomic save (temp file + rename),
  validation of magic, version, declared length and a 512 MiB body limit.
- OBJ exporter with vertex colors (`v x y z r g b`) and optional normals.

#### L3 `rizxel-app`
- `Document` (layers, palette, bounds, selection, dirty tracking via revisions).
- `History` undo/redo with reversible `Edit` commands and merging of continuous
  drags (bounds, palette color) into a single undo step; 512-step cap.
- `AppState` (tabs, active tool/op, plane options, view mode, orbit camera,
  cursor voxel, status message) with open/save/close logic.

#### L4 `rizxel-ui`
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

### Notes
- "Export OBJ…" is visible but disabled until voxel meshing exists (Phase 4).
- Dependency pins: `eframe`/`egui` 0.36 (required by `egui-phosphor` 0.14),
  `glam` 0.34 (camera functions live in `glam::camera`).

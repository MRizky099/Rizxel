# Changelog

All notable changes to Rizxel are documented here. Format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

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

# Rizxel — Phase Summaries

A short summary of each development phase. For full details, see each phase's
implementation plan and [`CHANGELOG.md`](CHANGELOG.md).

| Phase | Theme | Status | Commit |
|---|---|---|---|
| 1–2 | Workspace and UI skeleton | ✅ Done | `658a57a` |
| 3 | WGPU viewport | ✅ Done | `9ef40dc` |
| UI | Docking, view cube, Zen mode, icons | ✅ Done | `90632d3`, `eacbd28` |
| 4 | Core engine and voxel data | ✅ Done | `71aa78e` |
| 4.5 | `.rzxl` v3 layer data | ✅ Done | `8d982ed` |
| 5a | Interaction and core tools | ✅ Done | `8315e37`, `ea7a566` |
| 5b | Selection, fill, shape and performance | ✅ Done | `4b6b149` |
| 6 | Import and export | ✅ Done | `363f73d` |
| Slice | Qubicle-style Slice Mode | ✅ Done | `04bbdf7`, `d6f13b2` |
| 7 | Stabilize, CI and 0.2.0 | ✅ Done | `v0.2.0` |
| 8 | Safe editing and 0.3.0 | ✅ Done | `v0.3.0` |
| 9 | Visual fidelity and 0.4.0 | ✅ Done | `v0.4.0` |

---

## Phase 1–2 — Workspace and UI skeleton

Plan: [`rizxel_phase1-3_implementation_plan.md`](plan/rizxel_phase1-3_implementation_plan.md)

**Goal:** restart the project as pure Rust, replacing the Tauri/React prototype, and
reproduce `rizxel_ui.html` one to one in egui.

**Delivered**
- A five-crate Cargo workspace (`ui → app → io → engine → math`) with
  `unsafe_code = "forbid"` and clippy warnings on.
- L0 types: `Rgba8`, `Palette` (56 default swatches), `BoundSize` (32³ by default, up to
  256), `Aabb`.
- `.rzxl` v1: `RZXL` magic, version, length and a zstd JSON body. Saves are atomic and
  the body is capped at 512 MiB.
- `History` undo/redo with reversible `Edit` commands, merged drags and a 512-step cap.
- A frameless window with a custom title bar and resize zones, plus the menu bar,
  option bar, tabs, toolbar, sidebar (palette, layers, bounds) and status bar.
- A placeholder viewport drawn with the egui painter, with the view cube and axis gizmo.

## Phase 3 — WGPU viewport

**Goal:** replace the painter placeholder with a native wgpu render pass.

**Delivered**
- An offscreen 4× MSAA + `Depth32Float` pass blitted into egui through
  `egui_wgpu::Callback`.
- `scene.wgsl`: a procedural infinity grid (anti-aliased with `fwidth`, axis lines,
  distance fade), instanced thick screen-space lines and translucent fills.
- Shared `OrbitCamera` matrices for GPU rendering and CPU picking.
- WGSL is validated with `naga` in tests.

## UI optimization and docking

**Delivered**
- 26 view-cube targets (6 faces, 12 edges, 8 corners), Zen mode (`Ctrl+Shift+F`) and
  resizable panels.
- Pixel-art tool icons, the application icon and scrolling tab and option strips.
- Each tool keeps its own option settings, modelled on Goxel's tool panels.
- Dockable, floating workspace panels through `craft-ui`, shared with CADCraft. The
  layer panel follows Goxel's style.

## Phase 4 — Core engine and data

Plan: [`rizxel_phase4_implementation_plan.md`](plan/rizxel_phase4_implementation_plan.md)

**Goal:** real voxel data, meshing and rendering.

**Delivered**
- `VoxelVolume`: a Goxel-style sparse volume of 16³ tiles stored in a hash map. Voxels
  are `u8` palette indices.
- `greedy_mesh`: greedy meshing per tile, with faces culled across tiles and
  deterministic output.
- A lit voxel pipeline. Render mode uses Lambert, ambient and fill lighting; Model mode
  uses flat palette colors. The mesh is cached per revision.
- `.rzxl` v2 stores voxels as RLE runs in the zstd body. OBJ export is enabled.
- Shrinking the bounds can be undone without losing voxels.

## Phase 4.5 — `.rzxl` v3 layer data

Spec: [`rzxl_format.md`](rzxl_format.md)

**Delivered**
- Each layer gains a stable `id`, opacity, a parent group, a material, a crop box and a
  `kind`: voxels, group, linked clone or shape.
- A project-level `materials` list.
- `ProjectFile::validate` runs on every read and write. It checks ids, cycles, clone
  sources, ranges and caps.
- The document API covers groups, clones, shape layers and layer properties, with
  evaluated geometry cached per revision.

## Phase 5a — Interaction and core tools

Plan: [`rizxel_phase5a_implementation_plan.md`](plan/rizxel_phase5a_implementation_plan.md)

**Goal:** make the model editable.

**Delivered**
- Amanatides–Woo DDA raycasting (`raycast_grid`, `raycast_volume`, `raycast_floor`).
- Pure-function tools: Brush (cube, sphere and cylinder stamps, antialiasing, surface
  paint, gapless strokes), Move and Resize, Color Picker, Extrude (out or in, with a
  color threshold) and Symmetry.
- **Plane tool**: a work plane that drawing tools snap onto, and a Qubicle-style Slice
  mode that edits both sides of the plane. A destructive Cut with an optional cap, and a
  3D arrow gizmo for dragging the plane.
- Undo transactions, so one drag is one undo step and Cancel restores the model exactly.
- Hover, ghost and extrude previews, the `Shift`/`Alt`/`Ctrl` modifiers and
  single-letter tool shortcuts.

## Phase 5b — Selection, fill, shape and polish

Plan: [`rizxel_phase5b_implementation_plan.md`](plan/rizxel_phase5b_implementation_plan.md)

**Goal:** finish the toolset.

**Delivered**
- A `Selection` per layer (a sparse cell set with a cached AABB), with Set / Add /
  Subtract masks. Selection changes can be undone but aren't saved.
- Tools: box Selection, Fuzzy Select (magic wand with an RGBA threshold), Fill (recolor,
  or fill empty cells with a bounded flood), Shape (drag a cube, sphere or cylinder), and
  Move and Extrude that act on the selection.
- A guarded flood fill capped at 2 million cells.
- Selection rendering (faces and outline, falling back to the box above 20k faces) and a
  marquee.
- `TileMeshCache` re-meshes only changed tiles. A drag on a 128³ model drops from about
  21 ms to 4 ms per frame.
- A randomized undo-all / redo-all test that restores grids and selections exactly.

## Phase 6 — Import and export

Spec: [`interchange_formats.md`](interchange_formats.md)

**Delivered** (`363f73d`)
- MagicaVoxel `.vox`: import reads the scene graph, and repeated instances become linked
  clones. Export writes one model per layer.
- Goxel `.gox`: import reads PNG blocks and layer dicts, and groups, shapes, materials
  and clones survive the round trip.
- OBJ + MTL + palette PNG in MagicaVoxel's exact layout. Re-exporting `chr_knight.vox`
  gives 1012 triangles against MagicaVoxel's 1124, with a pixel-identical palette.
- OBJ import (vertex colors, `Kd`, `map_Kd`), MTL palette import as one undo step, and
  STL import and export.
- `voxelize`: meshes from voxel editors import exactly on their own grid. Other meshes
  are voxelized with a parity fill.
- `build_color_map` reduces imported colors to the palette with median cut.
- A headless `convert` example, round-trip tests and tests with hostile inputs.

## Slice Mode

Analysis: [`qubicle_feature_gap_analysis.md`](qubicle_feature_gap_analysis.md)

**Goal:** Qubicle-style slice editing from any tool, based on hands-on tests with
Qubicle 3.1.2.

**Delivered** (`04bbdf7`, `d6f13b2`)
- A `2D` button with a 1-based slice counter, `Space` from any tool (slices at the
  hovered voxel or restores the last slice) and *View ▸ Slice* with Next, Previous,
  First and Last.
- Only the slice layer can be picked and edited; Brush, Shape, Fill, box Selection and
  Fuzzy Select stay on it, and mirroring along the slice normal still works.
- The camera's side of the slice is hidden every frame. A viewport slider and the
  bounds frame move the slice; slice moves aren't undo steps.
- `/` hides the work plane and suspends Slice Mode; showing it again resumes the slice.

## Phase 7 — Stabilize, CI and 0.2.0

Plan: [`rizxel_phase7_implementation_plan.md`](plan/rizxel_phase7_implementation_plan.md)

**Delivered** (tag `v0.2.0`)
- B1 fixed (`c68aa10`): the model is visible from below the ground plane. From below,
  the grid writes no ground depth and its lines are drawn over the model as a faded
  overlay.
- GitHub Actions CI (`fmt`, `clippy -D warnings`, tests on Windows and Linux), a CI
  badge and a `.tmp-craft/` ignore rule.
- Everything since Phase 4 merged into `main` (PR #2), version `0.2.0`, one `[0.2.0]`
  CHANGELOG section and the `v0.2.0` tag.
- Planning documents moved to a local-only `docs/plan/`; screenshots of third-party
  applications removed from the repository history.

## Phase 8 — Safe editing and 0.3.0

Plan: [`rizxel_phase8_implementation_plan.md`](plan/rizxel_phase8_implementation_plan.md)

**Delivered** (PR #3, tag `v0.3.0`)
- Save / Don't Save / Cancel prompt before unsaved work is closed, from the tab, *File ▸
  Exit*, the title bar, `Alt+F4` and the OS (`e6e4a21`), and `name*.rzxl` on unsaved
  tabs (`187beb4`).
- App data folder and *File ▸ Open Recent* (`7fdd4c8`).
- Autosave every 2 minutes or 50 edits on a background thread, with a Restore / Discard
  dialog after a crash (`e2d69d8`, `be3f4ae`).
- Copy, cut, a floating paste that follows the cursor, paste in place and paste to a
  new layer, with palette mapping across documents (`6aa12e4`).
- Rotate 90°, flip X/Y/Z and a one-shot mirror for the selection or layer (`50f2000`).
- Complete Edit menu with Select All, Invert and Deselect, and an expanded Keyboard
  Shortcuts window (`18bba53`).
- The randomized undo-all / redo-all test covers paste, cut and every transform, each
  one undo step (`8131880`); 261 tests.

## Phase 9 — Visual fidelity and 0.4.0

Plan: [`rizxel_phase9_implementation_plan.md`](plan/rizxel_phase9_implementation_plan.md)

**Delivered** (PR #4, tag `v0.4.0`)
- Orthographic camera with `Numpad 5`, a view cube toggle, Auto Ortho on face and corner
  snaps, true isometric corners, parallel picking rays, and Frame Selection, Frame Work
  Area and Home View (`fd7d3a8`).
- View toggles for the floor grid, a voxel grid drawn on the model's faces in the
  shader, and the model bounds, in *View ▸ Show* and on viewport buttons (`709c51c`).
- Layer opacity in a sorted translucent pass with per-draw alpha (no re-mesh on opacity
  changes) and Ghosts (`54556fd`).
- Material preview in Render mode: per-vertex material slots, a 256-entry material
  table, tint, emission and a roughness/metallic highlight (`dc052ad`).
- Per-vertex ambient occlusion in the greedy mesher with AO-aware merging and diagonal
  flips; neighbours re-mesh only when a change touches their side (`22600dd`).
- Background tile meshing pool for imports and undo of big edits; small edits stay
  synchronous (`e4bafa4`); 289 tests.

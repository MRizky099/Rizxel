# Changelog

All notable changes to Rizxel are documented here. Format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

Phase 12 (milestone M6, the 0.9.0 beta): settings, saved layout, start screen,
packages, file association.

### Added
- **Settings dialog** (toolbar gear, *File ▸ Settings…*, `Ctrl+,`) with General (UI
  scale, undo steps per document, autosave interval or off, start screen), Viewport
  (start-up floor and voxel grid, projection, Model-mode background, camera controls),
  Keymap and Paths (default export folder) tabs. Stored in `settings.json` in the app
  data folder: versioned, unknown keys kept, a bad value falls back to its default.
- **Remappable keymap**: every shortcut is an action with one or more chords. Click a
  chord and press the new keys, add or remove chords, reset one or all; conflicts are
  shown and block OK. Menus, tooltips and *Help ▸ Keyboard Shortcuts* show the current
  keys. `Ctrl+C` / `X` / `V`, `Enter` and `Esc` stay fixed.
- **Alt + mouse camera controls** (Settings ▸ Viewport): `Alt` + left drag orbits,
  `Alt` + middle pans, `Alt` + right zooms.
- **Saved dock layout and window state**: both layouts (with closed and collapsed
  panels) and the window's size, position and maximized flag go to `layout.json` and
  come back on the next start; a broken file falls back to the default layout.
- **Start screen** with New, Open and the recent files as cards with thumbnails, which
  are rendered whenever a project is saved or opened.
- **Packages** built by a tagged release workflow: Windows MSI (per user, no admin
  rights) and portable zip, Linux AppImage, macOS universal dmg. Every package ships
  `LICENSE`, `THIRD_PARTY_LICENSES.html` (`cargo about`, GPL-compatible allow-list
  checked on every release) and the craft-ui notices; releases link their source.
- **`.rzxl` file association** with its own document icon (MSI, AppImage desktop file,
  macOS `Info.plist`), *Open with* entries for `.vox` and `.gox`, and *Settings ▸
  General ▸ Associate .rzxl Files with This Copy* for portable builds (Windows and
  Linux, undoable).
- **One window**: opening a file while Rizxel runs hands it to the running window, which
  opens it as a tab and comes to the front.
- `RIZXEL_DATA_DIR` points the app data folder elsewhere.
- Headless UI smoke tests (`egui_kittest`) in CI; macOS in the CI matrix.

### Performance
- Volume tiles are copy-on-write and the tile mesh cache assembles its output in one
  allocation: a brush frame on a 128³ model drops from 4.9 to 3.0 ms and on a 256³
  model with a million voxels from 22.5 to 10.0 ms. Orbiting that model renders at
  8.5 ms per 720p frame on a GTX 950M. New `mesh_bench --large` and `viewport_bench`
  examples.

### Changed
- **License: GPL-3.0-or-later** (was declared as MIT OR Apache-2.0, with no
  license file). The full text is in `LICENSE`, `Cargo.toml` declares
  `GPL-3.0-or-later`, and *Help ▸ About Rizxel* shows the license, the
  no-warranty notice and a link to the source. `vendor/craft-ui` keeps its
  MIT OR Apache-2.0 license.
- **README** rewritten: icon and badges (CI, version, license, Rust,
  platforms, `unsafe` forbidden), screenshots of Model mode, Render mode and a
  path-traced render, requirements, getting started, a file format table,
  roadmap, contributing, acknowledgements and the GPL notice. Screenshots live
  in `docs/images/readme/`.

## [0.6.0] — 2026-10-30

Milestone M5: pipeline, export and rendering. glTF / GLB, Qubicle and PLY
formats, OBJ layouts, slices, mesh voxelizing and heightmaps, then a Render
mode with a progressive GPU path tracer, Image, Photo-realistic and Turntable
outputs, voxel shapes and shaped-mesh export.

### Phase 11b: Render mode, path tracer and render output (2026-10-29)

- **Render mode is a rendering workspace** with its own panel layout (kept separately
  from Model mode's): a render bar instead of the tool options (pause, Raster / Path
  traced preview, preview resolution, sample progress, output size, Pers / Ortho / Iso),
  the **Render** panel and the Layers panel, which shows each layer's material. Tools,
  Color Palette, Bound Model and References are hidden, and the viewport doesn't edit:
  a click sets the depth of field focus, `Alt`+click picks the clicked voxel's layer.
  `F5` and *View ▸ Render Mode* switch modes.
- **Render panel** with Light (sun, sky color / gradient / HDR image, fog, studio
  lights), Sample, Display (voxel shape and cell size, ground and shadow catcher,
  background, bevel, seams, outline), Material (the active layer's material, presets
  Diffuse / Plastic / Metal / Glass / Emit, *Make unique*), Camera (field of view,
  aperture, focus, blades, exposure, ACES / filmic tone mapping, gamma, vignette,
  bloom) and Output tabs, built-in presets (**Studio White**, Outdoor Sun, Night
  Emissive, Flat Pixel) and user presets, and a **Render** button at its bottom-right.
  Every setting is undoable and saved in the file.
- **Progressive GPU path tracer** (wgpu compute, no new renderer crates): soft shadows,
  bounced light, glossy plastic with a clear coat, metal, glass, emission, rounded edges
  and seams, a shadow-catcher studio ground, depth of field and an edge-aware à-trous
  denoiser. The viewport shows it in Render mode, restarting on every change. On a GTX
  950M at full clocks: 21 samples per second at 720p (10.5 with Studio White and
  bricks), a 1080p image at 256 samples in 21 s (46 s). Without Vulkan or Metal
  (GL, DX12) Render mode keeps the raster preview and the *Image* output.
- **Outputs:** *Image* (fast raster PNG up to 8192², transparent background optional),
  *Photo-realistic* (tiled path-traced PNG, 8 or 16 bits, with progress, time left,
  time limit and Cancel) and *Turntable* (raster or path-traced frames around the model:
  PNG sequence, looping GIF, sprite sheet with a JSON of frame rectangles). File names
  follow a pattern with `{name}` and `{frame}`.
- **Voxel shapes** (view only): cube, brick with studs, rounded, sphere and cylinder,
  with a cell size for gaps, in the path tracer, the raster view and *Image*.
  **Export Shaped Mesh** (File ▸ Export, Render mode only) writes them as OBJ + MTL, STL
  or GLB, with quality, studs, scale and pivot, a triangle count and a size guard. Brick
  meshes are watertight (checked in Blender 5.2).
- **`.rzxl` version 5:** render settings and the new material fields (specular, IOR,
  transmission, clear coat, emission strength). Older files load with the defaults.
- `render` example (`cargo run -p rizxel-ui --example render -- in.rzxl out.png`) for
  headless renders and `render_bench` for path tracer timings.

### Phase 11: Pipeline and export (2026-10-24)

- **glTF 2.0 export** (`.glb`, or `.gltf` + `.bin`): one node per visible layer, groups
  as parent nodes, linked clones as instances of one mesh, vertex colors or the palette
  texture, layer materials as PBR with emission. Y-up, 1 voxel = 0.1 m, placed like the
  OBJ export.
- **Qubicle formats:** `.qb` import and export (RGBA/BGRA, RLE or raw, both Z
  orientations, visibility mask, one layer per matrix), `.qbt` (models and compounds as
  groups) and `.qef` (palette order kept). Files Rizxel writes re-export byte for byte.
- **PLY:** binary export with vertex colors (Z-up), and PLY import (ASCII or binary)
  through the voxelizer.
- **Fast load OBJ** layout, as an option: one index per face corner, a normal per
  vertex, faces grouped by color with plain `Kd` materials (plus `Ke`, `d`, `Pr`, `Pm`)
  and no texture, like BrickLink Studio's export. The MagicaVoxel layout stays the
  default and unchanged: it imports faster in Blender 5.2.
- **Mesh export options** dialog for OBJ, STL, PLY and glTF: scale, pivot (bottom center,
  center, corner), OBJ layout, glTF colors and one file per visible layer.
- **Export Slices:** one PNG per voxel slice along X, Y or Z, 1–16 pixels per voxel.
- **Heightmap import:** a grayscale PNG or JPEG becomes voxel columns, with a max height
  and an optional color image.
- **Voxelizer dialog** for OBJ, STL and PLY imports: grid detection, target size, quarter
  turns and a live voxel count.
- The `convert` example handles every format plus `--scale`, `--pivot`, `--obj-layout`,
  `--gltf-colors`, `--per-layer`, `--slices`, `--slice-scale`, `--heightmap` (with
  `--color`), `--resolution` and `--turn`.

### Fixed

- `.vox` exports with groups opened as an empty scene in MagicaVoxel: scene nodes are now
  written in id order.
- Qubicle `.qb` files flagged left-handed (Qubicle's default) loaded mirrored front to
  back, and Rizxel's own exports showed mirrored in Qubicle.
- Qubicle `.qbt` exports showed missing faces (front and back mask bits were swapped) and
  misplaced groups; groups are now nested models.

## [0.5.0] — 2026-10-24

Milestone M4: editing power. Selection operations and Mask Mode, structure
and core operations, palette tools, a History panel, the layer tree and
reference images, box selection, lines and curves, the Transform tool, and a
tabbed workspace.

### Phase 10d: Workspace, docking tabs and mode switch (2026-10-24)

- **Model / Render switch in the title bar**, left of the window controls. It no longer
  sits in the Tool Options bar, so hiding or moving panels can't hide it, and it stays
  in Zen mode. In narrow windows the title shrinks first, then the menus fold into one
  ☰ menu.
- **Tab groups for every panel.** The right side is now two tab groups: *Color Palette*
  with *References*, and *Layers* with *Bound Model* and *History*. Drop any panel on a
  group's tab strip to add it as a sub-tab, or on an edge to place it beside the group.
  A double-click on a tab collapses its group to the strip and back.
- **Stretched tabs:** tab titles share their strip's full width.
- **Scrolling tab strips:** when a group's tabs don't fit, the strip scrolls with the
  mouse wheel or the ‹ › arrows at its ends instead of hiding tabs in a "…" menu. The
  active tab scrolls into view; a dragged tab held near an end scrolls the strip.
- `craft-ui` is vendored in `vendor/craft-ui` (from `storytold/craft-libs` at `ae2f796`)
  with the opt-in `TabSizing::Fill` and `TabOverflow::Scroll` styles; the change is kept
  as a patch for upstream.

### Phase 10c: Box select, lines and curves, Transform tool (2026-10-24)

- **Box selection** (Selection tool, *Box* mode, default): a click or drag places a blue
  box that stays open. Face arrows resize it, dragging a face moves it (`Alt`: along the
  normal), and the option bar types its position and size. The voxels inside are previewed
  live; `Enter`, *Apply*, a click outside or a tool switch applies it as one undo step
  (`Shift` adds, `Ctrl` subtracts), `Esc` cancels. Locked to the slice in Slice Mode.
- **Selection ▸ To New Layer** (Cut `Ctrl+Shift+J`, Copy `Ctrl+J`): the selected voxels
  go into a new layer above the source, in its group. One undo step.
- **Line tool** (`L`): Line, Polyline and Curve (quadratic or cubic Bézier) with the
  Brush's settings, draggable points and a ghost preview. *Pixel perfect* (no L-corners)
  and *Solid* (6-connected) connectivity, also for size-1 Brush strokes. `Ctrl`+click in
  the Brush draws a line from the last click (`Shift` stays erase).
- **Transform tool** (`T`): rotate and resize the selection or layer with rotation rings,
  face arrows, corner cubes and typed rotation / scale / size / move / pivot. Exact at 90°
  and whole scales, hole-free nearest-neighbour resampling otherwise. Out-of-bounds voxels
  are drawn red and need *Clip to bounds*. One undo step per Apply.

### Phase 10b: Layer hierarchy and reference images (2026-10-24)

#### Added
- Layer groups in the Layers panel: rows are indented by depth, groups have
  a collapse caret and a folder icon, and collapsed groups fold their rows
  (the fold is saved with the file but is not an undo step).
- *New Group* (panel header and *Layer* menu), *Group Layer* (`Ctrl+G`),
  *Ungroup* (`Ctrl+Shift+G`, also dissolves the group holding the active
  layer), *Merge Group* (bakes the visible contents into one voxel layer)
  and *Delete Group and Contents*.
- Drag and drop in the Layers panel: the upper or lower part of a row drops
  above or below it, the middle of a group row drops into the group. A
  dragged group takes its contents along; drops into its own contents show
  no marker and are refused.
- Lock is inherited: layers inside a locked group refuse every tool ("The
  active layer is inside a locked group"); the panel shows inherited locks
  and hides dimmed.
- Reference images: PNG or JPEG concept art on the XZ, XY or YZ plane with
  an offset, size (aspect lock or pixels per voxel), quarter turns, flips,
  opacity, *Behind model* or *On top*, and a Slice Mode filter. Images over
  4096 px are scaled down on import.
- References panel (*View ▸ Panels ▸ References*): thumbnails, rename,
  show / hide, lock, plane badge, drag to change the draw order, Duplicate,
  Replace Image, Reset Size and Delete; *Add Images* (several files), *Show
  All / Hide All* (`Alt+R`, also *View ▸ Reference Images*) and *Align Work
  Plane to Reference*. PNG and JPEG files dropped on the window become
  references on the plane the camera faces most.
- `.rzxl` version 4: reference images are embedded in the project (their
  original PNG or JPEG bytes, base64). Version 1–3 files still load.

#### Changed
- Move Up / Move Down step over a whole neighboring group and stay inside the
  layer's group. Duplicating a group copies its contents; linked clones
  inside it point at the copies.
- New layers made while a group is active go into that group, as its top
  layer.
- `.vox` export writes groups as group nodes, so they survive a round trip
  through MagicaVoxel; `.vox` import keeps groups and models in scene order.
- Files that list a group's contents away from the group (Goxel lists groups
  first) are reordered on load so each group's contents sit right below it.
- Tools on a group report "select a voxel layer" in the status bar.

### Phase 10: Editing power (2026-10-24)

#### Added
- *Selection* menu: Select All, Deselect, Invert, Grow, Shrink and By Color
  (the selected swatch, within the Fuzzy Select threshold). Grow and Shrink
  are 6-connected; Grow only adds voxels of the active layer.
- Mask Mode (`Shift+M`, a shield button in the Tools panel, shown in the
  status bar): the selection is protected. Every tool, Fill and the Plane
  cut leave selected voxels alone and edit the rest of the layer; Delete,
  Cut and the transforms are refused while it protects a selection.
- *Layer ▸ Separate Parts*: each 6-connected part becomes its own layer
  (parts under a set size stay). *Layer ▸ Split at Work Plane* moves the
  plane's `+` side to a new layer.
- *Layer ▸ Boolean*: Union, Difference or Intersection of the active layer
  with another layer (shapes and linked clones count with their geometry).
  Only the active layer changes.
- *Model ▸ Fit Bounds to Content*: moves the model to the origin and shrinks
  the bounds around it (shape areas, crops and selections move along). Also
  a *Fit* button next to the Plane tool's *Cut*, and *Fit Bounds on Import*.
- *Model ▸ Core*: Fill Hollows (closed cavities get the selected color),
  Hollow and Advanced Hollow (shell thickness in voxels).
- Pyramid and cone shapes for Shape (pointing away from the pressed face)
  and Brush (pointing up).
- Palette files: load and save GIMP `.gpl` and `.hex`; *Load and Snap
  Model* gives every voxel the nearest new color.
- Palette tools in the Color Palette panel: sort by hue or luminance (the
  model looks the same), remove duplicates, a gradient between two
  swatches, remap one color to another on every layer, and a *Used only*
  view. `Shift+click` marks the second swatch.
- Favorite colors 0–9: `Ctrl+digit` selects, `Ctrl+Shift+digit` (or a
  right-click on the favorites row) stores. Kept in the app settings.
- History panel (*View ▸ Panels ▸ History*): every undo step by name;
  clicking one undoes or redoes to it.

#### Changed
- Every new operation is a single undo step, named in the History panel.
- The Selection commands moved from the Edit menu to the new Selection menu.

## [0.4.0] — 2026-10-24

Milestone M3: everything stored in the file is visible, and models look good
in Render mode.

### Phase 9: Visual fidelity (2026-10-23)

#### Added
- Orthographic camera: `Numpad 5` (or `5`) and a *Persp / Ortho* button under
  the view cube. Orthographic keeps the framing of the perspective view, and
  picking uses parallel rays, so face views pick the same voxel as
  perspective.
- *Auto Ortho on Face Snap* (on by default): snapping to a view cube face or
  corner switches to orthographic; orbiting by hand returns to perspective.
  Corners use true isometric angles (35.26°).
- *Frame Selection* (`.`), *Frame Work Area* (`Home`) and *Home View*
  (`Shift+Home`) in the View menu.
- *View ▸ Show*: Floor Grid (`G`), Voxel Grid on the model's faces
  (`Shift+G`, off by default) and Model Bounds (`Shift+B`), also as toggle
  buttons in the viewport's top-left corner. Hiding the bounds also hides
  the slice frame; dragging it still moves the slice.
- Layer opacity is rendered: translucent layers are drawn after the opaque
  ones, sorted back to front. A group's opacity multiplies its children's.
  An opacity slider for the active layer is in the Layers panel.
- *View ▸ Ghosts* (`Shift+H`): every layer but the active one at 25%.
- Material preview in Render mode: tint, emission, and a highlight driven by
  roughness and metallic. The Layers panel picks, adds and edits the active
  layer's material. The tint's alpha acts as opacity.
- Per-vertex ambient occlusion, on in Render mode and off in Model mode by
  default (*View ▸ Ambient Occlusion*, per mode). Exports are unchanged.
- Large updates (imports, undo of big edits) are meshed on background
  threads; the viewport keeps drawing and shows "Meshing...". Brush strokes
  and small edits stay immediate.

#### Changed
- Changing a layer's opacity or a material updates the viewport in the same
  frame without re-meshing or re-uploading the model.
- After an edit, only the neighbouring tiles that the change actually
  touches are re-meshed.

## [0.3.0] — 2026-10-23

Milestone M2: users never lose work, plus a voxel clipboard, transforms and a
complete Edit menu.

### Phase 8: Safe editing (2026-10-23)

#### Added
- Save / Don't Save / Cancel prompt before unsaved work is closed, from the
  tab's close button, *File ▸ Exit*, the title bar, `Alt+F4` and the OS
  closing the window. On exit it asks about each dirty document in turn;
  *Save* on an untitled one opens Save As, and cancelling that cancels the
  exit.
- Autosave: every 2 minutes or 50 edits, never mid-drag, dirty documents
  are written in the background to `recovery/` in the app data folder
  (`%APPDATA%\Rizxel`, `~/.local/share/rizxel`,
  `~/Library/Application Support/Rizxel`). After a crash the next start
  offers each copy with Restore / Discard; restored documents keep their
  original path and open as unsaved. A save, closing the document and a
  clean exit delete the copy.
- *File ▸ Open Recent* with the last 10 opened, imported or saved files, and
  *Clear Recent*.
- Copy (`Ctrl+C`), Cut (`Ctrl+X`), Paste (`Ctrl+V`), Paste in Place
  (`Ctrl+Alt+V`) and Paste to New Layer (`Ctrl+Shift+V`). Copy and cut take
  the selection, or the whole active layer without one. A paste floats and
  follows the cursor until a click or `Enter` places it (`Esc` cancels);
  colors map onto the target document's palette.
- Rotate 90° (`Ctrl+R` / `Ctrl+Shift+R` about the axis the camera faces, or
  any axis from the menu), Flip X/Y/Z (`Ctrl+Alt+X/Y/Z`) and a one-shot
  Mirror that copies one half of the bounds onto the other. They act on the
  selection or the active layer and are refused as a whole when voxels
  would leave the bounds.
- Complete Edit menu: Undo, Redo, Cut, Copy, Paste, Paste in Place, Paste to
  New Layer, Select All (`Ctrl+A`), Deselect (`Ctrl+D`), Invert Selection
  (`Ctrl+I`), Delete, New Layer, Duplicate Layer, Rotate 90°, Flip and
  Mirror. Items show their shortcut and are disabled when they can't apply.

#### Changed
- Document tabs mark unsaved changes with `*` between the file name and its
  extension (`hero*.rzxl`), next to the existing dot on the close button.
- *Help ▸ Keyboard Shortcuts* lists every command and tool key and scrolls.
- Every paste, cut and transform is one undo step; the randomized undo-all /
  redo-all test now covers them.

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
- Tests that compare against real sample files (`chr_knight.vox`, its
  MagicaVoxel `chr_knight.obj` export, `coba_goxel.gox`) find them through
  the `RIZXEL_SAMPLES` folder list instead of fixed local paths, and skip
  when it is unset.
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
- **Slice Mode** from any tool, without switching tools (`04bbdf7`):
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
- `/` toggles the work plane (`d6f13b2`). Hiding it (the key, the Visible
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
  reference clones of CADCraft (with its current crate layering) and Goxel
  are listed.

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

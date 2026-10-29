# Rizxel — Known Gaps

Gaps between the current build (`v0.6.0`: Phases 1–11b, 589 tests) and a
usable 1.0 editor. Sources: the "Out of Scope" sections in each phase plan, the limits
in `interchange_formats.md` and `rzxl_format.md`,
[`qubicle_feature_gap_analysis.md`](qubicle_feature_gap_analysis.md) and the current code.

Severity: **High** blocks real-world use · **Medium** is a noticeable limitation ·
**Low** is polish.

The **Phase** column names the plan that closes the gap (see
[`milestones.md`](plan/milestones.md)); `post` means after 1.0.

> Last updated 2026-10-29.

## Closed since the last revision

| Gap | Closed by |
|---|---|
| Phase 6 work is uncommitted | `363f73d` |
| `.tmp-craft/` is an untracked vendor checkout | Removed from the tree; ignored since Phase 7 |
| No Qubicle-style Slice Mode (global toggle, slice-only editing, navigation) | `04bbdf7`, `d6f13b2` |
| B1: the model can't be seen from below | `c68aa10` |
| No CI | `.github/workflows/ci.yml` (Phase 7): fmt, clippy and tests on Windows and Linux |
| `main` is at Phase 4 | Merged in Phase 7 (PR #2, `v0.2.0`) |
| Closing unsaved work doesn't ask first | Phase 8, `e6e4a21` |
| No recent files list | Phase 8, `7fdd4c8` |
| No autosave or crash recovery | Phase 8, `e2d69d8` |
| No copy, cut or paste | Phase 8, `6aa12e4` |
| No 90° rotate, flip or mirror | Phase 8, `50f2000` |
| The Edit menu is minimal | Phase 8, `18bba53` |
| No way to move or copy a selection into a new layer | Phase 8: Cut or Copy, then *Paste to New Layer* |
| No orthographic camera, isometric presets or frame commands | Phase 9, `fd7d3a8` |
| Floor grid and model bounds can't be hidden, and there's no voxel grid on the model | Phase 9, `709c51c` |
| Opacity is stored but not rendered; no ghosts | Phase 9, `54556fd` |
| Materials are stored but not rendered | Phase 9, `dc052ad` (Render mode) |
| No ambient occlusion in Render mode | Phase 9, `22600dd` |
| Meshing runs on the UI thread | Phase 9, `e4bafa4` (large updates; small edits stay on the UI thread on purpose) |
| No grow / shrink / select by color | Phase 10, `a179e73` |
| No Mask Mode (protect the selection) | Phase 10, `a179e73` |
| Slice can't split parts into separate layers | Phase 10, `f98408c` (Separate Parts, Split at Work Plane) |
| No core operations (Hollow, Fill Hollows) or layer Booleans | Phase 10, `f98408c` |
| Bounds don't auto-fit after Slice or Cut | Phase 10, `f98408c` (Fit Bounds, the Plane tool's Fit button, Fit on Import) |
| No pyramid or cone shapes | Phase 10, `f98408c` |
| No palette files (`.gpl`, `.hex`) or palette tools (sort, remap, gradient) | Phase 10, `8a01562` |
| The undo history can't be browsed | Phase 10, `6e25e4d` (History panel) |
| No reference images | Phase 10b, `beb30be`, `2f3d424` (References panel, `.rzxl` v4) |
| The Layers panel can't create or show groups; locks aren't inherited | Phase 10b, `af5b8a3`, `cfa405c` |
| `.vox` export flattens groups | Phase 10b, `a57e589` |
| The selection box can't be adjusted | Phase 10c, `8d556ac` (Box mode: handles, typed position and size) |
| No direct "selection to new layer" | Phase 10c, `05f3aa5` (`Ctrl+J` / `Ctrl+Shift+J`) |
| No line or curve tool, and no pixel-perfect option | Phase 10c, `3336579` (Line tool, Pixel perfect / Solid) |
| No free rotation or resize of a selection or layer | Phase 10c, `2d3c6dc` (Transform tool) |
| The Model / Render switch hides with the Tool Options panel | Phase 10d, `193871f` (now in the title bar) |
| Palette, Layers and Bound Model can't form tab groups | Phase 10d, `56e4bba` (tab groups instead of the stack) |
| Tab titles hug their labels; crowded strips hide tabs in a "…" menu | Phase 10d, `4d52dfb` (Fill widths, scrolling strips) |
| No glTF, PLY or Qubicle (`.qb`, `.qef`, `.qbt`) support | Phase 11, `a6625a0`, `68f07cf` |
| No Export Slices | Phase 11, `40bd3da` |
| OBJ export has only MagicaVoxel's layout | Phase 11, `40bd3da` (Fast load layout as an option) |
| No heightmap import, and `voxelize` has no UI | Phase 11, `3701e78` (Heightmap import, Voxelizer dialog) |
| No mesh export options (scale, pivot, per-layer files) | Phase 11, `5e825eb` |
| Qubicle files aren't checked in Qubicle | Phase 11 fixes: checked in Qubicle 3.1; `.qb` orientation flag, `.qbt` face mask and groups corrected |
| No image output: PNG render, photo-realistic render, turntables, sprite sheets | Phase 11b (*Image*, *Photo-realistic*, *Turntable* outputs) |
| No shadows or outline shading in Render mode | Phase 11b (path tracer: shadows, GI, outline) |
| Render mode has no layout or options of its own | Phase 11b (Render layout, render bar, Render panel) |
| No photo-realistic rendering | Phase 11b (GPU path tracer) |
| No render-only voxel shapes or shaped-mesh export | Phase 11b (voxel shapes, *Export Shaped Mesh*) |

## Known bugs

No open bugs are known. Fixed bugs move to the table above.

## Data model and files

| Gap | Severity | Phase | Notes |
|---|---|---|---|
| Materials show in Render mode only | Low | — | On purpose: Model mode stays flat for editing. The tint's alpha (opacity) works in both modes. |
| A document's 256th material shares the last renderer slot | Low | post | The renderer's table has 256 slots and slot 0 is the default material, so material 255 shows as material 254. Files are unaffected. |
| Bounds are limited to 256³ | Medium | post | `BoundSize` is clamped to `1..=256` and imports above 256 on any axis are refused. MagicaVoxel scenes can be larger. |
| One shared grid for all layers | Medium | post | Qubicle matrices have their own size and position; needed for Align/Distribute and exact `.qb` multi-matrix layouts. |
| Palette is limited to 255 colors, with no per-voxel RGBA | Medium | post | `Voxel = u8`. True-color imports (Goxel, OBJ textures) are quantized with median cut, so colors are lost. |
| Selections aren't saved | Low | post | Each layer's selection is lost on save and reload. |
| Dock layout isn't saved between sessions | Low | 12 | `Workspace::reset` exists, but the layout isn't written to disk. |

## Interchange

| Gap | Severity | Phase | Notes |
|---|---|---|---|
| glTF isn't checked in Godot | Low | 12 | Blender 5.2 imports the glTF (vertex colors and palette texture), both OBJ layouts, PLY and STL with matching colors; Godot wasn't available. |
| The Fast load OBJ doesn't load faster in Blender | Low | — | Blender 5.2 imports a 489k-triangle terrain in 1.38 s as Fast load and 0.99 s in the MagicaVoxel layout (best of three; PLY 0.40 s, GLB 4.4 s). Fast load has 38% more vertices and one material per color, so MagicaVoxel's layout stays the default; other loaders weren't timed. |
| Fast load `Kd` colors look darker in some viewers | Low | — | `Kd` is written as linear RGB, like BrickLink Studio and as Blender reads it; Windows 3D Viewer reads it as sRGB. Rizxel's importer restores the exact colors. |
| No glTF import | Low | post | glTF is export only. |
| Qubicle exports leave out hidden layers and `.qbt` group names; pivots and scales are ignored on import | Low | post | Qubicle files have no hidden flag. `.qbt` groups are written as nested models, which have no name (compounds are drawn from their own merged matrix, placed by rules that depend on pivot and size). Matrix pivots and local/global scales have no Rizxel equivalent. |
| The Voxelizer preview runs on the UI thread | Low | 12 | Each setting change voxelizes the whole mesh; very large meshes stall the dialog. Open… and drag and drop skip the dialog. |
| MagicaVoxel `MATL`, cameras and animation frames are dropped | Medium | post | |
| `.vox` export bakes linked clones | Low | — | Done on purpose, to avoid bounds growing on re-import. |
| Goxel previews, cameras, lights and image layers are dropped | Low | post | |
| STL has no color | Low | — | It's a limit of the format. PLY carries color. |

## Editing tools

| Gap | Severity | Phase | Notes |
|---|---|---|---|
| Transform drag modifiers (`Shift` / `Ctrl` snapping, `Alt` per-axis corners) and Brush `Ctrl`+click lines are untested by hand | Low | 12 | Injected modifier clicks don't arrive in manual tests; typed values and plain drags were checked. |
| Undo with an open box selection or transform only closes it | Low | post | Same as a floating paste; the next Undo undoes the last step. |
| No shear, smooth (filtered) resampling or multi-layer transforms | Low | post | The Transform tool resamples by nearest neighbour, on one layer. |
| Gradient and Remap need `Shift+click` to mark a swatch | Low | 12 | There is no other way to pick the second color yet. |
| Brush pyramids and cones always point up | Low | post | The Shape tool points them away from the pressed face; Brush stamps have no face direction. |
| Favorite colors use `Ctrl+digit`, not plain digits | Low | 12 | Plain `5` already toggles the projection, and egui can't tell the numpad from the main digits. Configurable shortcuts come with Phase 12. |
| No multi-select of layer rows | Low | post | *Group Layer* wraps the active layer (with its contents) only. |
| Group opacity doesn't fade its children in the Layers panel, and groups have no transform | Low | post | Opacity multiplies down the tree in the viewport; per-group transforms are Qubicle compounds. |
| Reference images can't be moved with a handle in the viewport, and have no mipmaps | Low | post | Placement is typed or dragged in the References panel; large images minified a lot can shimmer. |
| No onion skin | Low | post | Comes with animation. |
| No paint/lasso selection, and no sculpting or smoothing brushes | Medium | post | |

## UI and UX

| Gap | Severity | Phase | Notes |
|---|---|---|---|
| No Settings or Preferences dialog, and shortcuts are fixed | Medium | 12 | The toolbar settings button is a stub (`panels/tools.rs:45`). Shortcuts, theme, grid and the undo limit can't be configured. |
| No start screen | Low | 12 | |
| View toggles and the projection aren't remembered | Low | 12 | Floor grid, voxel grid, bounds, ghosts, AO and Auto Ortho reset on every start; Phase 12 stores them in settings. |
| The undo history is capped at 512 steps | Low | 12 | The History panel (Phase 10) lists them; the limit becomes configurable in Phase 12. |
| No localization | Low | post | The UI is English only. |
| Tab strips can't be scrolled by dragging | Low | post | Dragging a tab moves it to another group, so strips scroll by wheel, arrows, or a dragged tab held near an end. |
| A collapsed group only collapses when it sits above or below another group | Low | post | Groups beside the viewport or floating have nothing to give the space to. |

## Rendering and performance

| Gap | Severity | Phase | Notes |
|---|---|---|---|
| The raster *Image* has no shadows, bevels or outline | Low | post | Those come from the path tracer (*Photo-realistic*); *Image* is the fast Phase 9 raster look with the sun direction and the voxel shape. |
| The path tracer reaches the sky only by BSDF sampling | Low | post | No importance sampling of HDR skies, so small bright spots in an HDR image converge slowly. The sky image is referenced by path, not embedded in the `.rzxl`. |
| The raster shape preview hides translucency | Low | post | With a shape other than Cube the raster view and *Image* draw every layer opaque; the path tracer keeps layer and material opacity. Above 1M voxels the raster view keeps cubes. |
| *Photo-realistic* renders don't fill in in the viewport | Low | post | Progress shows in the Render panel's footer; the viewport keeps the preview (paused). |
| On the GL backend the window stops updating after the first frame | Medium | 12 | Seen with `WGPU_BACKEND=gl` on an Intel HD 4600 (GL 4.3, 2015 driver): the app runs and takes input, but the frameless window never shows a second frame. The `fb35090` build (before Phase 11b) does the same, so it isn't new. Headless rendering on GL works (raster *Image*). |
| No path tracer on GL or DX12 | Low | post | Only Vulkan and Metal run it; elsewhere Render mode keeps the raster preview and *Image*. The HD 4600's GL driver crashes on the kernel's first dispatch, and on DX12 wgpu's FXC compiler needs 140–290 s for it (GTX 950M). DXC or a smaller kernel could bring DX12 back. Windows GPUs normally have Vulkan drivers. |
| Shaped export has no per-layer objects or up-axis choice | Low | post | OBJ and GLB are Y up, STL Z up; GLB has one primitive per layer material; no `.gltf` text output. |
| Translucent layers sort per layer, not per triangle | Low | post | Interpenetrating translucent layers can blend in the wrong order; no order-independent transparency. |
| Ambient occlusion costs triangles | Low | 12 | AO-aware merging gives about 1.8× the triangles of the plain mesh (`mesh_bench`: 89,788 vs 49,984). |
| Loading a file still parses on the UI thread | Low | 12 | Meshing is in the background since Phase 9, but reading a large `.vox` or `.rzxl` blocks before the first frame. |
| Greedy merging stops at tile borders | Low | 12 | The mesh has more triangles than it needs. |
| No LOD for large or zoomed-out scenes | Low | post | |

## Engineering

| Gap | Severity | Phase | Notes |
|---|---|---|---|
| `.rzxl` files aren't associated with Rizxel | Medium | 12 | Double-clicking a project does nothing; there's no document icon, and a second launch would open a second window. The app already accepts a path argument. |
| `craft-ui` is a vendored copy | Low | post | `vendor/craft-ui` is upstream `ae2f796` plus the Fill / Scroll tab-strip patch (Phase 10d). Send the patch upstream, then go back to a git rev; the steps are in `vendor/craft-ui/README.md`. |
| No release packaging | Medium | 12 | There are no installers or signed binaries, and only Windows has an embedded icon resource. |
| UI behaviour isn't tested | Medium | 12 | Tests cover the engine, I/O and app layers. Viewport and panel interactions aren't automated. |
| No user documentation or tutorial | Medium | 12a | The docs are for developers only. |
| No `.rzxl` compatibility promise | Medium | 12a | A candidate promise for v5 lands in 0.10.0 and becomes binding at 1.0. |

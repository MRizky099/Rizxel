# Rizxel — Known Gaps

Gaps between the current build (`v0.4.0` plus Phase 10 on its branch, 325 tests) and a
usable 1.0 editor. Sources: the "Out of Scope" sections in each phase plan, the limits
in `interchange_formats.md` and `rzxl_format.md`,
[`qubicle_feature_gap_analysis.md`](qubicle_feature_gap_analysis.md) and the current code.

Severity: **High** blocks real-world use · **Medium** is a noticeable limitation ·
**Low** is polish.

The **Phase** column names the plan that closes the gap (see
[`milestones.md`](plan/milestones.md)); `post` means after 1.0.

> Last updated 2026-10-24.

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
| No glTF, PLY or Qubicle (`.qb`, `.qef`, `.qbt`) support | Medium | 11 | glTF is the common format for game and web pipelines; `.qb` is the main Qubicle format. |
| No image output: PNG render, photo-realistic render, turntables, sprite sheets | Medium | 11b | |
| No Export Slices | Low | 11 | |
| OBJ export loads slowly in other tools | Medium | 11 | The MagicaVoxel layout mixes v/vt/vn indices and colors through a palette PNG, so loaders must rebuild vertices and decode the texture. A fast-load layout (one index per corner, `Kd` colors) becomes the default. |
| No heightmap import, and `voxelize` has no UI | Medium | 11 | |
| No mesh export options (scale, pivot, per-layer files) | Medium | 11 | |
| MagicaVoxel `MATL`, cameras and animation frames are dropped | Medium | post | |
| `.vox` export bakes linked clones | Low | — | Done on purpose, to avoid bounds growing on re-import. |
| Goxel previews, cameras, lights and image layers are dropped | Low | post | |
| STL has no color | Low | — | It's a limit of the format. PLY (Phase 11) carries color. |

## Editing tools

| Gap | Severity | Phase | Notes |
|---|---|---|---|
| No free rotation or resize of a selection or layer | Medium | 10c | Phase 8 added 90° rotate, flip and one-shot mirror; the Transform tool (free angles, scaling, gizmo) comes in Phase 10c. |
| No line or curve tool, and no pixel-perfect option | Medium | 10c | Brush only interpolates between drag samples. The next Slice Mode workflow. |
| The selection box can't be adjusted | Medium | 10c | The Selection drag selects on release; there are no handles and no typed position or size. |
| Gradient and Remap need `Shift+click` to mark a swatch | Low | 12 | There is no other way to pick the second color yet. |
| Brush pyramids and cones always point up | Low | post | The Shape tool points them away from the pressed face; Brush stamps have no face direction. |
| Favorite colors use `Ctrl+digit`, not plain digits | Low | 12 | Plain `5` already toggles the projection, and egui can't tell the numpad from the main digits. Configurable shortcuts come with Phase 12. |
| No reference images | Medium | 10b | No way to load, place (XZ / XY / YZ plane with an offset) or toggle a reference image, and no panel to manage them. Needs `.rzxl` v4. |
| The Layers panel can't create or show groups | Medium | 10b | Groups exist in the data model (`parent`, `LayerKind::Group`) and survive `.gox`, but the panel is a flat list: no New Group, no indentation or collapse, no dragging into groups. Locks aren't inherited. |
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

## Rendering and performance

| Gap | Severity | Phase | Notes |
|---|---|---|---|
| No shadows or outline shading in Render mode | Medium | 11b | Phase 9 added per-vertex AO and material highlights; shadows, GI and edges come with the path tracer in Phase 11b. |
| Render mode has no layout or options of its own | Medium | 11b | It only switches the shading; panels and tools stay as in Model mode. |
| No photo-realistic rendering | Medium | 11b | |
| No render-only voxel shapes (bricks with studs, spheres, cylinders) or shaped-mesh export | Low | 11b | MagicaVoxel offers these under *Display ▸ Shape*. |
| Translucent layers sort per layer, not per triangle | Low | post | Interpenetrating translucent layers can blend in the wrong order; no order-independent transparency. |
| Ambient occlusion costs triangles | Low | 12 | AO-aware merging gives about 1.8× the triangles of the plain mesh (`mesh_bench`: 89,788 vs 49,984). |
| Loading a file still parses on the UI thread | Low | 12 | Meshing is in the background since Phase 9, but reading a large `.vox` or `.rzxl` blocks before the first frame. |
| Greedy merging stops at tile borders | Low | 12 | The mesh has more triangles than it needs. |
| No LOD for large or zoomed-out scenes | Low | post | |

## Engineering

| Gap | Severity | Phase | Notes |
|---|---|---|---|
| No release packaging | Medium | 12 | There are no installers or signed binaries, and only Windows has an embedded icon resource. |
| UI behaviour isn't tested | Medium | 12 | Tests cover the engine, I/O and app layers. Viewport and panel interactions aren't automated. |
| No user documentation or tutorial | Medium | 13 | The docs are for developers only. |
| No `.rzxl` compatibility promise | Medium | 13 | Needed before 1.0. |

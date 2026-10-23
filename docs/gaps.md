# Rizxel — Known Gaps

Gaps between the current build (Phases 1–6 plus Slice Mode and the B1 fix, on
`ui-docking` at `ca9a299`, 228 tests) and a usable 1.0 editor. Sources: the "Out of
Scope" sections in each phase plan, the limits in `interchange_formats.md` and `rzxl_format.md`,
[`qubicle_feature_gap_analysis.md`](qubicle_feature_gap_analysis.md) and the current code.

Severity: **High** blocks real-world use · **Medium** is a noticeable limitation ·
**Low** is polish.

The **Phase** column names the plan that closes the gap (see
[`milestones.md`](plan/milestones.md)); `post` means after 1.0.

> Last updated 2026-10-23.

## Closed since the last revision

| Gap | Closed by |
|---|---|
| Phase 6 work is uncommitted | `16a8ebc` |
| `.tmp-craft/` is an untracked vendor checkout | Removed from the tree; ignored since Phase 7 |
| No Qubicle-style Slice Mode (global toggle, slice-only editing, navigation) | `2b4f8e2`, `30db23c` |
| B1: the model can't be seen from below | `aa5d21e` |
| No CI | `.github/workflows/ci.yml` (Phase 7): fmt, clippy and tests on Windows and Linux; first run pending the first push |

## Known bugs

No open bugs are known. Fixed bugs move to the table above.

## Data model and files

| Gap | Severity | Phase | Notes |
|---|---|---|---|
| Opacity and materials are stored but not rendered | High | 9 | `.rzxl` v3 saves layer `opacity` and `materials` (metallic, roughness, emission, tint), but the renderer ignores them. Users can edit values that have no visible effect. |
| Closing unsaved work doesn't ask first | High | 8 | The tab's close button, *File ▸ Exit*, the title bar and `Alt+F4` all discard unsaved changes without a prompt. |
| No autosave or crash recovery | High | 8 | A crash or power loss loses everything since the last manual save. |
| Bounds are limited to 256³ | Medium | post | `BoundSize` is clamped to `1..=256` and imports above 256 on any axis are refused. MagicaVoxel scenes can be larger. |
| One shared grid for all layers | Medium | post | Qubicle matrices have their own size and position; needed for Align/Distribute and exact `.qb` multi-matrix layouts. |
| Palette is limited to 255 colors, with no per-voxel RGBA | Medium | post | `Voxel = u8`. True-color imports (Goxel, OBJ textures) are quantized with median cut, so colors are lost. |
| Selections aren't saved | Low | post | Each layer's selection is lost on save and reload. |
| No recent files list | Low | 8 | |
| Dock layout isn't saved between sessions | Low | 12 | `Workspace::reset` exists, but the layout isn't written to disk. |

## Interchange

| Gap | Severity | Phase | Notes |
|---|---|---|---|
| No glTF, PLY or Qubicle (`.qb`, `.qef`, `.qbt`) support | Medium | 11 | glTF is the common format for game and web pipelines; `.qb` is the main Qubicle format. |
| No image output: PNG render, sprite sheets, Export Slices | Medium | 11 | |
| No heightmap import, and `voxelize` has no UI | Medium | 11 | |
| No mesh export options (scale, pivot, per-layer files) | Medium | 11 | |
| MagicaVoxel `MATL`, cameras and animation frames are dropped | Medium | post | |
| `.vox` export bakes linked clones | Low | — | Done on purpose, to avoid bounds growing on re-import. |
| Goxel previews, cameras, lights and image layers are dropped | Low | post | |
| STL has no color | Low | — | It's a limit of the format. PLY (Phase 11) carries color. |

## Editing tools

| Gap | Severity | Phase | Notes |
|---|---|---|---|
| No copy, cut or paste | High | 8 | There is no clipboard, and no duplicating a selection into a new layer. |
| No transforms: rotate, flip, mirror or resize of a selection or layer | High | 8, 10c | Basic modeling operations. Live Symmetry only affects placement. Phase 8 adds 90° rotate, flip and mirror commands; the Transform tool (free angles, scaling, gizmo) comes in Phase 10c. |
| No line or curve tool, and no pixel-perfect option | Medium | 10c | Brush only interpolates between drag samples. The next Slice Mode workflow. |
| The selection box can't be adjusted | Medium | 10c | The Selection drag selects on release; there are no handles and no typed position or size. |
| No way to move or copy a selection into a new layer | Medium | 10c | |
| No select all / invert / grow / shrink / by color | Medium | 8, 10 | Selection is box-based or fuzzy only. Select All and Invert land with the Edit menu in Phase 8. |
| No Mask Mode (protect the selection) | Medium | 10 | Qubicle's third global tool option. |
| Slice can't split parts into separate layers | Medium | 10 | Listed as out of scope in 5a. |
| No core operations (Hollow, Fill Hollows) or layer Booleans | Medium | 10 | |
| No palette files (`.gpl`, `.hex`) or palette tools (sort, remap, gradient) | Medium | 10 | |
| Bounds don't auto-fit after Slice or Cut | Low | 10 | The empty region stays inside the yellow box. |
| No pyramid or cone shapes | Low | 10 | Shape and Brush cover cube, sphere and cylinder. |
| No reference images | Medium | 10b | No way to load, place (XZ / XY / YZ plane with an offset) or toggle a reference image, and no panel to manage them. Needs `.rzxl` v4. |
| The Layers panel can't create or show groups | Medium | 10b | Groups exist in the data model (`parent`, `LayerKind::Group`) and survive `.gox`, but the panel is a flat list: no New Group, no indentation or collapse, no dragging into groups. Locks aren't inherited. |
| No onion skin | Low | post | Comes with animation. |
| No paint/lasso selection, and no sculpting or smoothing brushes | Medium | post | |

## UI and UX

| Gap | Severity | Phase | Notes |
|---|---|---|---|
| The Edit menu is minimal | Medium | 8 | It has Undo, Redo and New Layer only (`panels/top_bar.rs`). Select All, Invert and Delete are reachable only by keyboard or the option bar. |
| No Settings or Preferences dialog, and shortcuts are fixed | Medium | 12 | The toolbar settings button is a stub (`panels/tools.rs:45`). Shortcuts, theme, grid and the undo limit can't be configured. |
| No start screen | Low | 12 | |
| No orthographic camera, isometric presets or frame commands | Medium | 9 | The view cube snaps angles, but the projection is always perspective. Rated High for slice work in the Qubicle analysis. |
| No ghosts (non-active layers semi-transparent) | Low | 9 | Needs opacity rendering. |
| Floor grid and model bounds can't be hidden, and there's no voxel grid on the model | Low | 9 | |
| The undo history is capped at 512 steps and can't be browsed | Low | 10, 12 | History panel in Phase 10; configurable limit in Phase 12. |
| No localization | Low | post | The UI is English only. |

## Rendering and performance

| Gap | Severity | Phase | Notes |
|---|---|---|---|
| No ambient occlusion, shadows or outline shading in Render mode | Medium | 9, post | Lighting is Lambert plus ambient and fill only. AO in Phase 9; shadows post-1.0. |
| Meshing runs on the UI thread | Medium | 9 | The tile cache helps with edits, but a full re-mesh of a large import blocks a frame. |
| Greedy merging stops at tile borders | Low | 12 | The mesh has more triangles than it needs. |
| No LOD for large or zoomed-out scenes | Low | post | |

## Engineering

| Gap | Severity | Phase | Notes |
|---|---|---|---|
| `main` is at Phase 4 | High | 7 | Everything since `0ceeb6a` lives only on `ui-docking`. |
| No release packaging | Medium | 12 | There are no installers or signed binaries, and only Windows has an embedded icon resource. |
| UI behaviour isn't tested | Medium | 12 | Tests cover the engine, I/O and app layers. Viewport and panel interactions aren't automated. |
| No user documentation or tutorial | Medium | 13 | The docs are for developers only. |
| No `.rzxl` compatibility promise | Medium | 13 | Needed before 1.0. |

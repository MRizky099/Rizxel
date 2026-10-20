# Rizxel — Known Gaps

Gaps between the current build (Phases 1–5b plus the uncommitted import/export work) and
a usable 1.0 editor. Sources: the "Out of Scope" sections in each phase plan, the limits
in `interchange_formats.md` and `rzxl_format.md`, and the current code.

Severity: **High** blocks real-world use · **Medium** is a noticeable limitation ·
**Low** is polish.

## Data model and files

| Gap | Severity | Notes |
|---|---|---|
| Opacity and materials are stored but not rendered | High | `.rzxl` v3 saves layer `opacity` and `materials` (metallic, roughness, emission, tint), but the renderer ignores them. Users can edit values that have no visible effect. |
| Bounds are limited to 256³ | Medium | `BoundSize` is clamped to `1..=256` and imports above 256 on any axis are refused. MagicaVoxel scenes can be larger. |
| Palette is limited to 255 colors, with no per-voxel RGBA | Medium | `Voxel = u8`. True-color imports (Goxel, OBJ textures) are quantized with median cut, so colors are lost. |
| Selections aren't saved | Low | Each layer's selection is lost on save and reload. |
| No autosave or crash recovery | High | A crash or power loss loses everything since the last manual save. |
| No recent files list | Low | |
| Dock layout isn't saved between sessions | Low | `DockState::reset` exists, but the layout isn't written to disk. |

## Interchange

| Gap | Severity | Notes |
|---|---|---|
| Phase 6 work is uncommitted | High | All of the import/export code is untracked or modified on `ui-docking`. |
| MagicaVoxel `MATL`, cameras and animation frames are dropped | Medium | |
| `.vox` export bakes linked clones | Low | Done on purpose, to avoid bounds growing on re-import. |
| Goxel previews, cameras, lights and image layers are dropped | Low | |
| STL has no color | Low | It's a limit of the format. Export to 3MF or PLY could carry color. |
| No glTF, PLY or Qubicle (`.qb`) support, and no image or sprite export | Medium | glTF is the common format for game and web pipelines. |

## Editing tools

| Gap | Severity | Notes |
|---|---|---|
| No copy, cut or paste | High | There is no clipboard, and no duplicating a selection into a new layer. |
| Slice can't split parts into separate layers or documents | Medium | Listed as out of scope in 5a. |
| Bounds don't auto-fit after Slice or Cut | Low | The empty region stays inside the yellow box. |
| No lasso or paint selection, invert, grow or shrink | Medium | Selection is box-based or fuzzy only. |
| No transforms: rotate, mirror or scale of a selection or layer | High | Basic modeling operations. |
| No line tool, and no sculpting or smoothing brushes | Medium | The Shape tool covers boxes, spheres and cylinders only. |
| No reference images or onion skin | Low | |

## UI and UX

| Gap | Severity | Notes |
|---|---|---|
| No Settings or Preferences dialog | Medium | The toolbar settings button is a stub (`panels/tools.rs:42`). Shortcuts, theme, grid and the undo limit can't be configured. |
| The Edit menu is minimal | Medium | It has Undo, Redo and New Layer only. Select All, Invert and Delete are reachable only by keyboard or the option bar. |
| No orthographic camera and no camera bookmarks | Low | The view cube snaps angles, but the projection is always perspective. |
| No localization | Low | The UI is English only. |
| The undo history is capped at 512 steps and can't be browsed | Low | |

## Rendering and performance

| Gap | Severity | Notes |
|---|---|---|
| No ambient occlusion, shadows or outline shading in Render mode | Medium | Lighting is Lambert plus ambient and fill only. |
| Meshing runs on the UI thread | Medium | The tile cache helps with edits, but a full re-mesh of a large import blocks a frame. |
| Greedy merging stops at tile borders | Low | The mesh has more triangles than it needs. |
| No LOD for large or zoomed-out scenes | Low | |

## Engineering

| Gap | Severity | Notes |
|---|---|---|
| No CI | High | There is no `.github/workflows`, so the 219 tests, clippy and fmt only run by hand. |
| No release packaging | Medium | There are no installers or signed binaries, and only Windows has an embedded icon resource. |
| UI behaviour isn't tested | Medium | Tests cover the engine, I/O and app layers. Viewport and panel interactions aren't automated. |
| `.tmp-craft/` is an untracked vendor checkout | Low | It should be added to `.gitignore` or removed before committing. |
| No user documentation or tutorial | Medium | The docs are for developers only. |

# Rizxel

<!-- TODO(Phase 7): no GitHub remote is configured yet. Once it exists, replace this
comment with the CI badge:
[![CI](https://github.com/OWNER/REPO/actions/workflows/ci.yml/badge.svg)](https://github.com/OWNER/REPO/actions/workflows/ci.yml) -->

A native voxel editor written in pure Rust. Rizxel uses `egui` for the interface and
`wgpu` for the 3D viewport. Its layered architecture follows
[CADCraft](https://github.com/storytold/cadcraft), and its voxel model and tools draw on
Goxel, MagicaVoxel and Qubicle.

> Status: pre-release (`0.1.0`). Phases 1–6 and Slice Mode are implemented on the
> `ui-docking` branch. See [`docs/CHANGELOG.md`](docs/CHANGELOG.md).

## Features

- **Editing tools**: Cursor, Brush, Shape, Plane (work plane and Qubicle-style Slice),
  Move, Color Picker, Selection, Fuzzy Select, Extrude and Fill. Each tool has its own
  options, plus Add / Subtract / Join / Paint modes, symmetry and antialiasing.
- **Selection** per layer, with Set / Add / Subtract masks. Move and Extrude work on the
  selection.
- **Undo/redo**: one drag is one undo step, and a cancelled drag restores the grid
  exactly.
- **Layers**: groups, linked clones, procedural shape layers (cube, sphere, cylinder),
  non-destructive crop, opacity, materials, visibility and lock.
- **Viewport**: wgpu with 4× MSAA, an infinity grid, greedy-meshed voxels, Model and
  Render shading, a 26-target view cube and incremental per-tile re-meshing.
- **Workspace**: dockable and floating panels, document tabs, Zen mode, and a frameless
  window with a custom title bar.
- **Files**: native `.rzxl` (zstd-compressed JSON). Rizxel can also import and export
  MagicaVoxel `.vox`, Goxel `.gox`, Wavefront `.obj` + `.mtl` and `.stl`.

## Quick start

Requires stable Rust ≥ 1.85 (pinned through `rust-toolchain.toml`).

```bash
cargo run -p rizxel-ui --release
```

Open a file at startup:

```bash
cargo run -p rizxel-ui --release -- path/to/model.rzxl
```

Write a demo project:

```bash
cargo run -p rizxel-app --example sample_model
```

Convert between formats without the UI:

```bash
cargo run -p rizxel-app --example convert -- input.vox output.obj
```

Run the tests and lints:

```bash
cargo test --workspace
```

```bash
cargo clippy --workspace --all-targets
```

## Controls

| Input | Action |
|---|---|
| Right drag | Orbit |
| Middle drag | Pan |
| Wheel | Zoom |
| Middle double-click | Reset camera |
| Left button | Use the active tool |
| `Shift` / `Ctrl` | Subtract / add (selection tools: mask mode) |
| `Alt` | Pick a color with any tool |
| `Esc` | Cancel the drag or clear the selection |
| `Delete` | Delete the selection |
| `V B S P M C R W E F` | Cursor, Brush, Shape, Plane, Move, Color Picker, Selection, Fuzzy, Extrude, Fill |
| `Ctrl+N/O/S`, `Ctrl+Shift+S` | New, Open, Save, Save As |
| `Ctrl+Z`, `Ctrl+Y` / `Ctrl+Shift+Z` | Undo, Redo |
| `Ctrl+Shift+F` | Zen mode |

## Architecture

Five crates, each depending only on the crates below it. Nothing below `rizxel-ui`
depends on egui or wgpu.

```text
rizxel-ui      L4  egui/wgpu frontend: panels, docking, viewport, renderer, tool input
   │
rizxel-app     L3  headless state: documents, tabs, undo/redo, selection, interchange
   │
rizxel-io      L2  .rzxl, .vox, .gox, .obj/.mtl, .stl, mesh voxelization
   │
rizxel-engine  L1  greedy mesher, DDA raycast, tool algorithms, shapes, quantization
   │
rizxel-math    L0  Rgba8, Palette, BoundSize, Aabb, VoxelVolume (16³ sparse tiles), layer types
```

Guarantees across the workspace:

- `unsafe_code = "forbid"`, with clippy `all` set to warn.
- Every file read is validated. A save is validated too, so Rizxel never writes a file
  it can't load again.
- Tool algorithms are pure functions that return `VoxelChange` lists, which keeps them
  unit-testable without a GPU.

## Documentation

| Document | Contents |
|---|---|
| [`docs/phase_summaries.md`](docs/phase_summaries.md) | A summary of each phase |
| [`docs/rzxl_format.md`](docs/rzxl_format.md) | `.rzxl` file format spec |
| [`docs/interchange_formats.md`](docs/interchange_formats.md) | Import/export behaviour and limits |
| [`docs/gaps.md`](docs/gaps.md) | Known gaps and limitations |
| [`docs/qubicle_feature_gap_analysis.md`](docs/qubicle_feature_gap_analysis.md) | Comparison with Qubicle |
| [`docs/CHANGELOG.md`](docs/CHANGELOG.md) | Change history |

Planning documents live in `docs/plan/`, which is kept local and isn't part of the
repository: the architecture plan (`rizxel_native_architecture_plan.md`), one
`rizxel_phase*_implementation_plan.md` per phase, `milestones.md` (0.2 to 1.0) and
`feature_recommendations.md`.

## License

MIT OR Apache-2.0. © Abati Technology.

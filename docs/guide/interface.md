# The interface

![Model mode](https://raw.githubusercontent.com/MRizky099/Rizxel/main/docs/images/readme/model_mode.png)

| Area | What it holds |
|---|---|
| **Title bar** | The [menus](menus.md), the document tabs, the **Model / Render** switch (`F5`) and the window buttons. The empty part of the bar moves the window. |
| **Document tabs** | In the title bar, one tab per open model. `+` (`Ctrl+N`) adds a new project; a dot on a tab marks unsaved changes and turns into the close button on hover. In a narrow window the menus fold into ☰ first, so the tabs keep their room. |
| **Tools rail** (left) | The [tools](tools/index.md), the *Slice* and *Mask* mode toggles, and the Settings button at the bottom. |
| **Option bar** (top) | Undo and redo, the voxel operation (Add, Subtract, Join, Paint) and the active tool's options. The `?` opens the tool's guide page. |
| **Viewport** (center) | The model, the grids, the work plane and the view cube. See [Viewport](panels/viewport.md). |
| **Side panels** (right) | [Color Palette](panels/palette.md), [Layers](panels/layers.md), [Reference Images](panels/references.md), [Bound Model](panels/bounds.md) and [History](panels/history.md). |
| **Status bar** (bottom) | The cursor position, voxel counts, bounds and the active tool's hint. See [Status bar](panels/status-bar.md). |

## Modes

- **Model mode** is for editing, with all tools and panels.
- **Render mode** (`F5`, or the switch in the title bar) is a separate workspace with a
  path-traced preview, the [Render panel](panels/render.md) and the render bar. See
  [Render mode](render-mode.md).

Each mode keeps its own panel layout.

## Docking

Every panel can be moved:

- Drag a panel's tab onto another panel to join its tab group, or to an edge to split
  the space. The option bar and the tools rail have no tab while they sit alone: drag
  them by the dotted grip on their left or top edge (hover it for the panel's name).
- Drag it out of the window to float it.
- Tab strips scroll when they hold more tabs than fit.
- *View ▸ Panels* shows or hides each panel; *View ▸ Panels ▸ Reset Layout* restores the
  default arrangement.

The layout is saved when Rizxel closes and restored at the next start.

**Zen mode** (`Ctrl+Shift+F`, *View ▸ Zen Mode*) hides everything except the title bar,
the viewport and the status bar.

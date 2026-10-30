# Tools

The tools rail holds twelve tools. The upper group draws; the lower group moves, selects
and fills.

| Tool | Key | Use |
|---|---|---|
| [Cursor](cursor.md) | `V` | Inspect voxels and layers without editing |
| [Brush](brush.md) | `B` | Paint voxels by dragging |
| [Line](line.md) | `L` | Lines, polylines and curves |
| [Shape](shape.md) | `S` | Cubes, spheres, cylinders, pyramids and cones |
| [Plane](plane.md) | `P` | Place the work plane; Slice Mode |
| [Move](move.md) | `M` | Move the selection or the layer; resize the bounds |
| [Transform](transform.md) | `T` | Free rotate, scale and move |
| [Color Picker](color-picker.md) | `C` | Pick a voxel's color |
| [Selection](selection.md) | `R` | Box selection |
| [Fuzzy Select](fuzzy-select.md) | `W` | Select connected voxels of a similar color |
| [Extrude](extrude.md) | `E` | Push or pull a face region |
| [Fill](fill.md) | `F` | Recolor a region or fill empty space |

The keys can be changed in [*Settings ▸ Keymap*](../settings.md).

## Voxel operations

The option bar's operation buttons apply to the drawing tools:

| Operation | Effect |
|---|---|
| **Add** | Places voxels of the selected color, replacing the voxels already there. |
| **Subtract** | Removes voxels. Holding `Shift` subtracts with the drawing tools. |
| **Join** | Places voxels in empty cells only; existing voxels keep their color. |
| **Paint** | Recolors existing voxels; never adds or removes any. |

## Common options and modifiers

- **Symmetry X / Y / Z** mirrors the stroke across the middle of the bounds.
- **Antialias** smooths the edges of strokes and shapes.
- `Alt` + click picks a color with any tool.
- `Esc` cancels and `Enter` applies an open drag, box, curve, paste or transform.
- The selection tools use `Shift` to add to and `Ctrl` to subtract from the selection.
- With a selection and **Mask Mode** on (`Shift+M`), the selected voxels are protected
  from edits.

Tools edit the active layer. Locked layers (or layers in a locked group) can't be
edited; the status bar says why an edit was refused.

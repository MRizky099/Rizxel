# Layers

Layers stack from bottom to top; where visible layers overlap, the upper one wins.

## Rows

- The eye shows or hides a layer; `Ctrl` + click shows only that layer.
- The lock protects a layer from edits. A locked group locks its contents.
- Double-click renames; drag to reorder or into and out of a group; right-click for
  more (Duplicate, Merge Down, Show Only This, Delete…).

## Selecting several layers

- **Click** selects one layer and makes it active.
- **`Ctrl` + click** adds a layer to the selection or removes it.
- **`Shift` + click** selects every row from the last clicked one to this one, in the
  order the panel shows them (a collapsed group counts as one row);
  **`Ctrl+Shift` + click** adds that range.
- With the pointer over the panel, **`Ctrl+A`** selects every layer and **`Esc`** goes
  back to the active layer alone.

The active layer keeps the strong highlight; the other selected rows are lighter. With
several layers selected:

- **`Ctrl+G`** (*Layer ▸ Group*, or *Group Selected* in the right-click menu) puts them
  in a new group, in their order, where the top-most one was. A selected group brings
  its contents. One undo step.
- **Dragging** any selected row moves the whole selection above, below or into a group,
  keeping its order (one undo step). A drop into one of the selected groups is refused.
- The **eye** and **lock** of a selected row apply to every selected layer.
- The right-click menu and the toolbar's delete and duplicate buttons act on all of
  them: *Duplicate*, *Merge Selected* (into the lowest selected voxel layer; no groups),
  *Delete* (children of a deleted group that aren't selected move up a level). The
  labels show how many layers.

## Select on click

Clicking a layer also selects all its voxels, so [Move](../tools/move.md), Delete, Copy
and [Transform](../tools/transform.md) act on the whole layer right away. Each selected
voxel layer gets its own full selection; groups, shapes and empty layers select nothing.
Clicking through several layers is one undo step (*Select Layer*), so undo goes back to
the selection before the first click.

Painting new voxels still works while a layer is fully selected, unless *Mask selection*
is on in the tool's options, which limits brushes to the selection. Turn select on click
off in [Settings ▸ General](../settings.md#general) (*Clicking a layer selects its
voxels*).

## Layer kinds

| Kind | What it is |
|---|---|
| **Voxels** | Hand-edited voxels. |
| **Group** | A folder (`Ctrl+G` groups the selected layers, `Ctrl+Shift+G` ungroups). Hiding or locking a group applies to its contents; group opacity multiplies. |
| **Linked clone** | Shows another layer's voxels at an offset and follows its later edits. |
| **Shape** | A procedural cube, sphere or cylinder in a box you can resize later. |

## Properties

| Property | Effect |
|---|---|
| **Opacity** | Draws the layer translucent, in both modes. |
| **Crop** | Shows (and exports) only the voxels inside a box; the stored voxels are kept. |
| **Material** | Metallic, roughness, emission and tint; Render mode uses more fields (see [Render](render.md)). Several layers can share a material. |

## Panel buttons

New layer, new group, duplicate, merge down, merge visible, delete, delete hidden
layers, lock or unlock all layers, and *Selection to New Layer* (copy `Ctrl+J`, cut
`Ctrl+Shift+J`).

The *Layer* menu adds Separate Parts, Split at Work Plane and layer Booleans.

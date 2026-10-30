# Layers

Layers stack from bottom to top; where visible layers overlap, the upper one wins.

## Rows

- The eye shows or hides a layer; `Ctrl` + click shows only that layer.
- The lock protects a layer from edits. A locked group locks its contents.
- Double-click renames; drag to reorder or into and out of a group; right-click for
  more (Duplicate, Merge Down, Show Only This, Delete…).

## Layer kinds

| Kind | What it is |
|---|---|
| **Voxels** | Hand-edited voxels. |
| **Group** | A folder (`Ctrl+G` groups the active layer, `Ctrl+Shift+G` ungroups). Hiding or locking a group applies to its contents; group opacity multiplies. |
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

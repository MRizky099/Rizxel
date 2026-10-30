# Wavefront .obj and .mtl

## Export

*File ▸ Export ▸ OBJ* opens an options dialog:

| Option | Values |
|---|---|
| **Scale** | Units per voxel (default 0.1). |
| **Pivot** | *Bottom center* (default, like MagicaVoxel), *Center* or *Corner*. |
| **Layout** | *MagicaVoxel* (default: `.obj` + `.mtl` + a 256×1 palette `.png`, smallest) or *Fast load* (one index per corner and plain `Kd` colors, for loaders that copy faces straight into buffers). |
| **Files** | *One file per visible layer*, all in the same frame. |

Hidden faces are always removed and same-color faces merged.

## Import

Opens the **Voxelizer** dialog. *Use the mesh's own voxel grid* (on by default) imports
exports of voxel editors (Rizxel, MagicaVoxel, Qubicle…) exactly; other meshes are
scaled so their longest side spans *Size* voxels. *Rotation* turns the model in quarter
turns. Colors come from vertex colors, the `map_Kd` texture or `Kd`.

## .mtl

*File ▸ Import ▸ MTL* replaces the active document's palette and materials (undoable):
a 256×1 palette texture maps one to one; otherwise the `Kd` colors are used in file
order. *Export ▸ MTL* writes the `.mtl` and palette `.png` only.

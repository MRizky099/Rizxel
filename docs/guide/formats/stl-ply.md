# STL and PLY

| Format | Import | Export |
|---|---|---|
| `.stl` | Binary or ASCII, Z-up, voxelized like OBJ (one color). | Binary, Z-up, centered on X/Y, 1 voxel = 1 mm by default. For 3D printing. |
| `.ply` | ASCII or binary, Z-up, voxelized like OBJ, with vertex colors. | Binary, Z-up, vertex colors, 1 voxel = 0.1 units by default. |

Imports go through the **Voxelizer** dialog (see [OBJ](obj.md#import)); exports of
voxel editors import exactly on their own grid. Export options are the same as for OBJ
(scale, pivot, one file per layer).

Render mode's **Export shape…** also writes STL, OBJ or GLB with the chosen voxel shape
(bricks, rounded cubes…).

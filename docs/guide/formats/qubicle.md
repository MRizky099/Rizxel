# Qubicle .qb, .qbt, .qef

Qubicle is Y-up like Rizxel. Each matrix becomes a layer at its position; exports write
the visible layers (Qubicle has no hidden flag), with clones and shapes baked.

| Format | Import | Export |
|---|---|---|
| `.qb` | One layer per matrix; RGBA or BGRA, compressed or not, either Z orientation. True colors are reduced to the palette. | One compressed matrix per visible layer, cut to its voxels. |
| `.qbt` | Matrices as layers; models and compounds as groups. | Matrices per visible layer; groups as nested models. |
| `.qef` | One layer; up to 255 colors keep their order. | The merged visible layers at the full bounds size, with every palette color. |

Models larger than 256 voxels on a side are refused.

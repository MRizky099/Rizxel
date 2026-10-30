# File formats

| Format | Import | Export | Page |
|---|:---:|:---:|---|
| Rizxel `.rzxl` | ✅ | ✅ | [Rizxel .rzxl](rzxl.md) |
| MagicaVoxel `.vox` | ✅ | ✅ | [MagicaVoxel .vox](vox.md) |
| Goxel `.gox` | ✅ | ✅ | [Goxel .gox](gox.md) |
| Qubicle `.qb`, `.qbt`, `.qef` | ✅ | ✅ | [Qubicle](qubicle.md) |
| Wavefront `.obj` + `.mtl` | ✅ | ✅ | [OBJ and MTL](obj.md) |
| glTF 2.0 `.gltf`, `.glb` | — | ✅ | [glTF](gltf.md) |
| `.stl`, `.ply` | ✅ | ✅ | [STL and PLY](stl-ply.md) |
| PNG, JPEG | Heightmaps, reference images | Slices, renders | [Images](images.md) |
| `.gpl`, `.hex` palettes | ✅ | ✅ | [Palettes](palettes.md) |

Imports open in a new tab (except `.mtl` and palettes, which change the active
document). Every file is checked while it loads: a damaged or hostile file is refused
with a message instead of crashing Rizxel.

Rizxel is Y-up. MagicaVoxel, Goxel, STL and PLY are Z-up; conversions rotate the model
and never mirror it.

Files can also be converted without the UI:

```text
cargo run -p rizxel-app --example convert -- input.vox output.glb
```

The technical details and limits are in
[`docs/interchange_formats.md`](https://github.com/MRizky099/Rizxel/blob/main/docs/interchange_formats.md).

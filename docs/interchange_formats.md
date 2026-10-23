# Interchange Formats

Rizxel's native format is `.rzxl` (see [`rzxl_format.md`](rzxl_format.md)).
These formats are supported for exchange with other tools:

| Format | Import (File ▸ Import / Open) | Export (File ▸ Export) |
|---|---|---|
| MagicaVoxel `.vox` | New tab. Each shape instance becomes a layer; groups, hidden flags and the palette come along; repeated instances of a model become linked clones. | One model per layer (shapes, clones and crops baked in), with layer names, hidden flags and the palette. |
| Goxel `.gox` | New tab. Layers, groups, opacity, lock, materials, shape layers, and clones that move by whole voxels; other clones are baked. True colors are reduced to the palette. | Groups, shapes, linked clones and materials are written natively; crops are baked in. The bounds become the image box. |
| Wavefront `.obj` | New tab. Voxel-editor exports (Rizxel, MagicaVoxel, Qubicle, …) import losslessly on their own grid. Other meshes are scaled to 64 voxels on the longest side. Colors come from vertex colors, the `map_Kd` texture or `Kd`. | `.obj` + `.mtl` + palette `.png` in MagicaVoxel's layout (below). |
| `.mtl` | Replaces the active document's palette and materials (undoable). A 256×1 palette texture maps one to one; otherwise the `Kd` colors are used in file order. | `.mtl` + palette `.png`. |
| STL `.stl` | New tab. Binary or ASCII, Z-up, voxelized like OBJ, in one color. | Binary, Z-up, centered on X/Y, 1 voxel = 1 mm. |

Code: `rizxel-io/src/format_{vox,gox,obj,mtl,stl}.rs`, `mesh_import.rs`, and
`rizxel-app/src/interchange.rs`. To convert files from the command line:

```text
cargo run -p rizxel-app --example convert -- input.vox output.obj
```

## Axes

Rizxel is Y-up. MagicaVoxel, Goxel and STL are Z-up. Conversion is a
rotation (Z-up `(x, y, z)` = Rizxel `(x, depth − 1 − z, y)`), never a
mirror. OBJ is Y-up like Rizxel.

## OBJ export layout

The layout follows MagicaVoxel's OBJ export (`chr_knight.obj`):

```text
# Rizxel @ MRizky099

# group
o 

# material
mtllib chr_knight.mtl
usemtl palette

# normals
vn -1 0 0          (−X, +X, +Z, −Z, −Y, +Y: always all six)
...

# texcoords
vt 0.0332031 0.5   (one per used color index i: u = (i − 0.5) / 256)
...

# verts
v -1 0.7 -0.1      (%g numbers, scale 0.1, X/Z centered with integer halves)
...

# faces
f 3/17/1 2/17/1 1/17/1
...
```

- **Line endings**: CRLF everywhere, OBJ and MTL alike.
- **Vertices**: shared within each normal group and sorted per group.
- **Faces**: quads split into `A B C` + `C B D`, outward-facing.
- **MTL**: one `palette` material with `illum 1`, `Ka 0`, `Kd 1`, `Ks 0`,
  `map_Kd name.png`.
- **PNG**: 256×1 RGBA. Pixel `p` holds color index `p + 1`, and the last
  pixel is transparent.

Checked against the original (`rizxel-io` test
`matches_magicavoxel_reference_when_present`):
- `chr_knight.vox` imported and exported again gives the same `vn` and `vt`
  lines, the same faces at the start of the file and a pixel-identical
  palette PNG.
- It covers exactly the same surface as MagicaVoxel's `chr_knight.obj`.

Rizxel merges same-color faces greedily across the whole model, so it writes
fewer triangles (1012 against MagicaVoxel's 1124 for the knight).

## Limits

- Imports larger than 256 voxels on any axis are refused with a message.
- MagicaVoxel per-color materials (`MATL`), cameras and animation frames
  are ignored.
- Goxel previews, cameras, lights and 2D image layers are ignored.
- STL carries no color.
- `.vox` export bakes linked clones into their own models. MagicaVoxel
  instancing would place the whole moved model box and grow the bounds on
  re-import. Use `.gox` or `.rzxl` to keep clones linked.

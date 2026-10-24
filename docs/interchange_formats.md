# Interchange Formats

Rizxel's native format is `.rzxl` (see [`rzxl_format.md`](rzxl_format.md)).
These formats are supported for exchange with other tools:

| Format | Import (File ▸ Import / Open) | Export (File ▸ Export) |
|---|---|---|
| MagicaVoxel `.vox` | New tab. Each shape instance becomes a layer; groups, hidden flags and the palette come along; repeated instances of a model become linked clones. | One model per layer (shapes, clones and crops baked in), with layer names, hidden flags and the palette. Groups become named, nested group nodes. |
| Goxel `.gox` | New tab. Layers, groups, opacity, lock, materials, shape layers, and clones that move by whole voxels; other clones are baked. True colors are reduced to the palette. | Groups, shapes, linked clones and materials are written natively; crops are baked in. The bounds become the image box. |
| Wavefront `.obj` | New tab. Voxel-editor exports (Rizxel, MagicaVoxel, Qubicle, …) import losslessly on their own grid. Other meshes are scaled to 64 voxels on the longest side. Colors come from vertex colors, the `map_Kd` texture or `Kd`. | `.obj` + `.mtl` in the Fast load layout (default), or + palette `.png` in MagicaVoxel's layout (below). |
| `.mtl` | Replaces the active document's palette and materials (undoable). A 256×1 palette texture maps one to one; otherwise the `Kd` colors are used in file order. | `.mtl` + palette `.png`. |
| STL `.stl` | New tab. Binary or ASCII, Z-up, voxelized like OBJ, in one color. | Binary, Z-up, centered on X/Y, 1 voxel = 1 mm. |
| Qubicle `.qb` | New tab. One layer per matrix at its position; RGBA or BGRA, RLE or raw, either Z orientation. True colors are reduced to the palette. | One RLE matrix per visible layer, cut to its voxels, at its position. |
| Qubicle `.qbt` | New tab. Matrices as layers; models and compounds as groups. | Matrices per visible layer, groups as compounds. |
| Qubicle `.qef` | New tab. One layer; up to 255 colors keep their order. | Merged visible layers at the full bounds size, every palette color. |
| Stanford `.ply` | New tab. ASCII or binary, Z-up, voxelized like STL, with vertex colors. | Binary little-endian, Z-up, vertex colors, 1 voxel = 0.1. |
| Heightmap (PNG, JPEG) | File ▸ Import ▸ Heightmap. New tab: brightness sets column heights; optional color image. | — |
| glTF 2.0 `.glb` / `.gltf` | — | One node per visible layer, groups as parent nodes, linked clones as instances of one mesh. Vertex colors or a palette texture; layer materials as PBR. |

Code: `rizxel-io/src/format_{vox,gox,obj,mtl,stl,gltf,qb,qef,qbt,ply}.rs`, `qubicle.rs`, `mesh_import.rs`, and
`rizxel-app/src/interchange.rs`. To convert files from the command line:

```text
cargo run -p rizxel-app --example convert -- input.vox output.obj
```

## Axes

Rizxel is Y-up. MagicaVoxel, Goxel and STL are Z-up. Conversion is a
rotation (Z-up `(x, y, z)` = Rizxel `(x, depth − 1 − z, y)`), never a
mirror. OBJ is Y-up like Rizxel.

## OBJ export layouts

The export dialog (and `convert --obj-layout fast|magicavoxel`) picks one
of two layouts. Both use the same merged quads and placement, so they
cover exactly the same surface and import back to the same voxels.

| | Fast load (default) | MagicaVoxel |
|---|---|---|
| Vertices | 4 per quad, one shared index per corner: `f 1/1/1 2/2/2 3/3/3` | shared per normal group |
| Normals | one per vertex | the six axis normals |
| Colors | one `usemtl color_<index>` group per color (and layer material), plain `Kd` | one `palette` material, 256×1 PNG |
| Files | `.obj` + `.mtl` | `.obj` + `.mtl` + `.png` |
| Size | about 3–5× larger | smallest |
| Use | Blender, game engines, viewers | MagicaVoxel parity |

### Fast load

Follows BrickLink Studio's OBJ export (`Kaki_Naga.obj`), which loads
quickly because loaders can copy every corner straight into a vertex
buffer: no v/vt/vn combinations to rebuild and no texture to decode.

```text
# Rizxel @ MRizky099
# layout: fast load (per-corner vertices, linear Kd)

mtllib model.mtl
o model

v -0.4 0 -0.2      (4 per quad, grouped by material)
...
vt 0.0332031 0.5   (one per vertex, into the palette strip)
...
vn 0 1 0           (one per vertex)
...

usemtl color_9
f 1/1/1 2/2/2 3/3/3
...
```

- **MTL**: per group `newmtl color_<index>` (or `color_<index>_m<n>` for a
  layer material), `Kd` as linear RGB times the material tint, `Ke` for
  emission, `d` below full opacity, and `Pr`/`Pm` for roughness and
  metallic. No `map_Kd`.
- **Linear `Kd`**: the second header line tells Rizxel's importer to turn
  `Kd` back into sRGB, so colors come back exact. Blender also reads `Kd`
  as linear.
- **CRLF** line endings, `%g` numbers, faces wound counter-clockwise.

### MagicaVoxel

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

## Voxelizer dialog

File ▸ Import ▸ OBJ, STL or PLY opens the Voxelizer before the new tab:

- **Grid**: *Use the mesh's own voxel grid* (on by default) imports
  voxel-editor exports exactly. Off, or for other meshes, the mesh is
  scaled so its longest side spans *Size* voxels (1–256).
- **Rotation**: quarter turns about X, Y and Z (applied in that order,
  after the file's up axis is converted), never a mirror.
- **Preview**: the result's size and voxel count update live.

Open… and drag and drop still import meshes directly with the defaults.
Headless: `convert --resolution N --turn y input.stl output.rzxl`.

## Heightmap import

File ▸ Import ▸ Heightmap reads a PNG or JPEG. Image X runs along +X and
image Y (down) along +Z, like a map seen from the top. A pixel's
brightness (Rec. 601 luma, transparent counts as black) sets its column
height, `round(luma / 255 × max height)` voxels standing on Y = 0; black
columns stay empty. Images over 256 pixels per side are scaled down. An
optional color image (any size, sampled at the same relative position,
reduced to the palette) colors the columns; otherwise they use the
default palette's highlighted color. Headless:
`convert --heightmap 32 terrain.png terrain.rzxl`.

## Export Slices

File ▸ Export ▸ *Slices* (and `convert --slices x|y|z --slice-scale N
input folder`) writes one PNG per voxel slice of the visible layers into a
folder: `name_y000.png`, `name_y001.png`, … Every image covers the whole
bounds, empty cells are transparent and each voxel is N×N pixels (1–16).
Slice 0 is the lowest coordinate. Images are seen from the positive end of
the axis: Y slices from the top (+X right, +Z down), X and Z slices with
+Y up (X from the right, Z from the front). It runs on the CPU, so tests
check pixel colors in CI.

## Qubicle formats

All three are Y-up like Rizxel. Each matrix becomes a layer at its
position; the bounds are the union of the matrix boxes, moved to the origin
when a matrix lies below zero, and refused above 256 per axis. Exports
write the visible layers only (Qubicle files have no hidden flag), with
linked clones and shapes baked, and re-exporting a file Rizxel wrote gives
the same bytes.

- **`.qb`**: six `u32` header fields (version 1.1.0.0, color format,
  Z orientation, compression, visibility mask, matrix count), then per
  matrix a `u8` name length, name, size, position and voxels (X fastest,
  then Y, then Z; RLE per Z slice with `2` = run and `6` = end of slice).
  Alpha 0 is empty; with the visibility mask any non-zero alpha is solid.
  Left-handed files are mirrored on Z (`z → −z − 1`). Rizxel writes RGBA,
  right-handed, RLE, no mask.
- **`.qbt`**: `QB 2`, a color map (with colors, a voxel's red byte is an
  index into it) and a node tree: matrix (zlib voxels, Y fastest, then Z,
  then X; mask 0 = empty), model (children) and compound (a merged matrix
  plus children). Models and compounds become groups; a compound's own
  merged matrix is skipped when it has children. Rizxel writes true colors,
  the face mask, and every group as a compound holding the merge of its
  content. Unknown node types are skipped. Read and written right-handed.
- **`.qef`**: text: three header lines, the size, the colors (`r g b`,
  0..1) and one `x y z index mask` line per voxel. Up to 255 colors become
  the palette in file order, so indices survive; more are reduced. The
  export writes every palette color, the full bounds size (so the model
  keeps its place) and the face mask (bit 0 solid, then left, right, top,
  bottom, front, back).

Hostile inputs are refused with a message: sides of 0 or over 256,
truncated data or RLE, runs past their slice, name lengths past the end,
unknown flags, huge counts, nesting deeper than 32, color indices past
the map, and zlib data that does not inflate to the matrix size (it stops
reading there).

## PLY

Export writes binary little-endian PLY: per merged quad 4 vertices
(`float x y z`, `float nx ny nz`, `uchar red green blue`) and one 4-sided
face wound counter-clockwise from outside. It is Z-up, like MagicaVoxel's
PLY export and Blender, centered on X/Y and standing on Z = 0, 1 voxel =
0.1 units by default. Import reads ASCII and binary (both byte orders),
`vertex_indices` faces split into fans, and `red green blue` as integers
or 0..1 floats; other elements are skipped. Voxel-editor PLY files import
exactly on their own grid, like OBJ.

## glTF export

`.glb` is one binary file; `.gltf` is JSON with a `.bin` of the same name
next to it. Both hold the same data:

- **Nodes**: one per visible layer with voxels, named after the layer.
  Groups that hold exported layers become parent nodes. A linked clone that
  is its source moved by whole voxels (no crop on either, nothing cut off
  at the bounds) reuses the source's mesh with a node translation.
- **Meshes**: the same merged quads as the OBJ export, 4 vertices and 2
  triangles each, flat normals, counter-clockwise front faces.
- **Frame**: Y-up, 1 voxel = 0.1 m by default, placed like the OBJ export
  (X/Z centered with integer halves, standing on Y = 0), so glTF and OBJ
  exports of a model overlap exactly.
- **Colors**: *Vertex colors* (default) writes linear `COLOR_0`. *Palette
  texture* writes `TEXCOORD_0` into the embedded 256×1 palette PNG (same
  UVs as the OBJ export), sampled nearest.
- **Materials**: one per distinct layer material and opacity:
  `baseColorFactor` from the tint and opacity, `metallicFactor`,
  `roughnessFactor`, `emissiveFactor`. Layers below full opacity use
  `alphaMode: BLEND`.
- **Buffers**: one buffer, 4-byte aligned views, `POSITION` accessors with
  `min`/`max`, 32-bit indices.

glTF is export only. Tests check the GLB header and chunk lengths, view
alignment, accessor bounds, one node per visible layer, clones sharing a
mesh, and that the triangles cover exactly the OBJ export's surface.

## Limits

- Imports larger than 256 voxels on any axis are refused with a message.
- MagicaVoxel per-color materials (`MATL`), cameras and animation frames
  are ignored.
- Goxel previews, cameras, lights and 2D image layers are ignored.
- STL carries no color.
- `.qef` holds one matrix: layers are merged on export.
- Qubicle matrix pivots, local scales and the `.qbt` global scale are
  ignored on import.
- glTF export writes no animation, cameras or lights. Bounds still cap
  models at 256 voxels per axis.
- `.vox` export bakes linked clones into their own models. MagicaVoxel
  instancing would place the whole moved model box and grow the bounds on
  re-import. Use `.gox` or `.rzxl` to keep clones linked.

# `.rzxl` Project Format — Version 4

`.rzxl` is Rizxel's native project format. Reader and writer live in
`rizxel-io/src/format_rzxl.rs`. The shared layer types (`LayerKind`,
`ShapeKind`, `VoxelBox`, `Material`) live in `rizxel-math/src/layer.rs`;
reference images (`ReferenceImage`, `RefPlane`, `ImageData`) in
`rizxel-math/src/reference.rs`.

## Container

All integers are little-endian.

| Offset | Size | Field |
|--------|------|-------|
| 0 | 4 | Magic `RZXL` |
| 4 | 4 | `u32` format version (currently `4`) |
| 8 | 4 | `u32` uncompressed body length (max 512 MiB) |
| 12 | n | zstd-compressed UTF-8 JSON body (`ProjectFile`) |

Writers save to `<file>.rzxl.tmp` and rename it over the target, so a failed
save never truncates an existing project.

## Body (`ProjectFile`)

```jsonc
{
  "generator": "Rizxel 0.5.0",        // writer, diagnostics only
  "name": "hero",
  "palette": { "colors": [{ "r": 0, "g": 0, "b": 0, "a": 255 }, ...] },
  "selected_color": 35,                // palette index, never 0
  "materials": [                       // v3; referenced by index
    { "name": "Neon", "metallic": 0.2, "roughness": 0.5,
      "emission": [0.2, 0.8, 1.0], "tint": [1, 1, 1, 1] }
  ],
  "layers": [ /* bottom to top, see below */ ],
  "active_layer": 0,
  "bounds": [32, 32, 32],              // 1..=256 per axis
  "references": [ /* v4, draw order; omitted when empty, see below */ ]
}
```

## Layers (`LayerData`)

```jsonc
{
  "id": 3,                  // unique, non-zero; links use ids, not indices
  "name": "Right Arm",
  "visible": true,
  "locked": false,
  "opacity": 0.5,           // 0..=1
  "parent": 10,             // optional: id of the containing group
  "material": 1,            // optional: index into "materials"
  "crop": { "min": [0,0,0], "max": [16,8,16] },  // optional, max exclusive
  "kind": { "type": "voxels" },
  "voxels": [ { "start": 0, "len": 12, "value": 9 } ]
}
```

`kind` is a tagged object:

| `type` | Fields | Meaning |
|--------|--------|---------|
| `voxels` | — | Hand-edited voxels stored in `voxels`. |
| `group` | `collapsed` (bool, default `false`) | Folder. Children name it as `parent`. Has no geometry. Hiding a group hides its children. |
| `clone` | `source` (layer id), `offset` (`[x,y,z]`, default `[0,0,0]`) | Linked clone: shows the source's voxels moved by `offset` and follows later edits to the source. The source must be a `voxels` or `shape` layer. |
| `shape` | `shape` (`cube` / `sphere` / `cylinder`), `area` (box), `color` (palette index) | Procedural primitive. Spheres and cylinders (along +Y) are inscribed in `area`. |

**Layer order.** Layers are listed bottom to top. The editor keeps each
group's contents directly below the group (the group's subtree is the
contiguous run of layers ending at the group). Files that list them
elsewhere (Goxel lists groups first) still load: the editor reorders them on
load, keeping the order of siblings.

Only `voxels` layers may have voxel runs. **Voxel runs** cover the project
bounds in Z-X-Y order (`y` changes fastest, then `x`, then `z`):
`index = (z * size_x + x) * size_y + y`. Runs must be sorted, must not
overlap, must stay inside the bounds, and use values `1..=255`.

**Evaluation order** for a visible layer:
1. Take the layer's own voxels, the rasterized shape, or the moved clone source.
2. Clip that to `crop` when it is set.

A layer contributes only when it and every group above it are visible. Where
layers overlap, the later layer wins. The viewport draws a layer with its
`opacity` times every enclosing group's (and its material tint's alpha);
fully opaque layers merge into one mesh, translucent ones are drawn after it,
sorted back to front, and fully transparent ones are not drawn.

## Reference images (`ReferenceImage`, version 4)

```jsonc
{
  "id": 1,                  // unique, non-zero
  "name": "front.png",
  "image": { "width": 1024, "height": 512, "data": "iVBORw0KGgo..." },
  "visible": true,          // default true
  "locked": false,          // default false; the panel keeps it in place
  "plane": "xy",            // "xy" (front), "xz" (floor) or "yz" (side)
  "offset": [0, 0, 0],      // voxels from the bounds' min corner
  "size": [32, 16],         // width, height in voxels (before rotation)
  "rotation": 0,            // quarter turns, counterclockwise, 0..=3
  "flip_u": false,
  "flip_v": false,
  "opacity": 0.5,           // 0..=1, default 0.5
  "depth": "behind",        // "behind" (depth-tested) or "on_top"
  "slice": "always"         // "always", "hidden_in_slice" or "slice_only"
}
```

- `image.data` is the image file itself (PNG or JPEG, its original bytes),
  base64-encoded (RFC 4648 with padding). The zstd body compression takes
  most of base64's overhead back. Imports larger than 4096 px on a side are
  scaled down and re-encoded first.
- `offset`: the component along the plane's normal (Z for `xy`, Y for `xz`,
  X for `yz`) is the plane position; the other two are the low corner of the
  image rectangle on the plane. The image reads unmirrored from the view the
  plane faces: Front for `xy`, Top for `xz` (image up is −Z), Right for `yz`
  (image right is −Z).
- `size` is the image's width and height; a quarter turn swaps the
  rectangle's extents on the plane.

## Visible effects

Every stored field changes what the editor shows:

| Field | Visible as |
|---|---|
| `name`, `palette`, `selected_color`, `active_layer` | Tab title, Color Palette panel, Layers panel |
| `bounds` | Yellow bounds wireframe (*View ▸ Show ▸ Model Bounds*), status bar |
| `layers[].name`, `visible`, `locked` | Layers panel (eye and lock buttons); hidden layers leave the viewport |
| `layers[].voxels`, `kind`, `crop` | Viewport geometry |
| `layers[].parent` | A hidden group hides its children; group opacity multiplies |
| `layers[].opacity` | Translucent layer in the viewport (both modes), Layers panel slider |
| `layers[].material`, `materials[]` | Render mode: tint color, emission glow, roughness and metallic highlight; the tint alpha also acts as opacity. Layers panel material editor |
| `layers[].kind.collapsed` | Layers panel: the group's rows are folded |
| `references[]` | Textured quads in the viewport; References panel (thumbnail, name, eye and lock, plane badge, properties) |
| `layers[].id`, `references[].id`, `generator` | Not shown: link targets and diagnostics |

## Validation

Every loaded project is checked before the document sees it, and writers
refuse projects that would fail the same check:

- At most 1024 layers, 256 materials and 256-character names. Group nesting
  is at most 32 levels deep.
- Layer ids are non-zero and unique.
- `opacity` and every material channel are finite and inside `0..=1`.
- `material` indices exist. `crop`, shape areas and clone offsets are
  non-empty and within ±1024 voxels.
- A `parent` must exist, be a group, and must not form a cycle.
- A clone's `source` must exist, must not be the clone itself, and must be a
  `voxels` or `shape` layer.
- Shape colors are palette colors (not `0`). Non-voxel layers carry no runs.
- At most 64 reference images holding at most 128 MiB of image bytes in
  total. Their ids are non-zero and unique; each image is 1..=4096 pixels per
  side. `size` is positive, `offset` and `size` are finite and within ±1024
  voxels, `rotation` is 0..=3 and `opacity` is inside `0..=1`.
- Each embedded image must be valid base64 and decode completely as a PNG or
  JPEG of the stated `width` × `height`; truncated or mislabeled images are
  rejected.

## Versions and compatibility

| Version | Adds |
|---------|------|
| 1 | Palette, layer metadata, bounds. |
| 2 | Per-layer voxel runs. |
| 3 | Layer ids, opacity, groups, linked clones, shape layers, crop boxes, materials, `generator`. |
| 4 | Reference images (`references`) with their image files embedded. |

- Each field added after version 1 has a default, so older bodies decode into
  the current structures. Version 1 and 2 layers become plain voxel layers
  with ids `1..=n`, opacity 1, no group and the default material. Version
  1–3 projects have no reference images.
- Writers always write the newest version.
- Files with a newer version are rejected (`UnsupportedVersion`). They are
  never partially loaded, because the next save would drop data the newer
  writer stored.
- Unknown JSON fields are ignored when reading. Any new field still requires
  a version bump, for the reason above.

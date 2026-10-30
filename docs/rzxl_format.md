# `.rzxl` Project Format — Version 6

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
| 4 | 4 | `u32` format version (currently `6`) |
| 8 | 4 | `u32` uncompressed body length (max 512 MiB) |
| 12 | n | zstd-compressed UTF-8 JSON body (`ProjectFile`) |

Writers save to `<file>.rzxl.tmp` and rename it over the target, so a failed
save never truncates an existing project.

## Body (`ProjectFile`)

```jsonc
{
  "generator": "Rizxel 0.11.0",       // writer, diagnostics only
  "name": "hero",
  "palette": { "colors": [{ "r": 0, "g": 0, "b": 0, "a": 255 }, ...] },
  "selected_color": 35,                // palette index, never 0
  "materials": [                       // v3; referenced by index
    { "name": "Neon", "metallic": 0.2, "roughness": 0.5,
      "emission": [0.2, 0.8, 1.0], "tint": [1, 1, 1, 1],
      // v5: path tracer fields
      "specular": 0.5, "ior": 1.5, "transmission": 0.0,
      "clearcoat": 0.0, "emission_strength": 1.0 }
  ],
  "layers": [ /* bottom to top, see below */ ],
  "active_layer": 0,
  "bounds": [32, 32, 32],              // 1..=256 per axis
  "references": [ /* v4, draw order; omitted when empty, see below */ ],
  "render": { /* v5, Render mode settings, see below */ }
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

## Render settings (`RenderSettings`, version 5; line fields version 6)

Render mode's settings are saved so a render can be reproduced. Every group
and field has a default, so a missing field (or a missing `render`, as in
version 1–4 files) takes the default; version 5 files load with voxel lines,
the floor grid and matte off. Colors are sRGB in `0..=1`, angles
degrees.

```jsonc
"render": {
  "light": {
    "sun": true, "sun_azimuth": 45, "sun_elevation": 50, "sun_size": 5,
    "sun_intensity": 3, "sun_color": [1, 0.97, 0.92],
    "sky": "gradient",             // uniform | gradient | image
    "sky_color": [0.78, 0.84, 0.92], "sky_zenith": [0.45, 0.6, 0.85],
    "sky_intensity": 1, "sky_rotation": 0,
    "sky_image": "",               // path of an equirectangular .hdr
    "fog": false, "fog_density": 0.01, "fog_color": [0.85, 0.88, 0.92],
    "studio_lights": false         // fill and rim lights next to the sun
  },
  "sample": {
    "samples": 256, "diffuse_bounces": 3, "specular_bounces": 6,
    "transmission_bounces": 8, "russian_roulette": true, "clamp": 10,
    "filter": "gaussian",          // box | gaussian
    "pixelated": false, "denoise": "auto"   // off | auto
  },
  "display": {
    "ground": true, "ground_color": [0.72, 0.72, 0.72], "shadow_catcher": false,
    "background": "gradient",      // color | gradient | sky | transparent
    "background_color": [0.36, 0.38, 0.42], "background_top": [0.62, 0.66, 0.72],
    "bevel": 0.06, "seams": false,
    "edges": false, "edge_color": [0.1, 0.1, 0.12], "edge_width": 1,
    "shape": "cube",               // cube | brick | rounded | sphere | cylinder
    "cell": 1,                     // 0.5..=1
    // v6: a line around every voxel face (cube shape with cell 1 only)
    "voxel_lines": false, "voxel_line_color": [0.02, 0.03, 0.12],
    "voxel_line_width": 0.04,      // voxels, 0.005..=0.25
    "voxel_line_opacity": 0.85,
    // v6: grid lines on the ground (drawn only with "ground": true)
    "floor_grid": false, "floor_grid_color": [1, 0.88, 0.8],
    "floor_grid_width": 0.03,      // voxels, 0.005..=0.25
    "floor_grid_spacing": 1,       // voxels between lines, 1..=16
    "floor_grid_fade": 1.5,        // fade radius in lengths of the bounds' longest side, 0 = none, ..=8
    "floor_grid_glow": 0.6,        // light the lines give off, 0..=4
    "matte": false                 // v6: show every material matte (view only)
  },
  "camera": {
    "fov": 45, "aperture": 0, "focus_distance": 64, "blades": 0,
    "exposure": 0, "tone_map": "aces",      // linear | aces | filmic
    "gamma": 2.2, "vignette": 0,
    "bloom": false, "bloom_threshold": 1, "bloom_strength": 0.15
  },
  "output": {
    "kind": "image",               // image | photo | turntable
    "width": 1920, "height": 1080, "transparent": false,
    "folder": "", "name": "{name}_render",
    "time_limit": 0, "png16": false,
    "turntable_frames": 36, "turntable_angle": 360,
    "turntable_path_traced": false, "turntable_samples": 32,
    "turntable_png": true, "turntable_gif": true, "turntable_sheet": false,
    "gif_fps": 12
  }
}
```

The voxel shape and `matte` are view settings: the stored voxels and
materials never change, and Model mode and its exports ignore them. Floor
grid lines run along voxel borders through the bounds' corner. The camera's orientation and
projection are view state and are not saved.

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
| `materials[].specular`, `ior`, `transmission`, `clearcoat`, `emission_strength` | Path-traced Render mode and outputs; Render panel's Material tab |
| `render` | Render mode: the viewport preview, the Render panel and render bar, and the files the Render button writes |
| `render.display.voxel_lines`, `floor_grid`, `matte` and their fields | Render mode: lines around the voxels, the grid on the ground, matte materials (raster and path traced); Render panel ▸ Display, *View ▸ Floor Grid* |
| `layers[].id`, `references[].id`, `generator` | Not shown: link targets and diagnostics |

## Validation

Every loaded project is checked before the document sees it, and writers
refuse projects that would fail the same check:

- `bounds` are `1..=256` per axis and the palette holds 1..=256 colors.
- At most 1024 layers, 256 materials and 256-character names. Group nesting
  is at most 32 levels deep.
- Layer ids are non-zero and unique.
- `opacity` and every material channel are finite and inside `0..=1`,
  except `ior` (`1..=3`) and `emission_strength` (`0..=100`).
- `render` values are not rejected but clamped into range on load (sample
  counts, bounces, sizes up to 8192, angles, colors, paths up to 1024
  characters); non-finite numbers fall back to their defaults.
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
- `active_layer` and `selected_color` are not rejected: the editor clamps
  them to the top layer and to the palette (never slot 0).
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
| 5 | Render settings (`render`) and the material fields `specular`, `ior`, `transmission`, `clearcoat`, `emission_strength`. |
| 6 | Render display fields `voxel_lines`, `voxel_line_color`, `voxel_line_width`, `voxel_line_opacity`, `floor_grid`, `floor_grid_color`, `floor_grid_width`, `floor_grid_spacing`, `floor_grid_fade`, `floor_grid_glow`, `matte`. |

- Each field added after version 1 has a default, so older bodies decode into
  the current structures. Version 1 and 2 layers become plain voxel layers
  with ids `1..=n`, opacity 1, no group and the default material. Version
  1–3 projects have no reference images; version 1–4 projects get the
  default render settings and material fields; version 1–5 projects have
  voxel lines, the floor grid and matte off.
- Writers always write the newest version.
- Files with a newer version are rejected (`UnsupportedVersion`). They are
  never partially loaded, because the next save would drop data the newer
  writer stored.
- Unknown JSON fields are ignored when reading. Any new field still requires
  a version bump, for the reason above.

## Compatibility promise (candidate)

Version 5, as written by Rizxel 0.10.0, is the candidate for the 1.0 format.
The promise below is a **candidate** during the 0.x betas and becomes
**binding with Rizxel 1.0**. Version 6 (0.11.0) follows it: it only adds
fields, each with a default, and bumps the version.

During the betas (0.10.x, 0.11.x):

- Rizxel keeps loading every file saved by 0.10.0 or later, and every file of
  versions 1–6.
- A breaking change (a field dropped or read differently) is still allowed,
  but only with a CHANGELOG entry that says how to migrate old files.

From 1.0 on, for all of 1.x:

1. Every 1.x reader loads every older version (1 to the current one) without
   losing data.
2. A new field needs a format version bump and a default, so older files
   decode into the newer structures. Older readers reject the newer version
   instead of dropping the field (see *Versions and compatibility*).
3. A change that drops or reinterprets stored data needs a new major version
   (Rizxel 2.0).
4. Unknown fields stay ignored, never an error.

The fixtures in `rizxel-io/tests/fixtures/rzxl_0_10/` were saved by 0.10.0
(version 5), the ones in `rzxl_0_11/` by 0.11.0 (version 6), and they are
never regenerated. Every CI build loads them and checks their contents
(`rizxel-io/tests/rzxl_fixtures.rs`); the 0.10.0 files are also opened as
documents (`rizxel-app/tests/rzxl_fuzz.rs`). Together they use every field
below.

| Fixture | Covers |
|---|---|
| `minimal.rzxl` | One empty layer, all defaults |
| `layers.rzxl` | Voxel runs, hidden and locked layers, opacity, nested groups (one collapsed), cube, sphere and cylinder shapes, linked clones of a voxel and a shape layer, crop boxes |
| `materials.rzxl` | A 16-color palette, every material field, a layer without material |
| `references.rzxl` | Reference images on all three planes, PNG and JPEG, every placement and display option |
| `render.rzxl` | Every render setting away from its default |
| `max_bounds.rzxl` | 256³ bounds, voxels in the corners |
| `rzxl_0_11/voxel_icon.rzxl` | Every version 6 display field away from its default |

### Fields and the version that added them

| Field | Version |
|---|---|
| `name`, `palette`, `selected_color`, `active_layer`, `bounds` | 1 |
| `layers[].name`, `visible`, `locked` | 1 |
| `layers[].voxels` | 2 |
| `generator`, `materials[]` (`name`, `metallic`, `roughness`, `emission`, `tint`) | 3 |
| `layers[].id`, `opacity`, `parent`, `material`, `crop`, `kind` (`voxels`, `group` with `collapsed`, `clone` with `source` and `offset`, `shape` with `shape`, `area` and `color`) | 3 |
| `references[]` (`id`, `name`, `image`, `visible`, `locked`, `plane`, `offset`, `size`, `rotation`, `flip_u`, `flip_v`, `opacity`, `depth`, `slice`) | 4 |
| `materials[].specular`, `ior`, `transmission`, `clearcoat`, `emission_strength` | 5 |
| `render` (`light`, `sample`, `display`, `camera`, `output` and their fields) | 5 |
| `render.display.voxel_lines`, `voxel_line_color`, `voxel_line_width`, `voxel_line_opacity`, `floor_grid`, `floor_grid_color`, `floor_grid_width`, `floor_grid_spacing`, `floor_grid_fade`, `floor_grid_glow`, `matte` | 6 |

### Robustness

Besides the hostile-input unit tests, `rizxel-app/tests/rzxl_fuzz.rs` is a
small mutation fuzzer: it mutates the fixtures (the JSON tree, the JSON text
and the raw file), loads each result, uses the document like the editor does
after opening it, and saves it again. CI runs 400 cases; set
`RIZXEL_FUZZ_SECS` for a longer run. (`cargo fuzz` needs a nightly
toolchain; the project builds on stable.)

# Images: heightmaps, slices, renders

## Heightmap import

*File ▸ Import ▸ Heightmap (.png, .jpg)...* builds terrain: a pixel's brightness sets
its column height (up to *Max height*), black columns stay empty. Image X runs along +X
and image Y along +Z, like a map seen from the top. An optional color image colors the
columns. Images over 256 pixels are scaled down.

## Reference images

PNG and JPEG files dropped on the viewport, or added in the
[Reference Images](../panels/references.md) panel, become reference planes.

## Export Slices

*File ▸ Export ▸ Slices (PNG)...* writes one PNG per voxel slice of the visible layers
into a folder (`name_y000.png`, `name_y001.png`, …), along X, Y or Z, at 1 to 16 pixels
per voxel. Empty cells are transparent.

## Renders

Render mode writes PNG images (8- or 16-bit, optionally transparent), turntable PNG
sequences, sprite sheets and GIFs. See [Render mode](../render-mode.md#outputs).

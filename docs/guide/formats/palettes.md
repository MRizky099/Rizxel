# Palettes: .gpl and .hex

The [Color Palette](../panels/palette.md) panel loads and saves palette files:

| Format | Contents |
|---|---|
| `.gpl` | GIMP palette: a header and one `R G B name` line per color. |
| `.hex` | One `RRGGBB` hex color per line (Lospec's format). |

*Load .gpl / .hex...* replaces the palette colors in order; the voxels keep their slot
numbers. *Load and Snap Model...* instead gives every voxel the nearest color of the new
palette. *Save* writes slots 1 and up.

`.mtl` files and the palettes of imported `.vox`, `.gox` and Qubicle files also bring
their colors along.

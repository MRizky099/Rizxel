# Color Palette

The palette holds up to 255 colors (slot 0 means empty). Click a swatch to select it;
click the selected swatch to edit its color. The count above the swatches says how many
colors there are (with *Used only*, how many the model uses).

| Control | Effect |
|---|---|
| Click | Select the color. |
| `Shift` + click | Mark a second color for *Gradient* and *Remap*. |
| **Used only** | Show only the colors the model uses. |
| **Gradient** | Blend the slots between the marked and the selected color. |
| **Remap** | Recolor every voxel of the marked color with the selected one. |
| **Sort by Hue / Luminance** | Reorder the palette; the voxels keep their look. |
| **Remove Duplicates** | Merge identical colors. |
| **Load .gpl / .hex...**, **Save .gpl / .hex...** | Palette files ([formats](../formats/palettes.md)). |
| **Load and Snap Model...** | Replace the palette and give every voxel the nearest new color. |

## Favorite colors

`Ctrl+Shift+0` … `9` stores the selected color as a favorite; `Ctrl+0` … `9` selects it
again.

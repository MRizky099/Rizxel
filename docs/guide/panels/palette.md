# Color Palette

The palette holds up to 255 colors (slot 0 means empty). The top of the panel shows the
selected color with its hex and RGB values, and the **⋯** button holds the palette menu.
Click a swatch to select it. The count above the swatches says how many colors there are
(with *Used only*, how many the model uses); the swatches scroll inside their well.

## Color picker

Click the large color chip to open the picker in a popup under it; click the chip again,
or anywhere outside the popup, to close it. The picker edits the selected palette color,
and every voxel of that color changes with it (one undo step per drag).

- The square sets saturation (across) and value (up); the bar beside it sets the hue.
- The **eyedropper** switches to the [Color Picker](../tools/color-picker.md) tool, to take
  a color off the model instead.
- **RGB** shows a slider and a value for red, green and blue; **HSV** for hue (0–359°),
  saturation and value (0–100%). Drag a track or a value, or type in the value.
- **Hex** takes `RRGGBB` or `RGB`; the button beside it copies the code.

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

The ten favorite slots stay at the foot of the panel, whatever the scroll. Click an empty
slot (`0` … `9`) to store the selected color there. Click a filled slot,
or press `Ctrl+0` … `9`, to select its color again; right-click a slot, or press
`Ctrl+Shift+0` … `9`, to replace it with the selected color.

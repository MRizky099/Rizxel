# Menus

The shortcuts are the defaults; [*Settings ▸ Keymap*](settings.md) changes them.

## File

| Item | Shortcut | Effect |
|---|---|---|
| New | `Ctrl+N` | New project in a new tab |
| Open... | `Ctrl+O` | Open a project or any importable model |
| Open Recent | | Recent files; *Clear Recent* |
| Save / Save As... | `Ctrl+S` / `Ctrl+Shift+S` | Save as `.rzxl` |
| Import | | `.vox`, `.gox`, `.qb`, `.qbt`, `.qef`, `.obj`, `.stl`, `.ply` in a new tab; `.mtl` replaces the palette and materials; *Heightmap (.png, .jpg)...* |
| Export | | `.vox`, `.gox`, `.qb`, `.qbt`, `.qef`, `.obj`, `.mtl`, `.gltf` / `.glb`, `.stl`, `.ply`, *Slices (PNG)...*, *Shaped Mesh...* |
| Settings... | `Ctrl+,` | [Settings](settings.md) |
| Exit | `Alt+F4` | Close Rizxel (asks to save changes) |

See [File formats](formats/index.md) for what each format keeps.

## Edit

Undo, Redo, Cut, Copy, Paste (at the cursor), Paste in Place, Paste to New Layer,
Delete, New Layer, Duplicate Layer. A paste floats under the cursor until you click or
press `Enter`.

## Selection

Select All (`Ctrl+A`), Deselect (`Ctrl+D`), Invert (`Ctrl+I`), Grow, Shrink, By Color,
To New Layer (Cut or Copy), Mask Mode (`Shift+M`).

## Layer

New Group, Group Layer (`Ctrl+G`), Ungroup (`Ctrl+Shift+G`), Separate Parts (split the
layer into its connected parts; parts under a size stay), Split at Work Plane, and
Boolean (combine the active layer with another one).

## Model

| Item | Effect |
|---|---|
| Fit Bounds to Content | Shrink the bounds to the voxels |
| Fit Bounds on Import | Fit imported models automatically |
| Core ▸ Fill Hollows | Fill closed cavities with the selected color |
| Core ▸ Hollow, Advanced Hollow | Keep only a shell of the given thickness |
| Rotate 90° | Quarter turns about X, Y or Z (`Ctrl+R` / `Ctrl+Shift+R` about the axis you face) |
| Flip | Flip X, Y or Z (`Ctrl+Alt+X / Y / Z`) |
| Mirror | Mirror one half onto the other |
| Slice | Slice Mode (`Space`), Show Work Plane (`/`) |

## View

Zen Mode, Orthographic (`5`), Auto Ortho on Face Snap, Frame Selection (`.`), Frame Work
Area (`Home`), Home View (`Shift+Home`), Faces / Edges / Corners (camera views), Show
(floor grid, voxel grid with a quick line width slider, model bounds), Ghosts, Ambient
Occlusion, Render Mode (`F5`), Reference Images (`Alt+R`), and Panels (show or hide each
panel, Reset Layout).

## Help

| Item | Effect |
|---|---|
| User Guide (`F1`) | Opens this guide in the browser |
| Keyboard Shortcuts | The current shortcuts, including your changes |
| Report an Issue | Opens the issue tracker |
| About Rizxel | Version, author and license |

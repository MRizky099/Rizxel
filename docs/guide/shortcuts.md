# Keyboard shortcuts

These are the default shortcuts. Every one except the clipboard commands can be changed
in [*Settings ▸ Keymap*](settings.md); *Help ▸ Keyboard Shortcuts* in the app lists
your current ones. On macOS, `Ctrl` is the `Cmd` key.

The tables below are generated from Rizxel's keymap.

<!-- BEGIN GENERATED: rizxel-app/tests/guide_shortcuts.rs -->
## File

| Command | Shortcut |
|---|---|
| New project | `Ctrl+N` |
| Open project | `Ctrl+O` |
| Save | `Ctrl+S` |
| Save as | `Ctrl+Shift+S` |
| Settings | `Ctrl+,` |

## Edit

| Command | Shortcut |
|---|---|
| Undo | `Ctrl+Z` |
| Redo | `Ctrl+Y` or `Ctrl+Shift+Z` |
| Cut (selection or layer) | `Ctrl+X` |
| Copy (selection or layer) | `Ctrl+C` |
| Paste at the cursor | `Ctrl+V` |
| Paste in place | `Ctrl+Alt+V` |
| Paste to new layer | `Ctrl+Shift+V` |
| Delete selected voxels | `Delete` or `Backspace` |

## Selection

| Command | Shortcut |
|---|---|
| Select all | `Ctrl+A` |
| Deselect | `Ctrl+D` |
| Invert selection | `Ctrl+I` |
| Selection to a new layer (copy) | `Ctrl+J` |
| Selection to a new layer (cut) | `Ctrl+Shift+J` |
| Mask Mode: protect the selection | `Shift+M` |

## Layer

| Command | Shortcut |
|---|---|
| Group the selected layers | `Ctrl+G` |
| Ungroup | `Ctrl+Shift+G` |

## Transform

| Command | Shortcut |
|---|---|
| Rotate 90° counterclockwise (facing axis) | `Ctrl+R` |
| Rotate 90° clockwise (facing axis) | `Ctrl+Shift+R` |
| Flip X | `Ctrl+Alt+X` |
| Flip Y | `Ctrl+Alt+Y` |
| Flip Z | `Ctrl+Alt+Z` |

## View

| Command | Shortcut |
|---|---|
| Slice Mode on / off | `Space` |
| Show or hide the work plane | `/` |
| Zen mode | `Ctrl+Shift+F` |
| Model / Render mode | `F5` |
| Perspective / orthographic | `5` |
| Frame selection (or the model) | `.` |
| Frame work area (model bounds) | `Home` |
| Home view (reset camera) | `Shift+Home` |
| Show or hide the floor grid | `G` |
| Show or hide the voxel grid on the model | `Shift+G` |
| Show or hide the model bounds | `Shift+B` |
| Ghosts: other layers semi-transparent | `Shift+H` |
| Show or hide all reference images | `Alt+R` |

## Tools

| Command | Shortcut |
|---|---|
| Cursor tool | `V` |
| Brush tool | `B` |
| Line tool | `L` |
| Shape tool | `S` |
| Plane tool | `P` |
| Move tool | `M` |
| Transform tool | `T` |
| Color Picker tool | `C` |
| Selection tool | `R` |
| Fuzzy Select tool | `W` |
| Extrude tool | `E` |
| Fill tool | `F` |

## Favorite colors

| Command | Shortcut |
|---|---|
| Select favorite color 0 | `Ctrl+0` |
| Select favorite color 1 | `Ctrl+1` |
| Select favorite color 2 | `Ctrl+2` |
| Select favorite color 3 | `Ctrl+3` |
| Select favorite color 4 | `Ctrl+4` |
| Select favorite color 5 | `Ctrl+5` |
| Select favorite color 6 | `Ctrl+6` |
| Select favorite color 7 | `Ctrl+7` |
| Select favorite color 8 | `Ctrl+8` |
| Select favorite color 9 | `Ctrl+9` |
| Store the selected color as favorite 0 | `Ctrl+Shift+0` |
| Store the selected color as favorite 1 | `Ctrl+Shift+1` |
| Store the selected color as favorite 2 | `Ctrl+Shift+2` |
| Store the selected color as favorite 3 | `Ctrl+Shift+3` |
| Store the selected color as favorite 4 | `Ctrl+Shift+4` |
| Store the selected color as favorite 5 | `Ctrl+Shift+5` |
| Store the selected color as favorite 6 | `Ctrl+Shift+6` |
| Store the selected color as favorite 7 | `Ctrl+Shift+7` |
| Store the selected color as favorite 8 | `Ctrl+Shift+8` |
| Store the selected color as favorite 9 | `Ctrl+Shift+9` |

## Help

| Command | Shortcut |
|---|---|
| User guide (opens in the browser) | `F1` |

<!-- END GENERATED -->

## Mouse and fixed keys

| Input | Action |
|---|---|
| `Enter` / click | Place the paste |
| `Esc` | Cancel the paste, drag, box, line or transform; deselect |
| `Enter` | Apply the open box, polyline, curve or transform |
| `Ctrl` + click (Brush) | Line from the last click |
| Right-click / double-click | Finish a polyline (Line tool) |
| `Shift` + click a swatch | Mark a color for Gradient and Remap |
| `Shift` / `Ctrl` (selection tools) | Add to / subtract from the selection |
| `Shift` (drawing tools) | Subtract |
| `Alt` + click | Pick a color with any tool |
| Right drag (viewport) | Orbit the camera |
| Middle drag (viewport) | Pan the camera |
| Middle double-click | Reset the camera |
| Scroll (viewport) | Zoom |

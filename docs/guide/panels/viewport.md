# Viewport

The viewport draws the model with ambient occlusion and 4× multisampling, in
perspective, orthographic or isometric views.

## Camera

| Input | Action |
|---|---|
| Right drag | Orbit |
| Middle drag | Pan |
| Scroll | Zoom |
| Middle double-click | Reset the camera |
| `5` (numpad) | Perspective / orthographic |
| `.` | Frame the selection (or the model) |
| `Home` / `Shift+Home` | Frame the work area / home view |

*Settings ▸ Viewport ▸ Camera controls* also offers `Alt` + mouse: `Alt` + left drag
orbits, `Alt` + middle pans, `Alt` + right zooms.

## View cube

The cube in the corner has 26 targets: click a face, an edge or a corner to look from
there. With *View ▸ Auto Ortho on Face Snap*, face views switch to orthographic. The
same views are in *View ▸ Faces*, *Edges* and *Corners*.

## What it shows

*View ▸ Show* toggles the floor grid (`G`), the voxel grid on the model (`Shift+G`) and
the model bounds (`Shift+B`). With the voxel grid on, the active work plane and the
slice sheet also show the voxel cells, in the plane's color. Line width and opacity are
in [Settings ▸ Viewport](../settings.md#viewport). *Ghosts* (`Shift+H`) draws the other layers
semi-transparent. Reference images draw on their planes; `Alt+R` hides or shows them
all.

Large models are meshed in the background, so editing stays responsive.

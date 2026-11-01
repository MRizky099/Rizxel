# Plane

**Key:** `P` · [All tools](index.md)

Click a face to place the **work plane**. The other tools then draw on it, which helps
to build in the air, and *Fill empty* stays on the clicked side of it (the plane is its
ground). While the plane is shown, drag its edge with any tool to move it; placing it on
a face takes the Plane tool.

| Option | Effect |
|---|---|
| **Visible** | Show or hide the plane (`/`). |
| **Axis** | X, Y or Z. |
| **Position** | The plane's coordinate along its axis. |
| **Slice** | Slice Mode on or off (`Space`). |
| **Reset** | Hide the plane and turn Slice off. |
| **Cut**, **Cap**, **Fit** | In Slice Mode: cut away the hidden side (with *Cap*, close the cut in the current color), and fit the plane to the model. |

## Slice Mode

Slice Mode hides everything on one side of the plane, so you can edit inside the model.
Turned on while the plane is shown, the slice starts where the plane is; turned off, the
plane stays on the slice. Drag to move the slice along its normal, or click a voxel to slice there. *Model ▸
Slice* holds the same commands. With *View ▸ Show ▸ Voxel Grid* on (`Shift+G`), the
slice sheet and the work plane show the voxel cells, so you can count voxels on the
plane.

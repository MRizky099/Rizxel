# Transform

**Key:** `T` · [All tools](index.md)

Free rotate, scale and move the selection (or the active layer). The gizmo's rings
rotate, its arrows and corners scale, and the box moves. `Shift` and `Ctrl` give finer
steps. `Enter` applies, `Esc` cancels.

| Option | Effect |
|---|---|
| **Rotate X / Y / Z** | Angles in degrees. |
| **Scale** | In *%* or as a *Size* in voxels, per axis. |
| **Move X / Y / Z** | Offset in voxels. |
| **Pivot** | *Center*, *Min corner* or *Cursor*, with an offset. |
| **Clip to bounds** | Drop voxels that end up outside the bounds instead of refusing *Apply*. |
| **Apply**, **Cancel**, **Reset** | Commit, discard or reset the transform. |

Free rotation and scaling resample the voxels; quarter turns and flips (*Model ▸ Rotate
90°*, *Flip*) are exact.

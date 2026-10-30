# Extrude

**Key:** `E` · [All tools](index.md)

Click or drag a face (or the selection) to extrude the connected face region.

**Add** (`+`) in the option bar builds outward; **Subtract** (`-`) carves inward. Holding
`Shift` switches to the other one until you let go: with Add selected, `Shift` carves; with
Subtract selected, `Shift` builds. The option bar shows which one a click will use, and
the face highlight turns red while it would carve. Dragging back past zero also reverses
the direction.

| Option | Effect |
|---|---|
| **Depth** | Depth of a click; dragging adds to it. |
| **Confine to layer** | Find the face region in the active layer only. |
| **Use color** / **Threshold** | Limit the region to faces of a similar color. |

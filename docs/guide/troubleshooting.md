# Troubleshooting

| Problem | What to do |
|---|---|
| Windows or macOS refuses to start Rizxel | The beta is unsigned: see [Getting started](getting-started.md#the-beta-builds-are-not-signed). |
| Render mode shows only the raster preview | The path tracer needs Vulkan or Metal. Update the GPU driver; on Windows most GPUs ship Vulkan drivers. |
| "unsupported .rzxl version" | The file was saved by a newer Rizxel. Update Rizxel: older versions refuse newer files instead of loading them partly, so no data is lost. |
| "invalid project" | The file is damaged. Check for an autosave at the next start. |
| An edit is refused | The status bar says why: a locked layer or group, a group as the active layer, or Mask Mode protecting the selection. |
| The panels are in a mess | *View ▸ Panels ▸ Reset Layout*. |
| A shortcut does nothing | Look for a conflict in *Settings ▸ Keymap*; shortcuts are off while a text field or dialog has the focus. |

Still stuck? *Help ▸ Report an Issue* opens the
[issue tracker](https://github.com/MRizky099/Rizxel/issues). Attach the file if you can.

# Settings

*File ▸ Settings...* (`Ctrl+,`) or the gear at the bottom of the tools rail. Changes
apply when you press **OK** and at every start.

## General

UI scale, theme (Dark; a light theme is planned), start screen, autosave interval, undo
steps per document, default export folder, *Associate .rzxl Files* (so
double-clicking a project opens Rizxel), and *Clicking a layer selects its voxels* (on by
default; see [Layers](panels/layers.md#select-on-click)).

## Viewport

Default projection (perspective or orthographic), camera controls (mouse buttons or
`Alt` + mouse), the Model mode background, the floor grid and the voxel grid. Both grids
have a line width (0.5–4 px, constant on screen at every zoom); the voxel grid also has
an opacity (default 0.35). *View ▸ Show* has a quick width slider for the voxel grid
that applies right away and is remembered.

## Keymap

Every keyboard command with its shortcuts. Click a shortcut to change it (press the new
keys), add a second one, remove it or reset it; **Reset All** restores the defaults.
Conflicts (two commands on one shortcut) are marked and must be resolved before OK. The
clipboard shortcuts can't be changed. The defaults are listed in
[Keyboard shortcuts](shortcuts.md).

## Where settings live

Settings, the panel layout, recent files and autosaves are kept in the per-user app data
folder: `%APPDATA%\Rizxel` on Windows, `~/.local/share/rizxel` on Linux and
`~/Library/Application Support/Rizxel` on macOS. The `RIZXEL_DATA_DIR` environment
variable points Rizxel to another folder, for example to try a build without touching
your own settings.

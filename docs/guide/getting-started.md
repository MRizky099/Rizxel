# Getting started

## Install

Download the package for your system from the
[Releases page](https://github.com/MRizky099/Rizxel/releases):

| System | Package | How |
|---|---|---|
| Windows 10/11 | `Rizxel-<version>-windows-x64.msi` | Run it. It installs for your user only (no admin rights), to `%LocalAppData%\Programs\Rizxel` unless you pick another folder you can write to, and opens `.rzxl` files with Rizxel. No other file type is claimed. |
| Windows 10/11 | `Rizxel-<version>-windows-x64.zip` | Unzip anywhere and run `rizxel.exe`. *Settings ▸ General ▸ Associate .rzxl Files* adds the file association. |
| Linux x86-64 | `Rizxel-<version>-x86_64.AppImage` | `chmod +x` the file and run it. |
| macOS 11+ | `Rizxel-<version>-macos-universal.dmg` | Open it and drag Rizxel to *Applications* (Apple silicon and Intel). |

### The beta builds are not signed

Code signing is planned for 1.0, so the operating system warns on the first start:

- **Windows SmartScreen** ("Windows protected your PC"): click *More info*, then *Run
  anyway*.
- **macOS Gatekeeper** ("Apple could not verify…"): right-click (or Ctrl-click) Rizxel
  in *Applications*, choose *Open*, then *Open* again. On recent macOS versions, use
  *System Settings ▸ Privacy & Security ▸ Open Anyway*.
- **Linux**: no warning; the AppImage only needs the executable bit.

Every release links the source code it was built from, so you can also build it yourself
(see the [README](https://github.com/MRizky099/Rizxel#build-from-source)).

### Requirements

Any GPU with Vulkan, Direct3D 12 or Metal draws the viewport. The path tracer in Render
mode needs **Vulkan or Metal**; on other backends Render mode keeps the raster preview
and the *Image* output.

## First steps

1. Start Rizxel. The start screen offers **New Project**, **Open...** and your recent
   files. (*Settings ▸ General ▸ Start screen* turns it off.)
2. Choose **New Project**. An empty 32³ model opens in Model mode.
3. Press `B` for the [Brush](tools/brush.md) and drag on the floor grid to place voxels.
   Pick colors in the [Color Palette](panels/palette.md).
4. Right-drag to orbit, middle-drag to pan and scroll to zoom.
5. Save with `Ctrl+S`. Rizxel's own format is [`.rzxl`](formats/rzxl.md); it keeps
   everything: layers, groups, materials, reference images and render settings.

### Sample models

The [`assets/samples/`](https://github.com/MRizky099/Rizxel/tree/main/assets/samples)
folder holds models made with Rizxel (CC0). Open one with *File ▸ Open...* or drop it on
the window.

### Opening files

- *File ▸ Open...* (`Ctrl+O`) opens `.rzxl` files and every importable format in a new
  tab.
- Drop files on the window: projects and models open in tabs; PNG and JPEG images become
  [reference images](panels/references.md).
- Double-clicking a `.rzxl` file while Rizxel runs opens it as a new tab in the running
  window.

## Autosave and recovery

Rizxel autosaves documents with unsaved changes (*Settings ▸ General ▸ Autosave every*).
After a crash, the next start offers each autosave: **Restore**, **Discard**, or
**Decide Later** to keep it for the next start. Closing a tab or the window with unsaved
changes always asks *Save*, *Don't Save* or *Cancel*. Saves go to a temporary file that
replaces the old one only when it is complete, so a failed save never damages a project.

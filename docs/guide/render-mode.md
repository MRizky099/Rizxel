# Render mode

![Render mode with the path tracer and the Brick voxel shape](https://raw.githubusercontent.com/MRizky099/Rizxel/main/docs/images/readme/render_mode.png)

Render mode (`F5`) turns the viewport into a progressive renderer. The voxels never
change: lighting, materials and the voxel shape are view settings, saved with the
project.

## Preview

The render bar picks a **Raster** or **Path traced** preview and its resolution, and
pauses or resumes it. The path tracer refines the image while the camera stands still.
It needs **Vulkan or Metal**; elsewhere only the raster preview and the *Image* output
are available.

## Workflow

1. Pick a preset, or set the light in the [Render panel](panels/render.md)'s *Light*
   tab.
2. Give layers materials in the *Material* tab (`Alt` + click the model to pick a
   layer): glass with transmission, metal, emission for lamps.
3. Choose a voxel shape in *Display* (bricks, rounded cubes, spheres…).
4. Set the camera: field of view, depth of field (aperture; click to focus), exposure.
5. Choose an output in *Output* and press **Render**.

## Outputs

| Output | Result |
|---|---|
| **Image** | A fast raster PNG at the output size. |
| **Photo-realistic** | A path-traced PNG up to 8192 px, optionally 16-bit or transparent, with a sample count or a time limit. |
| **Turntable** | A camera orbit: a PNG sequence, a sprite sheet and/or a GIF; raster or path traced. |

![A 1920×1080 photo-realistic render](https://raw.githubusercontent.com/MRizky099/Rizxel/main/docs/images/readme/photo_render.png)

Files go to the chosen folder (or next to the project) with the name pattern, where
`{name}` is the project name.

**Export shape…** writes the shaped model (bricks with studs, rounded cubes…) as OBJ,
STL or GLB.

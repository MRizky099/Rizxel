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

## The Voxel Icon look

![The rizxel_r.rzxl sample with the Voxel Icon preset, path traced at 1024×1024](https://raw.githubusercontent.com/MRizky099/Rizxel/main/docs/images/readme/voxel_icon.png)

The **Voxel Icon** preset draws the model the way Rizxel's icon does: matte voxels
with a thin dark line around every voxel face, on a dark floor with a fine glowing grid
that fades out away from the model, with a vignette. Each part is a separate setting in
the *Display* tab:

- **Voxel lines** outline every face of every voxel, outer edges included (unlike
  *Seams*, which only darken where two voxels touch). The width is in voxels, so the
  preview and a 4K output keep the same proportions. Far away, lines thinner than a
  pixel fade to their average tint instead of breaking up.
- **Floor grid** draws lines on the ground along the voxel borders, fading out at the
  *Fade* distance. Its *Glow* makes the lines give off light in the path tracer, so they
  read on a dark floor and bloom when bloom is on. *View ▸ Floor Grid* (`G`) toggles it
  in Render mode.
- **Matte** shows every material rough, without metal or clear coat, keeping colors,
  emission and glass. It only changes the view; the *Matte* button in the *Material*
  tab writes those values into a material instead.

Lines are anti-aliased in both renderers, and the *Image* output renders at twice the
size and scales down when voxel lines or the floor grid are on. Open
`assets/samples/rizxel_r.rzxl` to try it.

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

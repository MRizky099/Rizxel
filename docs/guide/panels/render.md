# Render

The Render panel holds Render mode's settings, saved with the project. Its tabs:

| Tab | Settings |
|---|---|
| **Light** | Sun (azimuth, elevation, size, intensity, color), sky (*Uniform*, *Gradient* or an *Image* `.hdr` with rotation), fog, studio lights. |
| **Material** | The selected layer's material: tint, metallic, roughness, specular, transmission (glass) and IOR, clear coat, emission and its strength. *Make unique* gives the layer its own copy. `Alt` + click the model picks a layer. |
| **Display** | Ground, shadow catcher, background (*Color*, *Gradient*, *Sky* or *Transparent*), bevel, seams, outline edges, and the **voxel shape**: cube, brick (with studs), rounded, sphere or cylinder. |
| **Camera** | Field of view, aperture, focus (click the model to focus), blades, exposure, tone map (*Linear*, *ACES*, *Filmic*), gamma, vignette, bloom. |
| **Sample** | Samples, diffuse / specular / transmission bounces, Russian roulette, clamp, pixel filter, pixelated, denoise. |
| **Output** | *Image*, *Photo-realistic* or *Turntable*; size, transparency, folder and name, time limit, 16-bit PNG; turntable frames and angle, PNG sequence, GIF, sprite sheet. |

**Presets** store the settings (without the output) under a name. **Export shape…**
exports the model with the chosen voxel shape as OBJ, STL or GLB.

See [Render mode](../render-mode.md) for the workflow.

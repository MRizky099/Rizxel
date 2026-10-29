//! Render-mode settings (Phase 11b): light, sampling, display, camera and
//! output, saved with the project so a render can be reproduced.
//!
//! Colors are sRGB in `0..=1` (what the color pickers show); renderers
//! convert them to linear. Angles are degrees. Every struct defaults its
//! missing fields, so settings written by an older build still load, and
//! [`RenderSettings::sanitize`] clamps hostile or broken values.

use serde::{Deserialize, Serialize};

/// Everything Render mode needs to reproduce a render.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RenderSettings {
    pub light: LightSettings,
    pub sample: SampleSettings,
    pub display: DisplaySettings,
    pub camera: CameraSettings,
    pub output: OutputSettings,
}

/// Sun, sky, fog and the studio lights.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LightSettings {
    pub sun: bool,
    /// Compass direction the sunlight comes from (0 = +Z, 90 = +X).
    pub sun_azimuth: f32,
    /// Height of the sun above the horizon.
    pub sun_elevation: f32,
    /// Angular diameter of the sun disk: larger = softer shadows.
    pub sun_size: f32,
    pub sun_intensity: f32,
    pub sun_color: [f32; 3],
    pub sky: SkyMode,
    /// Uniform sky color, or the horizon color of the gradient.
    pub sky_color: [f32; 3],
    /// Zenith color of the gradient sky.
    pub sky_zenith: [f32; 3],
    pub sky_intensity: f32,
    /// Turns the HDR image around the vertical axis.
    pub sky_rotation: f32,
    /// Equirectangular Radiance `.hdr` file for [`SkyMode::Image`].
    pub sky_image: String,
    pub fog: bool,
    /// Fog extinction per voxel.
    pub fog_density: f32,
    pub fog_color: [f32; 3],
    /// Fill and rim lights next to the sun (the key light), as in a photo
    /// studio.
    pub studio_lights: bool,
}

impl Default for LightSettings {
    fn default() -> Self {
        Self {
            sun: true,
            sun_azimuth: 45.0,
            sun_elevation: 50.0,
            sun_size: 5.0,
            sun_intensity: 3.0,
            sun_color: [1.0, 0.97, 0.92],
            sky: SkyMode::Gradient,
            sky_color: [0.78, 0.84, 0.92],
            sky_zenith: [0.45, 0.6, 0.85],
            sky_intensity: 1.0,
            sky_rotation: 0.0,
            sky_image: String::new(),
            fog: false,
            fog_density: 0.01,
            fog_color: [0.85, 0.88, 0.92],
            studio_lights: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkyMode {
    Uniform,
    #[default]
    Gradient,
    Image,
}

impl SkyMode {
    pub const ALL: [SkyMode; 3] = [SkyMode::Uniform, SkyMode::Gradient, SkyMode::Image];

    pub const fn label(self) -> &'static str {
        match self {
            SkyMode::Uniform => "Uniform",
            SkyMode::Gradient => "Gradient",
            SkyMode::Image => "HDR Image",
        }
    }
}

/// Path tracer sampling.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SampleSettings {
    /// Samples per pixel of a finished render.
    pub samples: u32,
    pub diffuse_bounces: u32,
    pub specular_bounces: u32,
    pub transmission_bounces: u32,
    /// Ends dim paths early (unbiased, faster).
    pub russian_roulette: bool,
    /// Largest sample brightness (0 = off); removes fireflies.
    pub clamp: f32,
    pub filter: PixelFilter,
    /// One ray through each pixel center: crisp voxel pixels, no
    /// anti-aliasing.
    pub pixelated: bool,
    pub denoise: Denoise,
}

impl Default for SampleSettings {
    fn default() -> Self {
        Self {
            samples: 256,
            diffuse_bounces: 3,
            specular_bounces: 6,
            transmission_bounces: 8,
            russian_roulette: true,
            clamp: 10.0,
            filter: PixelFilter::Gaussian,
            pixelated: false,
            denoise: Denoise::Auto,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PixelFilter {
    Box,
    #[default]
    Gaussian,
}

impl PixelFilter {
    pub const ALL: [PixelFilter; 2] = [PixelFilter::Box, PixelFilter::Gaussian];

    pub const fn label(self) -> &'static str {
        match self {
            PixelFilter::Box => "Box",
            PixelFilter::Gaussian => "Gaussian",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Denoise {
    Off,
    /// Edge-aware filter guided by albedo, normal and depth.
    #[default]
    Auto,
}

impl Denoise {
    pub const ALL: [Denoise; 2] = [Denoise::Off, Denoise::Auto];

    pub const fn label(self) -> &'static str {
        match self {
            Denoise::Off => "Off",
            Denoise::Auto => "Auto",
        }
    }
}

/// Ground, background, edges and the voxel shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DisplaySettings {
    pub ground: bool,
    pub ground_color: [f32; 3],
    /// The ground shows only the shadows and light bounced onto it, over
    /// the background.
    pub shadow_catcher: bool,
    pub background: BackgroundMode,
    /// Background color, or the bottom color of the gradient.
    pub background_color: [f32; 3],
    /// Top color of the gradient background.
    pub background_top: [f32; 3],
    /// Radius of the rounded voxel edges, in voxels (0 = sharp).
    pub bevel: f32,
    /// Thin dark grooves where two voxels touch.
    pub seams: bool,
    /// Outline along the model's silhouette and creases.
    pub edges: bool,
    pub edge_color: [f32; 3],
    /// Outline width in pixels.
    pub edge_width: f32,
    /// How each voxel is drawn in Render mode (view only).
    pub shape: VoxelShape,
    /// Size of the shape inside its cell (`0.5..=1`): below 1 gaps show.
    pub cell: f32,
}

impl Default for DisplaySettings {
    fn default() -> Self {
        Self {
            ground: true,
            ground_color: [0.72, 0.72, 0.72],
            shadow_catcher: false,
            background: BackgroundMode::Gradient,
            background_color: [0.36, 0.38, 0.42],
            background_top: [0.62, 0.66, 0.72],
            bevel: 0.06,
            seams: false,
            edges: false,
            edge_color: [0.1, 0.1, 0.12],
            edge_width: 1.0,
            shape: VoxelShape::Cube,
            cell: 1.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundMode {
    Color,
    #[default]
    Gradient,
    /// The sky itself.
    Sky,
    /// Alpha 0 in the output image.
    Transparent,
}

impl BackgroundMode {
    pub const ALL: [BackgroundMode; 4] = [
        BackgroundMode::Color,
        BackgroundMode::Gradient,
        BackgroundMode::Sky,
        BackgroundMode::Transparent,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            BackgroundMode::Color => "Color",
            BackgroundMode::Gradient => "Gradient",
            BackgroundMode::Sky => "Sky",
            BackgroundMode::Transparent => "Transparent",
        }
    }
}

/// Render-only voxel shapes (MagicaVoxel's *Display ▸ Shape*). The model's
/// voxels never change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoxelShape {
    #[default]
    Cube,
    /// Rounded cube with a stud on each top face that has no voxel above.
    Brick,
    /// Cube with a large bevel (MagicaVoxel's *Clay*).
    Rounded,
    Sphere,
    /// Cylinder along the vertical axis.
    Cylinder,
}

impl VoxelShape {
    pub const ALL: [VoxelShape; 5] = [
        VoxelShape::Cube,
        VoxelShape::Brick,
        VoxelShape::Rounded,
        VoxelShape::Sphere,
        VoxelShape::Cylinder,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            VoxelShape::Cube => "Cube",
            VoxelShape::Brick => "Brick",
            VoxelShape::Rounded => "Rounded",
            VoxelShape::Sphere => "Sphere",
            VoxelShape::Cylinder => "Cylinder",
        }
    }

    /// Stable index used by the shaders.
    pub const fn index(self) -> u32 {
        self as u32
    }
}

/// Stud diameter as a fraction of the voxel edge (LEGO: 4.8 mm on 8 mm).
pub const STUD_DIAMETER: f32 = 0.6;
/// Stud height as a fraction of the voxel edge.
pub const STUD_HEIGHT: f32 = 0.2;
/// Bevel radius of the Rounded shape, in voxels.
pub const ROUNDED_BEVEL: f32 = 0.3;
/// Smallest bevel of the Brick shape: it always shows its seams.
pub const BRICK_MIN_BEVEL: f32 = 0.04;

/// Lens and film.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CameraSettings {
    /// Vertical field of view of the perspective camera.
    pub fov: f32,
    /// Lens radius in voxels (0 = everything sharp).
    pub aperture: f32,
    /// Distance from the eye that is in focus (world units).
    pub focus_distance: f32,
    /// Aperture blades (0 = round bokeh, 3..=12 polygonal).
    pub blades: u32,
    /// Exposure in stops.
    pub exposure: f32,
    pub tone_map: ToneMap,
    pub gamma: f32,
    /// Darkening toward the image corners (0..=1).
    pub vignette: f32,
    pub bloom: bool,
    /// Brightness above which pixels bloom.
    pub bloom_threshold: f32,
    pub bloom_strength: f32,
}

impl Default for CameraSettings {
    fn default() -> Self {
        Self {
            fov: 45.0,
            aperture: 0.0,
            focus_distance: 64.0,
            blades: 0,
            exposure: 0.0,
            tone_map: ToneMap::Aces,
            gamma: 2.2,
            vignette: 0.0,
            bloom: false,
            bloom_threshold: 1.0,
            bloom_strength: 0.15,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToneMap {
    /// Clamp only.
    Linear,
    #[default]
    Aces,
    Filmic,
}

impl ToneMap {
    pub const ALL: [ToneMap; 3] = [ToneMap::Linear, ToneMap::Aces, ToneMap::Filmic];

    pub const fn label(self) -> &'static str {
        match self {
            ToneMap::Linear => "Linear",
            ToneMap::Aces => "ACES",
            ToneMap::Filmic => "Filmic",
        }
    }

    pub const fn index(self) -> u32 {
        self as u32
    }
}

/// What the Render button writes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OutputSettings {
    pub kind: OutputKind,
    pub width: u32,
    pub height: u32,
    /// Transparent background in the PNG (overrides the background mode).
    pub transparent: bool,
    /// Output folder (empty = next to the project, or the working folder).
    pub folder: String,
    /// File name without extension; `{name}` is the project name, `{frame}`
    /// the frame number of a turntable.
    pub name: String,
    /// Photo-realistic: stop after this many seconds (0 = no limit).
    pub time_limit: f32,
    /// Photo-realistic: 16 bits per channel PNG.
    pub png16: bool,
    pub turntable_frames: u32,
    /// Total turn of the turntable.
    pub turntable_angle: f32,
    /// Turntable frames are path traced (else raster).
    pub turntable_path_traced: bool,
    /// Samples per pixel of each path-traced turntable frame.
    pub turntable_samples: u32,
    pub turntable_png: bool,
    pub turntable_gif: bool,
    pub turntable_sheet: bool,
    /// GIF playback speed.
    pub gif_fps: u32,
}

impl Default for OutputSettings {
    fn default() -> Self {
        Self {
            kind: OutputKind::Image,
            width: 1920,
            height: 1080,
            transparent: false,
            folder: String::new(),
            name: "{name}_render".to_owned(),
            time_limit: 0.0,
            png16: false,
            turntable_frames: 36,
            turntable_angle: 360.0,
            turntable_path_traced: false,
            turntable_samples: 32,
            turntable_png: true,
            turntable_gif: true,
            turntable_sheet: false,
            gif_fps: 12,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputKind {
    /// Fast raster render.
    #[default]
    Image,
    /// Path traced.
    Photo,
    /// Frames around the model.
    Turntable,
}

impl OutputKind {
    pub const ALL: [OutputKind; 3] = [OutputKind::Image, OutputKind::Photo, OutputKind::Turntable];

    pub const fn label(self) -> &'static str {
        match self {
            OutputKind::Image => "Image",
            OutputKind::Photo => "Photo-realistic",
            OutputKind::Turntable => "Turntable",
        }
    }
}

/// Largest output side in pixels.
pub const MAX_OUTPUT_SIDE: u32 = 8192;
/// Most samples per pixel.
pub const MAX_SAMPLES: u32 = 65_536;
/// Most bounces of one kind.
pub const MAX_BOUNCES: u32 = 32;
/// Most turntable frames.
pub const MAX_TURNTABLE_FRAMES: u32 = 720;
/// Longest folder, name or sky image path, in characters.
pub const MAX_PATH_CHARS: usize = 1024;

/// Built-in setting presets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderPreset {
    /// Seamless white backdrop, shadow-catcher ground, soft key/fill/rim
    /// lights and glossy plastic: BrickLink Studio's look.
    StudioWhite,
    OutdoorSun,
    NightEmissive,
    /// Pixelated, no bounced light: crisp sprites.
    FlatPixel,
}

impl RenderPreset {
    pub const ALL: [RenderPreset; 4] = [
        RenderPreset::StudioWhite,
        RenderPreset::OutdoorSun,
        RenderPreset::NightEmissive,
        RenderPreset::FlatPixel,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            RenderPreset::StudioWhite => "Studio White",
            RenderPreset::OutdoorSun => "Outdoor Sun",
            RenderPreset::NightEmissive => "Night Emissive",
            RenderPreset::FlatPixel => "Flat Pixel",
        }
    }

    /// `base` with this preset's light, sampling, display and camera. The
    /// output settings, the voxel shape and the cell size are kept.
    pub fn apply(self, base: &RenderSettings) -> RenderSettings {
        let mut s = RenderSettings {
            output: base.output.clone(),
            ..RenderSettings::default()
        };
        s.display.shape = base.display.shape;
        s.display.cell = base.display.cell;
        match self {
            RenderPreset::StudioWhite => {
                let l = &mut s.light;
                l.sun_azimuth = 35.0;
                l.sun_elevation = 60.0;
                l.sun_size = 18.0;
                l.sun_intensity = 2.2;
                l.sun_color = [1.0, 1.0, 1.0];
                l.sky = SkyMode::Uniform;
                l.sky_color = [1.0, 1.0, 1.0];
                l.sky_intensity = 0.75;
                l.studio_lights = true;
                let d = &mut s.display;
                d.ground_color = [0.92, 0.92, 0.92];
                d.shadow_catcher = true;
                d.background = BackgroundMode::Gradient;
                d.background_color = [0.86, 0.86, 0.87];
                d.background_top = [0.97, 0.97, 0.98];
                d.bevel = 0.06;
                d.seams = true;
                s.camera.fov = 30.0;
                s.camera.exposure = 0.3;
                s.camera.tone_map = ToneMap::Filmic;
            }
            RenderPreset::OutdoorSun => {
                let l = &mut s.light;
                l.sun_elevation = 40.0;
                l.sun_size = 1.0;
                l.sun_intensity = 4.0;
                l.sun_color = [1.0, 0.93, 0.82];
                l.sky = SkyMode::Gradient;
                l.sky_intensity = 1.2;
                s.display.ground_color = [0.55, 0.6, 0.45];
                s.display.background = BackgroundMode::Sky;
            }
            RenderPreset::NightEmissive => {
                let l = &mut s.light;
                l.sun = false;
                l.sky = SkyMode::Uniform;
                l.sky_color = [0.08, 0.1, 0.18];
                l.sky_intensity = 0.3;
                s.display.ground_color = [0.2, 0.2, 0.22];
                s.display.background = BackgroundMode::Color;
                s.display.background_color = [0.02, 0.025, 0.05];
                s.camera.bloom = true;
                s.camera.bloom_strength = 0.3;
            }
            RenderPreset::FlatPixel => {
                s.sample.diffuse_bounces = 0;
                s.sample.specular_bounces = 1;
                s.sample.samples = 16;
                s.sample.pixelated = true;
                s.sample.denoise = Denoise::Off;
                s.light.sun_size = 0.5;
                s.display.bevel = 0.0;
                s.display.background = BackgroundMode::Color;
                s.camera.tone_map = ToneMap::Linear;
            }
        }
        s
    }
}

/// Finite and inside `range`, else `fallback` (clamped).
fn clamp_or(v: f32, range: std::ops::RangeInclusive<f32>, fallback: f32) -> f32 {
    if v.is_finite() {
        v.clamp(*range.start(), *range.end())
    } else {
        fallback
    }
}

fn clamp_color(c: &mut [f32; 3], fallback: [f32; 3]) {
    for (v, f) in c.iter_mut().zip(fallback) {
        *v = clamp_or(*v, 0.0..=1.0, f);
    }
}

fn clamp_text(s: &mut String) {
    if s.chars().count() > MAX_PATH_CHARS {
        *s = s.chars().take(MAX_PATH_CHARS).collect();
    }
}

impl RenderSettings {
    /// Clamps every value into its range (non-finite values fall back to
    /// the defaults), so files and drags can never feed the renderer
    /// hostile sizes, sample counts or angles.
    pub fn sanitize(&mut self) {
        let d = RenderSettings::default();
        let l = &mut self.light;
        l.sun_azimuth = clamp_or(l.sun_azimuth, -360.0..=360.0, d.light.sun_azimuth);
        l.sun_elevation = clamp_or(l.sun_elevation, -90.0..=90.0, d.light.sun_elevation);
        l.sun_size = clamp_or(l.sun_size, 0.0..=90.0, d.light.sun_size);
        l.sun_intensity = clamp_or(l.sun_intensity, 0.0..=100.0, d.light.sun_intensity);
        clamp_color(&mut l.sun_color, d.light.sun_color);
        clamp_color(&mut l.sky_color, d.light.sky_color);
        clamp_color(&mut l.sky_zenith, d.light.sky_zenith);
        l.sky_intensity = clamp_or(l.sky_intensity, 0.0..=100.0, d.light.sky_intensity);
        l.sky_rotation = clamp_or(l.sky_rotation, -360.0..=360.0, 0.0);
        clamp_text(&mut l.sky_image);
        l.fog_density = clamp_or(l.fog_density, 0.0..=1.0, d.light.fog_density);
        clamp_color(&mut l.fog_color, d.light.fog_color);

        let s = &mut self.sample;
        s.samples = s.samples.clamp(1, MAX_SAMPLES);
        s.diffuse_bounces = s.diffuse_bounces.min(MAX_BOUNCES);
        s.specular_bounces = s.specular_bounces.min(MAX_BOUNCES);
        s.transmission_bounces = s.transmission_bounces.min(MAX_BOUNCES);
        s.clamp = clamp_or(s.clamp, 0.0..=1000.0, d.sample.clamp);

        let p = &mut self.display;
        clamp_color(&mut p.ground_color, d.display.ground_color);
        clamp_color(&mut p.background_color, d.display.background_color);
        clamp_color(&mut p.background_top, d.display.background_top);
        p.bevel = clamp_or(p.bevel, 0.0..=0.5, d.display.bevel);
        clamp_color(&mut p.edge_color, d.display.edge_color);
        p.edge_width = clamp_or(p.edge_width, 0.5..=8.0, d.display.edge_width);
        p.cell = clamp_or(p.cell, 0.5..=1.0, 1.0);

        let c = &mut self.camera;
        c.fov = clamp_or(c.fov, 5.0..=120.0, d.camera.fov);
        c.aperture = clamp_or(c.aperture, 0.0..=10.0, 0.0);
        c.focus_distance = clamp_or(c.focus_distance, 0.1..=10_000.0, d.camera.focus_distance);
        if c.blades != 0 {
            c.blades = c.blades.clamp(3, 12);
        }
        c.exposure = clamp_or(c.exposure, -10.0..=10.0, 0.0);
        c.gamma = clamp_or(c.gamma, 1.0..=3.0, d.camera.gamma);
        c.vignette = clamp_or(c.vignette, 0.0..=1.0, 0.0);
        c.bloom_threshold = clamp_or(c.bloom_threshold, 0.0..=100.0, d.camera.bloom_threshold);
        c.bloom_strength = clamp_or(c.bloom_strength, 0.0..=1.0, d.camera.bloom_strength);

        let o = &mut self.output;
        o.width = o.width.clamp(1, MAX_OUTPUT_SIDE);
        o.height = o.height.clamp(1, MAX_OUTPUT_SIDE);
        clamp_text(&mut o.folder);
        clamp_text(&mut o.name);
        o.time_limit = clamp_or(o.time_limit, 0.0..=86_400.0, 0.0);
        o.turntable_frames = o.turntable_frames.clamp(1, MAX_TURNTABLE_FRAMES);
        o.turntable_angle = clamp_or(o.turntable_angle, -3600.0..=3600.0, 360.0);
        o.turntable_samples = o.turntable_samples.clamp(1, MAX_SAMPLES);
        o.gif_fps = o.gif_fps.clamp(1, 50);
    }

    /// Copy with [`RenderSettings::sanitize`] applied.
    pub fn sanitized(mut self) -> Self {
        self.sanitize();
        self
    }

    /// Bevel radius the shape is drawn with, in voxels.
    pub fn effective_bevel(&self) -> f32 {
        match self.display.shape {
            VoxelShape::Rounded => ROUNDED_BEVEL,
            VoxelShape::Brick => self.display.bevel.max(BRICK_MIN_BEVEL),
            _ => self.display.bevel,
        }
    }

    /// Seams are always on for bricks.
    pub fn effective_seams(&self) -> bool {
        self.display.seams || self.display.shape == VoxelShape::Brick
    }
}

/// Unit vector toward the sun for `azimuth` / `elevation` in degrees
/// (y up; azimuth 0 = +Z, 90 = +X).
pub fn sun_direction(azimuth: f32, elevation: f32) -> glam::Vec3 {
    let (az, el) = (azimuth.to_radians(), elevation.to_radians());
    glam::Vec3::new(el.cos() * az.sin(), el.sin(), el.cos() * az.cos())
}

/// sRGB component to linear.
pub fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_take_defaults() {
        let s: RenderSettings =
            serde_json::from_str(r#"{"light":{"sun_intensity":7.0},"display":{}}"#).unwrap();
        assert_eq!(s.light.sun_intensity, 7.0);
        assert_eq!(s.light.sun_azimuth, 45.0);
        assert_eq!(s.sample, SampleSettings::default());
        let json = serde_json::to_string(&RenderSettings::default()).unwrap();
        let back: RenderSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(back, RenderSettings::default());
    }

    #[test]
    fn hostile_values_are_clamped() {
        let mut s = RenderSettings::default();
        s.sample.samples = 0;
        s.sample.diffuse_bounces = 1_000_000;
        s.output.width = u32::MAX;
        s.output.height = 0;
        s.light.sun_elevation = 1e9;
        s.light.sun_azimuth = f32::NAN;
        s.camera.fov = f32::INFINITY;
        s.camera.blades = 1;
        s.display.cell = 0.0;
        s.display.ground_color = [2.0, -1.0, f32::NAN];
        s.output.name = "x".repeat(MAX_PATH_CHARS * 2);
        s.sanitize();
        assert_eq!(s.sample.samples, 1);
        assert_eq!(s.sample.diffuse_bounces, MAX_BOUNCES);
        assert_eq!((s.output.width, s.output.height), (MAX_OUTPUT_SIDE, 1));
        assert_eq!(s.light.sun_elevation, 90.0);
        assert_eq!(s.light.sun_azimuth, 45.0);
        assert_eq!(s.camera.fov, 45.0);
        assert_eq!(s.camera.blades, 3);
        assert_eq!(s.display.cell, 0.5);
        assert_eq!(s.display.ground_color, [1.0, 0.0, 0.72]);
        assert_eq!(s.output.name.chars().count(), MAX_PATH_CHARS);
        assert_eq!(
            RenderSettings::default().sanitized(),
            RenderSettings::default()
        );
    }

    #[test]
    fn presets_keep_output_and_shape() {
        let mut base = RenderSettings::default();
        base.output.width = 640;
        base.display.shape = VoxelShape::Brick;
        base.display.cell = 0.9;
        for p in RenderPreset::ALL {
            let s = p.apply(&base);
            assert_eq!(s.output, base.output, "{p:?}");
            assert_eq!(s.display.shape, VoxelShape::Brick);
            assert_eq!(s.display.cell, 0.9);
            assert_eq!(s.clone().sanitized(), s, "{p:?} is in range");
        }
        let studio = RenderPreset::StudioWhite.apply(&base);
        assert!(studio.display.shadow_catcher && studio.light.studio_lights);
        assert!(RenderPreset::FlatPixel.apply(&base).sample.pixelated);
    }

    #[test]
    fn effective_bevel_and_seams_follow_the_shape() {
        let mut s = RenderSettings::default();
        s.display.bevel = 0.0;
        assert_eq!(s.effective_bevel(), 0.0);
        assert!(!s.effective_seams());
        s.display.shape = VoxelShape::Brick;
        assert_eq!(s.effective_bevel(), BRICK_MIN_BEVEL);
        assert!(s.effective_seams());
        s.display.shape = VoxelShape::Rounded;
        assert_eq!(s.effective_bevel(), ROUNDED_BEVEL);
    }

    #[test]
    fn sun_direction_matches_the_compass() {
        let up = sun_direction(0.0, 90.0);
        assert!((up - glam::Vec3::Y).length() < 1e-5);
        let east = sun_direction(90.0, 0.0);
        assert!((east - glam::Vec3::X).length() < 1e-5);
        assert!((srgb_to_linear(1.0) - 1.0).abs() < 1e-6);
        assert!((srgb_to_linear(0.5) - 0.214).abs() < 1e-3);
    }
}

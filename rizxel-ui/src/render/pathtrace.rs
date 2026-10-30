//! Progressive GPU path tracer (Phase 11b).
//!
//! A wgpu compute pipeline over the merged visible volume, uploaded as a
//! 3D texture of `(palette index, material slot)` per voxel plus an 8³
//! brick occupancy texture for empty-space skipping (`pathtrace.wgsl`). No
//! BVH and no extra crates. Samples accumulate in storage buffers; the
//! resolve passes (`denoise.wgsl`) demodulate by albedo, run an edge-aware
//! à-trous filter, tone-map and write an `Rgba16Float` image that the
//! viewport blits and the outputs read back.
//!
//! Needs compute shaders with storage buffers and storage textures
//! on Vulkan or Metal ([`supported`]); on GL and DX12 Render mode
//! keeps the raster preview and the *Image* output only.

use std::sync::Arc;
use std::time::{Duration, Instant};

use bytemuck::{Pod, Zeroable};
use eframe::egui_wgpu::wgpu;
use glam::{Mat4, Vec3};
use rizxel_app::{Document, HdrImage};
use rizxel_math::render::{
    srgb_to_linear, sun_direction, BackgroundMode, Denoise, PixelFilter, SkyMode, ToneMap,
};
use rizxel_math::{Material, RenderSettings, VoxelBox, VoxelVolume, EMPTY_VOXEL};

/// The tracing kernel (shared prelude + `pathtrace.wgsl`).
pub const TRACE_SOURCE: &str = concat!(
    include_str!("pt_common.wgsl"),
    include_str!("pathtrace.wgsl")
);
/// Denoise and finish passes (shared prelude + `denoise.wgsl`).
pub const RESOLVE_SOURCE: &str =
    concat!(include_str!("pt_common.wgsl"), include_str!("denoise.wgsl"));

/// Edge of the occupancy bricks.
pub const BRICK: u32 = 8;
/// Format of the finished image (gamma-encoded, `0..=1`).
pub const IMAGE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// À-trous passes (steps 1, 2, 4, 8, 16 px).
const ATROUS_PASSES: usize = 5;
/// Pixels a tile's buffers reach past its edges, so the filter sees the
/// same neighbours as in one big image (2 × (1 + 2 + 4 + 8 + 16) = 62).
pub const APRON: u32 = 64;
/// Largest tile of a photo-realistic render.
pub const PHOTO_TILE: u32 = 2048;

/// Whether `adapter` can run the path tracer.
///
/// Only on Vulkan and Metal:
/// - GL: an Intel HD 4600 (GL 4.3) compiles the kernel but its 2015 driver
///   crashes on the first dispatch, and GL drivers vary too much to trust
///   with a kernel this size.
/// - DX12: wgpu compiles HLSL with FXC, which takes about 140 s (debug) to
///   290 s (release) for this kernel on a GTX 950M; Render mode would hang.
pub fn supported(adapter: &wgpu::Adapter, device: &wgpu::Device) -> bool {
    let caps = adapter.get_downlevel_capabilities();
    let limits = device.limits();
    matches!(
        adapter.get_info().backend,
        wgpu::Backend::Vulkan | wgpu::Backend::Metal
    ) && caps.flags.contains(wgpu::DownlevelFlags::COMPUTE_SHADERS)
        && limits.max_storage_buffers_per_shader_stage >= 6
        && limits.max_storage_textures_per_shader_stage >= 1
        && limits.max_compute_invocations_per_workgroup >= 64
        && limits.max_texture_dimension_3d >= 257
}

// ------------------------------------------------------------------ scene

/// The voxels the path tracer sees, on the CPU: two bytes per cell
/// (palette index, material slot), `x` fastest, then `y`, then `z`.
#[derive(Debug, Clone, PartialEq)]
pub struct PtScene {
    pub dims: [u32; 3],
    pub cells: Vec<u8>,
    /// Occupancy of 8³ bricks, dilated one cell up (studs stick out).
    pub brick_dims: [u32; 3],
    pub bricks: Vec<u8>,
}

impl PtScene {
    /// Merges `layers` (bottom to top, later layers win) inside `clip`.
    pub fn build<'a>(
        dims: [u32; 3],
        layers: impl IntoIterator<Item = (&'a VoxelVolume, u8)>,
        clip: Option<VoxelBox>,
    ) -> Self {
        let dims = dims.map(|d| d.max(1));
        let [dx, dy, dz] = dims.map(|d| d as usize);
        let mut cells = vec![0u8; dx * dy * dz * 2];
        for (volume, slot) in layers {
            for (p, v) in volume.iter() {
                if v == EMPTY_VOXEL
                    || p.min_element() < 0
                    || p.x as usize >= dx
                    || p.y as usize >= dy
                    || p.z as usize >= dz
                    || clip.is_some_and(|c| !c.contains(p))
                {
                    continue;
                }
                let i = ((p.z as usize * dy + p.y as usize) * dx + p.x as usize) * 2;
                cells[i] = v;
                cells[i + 1] = slot;
            }
        }
        let brick_dims = [
            dims[0].div_ceil(BRICK),
            (dims[1] + 1).div_ceil(BRICK),
            dims[2].div_ceil(BRICK),
        ];
        let [bx, by, _] = brick_dims.map(|d| d as usize);
        let mut bricks = vec![0u8; brick_dims.iter().map(|&d| d as usize).product()];
        for z in 0..dz {
            for y in 0..dy {
                for x in 0..dx {
                    if cells[((z * dy + y) * dx + x) * 2] == 0 {
                        continue;
                    }
                    let b = |y: usize| ((z / 8) * by + y / 8) * bx + x / 8;
                    bricks[b(y)] = 1;
                    bricks[b(y + 1)] = 1;
                }
            }
        }
        Self {
            dims,
            cells,
            brick_dims,
            bricks,
        }
    }

    /// The visible geometry of `doc` (slice-clipped like the viewport),
    /// each layer with its path tracer material slot from [`PtTables`].
    pub fn from_document(doc: &Document, tables: &PtTables) -> Self {
        let d = doc.data.bounds.0;
        doc.with_visible_layers(|layers| {
            Self::build(
                [d.x, d.y, d.z],
                layers.iter().map(|(i, v)| (*v, tables.slot_of(*i))),
                doc.view_clip(),
            )
        })
    }

    pub fn is_empty(&self) -> bool {
        self.bricks.iter().all(|&b| b == 0)
    }
}

/// One material slot of the path tracer. Layout must match `Material`
/// in `pathtrace.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct PtMaterial {
    tint: [f32; 4],
    emission: [f32; 4],
    params: [f32; 4],
    params2: [f32; 4],
}

impl PtMaterial {
    fn new(m: &Material, alpha: f32) -> Self {
        let strength = m.emission_strength;
        Self {
            tint: [m.tint[0], m.tint[1], m.tint[2], m.tint[3] * alpha],
            emission: [
                m.emission[0] * strength,
                m.emission[1] * strength,
                m.emission[2] * strength,
                0.0,
            ],
            params: [m.metallic, m.roughness, m.specular, m.ior],
            params2: [m.transmission, m.clearcoat, 0.0, 0.0],
        }
    }
}

/// Linear palette and material slots of a document. A slot is a material
/// with a layer opacity (layers share a slot when both match); slot 0 is
/// the default material, opaque.
#[derive(Debug, Clone, PartialEq)]
pub struct PtTables {
    pub palette: Box<[[f32; 4]; 256]>,
    pub materials: Box<[PtMaterial; 256]>,
    /// `(layer index, slot)` of every layer.
    layer_slots: Vec<(usize, u8)>,
}

impl PtTables {
    pub fn new(doc: &Document) -> Self {
        let mut palette = Box::new([[0.0; 4]; 256]);
        for (out, c) in palette.iter_mut().zip(doc.data.palette.colors()) {
            *out = [
                srgb_to_linear(f32::from(c.r) / 255.0),
                srgb_to_linear(f32::from(c.g) / 255.0),
                srgb_to_linear(f32::from(c.b) / 255.0),
                1.0,
            ];
        }
        // Render ▸ Matte shows every material without shine (view only).
        let matte = doc.data.render.display.matte;
        let look = |m: &Material| if matte { m.matte() } else { m.clone() };
        let default = Material::default();
        let mut keys: Vec<(Option<u32>, u8)> = vec![(None, 255)];
        let mut materials = Box::new([PtMaterial::new(&look(&default), 1.0); 256]);
        let mut layer_slots = Vec::new();
        for i in 0..doc.data.layers.len() {
            let material = doc.data.layers[i]
                .props
                .material
                .filter(|&m| (m as usize) < doc.data.materials.len());
            // Layer and group opacity only; the material's own alpha is in its tint.
            let tint_alpha = doc.layer_material(i).map_or(1.0, |m| m.tint[3]);
            let alpha = (doc.view_alpha(i, false) / tint_alpha.max(1e-3)).clamp(0.0, 1.0);
            let key = (material, (alpha * 255.0).round() as u8);
            let slot = match keys.iter().position(|k| *k == key) {
                Some(s) => s,
                None if keys.len() < 256 => {
                    keys.push(key);
                    let m = material.map_or(&default, |m| &doc.data.materials[m as usize]);
                    materials[keys.len() - 1] = PtMaterial::new(&look(m), f32::from(key.1) / 255.0);
                    keys.len() - 1
                }
                None => 255,
            };
            layer_slots.push((i, slot as u8));
        }
        Self {
            palette,
            materials,
            layer_slots,
        }
    }

    /// Same layer-to-slot assignment (the scene needs no rebuild).
    pub fn same_slots(&self, other: &PtTables) -> bool {
        self.layer_slots == other.layer_slots
    }

    /// Slot of layer `layer` (0 when unknown).
    pub fn slot_of(&self, layer: usize) -> u8 {
        self.layer_slots
            .iter()
            .find(|(i, _)| *i == layer)
            .map_or(0, |(_, s)| *s)
    }
}

// ------------------------------------------------------------------ params

/// Uniforms of the path tracer. Layout must match `Params` in
/// `pt_common.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct PtParams {
    inv_view_proj: [[f32; 4]; 4],
    eye: [f32; 4],
    forward: [f32; 4],
    right: [f32; 4],
    up: [f32; 4],
    image: [f32; 4],
    frame: [f32; 4],
    grid: [f32; 4],
    dims: [f32; 4],
    shape: [f32; 4],
    light_dir: [[f32; 4]; 3],
    light_irr: [[f32; 4]; 3],
    sky: [f32; 4],
    sky_zenith: [f32; 4],
    ground: [f32; 4],
    background: [f32; 4],
    background_top: [f32; 4],
    fog: [f32; 4],
    bounces: [f32; 4],
    misc: [f32; 4],
    unoccluded: [f32; 4],
    film: [f32; 4],
    bloom: [f32; 4],
    edges: [f32; 4],
    voxel_lines: [f32; 4],
    floor_grid: [f32; 4],
    lines: [f32; 4],
    footprint: [f32; 4],
}

/// Camera of a path-traced image.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PtCamera {
    pub view_proj: Mat4,
    pub eye: Vec3,
    pub forward: Vec3,
    pub ortho: bool,
}

/// A distant light: unit direction, cone half-angle cosine, irradiance
/// (linear RGB) and the cone's solid angle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Light {
    pub dir: Vec3,
    pub cos_half: f32,
    pub irradiance: [f32; 3],
    pub solid_angle: f32,
}

fn linear(c: [f32; 3]) -> [f32; 3] {
    c.map(srgb_to_linear)
}

fn light(azimuth: f32, elevation: f32, size_deg: f32, irradiance: [f32; 3]) -> Light {
    let half = (size_deg * 0.5).max(0.25).to_radians();
    let cos_half = half.cos();
    Light {
        dir: sun_direction(azimuth, elevation),
        cos_half,
        irradiance,
        solid_angle: 2.0 * std::f32::consts::PI * (1.0 - cos_half),
    }
}

/// The sun and, with *Studio lights*, a soft fill and a rim light: the
/// photo studio's three-point setup with the sun as the key.
pub fn lights(s: &RenderSettings) -> Vec<Light> {
    let l = &s.light;
    let mut out = Vec::new();
    let color = linear(l.sun_color);
    let scaled = |k: f32| color.map(|c| c * l.sun_intensity * k);
    if l.sun {
        out.push(light(
            l.sun_azimuth,
            l.sun_elevation,
            l.sun_size,
            scaled(1.0),
        ));
    }
    if l.studio_lights {
        out.push(light(l.sun_azimuth + 130.0, 25.0, 50.0, scaled(0.35)));
        out.push(light(l.sun_azimuth + 200.0, 30.0, 20.0, scaled(0.5)));
    }
    out
}

/// Sky radiance at the horizon and the zenith (linear).
fn sky_colors(s: &RenderSettings) -> ([f32; 3], [f32; 3]) {
    let l = &s.light;
    let k = l.sky_intensity;
    match l.sky {
        SkyMode::Uniform => {
            let c = linear(l.sky_color).map(|v| v * k);
            (c, c)
        }
        SkyMode::Gradient => (
            linear(l.sky_color).map(|v| v * k),
            linear(l.sky_zenith).map(|v| v * k),
        ),
        SkyMode::Image => ([k; 3], [k; 3]),
    }
}

/// Irradiance on the ground with nothing in the way: every light at its
/// elevation plus the sky's upper half (the shadow catcher's reference).
pub fn unoccluded_irradiance(s: &RenderSettings, sky_image: Option<&HdrImage>) -> [f32; 3] {
    let (h, z) = sky_colors(s);
    let pi = std::f32::consts::PI;
    let mut e = match (s.light.sky, sky_image) {
        (SkyMode::Uniform, _) => h.map(|v| pi * v),
        // 2π ∫ mix(h, z, √μ) μ dμ over μ = cos θ.
        (SkyMode::Gradient, _) => [0, 1, 2].map(|i| 2.0 * pi * (0.1 * h[i] + 0.4 * z[i])),
        (SkyMode::Image, Some(img)) => img.up_irradiance().map(|v| v * s.light.sky_intensity),
        (SkyMode::Image, None) => [0.0; 3],
    };
    for l in lights(s) {
        for (c, v) in e.iter_mut().zip(l.irradiance) {
            *c += v * l.dir.y.max(0.0);
        }
    }
    e
}

/// Everything about the image being traced besides the scene.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PtTarget {
    /// Full image size.
    pub image: [u32; 2],
    /// Origin of the traced region (a tile) in the image.
    pub origin: [u32; 2],
    /// Size of the traced region.
    pub size: [u32; 2],
    /// The background is transparent (output only).
    pub transparent: bool,
}

impl PtTarget {
    pub fn whole(size: [u32; 2]) -> Self {
        Self {
            image: size,
            origin: [0, 0],
            size,
            transparent: false,
        }
    }
}

/// Builds the uniforms; `frame` and the sample count are set per run.
pub fn build_params(
    s: &RenderSettings,
    camera: &PtCamera,
    grid_origin: Vec3,
    dims: [u32; 3],
    target: &PtTarget,
    sky_image: Option<&HdrImage>,
) -> PtParams {
    let v4 = |v: Vec3, w: f32| [v.x, v.y, v.z, w];
    let c4 = |c: [f32; 3], w: f32| [c[0], c[1], c[2], w];
    let right = camera
        .forward
        .cross(Vec3::Y)
        .try_normalize()
        .unwrap_or(Vec3::X);
    let up = right.cross(camera.forward);
    let mut light_dir = [[0.0; 4]; 3];
    let mut light_irr = [[0.0; 4]; 3];
    let ls = lights(s);
    for (i, l) in ls.iter().take(3).enumerate() {
        light_dir[i] = v4(l.dir, l.cos_half);
        light_irr[i] = c4(l.irradiance, l.solid_angle);
    }
    let (horizon, zenith) = sky_colors(s);
    let sky_mode = match (s.light.sky, sky_image) {
        (SkyMode::Uniform, _) => 0.0,
        (SkyMode::Gradient, _) => 1.0,
        (SkyMode::Image, Some(_)) => 2.0,
        // No image loaded: a dim uniform sky instead of black.
        (SkyMode::Image, None) => 0.0,
    };
    let d = &s.display;
    let mut ground_flags = 0.0;
    if d.ground {
        ground_flags += 1.0;
        if d.shadow_catcher {
            ground_flags += 2.0;
        }
    }
    let background_mode = if target.transparent {
        3.0
    } else {
        match d.background {
            BackgroundMode::Color => 0.0,
            BackgroundMode::Gradient => 1.0,
            BackgroundMode::Sky => 2.0,
            BackgroundMode::Transparent => 3.0,
        }
    };
    let diagonal = Vec3::new(dims[0] as f32, dims[1] as f32, dims[2] as f32).length();
    let p = &s.sample;
    let c = &s.camera;
    let filter = if p.pixelated {
        2.0
    } else {
        match p.filter {
            PixelFilter::Box => 0.0,
            PixelFilter::Gaussian => 1.0,
        }
    };
    PtParams {
        inv_view_proj: camera.view_proj.inverse().to_cols_array_2d(),
        eye: v4(camera.eye, if camera.ortho { 1.0 } else { 0.0 }),
        forward: v4(camera.forward, c.focus_distance),
        right: v4(right, c.aperture),
        up: v4(up, c.blades as f32),
        image: [
            target.image[0] as f32,
            target.image[1] as f32,
            target.origin[0] as f32,
            target.origin[1] as f32,
        ],
        frame: [0.0, 1.0, 1.0, filter],
        grid: v4(grid_origin, d.shape.index() as f32),
        dims: [dims[0] as f32, dims[1] as f32, dims[2] as f32, d.cell],
        shape: [
            s.effective_bevel(),
            if s.effective_seams() { 1.0 } else { 0.0 },
            target.size[0] as f32,
            target.size[1] as f32,
        ],
        light_dir,
        light_irr,
        sky: c4(horizon, sky_mode),
        sky_zenith: c4(zenith, s.light.sky_rotation.to_radians()),
        ground: c4(linear(d.ground_color), ground_flags),
        background: c4(d.background_color, background_mode),
        background_top: c4(d.background_top, 0.0),
        fog: c4(
            linear(s.light.fog_color),
            if s.light.fog {
                s.light.fog_density
            } else {
                0.0
            },
        ),
        bounces: [
            p.diffuse_bounces as f32,
            p.specular_bounces as f32,
            p.transmission_bounces as f32,
            if p.russian_roulette { 1.0 } else { 0.0 },
        ],
        misc: [
            p.clamp,
            ls.len().min(3) as f32,
            diagonal * 4.0,
            if p.denoise == Denoise::Auto { 1.0 } else { 0.0 },
        ],
        unoccluded: c4(unoccluded_irradiance(s, sky_image), 0.0),
        film: [
            2f32.powf(c.exposure),
            match c.tone_map {
                ToneMap::Linear => 0.0,
                ToneMap::Aces => 1.0,
                ToneMap::Filmic => 2.0,
            },
            1.0 / c.gamma,
            c.vignette,
        ],
        bloom: [
            if c.bloom { 1.0 } else { 0.0 },
            c.bloom_threshold,
            c.bloom_strength,
            0.0,
        ],
        edges: c4(d.edge_color, if d.edges { d.edge_width } else { 0.0 }),
        voxel_lines: c4(
            linear(d.voxel_line_color),
            if s.effective_voxel_lines() {
                d.voxel_line_width
            } else {
                0.0
            },
        ),
        floor_grid: c4(
            linear(d.floor_grid_color),
            if s.effective_floor_grid() {
                d.floor_grid_width
            } else {
                0.0
            },
        ),
        lines: [
            d.voxel_line_opacity,
            d.floor_grid_spacing as f32,
            floor_grid_fade_radius(s, dims),
            d.floor_grid_glow,
        ],
        footprint: {
            let (spread, width) = pixel_footprint(camera, target.image);
            let center = grid_origin + Vec3::new(dims[0] as f32, 0.0, dims[2] as f32) * 0.5;
            [spread, width, center.x, center.z]
        },
    }
}

/// Floor grid fade radius in voxels (0 = no fade): the setting is in
/// lengths of the bounds' longest side.
pub fn floor_grid_fade_radius(s: &RenderSettings, dims: [u32; 3]) -> f32 {
    s.display.floor_grid_fade * dims.into_iter().max().unwrap_or(1) as f32
}

/// Size of one pixel of an `image`-sized picture seen through `camera`:
/// the angle between neighboring pixels' rays (perspective) and the
/// width of a pixel in world units (orthographic), measured at the image
/// center. Lines thinner than this alias, so the shaders widen them to it.
pub fn pixel_footprint(camera: &PtCamera, image: [u32; 2]) -> (f32, f32) {
    let inv = camera.view_proj.inverse();
    let step = 2.0 / image[1].max(1) as f32;
    let ray = |y: f32| {
        let near = inv.project_point3(Vec3::new(0.0, y, 0.0));
        let far = inv.project_point3(Vec3::new(0.0, y, 1.0));
        (near, (far - near).normalize_or_zero())
    };
    let (n0, d0) = ray(0.0);
    let (n1, d1) = ray(step);
    if camera.ortho {
        (0.0, n0.distance(n1))
    } else {
        (d0.angle_between(d1), 0.0)
    }
}

impl PtParams {
    /// Whether the resolve runs the denoiser.
    pub fn denoise(&self) -> bool {
        self.misc[3] > 0.5
    }

    /// Size of the traced region.
    pub fn size(&self) -> [u32; 2] {
        [self.shape[2] as u32, self.shape[3] as u32]
    }

    /// Moves the traced region (tiles of a photo render).
    pub fn set_region(&mut self, origin: [u32; 2], size: [u32; 2]) {
        self.image[2] = origin[0] as f32;
        self.image[3] = origin[1] as f32;
        self.shape[2] = size[0] as f32;
        self.shape[3] = size[1] as f32;
    }
}

// ------------------------------------------------------------------ GPU

struct SceneGpu {
    dims: [u32; 3],
    voxels: wgpu::Texture,
    bricks: wgpu::Texture,
    /// What is on the GPU, to upload only the region that changed.
    cpu: Arc<PtScene>,
}

struct Targets {
    size: [u32; 2],
    accum: wgpu::Buffer,
    aux: wgpu::Buffer,
    albedo: wgpu::Buffer,
    normal_depth: wgpu::Buffer,
    ping: wgpu::Buffer,
    pong: wgpu::Buffer,
    /// Denoiser guides (normal, depth) packed by the prepare pass.
    guide: wgpu::Buffer,
    image: wgpu::Texture,
    image_view: wgpu::TextureView,
}

/// GPU state of the path tracer.
pub struct PathTracer {
    trace_pipeline: wgpu::ComputePipeline,
    test_pipeline: wgpu::ComputePipeline,
    prepare_pipeline: wgpu::ComputePipeline,
    atrous_pipeline: wgpu::ComputePipeline,
    finish_pipeline: wgpu::ComputePipeline,
    trace_layout: wgpu::BindGroupLayout,
    test_layout: wgpu::BindGroupLayout,
    resolve_layout: wgpu::BindGroupLayout,
    params: wgpu::Buffer,
    palette: wgpu::Buffer,
    materials: wgpu::Buffer,
    steps: Vec<(wgpu::Buffer, wgpu::BindGroup)>,
    tables: Option<PtTables>,
    scene: Option<SceneGpu>,
    sky: wgpu::Texture,
    /// Identity of the sky image on the GPU (`None` = the 1×1 default).
    sky_key: Option<usize>,
    targets: Option<Targets>,
    trace_group: Option<wgpu::BindGroup>,
    /// (src A dst B, src B dst A).
    resolve_groups: Option<(wgpu::BindGroup, wgpu::BindGroup)>,
    /// Bumped whenever the image texture is replaced.
    generation: u64,
}

fn storage(binding: u32, read_only: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn uniform(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn texture_entry(
    binding: u32,
    sample_type: wgpu::TextureSampleType,
    dim: wgpu::TextureViewDimension,
) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Texture {
            sample_type,
            view_dimension: dim,
            multisampled: false,
        },
        count: None,
    }
}

fn buffer(
    device: &wgpu::Device,
    label: &str,
    size: u64,
    usage: wgpu::BufferUsages,
) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: size.max(16),
        usage,
        mapped_at_creation: false,
    })
}

fn groups(size: [u32; 2]) -> (u32, u32) {
    (size[0].div_ceil(8), size[1].div_ceil(8))
}

impl PathTracer {
    pub fn new(device: &wgpu::Device) -> Self {
        let trace_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rizxel_pathtrace"),
            source: wgpu::ShaderSource::Wgsl(TRACE_SOURCE.into()),
        });
        let resolve_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rizxel_pt_resolve"),
            source: wgpu::ShaderSource::Wgsl(RESOLVE_SOURCE.into()),
        });
        let uint = wgpu::TextureSampleType::Uint;
        let d3 = wgpu::TextureViewDimension::D3;
        let trace_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rizxel_pt_trace_layout"),
            entries: &[
                uniform(0),
                texture_entry(1, uint, d3),
                texture_entry(2, uint, d3),
                uniform(3),
                uniform(4),
                storage(5, false),
                storage(6, false),
                storage(7, false),
                storage(8, false),
                texture_entry(
                    9,
                    wgpu::TextureSampleType::Float { filterable: false },
                    wgpu::TextureViewDimension::D2,
                ),
            ],
        });
        let test_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rizxel_pt_test_layout"),
            entries: &[storage(0, true), storage(1, false)],
        });
        let resolve_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rizxel_pt_resolve_layout"),
            entries: &[
                uniform(0),
                storage(1, true),
                storage(2, false),
                storage(3, true),
                storage(4, true),
                storage(5, true),
                storage(6, true),
                wgpu::BindGroupLayoutEntry {
                    binding: 7,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: IMAGE_FORMAT,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
                storage(8, false),
            ],
        });
        let step_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rizxel_pt_step_layout"),
            entries: &[uniform(0)],
        });
        let pipeline = |label: &str,
                        layouts: &[Option<&wgpu::BindGroupLayout>],
                        module: &wgpu::ShaderModule,
                        entry: &str| {
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: layouts,
                immediate_size: 0,
            });
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let trace_pipeline = pipeline(
            "rizxel_pt_trace",
            &[Some(&trace_layout)],
            &trace_module,
            "cs_trace",
        );
        let test_pipeline = pipeline(
            "rizxel_pt_raycast_test",
            &[Some(&trace_layout), Some(&test_layout)],
            &trace_module,
            "cs_raycast_test",
        );
        let resolve = [Some(&resolve_layout), Some(&step_layout)];
        let prepare_pipeline =
            pipeline("rizxel_pt_prepare", &resolve, &resolve_module, "cs_prepare");
        let atrous_pipeline = pipeline("rizxel_pt_atrous", &resolve, &resolve_module, "cs_atrous");
        let finish_pipeline = pipeline("rizxel_pt_finish", &resolve, &resolve_module, "cs_finish");

        let uniform_usage = wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST;
        let params = buffer(
            device,
            "rizxel_pt_params",
            std::mem::size_of::<PtParams>() as u64,
            uniform_usage,
        );
        let palette = buffer(device, "rizxel_pt_palette", 256 * 16, uniform_usage);
        let materials = buffer(
            device,
            "rizxel_pt_materials",
            256 * std::mem::size_of::<PtMaterial>() as u64,
            uniform_usage,
        );
        let steps = (0..=ATROUS_PASSES)
            .map(|_| {
                let b = buffer(device, "rizxel_pt_step", 16, uniform_usage);
                let g = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("rizxel_pt_step"),
                    layout: &step_layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: b.as_entire_binding(),
                    }],
                });
                (b, g)
            })
            .collect();
        let sky = Self::sky_texture(device, 1, 1);
        Self {
            trace_pipeline,
            test_pipeline,
            prepare_pipeline,
            atrous_pipeline,
            finish_pipeline,
            trace_layout,
            test_layout,
            resolve_layout,
            params,
            palette,
            materials,
            steps,
            tables: None,
            scene: None,
            sky,
            sky_key: None,
            targets: None,
            trace_group: None,
            resolve_groups: None,
            generation: 0,
        }
    }

    /// Changes whenever [`PathTracer::image_view`] is a new texture.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    fn sky_texture(device: &wgpu::Device, w: u32, h: u32) -> wgpu::Texture {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("rizxel_pt_sky"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        })
    }

    /// Uploads the palette and material slots when they changed.
    pub fn set_tables(&mut self, queue: &wgpu::Queue, tables: &PtTables) {
        if self.tables.as_ref() == Some(tables) {
            return;
        }
        queue.write_buffer(&self.palette, 0, bytemuck::cast_slice(&tables.palette[..]));
        queue.write_buffer(
            &self.materials,
            0,
            bytemuck::cast_slice(&tables.materials[..]),
        );
        self.tables = Some(tables.clone());
    }

    /// Uploads the sky image (`key` identifies it; `None` = no image).
    pub fn set_sky(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        image: Option<(usize, &HdrImage)>,
    ) {
        let key = image.map(|(k, _)| k);
        if key == self.sky_key {
            return;
        }
        self.sky_key = key;
        self.trace_group = None;
        let Some((_, img)) = image else {
            self.sky = Self::sky_texture(device, 1, 1);
            return;
        };
        self.sky = Self::sky_texture(device, img.width, img.height);
        let data: Vec<f32> = img
            .pixels
            .iter()
            .flat_map(|p| [p[0], p[1], p[2], 1.0])
            .collect();
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.sky,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(&data),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(img.width * 16),
                rows_per_image: Some(img.height),
            },
            wgpu::Extent3d {
                width: img.width,
                height: img.height,
                depth_or_array_layers: 1,
            },
        );
    }

    /// Uploads `scene`. With the same size as before only the box of cells
    /// that changed is written.
    pub fn set_scene(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, scene: &Arc<PtScene>) {
        if self
            .scene
            .as_ref()
            .is_some_and(|s| Arc::ptr_eq(&s.cpu, scene))
        {
            return;
        }
        let reuse = self.scene.as_ref().is_some_and(|s| s.dims == scene.dims);
        if !reuse {
            let tex = |label, size: [u32; 3], format| {
                device.create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width: size[0],
                        height: size[1],
                        depth_or_array_layers: size[2],
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D3,
                    format,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                })
            };
            self.scene = Some(SceneGpu {
                dims: scene.dims,
                voxels: tex("rizxel_pt_voxels", scene.dims, wgpu::TextureFormat::Rg8Uint),
                bricks: tex(
                    "rizxel_pt_bricks",
                    scene.brick_dims,
                    wgpu::TextureFormat::R8Uint,
                ),
                cpu: Arc::new(PtScene {
                    dims: scene.dims,
                    cells: vec![0xff; scene.cells.len()],
                    brick_dims: scene.brick_dims,
                    bricks: Vec::new(),
                }),
            });
            self.trace_group = None;
        }
        let Some(gpu) = &mut self.scene else { return };
        if let Some((lo, hi)) = changed_box(&gpu.cpu, scene) {
            let [dx, dy, _] = scene.dims.map(|d| d as usize);
            let w = hi[0] - lo[0];
            let mut data = Vec::with_capacity(w * (hi[1] - lo[1]) * (hi[2] - lo[2]) * 2);
            for z in lo[2]..hi[2] {
                for y in lo[1]..hi[1] {
                    let start = ((z * dy + y) * dx + lo[0]) * 2;
                    data.extend_from_slice(&scene.cells[start..start + w * 2]);
                }
            }
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &gpu.voxels,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: lo[0] as u32,
                        y: lo[1] as u32,
                        z: lo[2] as u32,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                &data,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(w as u32 * 2),
                    rows_per_image: Some((hi[1] - lo[1]) as u32),
                },
                wgpu::Extent3d {
                    width: w as u32,
                    height: (hi[1] - lo[1]) as u32,
                    depth_or_array_layers: (hi[2] - lo[2]) as u32,
                },
            );
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &gpu.bricks,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &scene.bricks,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(scene.brick_dims[0]),
                    rows_per_image: Some(scene.brick_dims[1]),
                },
                wgpu::Extent3d {
                    width: scene.brick_dims[0],
                    height: scene.brick_dims[1],
                    depth_or_array_layers: scene.brick_dims[2],
                },
            );
        }
        gpu.cpu = scene.clone();
    }

    /// (Re)allocates the sample buffers and the image for `size` pixels.
    /// A new size starts from zero samples.
    pub fn resize(&mut self, device: &wgpu::Device, size: [u32; 2]) {
        let size = size.map(|v| v.max(1));
        if self.targets.as_ref().is_some_and(|t| t.size == size) {
            return;
        }
        let bytes = u64::from(size[0]) * u64::from(size[1]) * 16;
        let usage = wgpu::BufferUsages::STORAGE;
        let image = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("rizxel_pt_image"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: IMAGE_FORMAT,
            usage: wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let image_view = image.create_view(&Default::default());
        self.targets = Some(Targets {
            size,
            accum: buffer(device, "rizxel_pt_accum", bytes, usage),
            aux: buffer(device, "rizxel_pt_aux", bytes, usage),
            albedo: buffer(device, "rizxel_pt_albedo", bytes, usage),
            normal_depth: buffer(device, "rizxel_pt_normal_depth", bytes, usage),
            ping: buffer(device, "rizxel_pt_ping", bytes, usage),
            pong: buffer(device, "rizxel_pt_pong", bytes, usage),
            guide: buffer(device, "rizxel_pt_guide", bytes, usage),
            image,
            image_view,
        });
        self.trace_group = None;
        self.resolve_groups = None;
        self.generation += 1;
    }

    pub fn size(&self) -> Option<[u32; 2]> {
        self.targets.as_ref().map(|t| t.size)
    }

    /// The finished image (after [`PathTracer::run`] with a resolve).
    pub fn image_view(&self) -> Option<&wgpu::TextureView> {
        self.targets.as_ref().map(|t| &t.image_view)
    }

    pub fn image_texture(&self) -> Option<&wgpu::Texture> {
        self.targets.as_ref().map(|t| &t.image)
    }

    fn ensure_groups(&mut self, device: &wgpu::Device) -> bool {
        let (Some(scene), Some(t)) = (&self.scene, &self.targets) else {
            return false;
        };
        if self.trace_group.is_none() {
            let voxels = scene.voxels.create_view(&Default::default());
            let bricks = scene.bricks.create_view(&Default::default());
            let sky = self.sky.create_view(&Default::default());
            self.trace_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("rizxel_pt_trace"),
                layout: &self.trace_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.params.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&voxels),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&bricks),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: self.palette.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: self.materials.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: t.accum.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 6,
                        resource: t.aux.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 7,
                        resource: t.albedo.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 8,
                        resource: t.normal_depth.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 9,
                        resource: wgpu::BindingResource::TextureView(&sky),
                    },
                ],
            }));
        }
        if self.resolve_groups.is_none() {
            let group = |src: &wgpu::Buffer, dst: &wgpu::Buffer| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("rizxel_pt_resolve"),
                    layout: &self.resolve_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: self.params.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: src.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: dst.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: t.albedo.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 4,
                            resource: t.normal_depth.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 5,
                            resource: t.accum.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 6,
                            resource: t.aux.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 7,
                            resource: wgpu::BindingResource::TextureView(&t.image_view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 8,
                            resource: t.guide.as_entire_binding(),
                        },
                    ],
                })
            };
            self.resolve_groups = Some((group(&t.ping, &t.pong), group(&t.pong, &t.ping)));
        }
        true
    }

    /// Adds the samples `samples` per pixel (starting at 0 restarts),
    /// then, with `resolve`, filters and writes the image. The region,
    /// camera and settings come from `params`.
    pub fn run(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        params: &PtParams,
        samples: std::ops::Range<u32>,
        resolve: bool,
    ) {
        let (first, count) = (samples.start, samples.len() as u32);
        if !self.ensure_groups(device) {
            return;
        }
        let size = params.size();
        let mut p = *params;
        p.frame[0] = first as f32;
        p.frame[1] = count as f32;
        p.bloom[3] = (first + count) as f32;
        queue.write_buffer(&self.params, 0, bytemuck::bytes_of(&p));
        let (gx, gy) = groups(size);
        let (Some(trace), Some((ab, ba))) = (&self.trace_group, &self.resolve_groups) else {
            return;
        };
        if count > 0 {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.trace_pipeline);
            pass.set_bind_group(0, trace, &[]);
            pass.dispatch_workgroups(gx, gy, 1);
        }
        if !resolve {
            return;
        }
        let samples = (first + count).max(1) as f32;
        // Looser luminance edges while the image is noisy.
        let sigma = 1.5 / samples.sqrt() + 0.15;
        for (i, (b, _)) in self.steps.iter().enumerate() {
            let step = if i < ATROUS_PASSES { 1u32 << i } else { 0 } as f32;
            let s = sigma * 0.75f32.powi(i as i32);
            queue.write_buffer(b, 0, bytemuck::cast_slice(&[step, s, 0.0, 0.0]));
        }
        let mut pass = encoder.begin_compute_pass(&Default::default());
        let last_step = &self.steps[ATROUS_PASSES].1;
        // prepare: -> ping (group BA writes ping).
        pass.set_pipeline(&self.prepare_pipeline);
        pass.set_bind_group(0, ba, &[]);
        pass.set_bind_group(1, last_step, &[]);
        pass.dispatch_workgroups(gx, gy, 1);
        let mut src_is_ping = true;
        if params.denoise() {
            pass.set_pipeline(&self.atrous_pipeline);
            for (_, step_group) in self.steps.iter().take(ATROUS_PASSES) {
                pass.set_bind_group(0, if src_is_ping { ab } else { ba }, &[]);
                pass.set_bind_group(1, step_group, &[]);
                pass.dispatch_workgroups(gx, gy, 1);
                src_is_ping = !src_is_ping;
            }
        }
        pass.set_pipeline(&self.finish_pipeline);
        pass.set_bind_group(0, if src_is_ping { ab } else { ba }, &[]);
        pass.set_bind_group(1, last_step, &[]);
        pass.dispatch_workgroups(gx, gy, 1);
    }

    /// Traces `rays` (voxel space origin, direction) through the uploaded
    /// scene and returns per ray `[cell x, y, z, t]` and `[normal, stud]`
    /// (cell `-1` = miss). Test helper; blocks.
    pub fn raycast(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        params: &PtParams,
        rays: &[(Vec3, Vec3)],
    ) -> Vec<([f32; 4], [f32; 4])> {
        self.resize(device, [1, 1]);
        if !self.ensure_groups(device) || rays.is_empty() {
            return Vec::new();
        }
        queue.write_buffer(&self.params, 0, bytemuck::bytes_of(params));
        let data: Vec<f32> = rays
            .iter()
            .flat_map(|(o, d)| [o.x, o.y, o.z, 0.0, d.x, d.y, d.z, 0.0])
            .collect();
        let input = buffer(
            device,
            "rizxel_pt_test_rays",
            (data.len() * 4) as u64,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        );
        queue.write_buffer(&input, 0, bytemuck::cast_slice(&data));
        let out_size = (rays.len() * 32) as u64;
        let output = buffer(
            device,
            "rizxel_pt_test_hits",
            out_size,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        );
        let readback = buffer(
            device,
            "rizxel_pt_test_readback",
            out_size,
            wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        );
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rizxel_pt_test"),
            layout: &self.test_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: input.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: output.as_entire_binding(),
                },
            ],
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.test_pipeline);
            if let Some(trace) = &self.trace_group {
                pass.set_bind_group(0, trace, &[]);
            }
            pass.set_bind_group(1, &group, &[]);
            pass.dispatch_workgroups((rays.len() as u32).div_ceil(64), 1, 1);
        }
        encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, out_size);
        queue.submit([encoder.finish()]);
        let bytes = super::offscreen::map_read(device, &readback);
        let floats: &[f32] = bytemuck::cast_slice(&bytes);
        floats
            .chunks_exact(8)
            .take(rays.len())
            .map(|c| ([c[0], c[1], c[2], c[3]], [c[4], c[5], c[6], c[7]]))
            .collect()
    }

    /// Reads `size` pixels at `origin` of the finished image as RGBA16.
    pub fn read_image(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        origin: [u32; 2],
        size: [u32; 2],
    ) -> Vec<u16> {
        let Some(t) = &self.targets else {
            return Vec::new();
        };
        let bytes = super::offscreen::read_texture(device, queue, &t.image, origin, size, 8);
        bytemuck::cast_slice::<u8, u16>(&bytes)
            .iter()
            .map(|&h| {
                let v = f16_to_f32(h).clamp(0.0, 1.0);
                (v * 65535.0).round() as u16
            })
            .collect()
    }
}

/// Smallest box `[lo, hi)` of cells that differ between two scenes of the
/// same size (`None` when nothing changed).
fn changed_box(old: &PtScene, new: &PtScene) -> Option<([usize; 3], [usize; 3])> {
    let [dx, dy, _] = new.dims.map(|d| d as usize);
    let mut lo = [usize::MAX; 3];
    let mut hi = [0usize; 3];
    for (i, (a, b)) in old
        .cells
        .chunks_exact(2)
        .zip(new.cells.chunks_exact(2))
        .enumerate()
    {
        if a != b {
            let p = [i % dx, (i / dx) % dy, i / (dx * dy)];
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k] + 1);
            }
        }
    }
    (lo[0] != usize::MAX).then_some((lo, hi))
}

/// IEEE half to f32.
pub fn f16_to_f32(h: u16) -> f32 {
    let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exp = i32::from((h >> 10) & 0x1f);
    let frac = f32::from(h & 0x3ff);
    match exp {
        0 => sign * frac * 2f32.powi(-24),
        31 => {
            if frac == 0.0 {
                sign * f32::INFINITY
            } else {
                f32::NAN
            }
        }
        _ => sign * (1.0 + frac / 1024.0) * 2f32.powi(exp - 15),
    }
}

// ------------------------------------------------------------------ jobs

/// A finished render as RGBA16 (`0..=65535`, gamma-encoded).
#[derive(Debug, Clone, PartialEq)]
pub struct Rgba16 {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u16>,
}

impl Rgba16 {
    pub fn to_rgba8(&self) -> rizxel_app::RgbaImage {
        rizxel_app::RgbaImage {
            width: self.width,
            height: self.height,
            pixels: self
                .pixels
                .iter()
                .map(|&v| ((u32::from(v) * 255 + 32767) / 65535) as u8)
                .collect(),
        }
    }
}

/// An offline path-traced image, rendered tile by tile in small steps so
/// the UI stays responsive and the GPU never runs long enough to reset.
pub struct PhotoJob {
    params: PtParams,
    size: [u32; 2],
    samples: u32,
    tiles: Vec<[u32; 4]>,
    tile: usize,
    done: u32,
    per_step: u32,
    time_limit: Option<Duration>,
    started: Instant,
    tile_started: Instant,
    image: Rgba16,
}

impl PhotoJob {
    /// `params` describe the whole image (region = everything).
    pub fn new(params: PtParams, samples: u32, time_limit: Option<Duration>) -> Self {
        let size = [params.image[0] as u32, params.image[1] as u32];
        Self {
            params,
            size,
            samples: samples.max(1),
            tiles: super::offscreen::tiles(size, PHOTO_TILE),
            tile: 0,
            done: 0,
            per_step: 1,
            time_limit,
            started: Instant::now(),
            tile_started: Instant::now(),
            image: Rgba16 {
                width: size[0],
                height: size[1],
                pixels: vec![0; (size[0] * size[1] * 4) as usize],
            },
        }
    }

    pub fn is_done(&self) -> bool {
        self.tile >= self.tiles.len()
    }

    /// `0..=1` across all tiles and samples.
    pub fn progress(&self) -> f32 {
        let total = self.tiles.len().max(1) as f32;
        let tile = self.done as f32 / self.samples as f32;
        ((self.tile as f32 + tile) / total).clamp(0.0, 1.0)
    }

    /// Estimated time left from the speed so far.
    pub fn eta(&self) -> Option<Duration> {
        let p = self.progress();
        (p > 0.01).then(|| self.started.elapsed().mul_f32((1.0 - p) / p))
    }

    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    pub fn samples(&self) -> u32 {
        self.samples
    }

    /// The image so far (finished tiles only).
    pub fn image(&self) -> &Rgba16 {
        &self.image
    }

    /// Does about `budget` of GPU work: some samples of the current tile,
    /// or its resolve and readback. Returns whether the job is finished.
    pub fn step(
        &mut self,
        pt: &mut PathTracer,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        budget: Duration,
    ) -> bool {
        let Some(&core) = self.tiles.get(self.tile) else {
            return true;
        };
        // Buffers reach past the tile so the filter sees its neighbours.
        let lo = [core[0].saturating_sub(APRON), core[1].saturating_sub(APRON)];
        let hi = [
            (core[2] + APRON).min(self.size[0]),
            (core[3] + APRON).min(self.size[1]),
        ];
        let region = [hi[0] - lo[0], hi[1] - lo[1]];
        let mut params = self.params;
        params.set_region(lo, region);
        if self.done == 0 {
            pt.resize(device, region);
            self.tile_started = Instant::now();
        }
        let tile_area = f64::from(core[2] - core[0]) * f64::from(core[3] - core[1]);
        let image_area = f64::from(self.size[0]) * f64::from(self.size[1]);
        let out_of_time = self.time_limit.is_some_and(|limit| {
            self.done > 0
                && self.tile_started.elapsed().as_secs_f64()
                    > limit.as_secs_f64() * tile_area / image_area
        });
        let count = if out_of_time {
            0
        } else {
            self.per_step.min(self.samples - self.done)
        };
        let last = count == 0 || self.done + count >= self.samples;
        let t0 = Instant::now();
        let mut encoder = device.create_command_encoder(&Default::default());
        pt.run(
            device,
            queue,
            &mut encoder,
            &params,
            self.done..self.done + count,
            last,
        );
        queue.submit([encoder.finish()]);
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        let took = t0.elapsed();
        self.done += count;
        if count > 0 {
            if took < budget / 3 {
                self.per_step = (self.per_step * 2).min(256);
            } else if took > budget && self.per_step > 1 {
                self.per_step /= 2;
            }
        }
        if last {
            let size = [core[2] - core[0], core[3] - core[1]];
            let pixels = pt.read_image(device, queue, [core[0] - lo[0], core[1] - lo[1]], size);
            let row = (size[0] * 4) as usize;
            for (y, src) in pixels.chunks_exact(row).enumerate() {
                let start = ((core[1] as usize + y) * self.size[0] as usize + core[0] as usize) * 4;
                self.image.pixels[start..start + row].copy_from_slice(src);
            }
            self.tile += 1;
            self.done = 0;
        }
        self.is_done()
    }

    /// Runs the job to the end (headless renders).
    pub fn finish(
        &mut self,
        pt: &mut PathTracer,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> &Rgba16 {
        while !self.step(pt, device, queue, Duration::from_millis(200)) {}
        &self.image
    }
}

/// Uniforms for a `size`-pixel image of `doc` seen through `camera`, with
/// the document's render settings (the same framing as the viewport).
pub fn document_params(
    doc: &Document,
    camera: &rizxel_app::OrbitCamera,
    size: [u32; 2],
    transparent: bool,
    sky: Option<&HdrImage>,
) -> PtParams {
    let proj = crate::viewport::output_projection(doc, camera, size);
    let (bmin, _) = rizxel_app::world_bounds(doc.data.bounds);
    let d = doc.data.bounds.0;
    let target = PtTarget {
        transparent,
        ..PtTarget::whole(size)
    };
    let cam = PtCamera {
        view_proj: proj.view_proj(),
        eye: proj.eye(),
        forward: proj.forward(),
        ortho: camera.is_ortho(),
    };
    build_params(&doc.data.render, &cam, bmin, [d.x, d.y, d.z], &target, sky)
}

/// The sky image of the render settings, loaded once per path.
#[derive(Default)]
pub struct SkyCache {
    path: String,
    image: Option<(usize, Arc<HdrImage>)>,
    /// Why the last path could not be used.
    pub error: Option<String>,
}

impl SkyCache {
    /// The image for `s` (`None` unless the sky mode is *HDR Image* and
    /// the file loads).
    pub fn get(&mut self, s: &RenderSettings) -> Option<(usize, Arc<HdrImage>)> {
        if s.light.sky != SkyMode::Image || s.light.sky_image.is_empty() {
            return None;
        }
        if self.path != s.light.sky_image {
            self.path = s.light.sky_image.clone();
            self.error = None;
            self.image = match std::fs::read(&self.path)
                .map_err(|e| e.to_string())
                .and_then(|b| rizxel_app::read_hdr(&b).map_err(|e| e.to_string()))
            {
                Ok(img) => {
                    let img = Arc::new(img);
                    Some((Arc::as_ptr(&img) as usize, img))
                }
                Err(e) => {
                    log::warn!("sky image {}: {e}", self.path);
                    self.error = Some(e);
                    None
                }
            };
        }
        self.image.clone()
    }
}

/// Inputs shared by the preview and the outputs: the scene, tables and
/// sky of a document.
#[derive(Clone)]
pub struct PtInputs {
    pub scene: Arc<PtScene>,
    pub tables: Arc<PtTables>,
    pub sky: Option<(usize, Arc<HdrImage>)>,
}

impl PtInputs {
    pub fn from_document(doc: &Document, sky: Option<(usize, Arc<HdrImage>)>) -> Self {
        let tables = PtTables::new(doc);
        Self {
            scene: Arc::new(PtScene::from_document(doc, &tables)),
            tables: Arc::new(tables),
            sky,
        }
    }

    /// Uploads everything to `pt`.
    pub fn upload(&self, pt: &mut PathTracer, device: &wgpu::Device, queue: &wgpu::Queue) {
        pt.set_scene(device, queue, &self.scene);
        pt.set_tables(queue, &self.tables);
        pt.set_sky(
            device,
            queue,
            self.sky.as_ref().map(|(k, i)| (*k, i.as_ref())),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::IVec3;
    use rizxel_math::BoundSize;

    #[test]
    fn struct_sizes_match_wgsl() {
        // mat4 + 9 vec4 + 6 vec4 (lights) + 16 vec4.
        assert_eq!(std::mem::size_of::<PtParams>(), 64 + 31 * 16);
        assert_eq!(std::mem::size_of::<PtMaterial>(), 64);
    }

    #[test]
    fn scene_merges_layers_and_dilates_bricks_upward() {
        let bounds = BoundSize::new(10, 9, 3);
        let mut a = VoxelVolume::new(bounds);
        a.set(IVec3::new(1, 7, 0), 5);
        a.set(IVec3::new(9, 0, 2), 6);
        let mut b = VoxelVolume::new(bounds);
        b.set(IVec3::new(1, 7, 0), 7);
        let scene = PtScene::build([10, 9, 3], [(&a, 1), (&b, 2)], None);
        let cell = |x: usize, y: usize, z: usize| {
            let i = ((z * 9 + y) * 10 + x) * 2;
            (scene.cells[i], scene.cells[i + 1])
        };
        assert_eq!(cell(1, 7, 0), (7, 2), "the upper layer wins");
        assert_eq!(cell(9, 0, 2), (6, 1));
        assert_eq!(scene.brick_dims, [2, 2, 1]);
        // (1, 7, 0) marks brick (0, 0, 0) and, one cell up, (0, 1, 0).
        assert_eq!(scene.bricks, vec![1, 1, 1, 0]);
        // A clip box drops the voxels outside it.
        let clipped = PtScene::build(
            [10, 9, 3],
            [(&a, 1)],
            Some(VoxelBox::from_corners(IVec3::ZERO, IVec3::new(4, 8, 2))),
        );
        assert!(!clipped.is_empty());
        assert_eq!(clipped.cells.iter().filter(|&&v| v == 6).count(), 0);
    }

    #[test]
    fn changed_box_covers_exactly_the_edits() {
        let bounds = BoundSize::new(8, 8, 8);
        let mut v = VoxelVolume::new(bounds);
        let a = PtScene::build([8, 8, 8], [(&v, 0)], None);
        v.set(IVec3::new(2, 3, 4), 1);
        v.set(IVec3::new(5, 1, 4), 1);
        let b = PtScene::build([8, 8, 8], [(&v, 0)], None);
        assert_eq!(changed_box(&a, &b), Some(([2, 1, 4], [6, 4, 5])));
        assert_eq!(changed_box(&b, &b), None);
    }

    #[test]
    fn material_slots_follow_material_and_opacity() {
        let mut doc = Document::new("slots");
        doc.add_layer();
        doc.add_layer();
        doc.set_materials(vec![Material::new("A")]);
        let props = doc.data.layers[1].props.clone();
        doc.set_layer_props(
            1,
            rizxel_app::LayerProps {
                material: Some(0),
                ..props
            },
            false,
        );
        let props = doc.data.layers[2].props.clone();
        doc.set_layer_props(
            2,
            rizxel_app::LayerProps {
                opacity: 0.5,
                ..props
            },
            false,
        );
        let t = PtTables::new(&doc);
        assert_eq!(t.slot_of(0), 0, "default, opaque");
        assert_eq!(t.slot_of(1), 1);
        assert_eq!(t.slot_of(2), 2, "default at half opacity");
        assert!((t.materials[2].tint[3] - 128.0 / 255.0).abs() < 1e-3);
    }

    #[test]
    fn studio_lights_add_fill_and_rim() {
        let mut s = RenderSettings::default();
        assert_eq!(lights(&s).len(), 1);
        s.light.studio_lights = true;
        let l = lights(&s);
        assert_eq!(l.len(), 3);
        assert!(l[1].irradiance[0] < l[0].irradiance[0]);
        // Uniform sky of radiance 1 and no lights: pi on the ground.
        s.light.sun = false;
        s.light.studio_lights = false;
        s.light.sky = SkyMode::Uniform;
        s.light.sky_color = [1.0; 3];
        s.light.sky_intensity = 1.0;
        let e = unoccluded_irradiance(&s, None);
        assert!((e[0] - std::f32::consts::PI).abs() < 1e-4);
    }

    #[test]
    fn half_floats_decode() {
        assert_eq!(f16_to_f32(0x3c00), 1.0);
        assert_eq!(f16_to_f32(0x3800), 0.5);
        assert_eq!(f16_to_f32(0x0000), 0.0);
        assert_eq!(f16_to_f32(0xc000), -2.0);
    }

    // ------------------------------------------------------------- GPU

    fn gpu() -> Option<(wgpu::Device, wgpu::Queue)> {
        match super::super::offscreen::headless_device() {
            Some(h) if h.path_tracing => {
                eprintln!("adapter: {} ({:?})", h.info.name, h.info.backend);
                Some((h.device, h.queue))
            }
            Some(h) => {
                eprintln!("{} can't run the path tracer: skipped", h.info.name);
                None
            }
            None => {
                eprintln!("no GPU adapter: skipped");
                None
            }
        }
    }

    /// Settings for `shape` with nothing that changes the geometry.
    fn shape_settings(shape: rizxel_math::VoxelShape) -> RenderSettings {
        let mut s = RenderSettings::default();
        s.display.shape = shape;
        s
    }

    fn params_for(
        s: &RenderSettings,
        dims: [u32; 3],
        origin: Vec3,
        cam: PtCamera,
        size: [u32; 2],
    ) -> PtParams {
        build_params(s, &cam, origin, dims, &PtTarget::whole(size), None)
    }

    fn test_camera() -> PtCamera {
        PtCamera {
            view_proj: Mat4::IDENTITY,
            eye: Vec3::ZERO,
            forward: Vec3::NEG_Z,
            ortho: false,
        }
    }

    /// A pseudo-random 20³ scene and rays from outside it.
    fn random_scene() -> (VoxelVolume, Vec<(Vec3, Vec3)>) {
        let bounds = BoundSize::new(20, 20, 20);
        let mut v = VoxelVolume::new(bounds);
        let mut seed = 12345u32;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed
        };
        for _ in 0..600 {
            let p = IVec3::new(
                (next() % 20) as i32,
                (next() % 20) as i32,
                (next() % 20) as i32,
            );
            v.set(p, 1 + (next() % 200) as u8);
        }
        let mut rays = Vec::new();
        for _ in 0..400 {
            let f = |n: u32| (n % 10_000) as f32 / 10_000.0;
            let origin = Vec3::new(f(next()) * 60.0 - 20.0, f(next()) * 60.0 - 20.0, -15.0);
            let target = Vec3::new(f(next()) * 20.0, f(next()) * 20.0, f(next()) * 20.0);
            rays.push((origin, (target - origin).normalize()));
        }
        // Axis-aligned rays hit the zero-direction branches.
        rays.push((Vec3::new(3.5, 4.5, -5.0), Vec3::Z));
        rays.push((Vec3::new(3.5, 30.0, 4.5), Vec3::NEG_Y));
        (v, rays)
    }

    #[test]
    fn gpu_traversal_matches_raycast_grid() {
        let Some((device, queue)) = gpu() else { return };
        let (volume, rays) = random_scene();
        let scene = Arc::new(PtScene::build([20, 20, 20], [(&volume, 0)], None));
        let mut pt = PathTracer::new(&device);
        pt.set_scene(&device, &queue, &scene);
        pt.set_tables(&queue, &PtTables::new(&Document::new("t")));
        let s = shape_settings(rizxel_math::VoxelShape::Cube);
        let params = params_for(&s, [20, 20, 20], Vec3::ZERO, test_camera(), [1, 1]);
        let hits = pt.raycast(&device, &queue, &params, &rays);
        assert_eq!(hits.len(), rays.len());
        let area = VoxelBox::from_bounds(BoundSize::new(20, 20, 20));
        let mut compared = 0;
        for ((origin, dir), (cell, normal)) in rays.iter().zip(&hits) {
            let ray = rizxel_engine::Ray::new(*origin, *dir);
            let cpu =
                rizxel_engine::raycast::raycast_grid(&ray, &area, |p| volume.get(p) != EMPTY_VOXEL);
            match cpu {
                None => assert_eq!(
                    cell[0], -1.0,
                    "GPU hit where the CPU missed: {origin} {dir}"
                ),
                Some(h) => {
                    compared += 1;
                    let gpu_cell = IVec3::new(cell[0] as i32, cell[1] as i32, cell[2] as i32);
                    assert_eq!(gpu_cell, h.voxel, "ray {origin} {dir}");
                    assert!((cell[3] - h.t).abs() < 1e-3, "t {} vs {}", cell[3], h.t);
                    let n = IVec3::new(normal[0] as i32, normal[1] as i32, normal[2] as i32);
                    assert_eq!(n, h.normal, "normal of ray {origin} {dir}");
                }
            }
        }
        assert!(compared > 100, "only {compared} hits");
    }

    #[test]
    fn missing_rays_stop_early() {
        let Some((device, queue)) = gpu() else { return };
        let (volume, _) = random_scene();
        let scene = Arc::new(PtScene::build([20, 20, 20], [(&volume, 0)], None));
        let mut pt = PathTracer::new(&device);
        pt.set_scene(&device, &queue, &scene);
        pt.set_tables(&queue, &PtTables::new(&Document::new("t")));
        let s = shape_settings(rizxel_math::VoxelShape::Cube);
        let params = params_for(&s, [20, 20, 20], Vec3::ZERO, test_camera(), [1, 1]);
        // Rays from inside and around the grid in many directions.
        let mut rays = Vec::new();
        for i in 0..2000 {
            let f = |k: u32| {
                ((i as u32).wrapping_mul(2654435761).wrapping_add(k * 40503) % 1000) as f32 / 1000.0
            };
            let o = Vec3::new(f(1) * 30.0 - 5.0, f(2) * 30.0 - 5.0, f(3) * 30.0 - 5.0);
            let d = Vec3::new(f(4) - 0.5, f(5) - 0.5, f(6) - 0.5).normalize_or(Vec3::X);
            rays.push((o, d));
        }
        let hits = pt.raycast(&device, &queue, &params, &rays);
        let worst = hits.iter().map(|(_, n)| n[2] as i32).max().unwrap_or(0);
        let misses: Vec<i32> = hits
            .iter()
            .filter(|(c, _)| c[0] < 0.0)
            .map(|(_, n)| n[2] as i32)
            .collect();
        eprintln!("worst miss steps {worst}, misses {}", misses.len());
        for ((o, d), (c, n)) in rays
            .iter()
            .zip(&hits)
            .filter(|(_, (c, n))| c[0] < 0.0 && n[2] > 199.0)
            .take(6)
        {
            eprintln!("slow miss: o {o} d {d} steps {} {c:?}", n[2]);
        }
        // Crossing a 20³ grid takes at most ~60 cells and a few bricks.
        assert!(misses.iter().all(|&s| s < 200), "a miss took {worst} steps");
    }

    #[test]
    fn gpu_studs_sit_where_shape_mesh_puts_them() {
        let Some((device, queue)) = gpu() else { return };
        let mut v = VoxelVolume::new(BoundSize::new(4, 4, 4));
        v.set(IVec3::new(1, 0, 1), 5);
        let scene = Arc::new(PtScene::build([4, 4, 4], [(&v, 0)], None));
        let mut pt = PathTracer::new(&device);
        pt.set_scene(&device, &queue, &scene);
        pt.set_tables(&queue, &PtTables::new(&Document::new("t")));
        let s = shape_settings(rizxel_math::VoxelShape::Brick);
        let params = params_for(&s, [4, 4, 4], Vec3::ZERO, test_camera(), [1, 1]);
        // The CPU stud: shape_mesh's cylinder above the brick.
        let mesh = rizxel_engine::shape_mesh(
            &[(&v, 0)],
            &rizxel_math::Palette::default(),
            &rizxel_engine::ShapeMeshOptions::new(rizxel_math::VoxelShape::Brick),
        );
        let stud_top = mesh.positions.iter().map(|p| p.y).fold(f32::MIN, f32::max);
        let stud_radius = mesh
            .positions
            .iter()
            .filter(|p| p.y > 1.01)
            .map(|p| (Vec3::new(p.x, 0.0, p.z) - Vec3::new(1.5, 0.0, 1.5)).length())
            .fold(0.0, f32::max);
        let rays = [
            // Down through the stud's center: its top.
            (Vec3::new(1.5, 3.9, 1.5), Vec3::NEG_Y),
            // Down beside the stud: the brick's top.
            (Vec3::new(1.5 + stud_radius + 0.08, 3.9, 1.5), Vec3::NEG_Y),
            // Sideways at stud height: the stud's side.
            (Vec3::new(0.01, 1.1, 1.5), Vec3::X),
        ];
        let hits = pt.raycast(&device, &queue, &params, &rays);
        let y = |i: usize| rays[i].0.y - hits[i].0[3];
        assert!(
            (y(0) - stud_top).abs() < 1e-3,
            "stud top {} vs {stud_top}",
            y(0)
        );
        assert_eq!(hits[0].1[3], 1.0, "flagged as a stud");
        assert!((y(1) - 1.0).abs() < 1e-3, "brick top {}", y(1));
        let x = rays[2].0.x + hits[2].0[3];
        assert!(
            (x - (1.5 - stud_radius)).abs() < 0.02,
            "stud side at x {x} vs {}",
            1.5 - stud_radius
        );
    }

    /// Mean brightness of the middle of a small path-traced image of one
    /// floating white voxel, seen from `pitch`.
    fn white_voxel_brightness(device: &wgpu::Device, queue: &wgpu::Queue, pitch: f32) -> f32 {
        let mut doc = Document::new("white");
        doc.set_palette_color(1, rizxel_math::Rgba8::rgb(255, 255, 255), false);
        let mut tables = PtTables::new(&doc);
        tables.palette[1] = [1.0; 4];
        let mut v = VoxelVolume::new(BoundSize::new(1, 1, 1));
        v.set(IVec3::ZERO, 1);
        let scene = Arc::new(PtScene::build([1, 1, 1], [(&v, 0)], None));
        let mut s = RenderSettings::default();
        s.light.sun = false;
        s.light.sky = SkyMode::Uniform;
        s.light.sky_color = [1.0; 3];
        s.display.background = BackgroundMode::Color;
        s.sample.denoise = Denoise::Off;
        let origin = Vec3::new(-0.5, 6.0, -0.5);
        let target = Vec3::new(0.0, 6.5, 0.0);
        let cam = rizxel_app::OrbitCamera {
            yaw: 30.0,
            pitch,
            distance: 4.0,
            ..Default::default()
        };
        let camera = PtCamera {
            view_proj: cam.view_proj_fov(target, 1.0, 0.6),
            eye: cam.eye(target),
            forward: -cam.eye_direction(),
            ortho: false,
        };
        assert!(camera.eye.y > 0.0, "the camera stays above the ground");
        let params = params_for(&s, [1, 1, 1], origin, camera, [32, 32]);
        let mut pt = PathTracer::new(device);
        pt.set_scene(device, queue, &scene);
        pt.set_tables(queue, &tables);
        let mut job = PhotoJob::new(params, 4, None);
        let img = job.finish(&mut pt, device, queue).clone();
        let mut sum = 0.0;
        for y in 12..20 {
            for x in 12..20 {
                let i = ((y * 32 + x) * 4) as usize;
                sum += f32::from(img.pixels[i]) / 65535.0;
            }
        }
        sum / 64.0
    }

    #[test]
    fn white_voxel_is_brighter_on_top_than_below() {
        let Some((device, queue)) = gpu() else { return };
        let top = white_voxel_brightness(&device, &queue, 60.0);
        let bottom = white_voxel_brightness(&device, &queue, -60.0);
        eprintln!("top {top:.3}, bottom {bottom:.3}");
        assert!(top > 0.3, "the lit top is bright ({top})");
        assert!(top > bottom + 0.1, "top {top} vs bottom {bottom}");
    }
}

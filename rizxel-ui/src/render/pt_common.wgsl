// Shared by the path tracer (pathtrace.wgsl) and its denoise / finish
// passes (denoise.wgsl). Must match `PtParams` in pathtrace.rs.
//
// Colors are linear; the finish pass tone-maps and gamma-encodes them.

struct Params {
    inv_view_proj: mat4x4<f32>,
    // xyz = eye (world), w = 1 for orthographic.
    eye: vec4<f32>,
    // xyz = unit view direction, w = focus distance.
    forward: vec4<f32>,
    // xyz = camera right, w = lens radius.
    right: vec4<f32>,
    // xyz = camera up, w = aperture blades (0 = round).
    up: vec4<f32>,
    // xy = full image size, zw = origin of the buffers in the image (tiles).
    image: vec4<f32>,
    // x = first sample index, y = samples this dispatch, z = seed,
    // w = pixel filter (0 box, 1 Gaussian, 2 pixelated).
    frame: vec4<f32>,
    // xyz = world position of voxel (0, 0, 0), w = voxel shape
    // (0 cube, 1 brick, 2 rounded, 3 sphere, 4 cylinder).
    grid: vec4<f32>,
    // xyz = grid size in voxels, w = cell size (0.5..1).
    dims: vec4<f32>,
    // x = bevel radius, y = seams (0/1), zw = buffer size (width, height).
    shape: vec4<f32>,
    // Up to three distant lights: xyz = unit direction toward the light,
    // w = cosine of the cone half-angle.
    light_dir: array<vec4<f32>, 3>,
    // rgb = irradiance at normal incidence, w = cone solid angle.
    light_irr: array<vec4<f32>, 3>,
    // rgb = horizon (uniform) sky radiance, w = sky mode (0 uniform,
    // 1 gradient, 2 image).
    sky: vec4<f32>,
    // rgb = zenith radiance, w = image rotation (radians).
    sky_zenith: vec4<f32>,
    // rgb = ground albedo, w = flags (1 ground, 2 shadow catcher).
    ground: vec4<f32>,
    // rgb = background bottom color (sRGB), w = mode (0 color, 1 gradient,
    // 2 sky, 3 transparent).
    background: vec4<f32>,
    // rgb = background top color (sRGB).
    background_top: vec4<f32>,
    // rgb = fog single-scattering albedo, w = extinction per voxel.
    fog: vec4<f32>,
    // Max diffuse, specular and transmission bounces; w = Russian roulette.
    bounces: vec4<f32>,
    // x = firefly clamp (0 off), y = light count, z = fog range, w = denoise.
    misc: vec4<f32>,
    // rgb = ground irradiance with nothing in the way (shadow catcher).
    unoccluded: vec4<f32>,
    // x = exposure multiplier, y = tone map (0 linear, 1 ACES, 2 filmic),
    // z = 1 / gamma, w = vignette.
    film: vec4<f32>,
    // x = bloom on, y = threshold, z = strength, w = samples accumulated.
    bloom: vec4<f32>,
    // rgb = outline color (sRGB), w = outline width in px (0 = off).
    edges: vec4<f32>,
    // rgb = voxel line color, w = line width in voxels (0 = off).
    voxel_lines: vec4<f32>,
    // rgb = floor grid line color, w = line width in voxels (0 = off).
    floor_grid: vec4<f32>,
    // x = voxel line opacity, y = floor grid spacing (voxels), z = floor
    // grid fade radius (0 = no fade), w = floor grid glow.
    lines: vec4<f32>,
    // x = angle between neighboring pixels' rays (perspective), y = width
    // of a pixel in world units (orthographic), zw = XZ center of the floor
    // grid fade.
    footprint: vec4<f32>,
};

@group(0) @binding(0) var<uniform> params: Params;

const PI: f32 = 3.14159265358979;
const MISS_DEPTH: f32 = 1e9;

fn luminance(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

fn buffer_size() -> vec2<u32> {
    return vec2<u32>(params.shape.zw);
}

fn pixel_index(p: vec2<u32>) -> u32 {
    return p.y * buffer_size().x + p.x;
}

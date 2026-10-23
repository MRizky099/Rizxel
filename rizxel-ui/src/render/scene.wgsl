// Rizxel viewport scene shaders (Phase 3 grid/overlays, Phase 4 voxels).
//
// Rendered into an offscreen MSAA target; colors are in gamma space to match
// egui's non-sRGB framebuffer (and the CSS colors of the HTML prototype).

struct Camera {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    // xyz = eye position, w = orbit distance.
    eye: vec4<f32>,
    // xy = target size in px, z = view mode (0 Model, 1 Render), w = grid fade distance.
    viewport: vec4<f32>,
    // xy = grid offset on XZ (keeps lines aligned with the model bounds).
    grid_origin: vec4<f32>,
    background: vec4<f32>,
    grid_color: vec4<f32>,
    axis_x_color: vec4<f32>,
    axis_z_color: vec4<f32>,
    // xyz = unit view direction, w = 1 for orthographic.
    forward: vec4<f32>,
    // xyz = center of the grid fade (eye, or orbit target in orthographic).
    focus: vec4<f32>,
};

@group(0) @binding(0) var<uniform> cam: Camera;

/// Clip-space w of the near plane (must match `NEAR` in Rust).
const NEAR_W: f32 = 0.1;
/// Fraction of the eye distance by which overlays are pulled toward the
/// camera so they never z-fight with the ground grid.
const OVERLAY_PULL: f32 = 0.0015;

fn is_ortho() -> bool {
    return cam.forward.w > 0.5;
}

fn pull_to_eye(p: vec3<f32>) -> vec3<f32> {
    if is_ortho() {
        // Parallel rays: pull straight back along the view direction (the
        // far orthographic eye would shift overlays sideways), by the same
        // amount as a perspective eye at the orbit distance.
        return p - cam.forward.xyz * (cam.eye.w * OVERLAY_PULL);
    }
    return p + (cam.eye.xyz - p) * OVERLAY_PULL;
}

// ---------------------------------------------------------------- grid

struct GridOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

@vertex
fn vs_grid(@builtin(vertex_index) i: u32) -> GridOut {
    // Fullscreen triangle.
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    let ndc = uv * 2.0 - 1.0;
    var out: GridOut;
    out.pos = vec4<f32>(ndc, 0.0, 1.0);
    out.ndc = ndc;
    return out;
}

fn unproject(ndc: vec2<f32>, z: f32) -> vec3<f32> {
    let p = cam.inv_view_proj * vec4<f32>(ndc, z, 1.0);
    return p.xyz / p.w;
}

/// Anti-aliased line coverage for a unit grid in `coord` space, about
/// `width_px` pixels wide.
fn grid_coverage(coord: vec2<f32>, width_px: f32) -> f32 {
    let d = max(fwidth(coord), vec2<f32>(1e-6));
    let g = abs(fract(coord - 0.5) - 0.5) / d;
    return 1.0 - min(min(g.x, g.y) / width_px, 1.0);
}

fn axis_coverage(v: f32, width_px: f32) -> f32 {
    return 1.0 - min(abs(v) / max(fwidth(v), 1e-6) / width_px, 1.0);
}

struct GridFrag {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
};

/// Strength of the grid lines seen from below the ground, where they are
/// drawn over the model (see `fs_grid_under`).
const UNDER_GROUND_STRENGTH: f32 = 0.5;

/// The ground grid at one pixel: line weights (already faded) and depth.
struct GridSample {
    lines: f32,
    axis_x: f32,
    axis_z: f32,
    depth: f32,
    hit: bool,
};

fn grid_sample(ndc: vec2<f32>) -> GridSample {
    // Intersect the pixel's view ray with the ground plane y = 0.
    let near = unproject(ndc, 0.0);
    let far = unproject(ndc, 1.0);
    let dir = far - near;
    let safe_y = select(dir.y, -1e-6, abs(dir.y) < 1e-6);
    let t = -near.y / safe_y;
    let hit = t > 0.0;
    let p = near + dir * clamp(t, 0.0, 1.0);

    // Derivatives are evaluated unconditionally (uniform control flow).
    let coord = p.xz - cam.grid_origin.xy;
    let lod = max(fwidth(coord).x, fwidth(coord).y);
    let minor = grid_coverage(coord, 1.0) * (1.0 - smoothstep(0.12, 0.45, lod));
    let major = grid_coverage(coord / 8.0, 1.2) * (1.0 - smoothstep(0.12, 0.45, lod / 8.0));
    let axis_x = axis_coverage(p.z, 1.6);
    let axis_z = axis_coverage(p.x, 1.6);

    // "Infinity" look: fade with distance and at grazing angles.
    let dist = length(p.xz - cam.focus.xz);
    let fade_far = cam.viewport.w;
    var fade = 1.0 - smoothstep(fade_far * 0.25, fade_far, dist);
    fade *= smoothstep(0.0, 0.12, abs(normalize(dir).y));
    fade *= f32(hit);
    fade *= select(1.0, 0.45, cam.viewport.z > 0.5);

    let clip = cam.view_proj * vec4<f32>(p, 1.0);
    let ground_depth = clamp(clip.z / max(clip.w, 1e-6), 0.0, 1.0);
    let on_ground = hit && clip.w > NEAR_W;

    var s: GridSample;
    s.lines = max(minor * 0.32, major * 0.6) * cam.grid_color.a * fade;
    s.axis_x = axis_x * 0.85 * fade;
    s.axis_z = axis_z * 0.85 * fade;
    s.depth = select(1.0, ground_depth, on_ground);
    s.hit = on_ground;
    return s;
}

/// The camera is under the ground plane: the ground lies between the eye
/// and the model, so it must not occlude anything.
fn eye_below_ground() -> bool {
    return cam.eye.y < 0.0;
}

/// Background and ground grid, drawn first. Writes the ground's depth so the
/// voxels are ordered against it, except from below the ground, where it is
/// background only and `fs_grid_under` draws the lines after the voxels.
@fragment
fn fs_grid(in: GridOut) -> GridFrag {
    let s = grid_sample(in.ndc);
    let above = !eye_below_ground();

    var bg = cam.background.rgb;
    if cam.viewport.z > 0.5 {
        // Soft vertical studio gradient.
        bg = mix(bg * 0.55, bg * 1.3, in.ndc.y * 0.5 + 0.5);
    }

    let lines = f32(above);
    var color = bg;
    color = mix(color, cam.grid_color.rgb, s.lines * lines);
    color = mix(color, cam.axis_x_color.rgb, s.axis_x * lines);
    color = mix(color, cam.axis_z_color.rgb, s.axis_z * lines);

    var out: GridFrag;
    out.color = vec4<f32>(color, 1.0);
    out.depth = select(1.0, s.depth, above && s.hit);
    return out;
}

/// Ground grid lines seen from below, drawn after the voxels as a faded,
/// premultiplied overlay that does not write depth.
@fragment
fn fs_grid_under(in: GridOut) -> GridFrag {
    let s = grid_sample(in.ndc);
    let k = UNDER_GROUND_STRENGTH;

    // Same layer order as `fs_grid`, composited onto transparent black.
    var rgb = vec3<f32>(0.0);
    var a = 0.0;
    for (var i = 0; i < 3; i++) {
        var c = cam.grid_color.rgb;
        var w = s.lines;
        if i == 1 {
            c = cam.axis_x_color.rgb;
            w = s.axis_x;
        } else if i == 2 {
            c = cam.axis_z_color.rgb;
            w = s.axis_z;
        }
        w *= k;
        rgb = mix(rgb, c, w);
        a = mix(a, 1.0, w);
    }

    var out: GridFrag;
    out.color = vec4<f32>(rgb, a);
    out.depth = s.depth;
    return out;
}

// ---------------------------------------------------------------- thick lines

struct LineIn {
    @location(0) a: vec3<f32>,
    @location(1) b: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) width: f32,
};

struct ColorOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec4<f32>,
};

/// One instance per segment, expanded to a screen-space quad (6 vertices)
/// `width` pixels wide. Segments are clipped against the near plane.
@vertex
fn vs_line(@builtin(vertex_index) vi: u32, line: LineIn) -> ColorOut {
    var ca = cam.view_proj * vec4<f32>(pull_to_eye(line.a), 1.0);
    var cb = cam.view_proj * vec4<f32>(pull_to_eye(line.b), 1.0);

    var out: ColorOut;
    out.color = line.color;
    if ca.w < NEAR_W && cb.w < NEAR_W {
        // Fully behind the camera: emit a vertex outside the clip volume.
        out.pos = vec4<f32>(0.0, 0.0, 2.0, 1.0);
        return out;
    }
    if ca.w < NEAR_W {
        ca = mix(ca, cb, (NEAR_W - ca.w) / (cb.w - ca.w));
    }
    if cb.w < NEAR_W {
        cb = mix(cb, ca, (NEAR_W - cb.w) / (ca.w - cb.w));
    }

    let half_px = cam.viewport.xy * 0.5;
    let sa = ca.xy / ca.w * half_px;
    let sb = cb.xy / cb.w * half_px;
    var dir = sb - sa;
    let len = length(dir);
    dir = select(vec2<f32>(1.0, 0.0), dir / len, len > 1e-5);
    let normal = vec2<f32>(-dir.y, dir.x) * line.width * 0.5;

    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(0.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(0.0, 1.0),
    );
    let c = corners[vi];
    let base = select(ca, cb, c.x > 0.5);
    let offset_ndc = normal * c.y / half_px;
    out.pos = vec4<f32>(base.xy + offset_ndc * base.w, base.z, base.w);
    return out;
}

@fragment
fn fs_color(in: ColorOut) -> @location(0) vec4<f32> {
    return in.color;
}

// ---------------------------------------------------------------- filled triangles

struct FillIn {
    @location(0) pos: vec3<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_fill(v: FillIn) -> ColorOut {
    var out: ColorOut;
    out.pos = cam.view_proj * vec4<f32>(pull_to_eye(v.pos), 1.0);
    out.color = v.color;
    return out;
}

// ---------------------------------------------------------------- voxels

struct VoxelIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
};

struct VoxelOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) color: vec4<f32>,
};

/// Key light: above, slightly front-right of the default camera, so the top
/// face is brightest and the two visible sides read differently.
const KEY_LIGHT: vec3<f32> = vec3<f32>(0.4, 0.85, 0.35);
const AMBIENT: f32 = 0.35;

@vertex
fn vs_voxel(v: VoxelIn) -> VoxelOut {
    var out: VoxelOut;
    out.pos = cam.view_proj * vec4<f32>(v.pos, 1.0);
    out.normal = v.normal;
    out.color = v.color;
    return out;
}

@fragment
fn fs_voxel(in: VoxelOut) -> @location(0) vec4<f32> {
    let n = normalize(in.normal);
    var shade: f32;
    if cam.viewport.z > 0.5 {
        // Render: Lambert key light + ambient + a half-Lambert fill so faces
        // turned away from the light never go fully black.
        let ndl = dot(n, normalize(KEY_LIGHT));
        let lambert = max(ndl, 0.0);
        let half_lambert = ndl * 0.5 + 0.5;
        shade = AMBIENT + 0.6 * lambert + 0.15 * half_lambert * half_lambert;
    } else {
        // Model: palette colors kept almost flat, with a fixed per-axis tint
        // so adjacent faces of one color stay distinguishable.
        shade = 1.0 - 0.08 * abs(n.x) - 0.16 * abs(n.z) - select(0.0, 0.3, n.y < -0.5);
    }
    return vec4<f32>(min(in.color.rgb * shade, vec3<f32>(1.0)), 1.0);
}

// Rizxel viewport scene shaders (Phase 3).
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
};

@group(0) @binding(0) var<uniform> cam: Camera;

/// Clip-space w of the near plane (must match `NEAR` in Rust).
const NEAR_W: f32 = 0.1;
/// Fraction of the eye distance by which overlays are pulled toward the
/// camera so they never z-fight with the ground grid.
const OVERLAY_PULL: f32 = 0.0015;

fn pull_to_eye(p: vec3<f32>) -> vec3<f32> {
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

@fragment
fn fs_grid(in: GridOut) -> GridFrag {
    // Intersect the pixel's view ray with the ground plane y = 0.
    let near = unproject(in.ndc, 0.0);
    let far = unproject(in.ndc, 1.0);
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
    let dist = length(p.xz - cam.eye.xz);
    let fade_far = cam.viewport.w;
    var fade = 1.0 - smoothstep(fade_far * 0.25, fade_far, dist);
    fade *= smoothstep(0.0, 0.12, abs(normalize(dir).y));
    fade *= f32(hit);

    let render_mode = cam.viewport.z > 0.5;
    var bg = cam.background.rgb;
    if render_mode {
        // Soft vertical studio gradient.
        bg = mix(bg * 0.55, bg * 1.3, in.ndc.y * 0.5 + 0.5);
    }
    let strength = select(1.0, 0.45, render_mode);

    var color = bg;
    color = mix(color, cam.grid_color.rgb, max(minor * 0.32, major * 0.6) * cam.grid_color.a * fade * strength);
    color = mix(color, cam.axis_x_color.rgb, axis_x * 0.85 * fade * strength);
    color = mix(color, cam.axis_z_color.rgb, axis_z * 0.85 * fade * strength);

    let clip = cam.view_proj * vec4<f32>(p, 1.0);
    let ground_depth = clamp(clip.z / max(clip.w, 1e-6), 0.0, 1.0);

    var out: GridFrag;
    out.color = vec4<f32>(color, 1.0);
    out.depth = select(1.0, ground_depth, hit && clip.w > NEAR_W);
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

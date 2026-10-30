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
    // xy = grid offset on XZ (keeps lines aligned with the model bounds),
    // zw = NDC y scale and offset to the full image (tiled output).
    grid_origin: vec4<f32>,
    background: vec4<f32>,
    grid_color: vec4<f32>,
    axis_x_color: vec4<f32>,
    axis_z_color: vec4<f32>,
    // xyz = unit view direction, w = 1 for orthographic.
    forward: vec4<f32>,
    // xyz = center of the grid fade (eye, or orbit target in orthographic).
    focus: vec4<f32>,
    // View toggles (0 / 1): x = floor grid, y = voxel grid, z = ambient occlusion,
    // w = transparent background.
    options: vec4<f32>,
    // Render mode key light: xyz = unit direction toward it, w = strength.
    light: vec4<f32>,
    // rgb = key light color.
    light_color: vec4<f32>,
    // rgb = ground color, w = 1 when the ground is drawn (Render mode).
    ground: vec4<f32>,
    // rgb = top color of the background gradient, w = 1 for a gradient.
    background_top: vec4<f32>,
    // x = voxel grid width (px), y = voxel grid opacity, z = floor grid width (px),
    // w = 1 when the plane sheets show cell lines.
    grid_style: vec4<f32>,
    // x = voxel grid width in voxel units (0: pixels, grid_style.x).
    voxel_units: vec4<f32>,
    // Render mode voxel lines: rgb = color, w = opacity.
    voxel_lines: vec4<f32>,
    // Render mode floor grid: rgb = color, w = glow.
    floor_lines: vec4<f32>,
    // x = voxel line width in voxels (0 = off), y = floor grid width in voxels
    // (0 = off), z = floor grid spacing, w = floor grid fade radius (0 = none).
    line_style: vec4<f32>,
    // xy = XZ center of the floor grid fade, zw = XZ of a floor line crossing.
    floor_center: vec4<f32>,
};

@group(0) @binding(0) var<uniform> cam: Camera;

/// Material table entry (Render mode). Must match `GpuMaterial` in Rust.
struct Material {
    // RGBA multiplier of the palette color.
    tint: vec4<f32>,
    // rgb = emission.
    emission: vec4<f32>,
    // x = metallic, y = roughness.
    params: vec4<f32>,
};

/// Slot 0 is the default material, slot n the document's material n - 1.
@group(0) @binding(1) var<uniform> materials: array<Material, 256>;

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

/// Coverage of a line `width_px` pixels wide at `px` pixels from its
/// center: a smooth 1 px falloff on each side. Lines narrower than a pixel
/// keep a 1 px footprint and lose opacity instead.
fn line_coverage(px: f32, width_px: f32) -> f32 {
    let w = max(width_px, 1.0);
    return saturate(w * 0.5 + 0.5 - px) * saturate(width_px);
}

/// Anti-aliased line coverage for a unit grid in `coord` space, about
/// `width_px` pixels wide.
fn grid_coverage(coord: vec2<f32>, width_px: f32) -> f32 {
    let d = max(fwidth(coord), vec2<f32>(1e-6));
    let g = abs(fract(coord - 0.5) - 0.5) / d;
    return line_coverage(min(g.x, g.y), width_px);
}

fn axis_coverage(v: f32, width_px: f32) -> f32 {
    return line_coverage(abs(v) / max(fwidth(v), 1e-6), width_px);
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
    // Distance and grazing-angle fade of the ground (1 = near).
    fade: f32,
    // Render mode floor grid line coverage, faded (Phase 12e).
    floor: f32,
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
    let w = cam.grid_style.z;
    let minor = grid_coverage(coord, w) * (1.0 - smoothstep(0.12, 0.45, lod));
    let major = grid_coverage(coord / 8.0, 1.2 * w) * (1.0 - smoothstep(0.12, 0.45, lod / 8.0));
    let axis_x = axis_coverage(p.z, 1.6 * w);
    let axis_z = axis_coverage(p.x, 1.6 * w);
    let floor = floor_grid_coverage(p);

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
    s.fade = fade / select(1.0, 0.45, cam.viewport.z > 0.5);
    s.floor = floor;
    return s;
}

/// Render mode floor grid at ground point `p`: lines `line_style.y` voxels
/// wide every `line_style.z` voxels through `floor_center.zw`, faded out
/// toward `line_style.w` from the model's center. Needs uniform control
/// flow (derivatives).
fn floor_grid_coverage(p: vec3<f32>) -> f32 {
    let spacing = max(cam.line_style.z, 1.0);
    let coord = (p.xz - cam.floor_center.zw) / spacing;
    let d = max(fwidth(coord), vec2<f32>(1e-6));
    let g = abs(fract(coord - 0.5) - 0.5) / d;
    // Width in grid cells; one axis covers this share of a cell on average.
    let units = cam.line_style.y / spacing;
    let average = saturate(units);
    let cx = unit_line_coverage(vec2<f32>(g.x, d.x), units, average);
    let cz = unit_line_coverage(vec2<f32>(g.y, d.y), units, average);
    var k = 1.0 - (1.0 - cx) * (1.0 - cz);
    let radius = cam.line_style.w;
    if radius > 0.0 {
        k *= 1.0 - smoothstep(0.25 * radius, radius, length(p.xz - cam.floor_center.xy));
    }
    return k * f32(cam.line_style.y > 0.0);
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
    let render = cam.viewport.z > 0.5;
    if render && cam.background_top.w > 0.5 {
        // Screen-space gradient of the whole image (tiles map their NDC y).
        let y = in.ndc.y * cam.grid_origin.z + cam.grid_origin.w;
        bg = mix(bg, cam.background_top.rgb, clamp(y * 0.5 + 0.5, 0.0, 1.0));
    }
    // Render mode ground: lit by the key light, fading into the background.
    var ground = 0.0;
    if render && cam.ground.w > 0.5 && above && s.hit {
        ground = s.fade;
        var lit = cam.ground.rgb * mix(0.7, 1.0, clamp(cam.light.y, 0.0, 1.0));
        // Floor grid lines (their glow only lights the path tracer).
        lit = mix(lit, cam.floor_lines.rgb, s.floor);
        bg = mix(bg, lit, ground);
    }

    // Hidden floor grid: background only.
    let lines = f32(above) * cam.options.x;
    var color = bg;
    color = mix(color, cam.grid_color.rgb, s.lines * lines);
    color = mix(color, cam.axis_x_color.rgb, s.axis_x * lines);
    color = mix(color, cam.axis_z_color.rgb, s.axis_z * lines);

    var out: GridFrag;
    out.color = vec4<f32>(color, 1.0);
    if cam.options.w > 0.5 {
        // Transparent output: only the voxels (premultiplied by the resolve).
        out.color = vec4<f32>(0.0);
    }
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

// ---------------------------------------------------------------- plane sheets

struct PlaneIn {
    @location(0) pos: vec3<f32>,
    @location(1) fill: vec4<f32>,
    @location(2) line: vec4<f32>,
    // Normal axis of the sheet: 0 = X, 1 = Y, 2 = Z.
    @location(3) axis: u32,
};

struct PlaneOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) fill: vec4<f32>,
    @location(2) line: vec4<f32>,
    @location(3) @interpolate(flat) axis: u32,
};

@vertex
fn vs_plane(v: PlaneIn) -> PlaneOut {
    var out: PlaneOut;
    out.pos = cam.view_proj * vec4<f32>(pull_to_eye(v.pos), 1.0);
    // The cell lines use the unpulled position, so they stay on the voxel
    // boundaries.
    out.world = v.pos;
    out.fill = v.fill;
    out.line = v.line;
    out.axis = v.axis;
    return out;
}

/// Distance in px to the nearest voxel boundary on the two axes of a plane
/// with normal axis `axis` (x), and the voxel size in world units per px (y).
fn cell_distance(world: vec3<f32>, axis: u32) -> vec2<f32> {
    // Voxel corners sit on integers once the bounds' XZ offset is removed.
    let coord = world - vec3<f32>(cam.grid_origin.x, 0.0, cam.grid_origin.y);
    // Derivatives first, in uniform control flow, per axis so a face edge
    // inside a 2x2 pixel quad does not blow them up.
    let d = max(fwidth(coord), vec3<f32>(1e-6));
    let g = abs(fract(coord - 0.5) - 0.5) / d;
    if axis == 1u {
        return vec2<f32>(min(g.x, g.z), max(d.x, d.z));
    } else if axis == 2u {
        return vec2<f32>(min(g.x, g.y), max(d.x, d.y));
    }
    return vec2<f32>(min(g.y, g.z), max(d.y, d.z));
}

/// Voxels under ~4 px: the lines fade out instead of a solid tint.
fn cell_lod_fade(lod: f32) -> f32 {
    return 1.0 - smoothstep(0.15, 0.3, lod);
}

/// The sheet's fill with the voxel cell lines composited over it (straight
/// alpha). Without the voxel grid the sheet is the plain fill.
@fragment
fn fs_plane(in: PlaneOut) -> @location(0) vec4<f32> {
    let c = cell_distance(in.world, in.axis);
    let k = line_coverage(c.x, cam.grid_style.x) * cell_lod_fade(c.y) * cam.grid_style.y
        * cam.grid_style.w * in.line.a;
    let a = in.fill.a + (1.0 - in.fill.a) * k;
    let rgb = (in.fill.rgb * in.fill.a * (1.0 - k) + in.line.rgb * k) / max(a, 1e-6);
    return vec4<f32>(rgb, a);
}

// ---------------------------------------------------------------- voxels

struct VoxelIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) material: u32,
    @location(4) ao: f32,
};

struct VoxelOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) color: vec4<f32>,
    @location(2) world: vec3<f32>,
    // Layer opacity (1 in the opaque pass).
    @location(3) alpha: f32,
    @location(4) @interpolate(flat) material: u32,
    // 0 = most occluded, 1 = open (interpolated across the quad).
    @location(5) ao: f32,
};

const AMBIENT: f32 = 0.35;

fn voxel_out(v: VoxelIn, alpha: f32) -> VoxelOut {
    var out: VoxelOut;
    out.pos = cam.view_proj * vec4<f32>(v.pos, 1.0);
    out.normal = v.normal;
    out.color = v.color;
    out.world = v.pos;
    out.alpha = alpha;
    out.material = min(v.material, 255u);
    out.ao = v.ao;
    return out;
}

@vertex
fn vs_voxel(v: VoxelIn) -> VoxelOut {
    return voxel_out(v, 1.0);
}

/// Translucent layers: one instance per layer, its alpha in an
/// instance-rate attribute so opacity changes never touch the mesh.
@vertex
fn vs_voxel_translucent(v: VoxelIn, @location(7) alpha: f32) -> VoxelOut {
    return voxel_out(v, alpha);
}

/// Brightness of a fully occluded vertex.
const AO_MIN: f32 = 0.5;

/// Thinnest voxel line in px when the width is in voxel units: thinner
/// lines lose opacity instead, so they never break up.
const MIN_VOXEL_LINE_PX: f32 = 0.75;

/// Coverage of a line `units` cells wide at `c.x` px from its center, where
/// a cell is `1 / c.y` px. Lines under `MIN_VOXEL_LINE_PX` keep that width
/// and lose opacity instead, so they never break up; when cells get smaller
/// than a few px the coverage blends to `average`, the line's share of a
/// cell, so far lines fade toward their average tint instead of aliasing.
fn unit_line_coverage(c: vec2<f32>, units: f32, average: f32) -> f32 {
    let width_px = units / c.y;
    let thin = saturate(width_px / MIN_VOXEL_LINE_PX);
    let line = line_coverage(c.x, max(width_px, MIN_VOXEL_LINE_PX)) * thin;
    return mix(line, average, smoothstep(0.15, 0.3, c.y));
}

/// Plane axis (0 = x, 1 = y, 2 = z) of a face with normal `n`.
fn face_axis(n: vec3<f32>) -> u32 {
    let a = abs(n);
    if a.y > 0.5 {
        return 1u;
    } else if a.z > 0.5 {
        return 2u;
    }
    return 0u;
}

/// Share of a voxel face both axes' lines of width `w` cover.
fn face_line_average(w: f32) -> f32 {
    let k = saturate(w);
    return 1.0 - (1.0 - k) * (1.0 - k);
}

/// Render mode voxel lines (Phase 12e): coverage of the line around every
/// voxel face at a fragment on a face with normal `n`, opacity included.
/// The width is in voxels, so the preview and a large output match.
fn voxel_line_coverage(world: vec3<f32>, n: vec3<f32>) -> f32 {
    let c = cell_distance(world, face_axis(n));
    let units = cam.line_style.x;
    return unit_line_coverage(c, units, face_line_average(units)) * cam.voxel_lines.w;
}

/// Darkening by the voxel edge lines at a fragment on a face with normal
/// `n` (Voxel Grid), opacity included. In pixels (`grid_style.x`) the lines
/// fade out when voxels get smaller than a few pixels; in voxel units
/// (`voxel_units.x`) they fade toward their average tint instead, so a
/// large output keeps the look. Works on greedy-merged quads because it
/// only uses the position.
fn voxel_grid_coverage(world: vec3<f32>, n: vec3<f32>) -> f32 {
    let c = cell_distance(world, face_axis(n));
    let units = cam.voxel_units.x;
    if units > 0.0 {
        return unit_line_coverage(c, units, face_line_average(units)) * cam.grid_style.y;
    }
    return line_coverage(c.x, cam.grid_style.x) * cell_lod_fade(c.y) * cam.grid_style.y;
}

/// Unit vector from a fragment toward the viewer.
fn view_dir(world: vec3<f32>) -> vec3<f32> {
    if is_ortho() {
        return -cam.forward.xyz;
    }
    return normalize(cam.eye.xyz - world);
}

/// Render mode: palette color times the material tint, lit by the key light
/// with a Blinn-Phong highlight (roughness sets its size, metallic tints it
/// and darkens the diffuse part, Schlick Fresnel), plus emission.
fn render_shade(albedo: vec3<f32>, n: vec3<f32>, world: vec3<f32>, m: Material) -> vec3<f32> {
    // Key light from the render settings (sun direction, color, strength).
    let l = normalize(cam.light.xyz);
    let k = cam.light.w * cam.light_color.rgb;
    let v = view_dir(world);
    let h = normalize(l + v);
    let metallic = m.params.x;
    let roughness = clamp(m.params.y, 0.02, 1.0);

    // Lambert key light + ambient + a half-Lambert fill so faces turned away
    // from the light never go fully black.
    let ndl = dot(n, l);
    let lambert = max(ndl, 0.0);
    let half_lambert = ndl * 0.5 + 0.5;
    let light = AMBIENT + (0.6 * lambert + 0.15 * half_lambert * half_lambert) * k;
    let diffuse = albedo * light * (1.0 - 0.75 * metallic);

    let shininess = mix(512.0, 4.0, roughness);
    let f0 = mix(vec3<f32>(0.04), albedo, metallic);
    let fresnel = f0 + (vec3<f32>(1.0) - f0) * pow(1.0 - max(dot(h, v), 0.0), 5.0);
    // Normalized so rough highlights get wider, not brighter.
    let spec = pow(max(dot(n, h), 0.0), shininess) * (shininess + 8.0) / 64.0;
    let specular = fresnel * spec * lambert * (1.0 - 0.6 * roughness) * k;

    return diffuse + specular + m.emission.rgb * m.tint.rgb;
}

/// Lit color of a voxel fragment (shared by both passes).
fn voxel_color(in: VoxelOut) -> vec3<f32> {
    let n = normalize(in.normal);
    let grid = voxel_grid_coverage(in.world, n) * cam.options.y;
    let lines = voxel_line_coverage(in.world, n);
    var color: vec3<f32>;
    if cam.viewport.z > 0.5 {
        let m = materials[in.material];
        let albedo = mix(in.color.rgb * m.tint.rgb, cam.voxel_lines.rgb, lines);
        color = render_shade(albedo, n, in.world, m);
    } else {
        // Model: palette colors kept almost flat, with a fixed per-axis tint
        // so adjacent faces of one color stay distinguishable.
        let shade = 1.0 - 0.08 * abs(n.x) - 0.16 * abs(n.z) - select(0.0, 0.3, n.y < -0.5);
        color = in.color.rgb * shade;
    }
    // Ambient occlusion darkens corners and creases, never below half.
    let ao = mix(1.0, AO_MIN + (1.0 - AO_MIN) * in.ao, cam.options.z);
    return min(color * ao * (1.0 - grid), vec3<f32>(1.0));
}

@fragment
fn fs_voxel(in: VoxelOut) -> @location(0) vec4<f32> {
    return vec4<f32>(voxel_color(in), 1.0);
}

/// Straight alpha, blended over what the opaque pass drew.
@fragment
fn fs_voxel_translucent(in: VoxelOut) -> @location(0) vec4<f32> {
    return vec4<f32>(voxel_color(in), in.alpha);
}

// ---------------------------------------------------------------- reference images

@group(1) @binding(0) var ref_texture: texture_2d<f32>;
@group(1) @binding(1) var ref_sampler: sampler;

struct RefIn {
    @location(0) pos: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) opacity: f32,
};

struct RefOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) opacity: f32,
};

@vertex
fn vs_reference(v: RefIn) -> RefOut {
    var out: RefOut;
    // Pulled toward the eye like the overlays, so a floor image never
    // z-fights with the ground grid.
    out.pos = cam.view_proj * vec4<f32>(pull_to_eye(v.pos), 1.0);
    out.uv = v.uv;
    out.opacity = v.opacity;
    return out;
}

/// Straight alpha. The back side shows the image mirrored (as seen through
/// it) and dimmed.
@fragment
fn fs_reference(in: RefOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let c = textureSample(ref_texture, ref_sampler, in.uv);
    let dim = select(0.55, 1.0, front);
    return vec4<f32>(c.rgb * dim, c.a * in.opacity);
}

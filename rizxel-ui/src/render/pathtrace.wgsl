// Rizxel progressive path tracer (Phase 11b). Appended to pt_common.wgsl.
//
// Scene: a 3D texture of (palette index, material slot) per voxel plus an
// occupancy texture of 8³ bricks. Rays walk the bricks and, inside
// occupied ones, the voxels with an Amanatides–Woo DDA (the GPU twin of
// `raycast_grid`); no BVH. Each solid cell is intersected with the render
// shape (cube, rounded brick with stud, rounded cube, sphere, cylinder).
//
// Shading: Lambert diffuse + GGX specular (Schlick Fresnel, VNDF sampling),
// metallic workflow, a clear coat, dielectric transmission and emission.
// Next-event estimation to up to three soft distant lights with MIS;
// the sky is reached by BSDF sampling. Optional homogeneous fog.
//
// Every dispatch adds `frame.y` samples per pixel to the accumulation
// buffers: radiance + coverage, the background multiplier (shadow catcher)
// and the denoiser guides (albedo, normal, depth).

@group(0) @binding(1) var voxels: texture_3d<u32>;
@group(0) @binding(2) var bricks: texture_3d<u32>;
@group(0) @binding(3) var<uniform> palette: array<vec4<f32>, 256>;

struct Material {
    // rgb = tint (linear), a = opacity.
    tint: vec4<f32>,
    // rgb = emission (linear, times strength).
    emission: vec4<f32>,
    // x = metallic, y = roughness, z = specular, w = ior.
    params: vec4<f32>,
    // x = transmission, y = clear coat.
    params2: vec4<f32>,
};

@group(0) @binding(4) var<uniform> materials: array<Material, 256>;
// rgb = radiance sum of covered samples, a = coverage sum.
@group(0) @binding(5) var<storage, read_write> accum: array<vec4<f32>>;
// rgb = background multiplier sum, a = background sample count.
@group(0) @binding(6) var<storage, read_write> aux: array<vec4<f32>>;
// rgb = first-hit albedo sum, a = hit count.
@group(0) @binding(7) var<storage, read_write> albedo_sum: array<vec4<f32>>;
// xyz = first-hit normal sum, w = view depth sum.
@group(0) @binding(8) var<storage, read_write> normal_depth: array<vec4<f32>>;
@group(0) @binding(9) var sky_image: texture_2d<f32>;

// ------------------------------------------------------------------ random

var<private> rng: u32;
var<private> pixel_rot: vec4<f32>;
var<private> sample_index: u32;
// DDA steps (cells and bricks) of the last `trace_voxels` (tests).
var<private> trace_steps: i32;

fn pcg(v: u32) -> u32 {
    let state = v * 747796405u + 2891336453u;
    let word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    return (word >> 22u) ^ word;
}

fn rand() -> f32 {
    rng = pcg(rng);
    return f32(rng >> 8u) * (1.0 / 16777216.0);
}

/// R2 low-discrepancy point `sample_index`, rotated per pixel
/// (Cranley–Patterson) so neighbouring pixels decorrelate.
fn r2(offset: vec2<f32>) -> vec2<f32> {
    let n = f32(sample_index);
    return fract(vec2<f32>(0.5 + n * 0.7548776662466927, 0.5 + n * 0.5698402909980532) + offset);
}

// ------------------------------------------------------------------ grid

fn dims() -> vec3<i32> {
    return vec3<i32>(params.dims.xyz);
}

fn cell_value(c: vec3<i32>) -> vec2<u32> {
    if any(c < vec3<i32>(0)) || any(c >= dims()) {
        return vec2<u32>(0u);
    }
    return textureLoad(voxels, c, 0).rg;
}

fn solid(c: vec3<i32>) -> bool {
    return cell_value(c).x != 0u;
}

fn brick_occupied(b: vec3<i32>) -> bool {
    let size = vec3<i32>(textureDimensions(bricks));
    if any(b < vec3<i32>(0)) || any(b >= size) {
        return false;
    }
    return textureLoad(bricks, b, 0).r != 0u;
}

fn shape_kind() -> u32 {
    return u32(params.grid.w);
}

fn safe_inv(d: vec3<f32>) -> vec3<f32> {
    return 1.0 / select(d, vec3<f32>(1e-12), abs(d) < vec3<f32>(1e-12));
}

/// Slab test: (t_near, t_far, entry axis).
fn slab(ro: vec3<f32>, inv: vec3<f32>, lo: vec3<f32>, hi: vec3<f32>) -> vec3<f32> {
    let t0 = (lo - ro) * inv;
    let t1 = (hi - ro) * inv;
    let tn = min(t0, t1);
    let tf = max(t0, t1);
    var axis = 0.0;
    var near = tn.x;
    if tn.y > near {
        near = tn.y;
        axis = 1.0;
    }
    if tn.z > near {
        near = tn.z;
        axis = 2.0;
    }
    return vec3<f32>(near, min(tf.x, min(tf.y, tf.z)), axis);
}

fn axis_vec(axis: i32) -> vec3<f32> {
    var v = vec3<f32>(0.0);
    v[axis] = 1.0;
    return v;
}

// ------------------------------------------------------------------ shapes

struct ShapeHit {
    t: f32,
    n: vec3<f32>,
    // 1 when the hit is a stud.
    stud: u32,
};

fn no_hit() -> ShapeHit {
    return ShapeHit(-1.0, vec3<f32>(0.0), 0u);
}

/// Integer vectors step with `step * vec3<i32>(mask)`, not `select`: GLSL
/// before 4.50 (the GL backend on older GPUs) has no integer `mix`.
/// Unit mask of the smallest component (ties pick the lowest axis).
fn min_mask(v: vec3<f32>) -> vec3<bool> {
    let x = v.x <= v.y && v.x <= v.z;
    let y = !x && v.y <= v.z;
    return vec3<bool>(x, y, !x && !y);
}

/// Unit mask of the largest component (ties pick the lowest axis).
fn max_mask(v: vec3<f32>) -> vec3<bool> {
    let x = v.x >= v.y && v.x >= v.z;
    let y = !x && v.y >= v.z;
    return vec3<bool>(x, y, !x && !y);
}

/// Entry and exit distances of the box `lo..hi` and the entry face normal
/// (`near > far` = miss).
struct BoxEntry {
    near: f32,
    far: f32,
    n: vec3<f32>,
};

fn box_entry(ro: vec3<f32>, rd: vec3<f32>, lo: vec3<f32>, hi: vec3<f32>) -> BoxEntry {
    let inv = safe_inv(rd);
    let t0 = (lo - ro) * inv;
    let t1 = (hi - ro) * inv;
    let tn = min(t0, t1);
    let tf = max(t0, t1);
    let n = select(vec3<f32>(0.0), -sign(rd), max_mask(tn));
    return BoxEntry(max(tn.x, max(tn.y, tn.z)), min(tf.x, min(tf.y, tf.z)), n);
}

/// Cell-centred box of half size `h`.
fn box_hit(ro: vec3<f32>, rd: vec3<f32>, center: vec3<f32>, h: f32, t_min: f32) -> ShapeHit {
    let e = box_entry(ro, rd, center - vec3<f32>(h), center + vec3<f32>(h));
    if e.near > e.far || e.near < t_min {
        return no_hit();
    }
    return ShapeHit(e.near, e.n, 0u);
}

fn sd_round_box(p: vec3<f32>, b: vec3<f32>, r: f32) -> f32 {
    let q = abs(p) - b + vec3<f32>(r);
    return length(max(q, vec3<f32>(0.0))) + min(max(q.x, max(q.y, q.z)), 0.0) - r;
}

fn round_box_normal(p: vec3<f32>, b: vec3<f32>, r: f32) -> vec3<f32> {
    let q = abs(p) - b + vec3<f32>(r);
    let m = max(q, vec3<f32>(0.0));
    if dot(m, m) > 1e-12 {
        return normalize(sign(p) * m);
    }
    // On a flat face: the axis closest to the surface.
    var axis = 0;
    if q.y > q.x && q.y >= q.z {
        axis = 1;
    } else if q.z > q.x && q.z > q.y {
        axis = 2;
    }
    return axis_vec(axis) * sign(p[axis]);
}

/// Rounded box (half size `h`, edge radius `r`) by sphere tracing inside
/// its bounding slab.
fn round_box_hit(
    ro: vec3<f32>,
    rd: vec3<f32>,
    center: vec3<f32>,
    h: f32,
    r: f32,
    t_min: f32,
) -> ShapeHit {
    let e = box_entry(ro, rd, center - vec3<f32>(h), center + vec3<f32>(h));
    if e.near > e.far || e.far < t_min {
        return no_hit();
    }
    let b = vec3<f32>(h);
    // Most hits land on the flat part of a face: exact, no marching.
    if e.near >= t_min {
        let p = abs(ro + rd * e.near - center);
        // In-plane offsets only (the face normal axis is ignored).
        let inside = select(p, vec3<f32>(0.0), e.n != vec3<f32>(0.0));
        if all(inside <= vec3<f32>(h - r)) {
            return ShapeHit(e.near, e.n, 0u);
        }
    }
    let s = vec2<f32>(e.near, e.far);
    var t = max(s.x, t_min);
    // Grazing rays creep along the surface: give up only far from it.
    var closest = 1e9;
    for (var i = 0; i < 40; i++) {
        let p = ro + rd * t - center;
        let d = sd_round_box(p, b, r);
        closest = min(closest, d);
        if d < 2e-4 {
            if t < t_min {
                return no_hit();
            }
            return ShapeHit(t, round_box_normal(p, b, r), 0u);
        }
        t += d;
        if t > s.y {
            return no_hit();
        }
    }
    // Out of steps while hugging the surface: count it as a hit.
    if closest < 2e-3 && t >= t_min {
        let p = ro + rd * t - center;
        return ShapeHit(t, round_box_normal(p, b, r), 0u);
    }
    return no_hit();
}

fn sphere_hit(ro: vec3<f32>, rd: vec3<f32>, center: vec3<f32>, radius: f32, t_min: f32) -> ShapeHit {
    let oc = ro - center;
    let b = dot(oc, rd);
    let c = dot(oc, oc) - radius * radius;
    let disc = b * b - c;
    if disc < 0.0 {
        return no_hit();
    }
    let t = -b - sqrt(disc);
    if t < t_min {
        return no_hit();
    }
    return ShapeHit(t, normalize(oc + rd * t), 0u);
}

/// Vertical cylinder from `y0` to `y1` around the axis through `center`.
fn cylinder_hit(
    ro: vec3<f32>,
    rd: vec3<f32>,
    center: vec3<f32>,
    radius: f32,
    y0: f32,
    y1: f32,
    t_min: f32,
) -> ShapeHit {
    var best = no_hit();
    let o = ro.xz - center.xz;
    let d = rd.xz;
    let a = dot(d, d);
    if a > 1e-12 {
        let b = dot(o, d);
        let c = dot(o, o) - radius * radius;
        let disc = b * b - a * c;
        if disc >= 0.0 {
            let t = (-b - sqrt(disc)) / a;
            let y = ro.y + rd.y * t;
            if t >= t_min && y >= y0 && y <= y1 {
                let p = o + d * t;
                best = ShapeHit(t, normalize(vec3<f32>(p.x, 0.0, p.y)), 0u);
            }
        }
    }
    if abs(rd.y) > 1e-12 {
        for (var i = 0; i < 2; i++) {
            let y = select(y0, y1, i == 1);
            let t = (y - ro.y) / rd.y;
            let p = o + d * t;
            if t >= t_min && dot(p, p) <= radius * radius && (best.t < 0.0 || t < best.t) {
                best = ShapeHit(t, vec3<f32>(0.0, select(-1.0, 1.0, i == 1), 0.0), 0u);
            }
        }
    }
    return best;
}

/// Stud on top of the brick in cell `c`.
fn stud_hit(ro: vec3<f32>, rd: vec3<f32>, c: vec3<i32>, t_min: f32) -> ShapeHit {
    let s = params.dims.w;
    let center = vec3<f32>(c) + vec3<f32>(0.5);
    let top = center.y + 0.5 * s;
    var h = cylinder_hit(ro, rd, center, 0.3 * s, top - 0.01, top + 0.2 * s, t_min);
    h.stud = 1u;
    return h;
}

/// The render shape of solid cell `c`.
fn cell_shape_hit(ro: vec3<f32>, rd: vec3<f32>, c: vec3<i32>, t_min: f32) -> ShapeHit {
    let s = params.dims.w;
    let center = vec3<f32>(c) + vec3<f32>(0.5);
    let h = 0.5 * s;
    switch shape_kind() {
        case 1u: {
            let r = max(params.shape.x, 0.04) * s;
            return round_box_hit(ro, rd, center, h, min(r, h), t_min);
        }
        case 2u: {
            return round_box_hit(ro, rd, center, h, 0.3 * s, t_min);
        }
        case 3u: {
            return sphere_hit(ro, rd, center, h, t_min);
        }
        case 4u: {
            return cylinder_hit(ro, rd, center, h, center.y - h, center.y + h, t_min);
        }
        default: {
            return box_hit(ro, rd, center, h, t_min);
        }
    }
}

// ------------------------------------------------------------------ traversal

struct Hit {
    // 0 = nothing, 1 = voxel, 2 = ground.
    kind: u32,
    t: f32,
    n: vec3<f32>,
    cell: vec3<i32>,
    stud: u32,
};

/// Tests what lies in cell `c`: its own shape when solid, or the stud of
/// the brick below (studs stick out into the cell above).
fn test_cell(ro: vec3<f32>, rd: vec3<f32>, c: vec3<i32>, t_min: f32) -> Hit {
    var hit = Hit(0u, 0.0, vec3<f32>(0.0), c, 0u);
    let studs = shape_kind() == 1u;
    if solid(c) {
        var best = cell_shape_hit(ro, rd, c, t_min);
        if studs && !solid(c + vec3<i32>(0, 1, 0)) {
            let st = stud_hit(ro, rd, c, t_min);
            if st.t >= 0.0 && (best.t < 0.0 || st.t < best.t) {
                best = st;
            }
        }
        if best.t >= 0.0 {
            hit = Hit(1u, best.t, best.n, c, best.stud);
        }
    } else if studs && solid(c - vec3<i32>(0, 1, 0)) {
        let below = c - vec3<i32>(0, 1, 0);
        let st = stud_hit(ro, rd, below, t_min);
        if st.t >= 0.0 {
            hit = Hit(1u, st.t, st.n, below, 1u);
        }
    }
    return hit;
}

/// Fine cells visited per brick at most (a diagonal crosses 22).
const MAX_BRICK_STEPS: i32 = 32;
/// Bricks visited per ray at most (a 256³ grid diagonal crosses ~96).
const MAX_BRICKS: i32 = 512;

/// First voxel along a ray in voxel space, nearer than `t_max`. A
/// two-level Amanatides–Woo DDA: an outer walk over 8³ bricks skips the
/// empty ones, an inner walk visits the cells of occupied bricks. Both only
/// move forward, so every ray ends after the bricks it crosses.
fn trace_voxels(ro: vec3<f32>, rd: vec3<f32>, t_min: f32, t_max: f32) -> Hit {
    let none = Hit(0u, t_max, vec3<f32>(0.0), vec3<i32>(0), 0u);
    trace_steps = 0;
    let inv = safe_inv(rd);
    let grid_hi = vec3<f32>(dims() + vec3<i32>(0, 1, 0));
    let box = slab(ro, inv, vec3<f32>(0.0), grid_hi);
    if box.x > box.y || box.y < t_min {
        return none;
    }
    let t_end = min(box.y, t_max);
    var t = max(box.x, 0.0);
    let step = vec3<i32>(sign(rd));
    let moving = step != vec3<i32>(0);
    let up = max(step, vec3<i32>(0));
    let hi_cell = vec3<i32>(grid_hi) - vec3<i32>(1);
    let brick_count = vec3<i32>(textureDimensions(bricks));
    var b = clamp(vec3<i32>(floor((ro + rd * t) / 8.0)), vec3<i32>(0), brick_count - vec3<i32>(1));
    var bt_next = select(vec3<f32>(1e30), (vec3<f32>((b + up) * 8) - ro) * inv, moving);
    let bt_delta = select(vec3<f32>(1e30), abs(inv) * 8.0, moving);
    let t_delta = select(vec3<f32>(1e30), abs(inv), moving);
    for (var i = 0; i < MAX_BRICKS; i++) {
        let bm = min_mask(bt_next);
        let b_exit = min(bt_next.x, min(bt_next.y, bt_next.z));
        if brick_occupied(b) {
            let seg_end = min(b_exit, t_end);
            let lo = b * 8;
            let hi = min(lo + vec3<i32>(7), hi_cell);
            var c = clamp(vec3<i32>(floor(ro + rd * t)), lo, hi);
            var t_next = select(vec3<f32>(1e30), (vec3<f32>(c + up) - ro) * inv, moving);
            for (var k = 0; k < MAX_BRICK_STEPS; k++) {
                trace_steps += 1;
                let h = test_cell(ro, rd, c, t_min);
                if h.kind != 0u && h.t <= t_end {
                    return h;
                }
                if min(t_next.x, min(t_next.y, t_next.z)) > seg_end {
                    break;
                }
                let m = min_mask(t_next);
                c += step * vec3<i32>(m);
                t_next += select(vec3<f32>(0.0), t_delta, m);
                if any(c < lo) || any(c > hi) {
                    break;
                }
            }
        }
        trace_steps += 1;
        if b_exit >= t_end {
            break;
        }
        t = b_exit;
        b += step * vec3<i32>(bm);
        bt_next += select(vec3<f32>(0.0), bt_delta, bm);
        if any(b < vec3<i32>(0)) || any(b >= brick_count) {
            break;
        }
    }
    return none;
}

fn to_voxel(p: vec3<f32>) -> vec3<f32> {
    return p - params.grid.xyz;
}

fn ground_on() -> bool {
    return (u32(params.ground.w) & 1u) != 0u;
}

fn catcher_on() -> bool {
    return (u32(params.ground.w) & 2u) != 0u;
}

/// Nearest voxel or ground hit of a world-space ray.
fn trace_scene(ro: vec3<f32>, rd: vec3<f32>, t_max: f32) -> Hit {
    var hit = trace_voxels(to_voxel(ro), rd, 1e-4, t_max);
    if ground_on() && rd.y < 0.0 {
        let t = -ro.y / rd.y;
        if t > 1e-4 && t < hit.t {
            hit = Hit(2u, t, vec3<f32>(0.0, 1.0, 0.0), vec3<i32>(0), 0u);
        }
    }
    return hit;
}

// ------------------------------------------------------------------ surface

struct Surface {
    base: vec3<f32>,
    metallic: f32,
    alpha: f32,
    f0: f32,
    ior: f32,
    transmission: f32,
    coat: f32,
    emission: vec3<f32>,
    opacity: f32,
};

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3<f32>(2.4)), c / 12.92, c <= vec3<f32>(0.04045));
}

fn voxel_surface(c: vec3<i32>) -> Surface {
    let v = cell_value(c);
    let m = materials[min(v.y, 255u)];
    let color = palette[min(v.x, 255u)].rgb;
    let rough = clamp(m.params.y, 0.02, 1.0);
    return Surface(
        color * m.tint.rgb,
        m.params.x,
        max(rough * rough, 0.002),
        0.08 * m.params.z,
        m.params.w,
        m.params2.x,
        m.params2.y,
        m.emission.rgb,
        m.tint.a,
    );
}

fn ground_surface() -> Surface {
    return Surface(params.ground.rgb, 0.0, 0.81, 0.02, 1.5, 0.0, 0.0, vec3<f32>(0.0), 1.0);
}

/// The two in-plane axes of a face with normal `n` (unit vectors).
fn face_axes(n: vec3<f32>) -> mat2x3<f32> {
    let a = abs(n);
    if a.x > 0.5 {
        return mat2x3<f32>(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(0.0, 0.0, 1.0));
    }
    if a.y > 0.5 {
        return mat2x3<f32>(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, 0.0, 1.0));
    }
    return mat2x3<f32>(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, 1.0, 0.0));
}

/// Rounded edges on cubes without extra geometry: near an edge whose
/// neighbour is empty the normal leans toward that side.
fn bevel_normal(c: vec3<i32>, n: vec3<f32>, p: vec3<f32>) -> vec3<f32> {
    let r = params.shape.x;
    if r <= 0.0 || shape_kind() != 0u {
        return n;
    }
    let s = params.dims.w;
    let local = (p - vec3<f32>(c) - vec3<f32>(0.5)) / s + vec3<f32>(0.5);
    let shrunk = s < 0.999;
    let axes = face_axes(n);
    var out = n;
    for (var i = 0; i < 4; i++) {
        let dir = select(-1.0, 1.0, (i & 1) == 1);
        let axis = select(axes[0], axes[1], i >= 2);
        let e = axis * dir;
        // Distance from the face edge on the `e` side.
        let x = dot(local, axis);
        let d = select(x, 1.0 - x, dir > 0.0);
        if d < r / s && (shrunk || !solid(c + vec3<i32>(e))) {
            out += e * (1.0 - d * s / r);
        }
    }
    return normalize(out);
}

/// Dark grooves where two cube voxels meet on the same face plane.
fn seam_factor(c: vec3<i32>, n: vec3<f32>, p: vec3<f32>) -> f32 {
    if params.shape.y < 0.5 || shape_kind() != 0u || params.dims.w < 0.999 {
        return 1.0;
    }
    let local = p - vec3<f32>(c);
    let axes = face_axes(n);
    let ni = vec3<i32>(round(n));
    let w = 0.035;
    var f = 1.0;
    for (var i = 0; i < 4; i++) {
        let dir = select(-1.0, 1.0, (i & 1) == 1);
        let axis = select(axes[0], axes[1], i >= 2);
        let x = dot(local, axis);
        let d = select(x, 1.0 - x, dir > 0.0);
        if d < w {
            let ei = vec3<i32>(axis * dir);
            if solid(c + ei) && !solid(c + ei + ni) {
                f *= mix(0.3, 1.0, smoothstep(0.0, w, d));
            }
        }
    }
    return f;
}

// ------------------------------------------------------------------ BSDF

fn onb(n: vec3<f32>) -> mat3x3<f32> {
    let s = select(-1.0, 1.0, n.z >= 0.0);
    let a = -1.0 / (s + n.z);
    let b = n.x * n.y * a;
    let t = vec3<f32>(1.0 + s * n.x * n.x * a, s * b, -s * n.x);
    let bt = vec3<f32>(b, s + n.y * n.y * a, -n.y);
    return mat3x3<f32>(t, bt, n);
}

fn schlick(f0: vec3<f32>, c: f32) -> vec3<f32> {
    return f0 + (vec3<f32>(1.0) - f0) * pow(clamp(1.0 - c, 0.0, 1.0), 5.0);
}

fn schlick1(f0: f32, c: f32) -> f32 {
    return f0 + (1.0 - f0) * pow(clamp(1.0 - c, 0.0, 1.0), 5.0);
}

fn ggx_d(nh: f32, a: f32) -> f32 {
    let a2 = a * a;
    let d = nh * nh * (a2 - 1.0) + 1.0;
    return a2 / (PI * d * d);
}

fn smith_g1(x: f32, a: f32) -> f32 {
    let a2 = a * a;
    return 2.0 * x / (x + sqrt(a2 + (1.0 - a2) * x * x));
}

/// Visible-normal sample (Heitz 2018) in the local frame (z = normal).
fn sample_vndf(v: vec3<f32>, a: f32, u: vec2<f32>) -> vec3<f32> {
    let vh = normalize(vec3<f32>(a * v.x, a * v.y, v.z));
    let len2 = vh.x * vh.x + vh.y * vh.y;
    let t1 = select(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(-vh.y, vh.x, 0.0) * inverseSqrt(len2), len2 > 0.0);
    let t2 = cross(vh, t1);
    let r = sqrt(u.x);
    let phi = 2.0 * PI * u.y;
    let p1 = r * cos(phi);
    let s = 0.5 * (1.0 + vh.z);
    let p2 = (1.0 - s) * sqrt(max(1.0 - p1 * p1, 0.0)) + s * r * sin(phi);
    let nh = p1 * t1 + p2 * t2 + sqrt(max(0.0, 1.0 - p1 * p1 - p2 * p2)) * vh;
    return normalize(vec3<f32>(a * nh.x, a * nh.y, max(nh.z, 1e-6)));
}

const COAT_ALPHA: f32 = 0.04;

/// Lobe selection weights (diffuse, specular, transmission, coat) for the
/// view direction; their sum is in `w` of the second vector.
struct Lobes {
    d: f32,
    s: f32,
    t: f32,
    c: f32,
    sum: f32,
    f0: vec3<f32>,
    fs: f32,
    fc: f32,
};

fn lobes(m: Surface, nv: f32) -> Lobes {
    let f0 = mix(vec3<f32>(m.f0), m.base, m.metallic);
    let fs = luminance(schlick(f0, nv));
    let fc = m.coat * schlick1(0.04, nv);
    let s = fs * (1.0 - fc);
    let rest = (1.0 - m.metallic) * (1.0 - fs) * (1.0 - fc);
    let t = rest * m.transmission;
    let d = rest * (1.0 - m.transmission);
    return Lobes(d, s, t, fc, max(d + s + t + fc, 1e-6), f0, max(fs, 1e-6), fc);
}

/// BSDF times cosine for light direction `l` (reflection only) and the
/// sampling pdf of `l`.
fn bsdf_eval(m: Surface, n: vec3<f32>, v: vec3<f32>, l: vec3<f32>) -> vec4<f32> {
    let nl = dot(n, l);
    let nv = dot(n, v);
    if nl <= 0.0 || nv <= 0.0 {
        return vec4<f32>(0.0);
    }
    let h = normalize(v + l);
    let nh = max(dot(n, h), 0.0);
    let vh = max(dot(v, h), 0.0);
    let lb = lobes(m, nv);
    let d_s = ggx_d(nh, m.alpha);
    let g1v = smith_g1(nv, m.alpha);
    let spec = d_s * g1v * smith_g1(nl, m.alpha) * schlick(lb.f0, vh) / (4.0 * nv) * (1.0 - lb.fc);
    let diffuse = m.base / PI * nl * (1.0 - m.metallic) * (1.0 - m.transmission)
        * (1.0 - lb.fs) * (1.0 - lb.fc);
    let d_c = ggx_d(nh, COAT_ALPHA);
    let g1c = smith_g1(nv, COAT_ALPHA);
    let coat = m.coat * d_c * g1c * smith_g1(nl, COAT_ALPHA) * schlick1(0.04, vh) / (4.0 * nv);
    let pdf = (lb.d * nl / PI + lb.s * d_s * g1v / (4.0 * nv) + lb.c * d_c * g1c / (4.0 * nv)) / lb.sum;
    return vec4<f32>(diffuse + spec + vec3<f32>(coat), pdf);
}

struct BsdfSample {
    dir: vec3<f32>,
    weight: vec3<f32>,
    pdf: f32,
    // 0 diffuse, 1 specular, 2 transmission (into the surface).
    lobe: u32,
};

fn cosine_dir(n: vec3<f32>, u: vec2<f32>) -> vec3<f32> {
    let r = sqrt(u.x);
    let phi = 2.0 * PI * u.y;
    let local = vec3<f32>(r * cos(phi), r * sin(phi), sqrt(max(0.0, 1.0 - u.x)));
    return normalize(onb(n) * local);
}

fn sample_bsdf(m: Surface, n: vec3<f32>, v: vec3<f32>) -> BsdfSample {
    let nv = max(dot(n, v), 1e-4);
    let lb = lobes(m, nv);
    let pick = rand() * lb.sum;
    let u = vec2<f32>(rand(), rand());
    let frame = onb(n);
    let v_local = transpose(frame) * v;
    var out: BsdfSample;
    if pick < lb.d {
        out.dir = cosine_dir(n, u);
        out.weight = m.base * lb.sum;
        out.pdf = lb.d / lb.sum * max(dot(n, out.dir), 1e-4) / PI;
        out.lobe = 0u;
        return out;
    }
    if pick < lb.d + lb.s {
        let h = frame * sample_vndf(v_local, m.alpha, u);
        out.dir = reflect(-v, h);
        let nl = dot(n, out.dir);
        if nl <= 0.0 {
            out.weight = vec3<f32>(0.0);
            return out;
        }
        let vh = max(dot(v, h), 0.0);
        out.weight = schlick(lb.f0, vh) / lb.fs * smith_g1(nl, m.alpha) * lb.sum;
        out.pdf = lb.s / lb.sum * ggx_d(max(dot(n, h), 0.0), m.alpha) * smith_g1(nv, m.alpha) / (4.0 * nv);
        out.lobe = 1u;
        return out;
    }
    if pick < lb.d + lb.s + lb.t {
        let h = frame * sample_vndf(v_local, m.alpha, u);
        let eta = 1.0 / m.ior;
        let r = refract(-v, h, eta);
        out.lobe = 2u;
        out.pdf = 1e6;
        out.weight = m.base * lb.sum;
        if dot(r, r) < 1e-6 {
            out.dir = reflect(-v, h);
            out.lobe = 1u;
        } else {
            out.dir = normalize(r);
        }
        return out;
    }
    let h = frame * sample_vndf(v_local, COAT_ALPHA, u);
    out.dir = reflect(-v, h);
    let nl = dot(n, out.dir);
    if nl <= 0.0 {
        out.weight = vec3<f32>(0.0);
        return out;
    }
    let vh = max(dot(v, h), 0.0);
    out.weight = vec3<f32>(schlick1(0.04, vh) / max(schlick1(0.04, nv), 1e-4) * smith_g1(nl, COAT_ALPHA) * lb.sum);
    out.pdf = lb.c / lb.sum * ggx_d(max(dot(n, h), 0.0), COAT_ALPHA) * smith_g1(nv, COAT_ALPHA) / (4.0 * nv);
    out.lobe = 1u;
    return out;
}

// ------------------------------------------------------------------ lights

fn sky_radiance(d: vec3<f32>) -> vec3<f32> {
    let mode = u32(params.sky.w);
    if mode == 2u {
        let size = vec2<f32>(textureDimensions(sky_image));
        let phi = atan2(d.x, -d.z) + params.sky_zenith.w;
        let u = fract(phi / (2.0 * PI) + 0.5);
        let v = acos(clamp(d.y, -1.0, 1.0)) / PI;
        let texel = vec2<i32>(min(vec2<f32>(u, v) * size, size - vec2<f32>(1.0)));
        return textureLoad(sky_image, texel, 0).rgb * params.sky.rgb;
    }
    if mode == 1u {
        if d.y >= 0.0 {
            return mix(params.sky.rgb, params.sky_zenith.rgb, sqrt(d.y));
        }
        return params.sky.rgb * mix(1.0, 0.35, sqrt(-d.y));
    }
    return params.sky.rgb;
}

fn light_count() -> i32 {
    return i32(params.misc.y);
}

/// Uniform direction in the cone around `axis` with half-angle cosine `cos_max`.
fn sample_cone(axis: vec3<f32>, cos_max: f32, u: vec2<f32>) -> vec3<f32> {
    let cos_t = 1.0 - u.x * (1.0 - cos_max);
    let sin_t = sqrt(max(0.0, 1.0 - cos_t * cos_t));
    let phi = 2.0 * PI * u.y;
    return normalize(onb(axis) * vec3<f32>(sin_t * cos(phi), sin_t * sin(phi), cos_t));
}

fn mis(a: f32, b: f32) -> f32 {
    let a2 = a * a;
    return a2 / max(a2 + b * b, 1e-20);
}

/// Light reaching a world point along `l` (voxels block it; transmissive
/// and translucent voxels tint it).
fn shadow(p: vec3<f32>, l: vec3<f32>) -> vec3<f32> {
    if ground_on() && l.y < 0.0 {
        return vec3<f32>(0.0);
    }
    var t_vis = vec3<f32>(1.0);
    var ro = to_voxel(p);
    for (var i = 0; i < 6; i++) {
        let h = trace_voxels(ro, l, 1e-4, 1e30);
        if h.kind == 0u {
            break;
        }
        let m = voxel_surface(h.cell);
        let through = max(m.transmission * (1.0 - m.metallic), 1.0 - m.opacity);
        if through <= 0.0 {
            return vec3<f32>(0.0);
        }
        t_vis *= mix(vec3<f32>(1.0), m.base, m.transmission) * through;
        ro = ro + l * (h.t + 2e-3);
    }
    if params.fog.w > 0.0 {
        t_vis *= exp(-params.fog.w * params.misc.z * 0.25);
    }
    return t_vis;
}

/// One distant light picked in proportion to its brightness, and the
/// probability of that pick (one shadow ray per shading point, however
/// many lights).
struct LightPick {
    index: i32,
    prob: f32,
};

fn pick_light() -> LightPick {
    let n = light_count();
    var total = 0.0;
    for (var i = 0; i < n; i++) {
        total += luminance(params.light_irr[i].rgb);
    }
    if n == 0 || total <= 0.0 {
        return LightPick(-1, 0.0);
    }
    let u = rand() * total;
    var acc = 0.0;
    for (var i = 0; i < n; i++) {
        let w = luminance(params.light_irr[i].rgb);
        acc += w;
        if u <= acc || i == n - 1 {
            return LightPick(i, w / total);
        }
    }
    return LightPick(-1, 0.0);
}

/// Direct light from the distant lights at a surface point (next-event
/// estimation with MIS against the BSDF).
fn direct(m: Surface, p: vec3<f32>, n: vec3<f32>, v: vec3<f32>) -> vec3<f32> {
    let pick = pick_light();
    if pick.index < 0 {
        return vec3<f32>(0.0);
    }
    let i = pick.index;
    let l = sample_cone(params.light_dir[i].xyz, params.light_dir[i].w, vec2<f32>(rand(), rand()));
    if dot(n, l) <= 0.0 {
        return vec3<f32>(0.0);
    }
    let f = bsdf_eval(m, n, v, l);
    if luminance(f.rgb) <= 0.0 {
        return vec3<f32>(0.0);
    }
    let vis = shadow(p + n * 2e-3, l);
    if luminance(vis) <= 0.0 {
        return vec3<f32>(0.0);
    }
    let solid_angle = params.light_irr[i].w;
    let w = mis(1.0 / solid_angle, f.w);
    return f.rgb * params.light_irr[i].rgb * vis * w / pick.prob;
}

/// Sun disks seen by a ray that left the scene after a BSDF sample.
fn lights_hit(d: vec3<f32>, bsdf_pdf: f32) -> vec3<f32> {
    var sum = vec3<f32>(0.0);
    for (var i = 0; i < light_count(); i++) {
        if dot(d, params.light_dir[i].xyz) >= params.light_dir[i].w {
            let solid_angle = params.light_irr[i].w;
            sum += params.light_irr[i].rgb / solid_angle * mis(bsdf_pdf, 1.0 / solid_angle);
        }
    }
    return sum;
}

// ------------------------------------------------------------------ glass

struct Exit {
    // World point where the ray leaves the glass.
    p: vec3<f32>,
    // Outward normal of the exit face.
    n: vec3<f32>,
    // Distance inside.
    dist: f32,
    // 1 when an opaque voxel was met inside instead.
    blocked: u32,
};

/// Walks a ray through transmissive voxels until it reaches a cell that is
/// empty (or opaque).
fn trace_exit(ro: vec3<f32>, rd: vec3<f32>) -> Exit {
    let o = to_voxel(ro);
    let inv = safe_inv(rd);
    var c = vec3<i32>(floor(o + rd * 1e-3));
    let step = vec3<i32>(sign(rd));
    let next = vec3<f32>(c + max(step, vec3<i32>(0)));
    var t_next = select(vec3<f32>(1e30), (next - o) * inv, step != vec3<i32>(0));
    let t_delta = select(vec3<f32>(1e30), abs(inv), step != vec3<i32>(0));
    var t = 0.0;
    var n = vec3<f32>(0.0, 1.0, 0.0);
    for (var i = 0; i < 512; i++) {
        let m = min_mask(t_next);
        t = min(t_next.x, min(t_next.y, t_next.z));
        c += step * vec3<i32>(m);
        t_next += select(vec3<f32>(0.0), t_delta, m);
        n = select(vec3<f32>(0.0), vec3<f32>(step), m);
        if !solid(c) {
            return Exit(ro + rd * t, n, t, 0u);
        }
        let mat = voxel_surface(c);
        if mat.transmission <= 0.0 {
            return Exit(ro + rd * t, -n, t, 1u);
        }
    }
    return Exit(ro + rd * t, n, t, 0u);
}

// ------------------------------------------------------------------ camera

struct CameraRay {
    o: vec3<f32>,
    d: vec3<f32>,
};

fn polygon_sample(u: vec2<f32>, blades: f32) -> vec2<f32> {
    if blades < 3.0 {
        let r = sqrt(u.x);
        let phi = 2.0 * PI * u.y;
        return vec2<f32>(r * cos(phi), r * sin(phi));
    }
    // A uniform point in one triangle of the regular polygon.
    let k = floor(u.y * blades);
    let a0 = 2.0 * PI * k / blades;
    let a1 = 2.0 * PI * (k + 1.0) / blades;
    var s = u.x;
    var t = fract(u.y * blades);
    if s + t > 1.0 {
        s = 1.0 - s;
        t = 1.0 - t;
    }
    return vec2<f32>(cos(a0), sin(a0)) * s + vec2<f32>(cos(a1), sin(a1)) * t;
}

fn camera_ray(pixel: vec2<f32>) -> CameraRay {
    let size = params.image.xy;
    let pixel_filter = u32(params.frame.w);
    var jitter = vec2<f32>(0.0);
    if pixel_filter != 2u {
        let u = r2(pixel_rot.xy);
        if pixel_filter == 1u {
            // Gaussian (sigma 0.4 px), Box–Muller.
            let r = 0.4 * sqrt(-2.0 * log(max(u.x, 1e-6)));
            jitter = r * vec2<f32>(cos(2.0 * PI * u.y), sin(2.0 * PI * u.y));
        } else {
            jitter = u - vec2<f32>(0.5);
        }
    }
    let p = pixel + vec2<f32>(0.5) + jitter;
    let ndc = vec2<f32>(p.x / size.x * 2.0 - 1.0, 1.0 - p.y / size.y * 2.0);
    let near = params.inv_view_proj * vec4<f32>(ndc, 0.0, 1.0);
    let far = params.inv_view_proj * vec4<f32>(ndc, 1.0, 1.0);
    let pn = near.xyz / near.w;
    let pf = far.xyz / far.w;
    var ray: CameraRay;
    if params.eye.w > 0.5 {
        ray.o = pn;
        ray.d = params.forward.xyz;
        return ray;
    }
    ray.o = params.eye.xyz;
    ray.d = normalize(pf - pn);
    let lens = params.right.w;
    if lens > 0.0 && pixel_filter != 2u {
        let focus = params.forward.w;
        let focal = ray.o + ray.d * (focus / max(dot(ray.d, params.forward.xyz), 1e-4));
        let q = polygon_sample(r2(pixel_rot.zw), params.up.w) * lens;
        ray.o = ray.o + params.right.xyz * q.x + params.up.xyz * q.y;
        ray.d = normalize(focal - ray.o);
    }
    return ray;
}

// ------------------------------------------------------------------ paths

struct PathResult {
    radiance: vec3<f32>,
    // 1 when the pixel shows the model or a lit ground (tone-mapped).
    coverage: f32,
    // Background multiplier (shadow catcher) when not covered.
    background: vec3<f32>,
    albedo: vec3<f32>,
    normal: vec3<f32>,
    depth: f32,
};

/// Shadow-catcher ground at `p`: the share of the unoccluded light that
/// reaches it, per channel (light bounced off the model can tint it).
fn catcher_ratio(p: vec3<f32>) -> vec3<f32> {
    let n = vec3<f32>(0.0, 1.0, 0.0);
    var e = vec3<f32>(0.0);
    let pick = pick_light();
    if pick.index >= 0 {
        let i = pick.index;
        let l = sample_cone(params.light_dir[i].xyz, params.light_dir[i].w, vec2<f32>(rand(), rand()));
        e += params.light_irr[i].rgb * max(l.y, 0.0) * shadow(p + n * 2e-3, l) / pick.prob;
    }
    let d = cosine_dir(n, vec2<f32>(rand(), rand()));
    let h = trace_scene(p + n * 2e-3, d, 1e30);
    if h.kind == 0u {
        e += PI * sky_radiance(d);
    } else if h.kind == 1u {
        let q = p + n * 2e-3 + d * h.t;
        let m = voxel_surface(h.cell);
        let lit = direct(m, q, h.n, -d) + m.emission;
        e += PI * lit;
    }
    return clamp(e / max(params.unoccluded.rgb, vec3<f32>(1e-4)), vec3<f32>(0.0), vec3<f32>(1.5));
}

fn trace_path(cam: CameraRay) -> PathResult {
    var res = PathResult(vec3<f32>(0.0), 0.0, vec3<f32>(1.0), vec3<f32>(0.0), vec3<f32>(0.0), MISS_DEPTH);
    var ro = cam.o;
    var rd = cam.d;
    var throughput = vec3<f32>(1.0);
    var diffuse_left = i32(params.bounces.x);
    var specular_left = i32(params.bounces.y);
    var transmission_left = i32(params.bounces.z);
    var last_pdf = 1e6;
    var primary = true;
    let fog = params.fog.w;
    for (var bounce = 0; bounce < 64; bounce++) {
        let hit = trace_scene(ro, rd, 1e30);
        // Fog: a scattering event before the surface.
        if fog > 0.0 {
            let reach = select(params.misc.z, hit.t, hit.kind != 0u);
            let d = -log(max(1.0 - rand(), 1e-7)) / fog;
            if d < reach {
                let p = ro + rd * d;
                if primary {
                    res.coverage = 1.0;
                    res.depth = d;
                    primary = false;
                }
                // Isotropic phase: NEE to the lights, then a new direction.
                let pick = pick_light();
                if pick.index >= 0 {
                    let i = pick.index;
                    let l = sample_cone(params.light_dir[i].xyz, params.light_dir[i].w, vec2<f32>(rand(), rand()));
                    res.radiance += throughput * params.fog.rgb * params.light_irr[i].rgb * shadow(p, l)
                        / (4.0 * PI * pick.prob);
                }
                throughput *= params.fog.rgb;
                let z = 1.0 - 2.0 * rand();
                let phi = 2.0 * PI * rand();
                let r = sqrt(max(0.0, 1.0 - z * z));
                ro = p;
                rd = vec3<f32>(r * cos(phi), r * sin(phi), z);
                last_pdf = 1.0 / (4.0 * PI);
                diffuse_left -= 1;
                if diffuse_left < 0 {
                    break;
                }
                continue;
            }
        }
        if hit.kind == 0u {
            if primary {
                if u32(params.background.w) == 2u {
                    res.radiance = sky_radiance(rd);
                    res.coverage = 1.0;
                }
                return res;
            }
            res.radiance += throughput * (sky_radiance(rd) + lights_hit(rd, last_pdf));
            break;
        }
        let p = ro + rd * hit.t;
        if hit.kind == 2u && primary && catcher_on() {
            res.background = catcher_ratio(p);
            return res;
        }
        var m: Surface;
        var n = hit.n;
        if hit.kind == 1u {
            m = voxel_surface(hit.cell);
            if hit.stud == 0u {
                n = bevel_normal(hit.cell, n, to_voxel(p));
                m.base *= seam_factor(hit.cell, hit.n, to_voxel(p));
            }
        } else {
            m = ground_surface();
        }
        // Translucent voxels: let the ray through with the missing opacity.
        if m.opacity < 1.0 && rand() > m.opacity {
            ro = p + rd * 2e-3;
            continue;
        }
        if primary {
            res.coverage = 1.0;
            res.albedo = m.base;
            res.normal = n;
            res.depth = hit.t;
            primary = false;
        }
        let v = -rd;
        if dot(n, v) <= 0.0 {
            n = hit.n;
        }
        res.radiance += throughput * m.emission;
        res.radiance += throughput * direct(m, p, n, v);
        let s = sample_bsdf(m, n, v);
        if luminance(s.weight) <= 0.0 {
            break;
        }
        throughput *= s.weight;
        last_pdf = s.pdf;
        if s.lobe == 0u {
            diffuse_left -= 1;
            if diffuse_left < 0 {
                break;
            }
            ro = p + hit.n * 2e-3;
            rd = s.dir;
        } else if s.lobe == 1u {
            specular_left -= 1;
            if specular_left < 0 {
                break;
            }
            ro = p + hit.n * 2e-3;
            rd = s.dir;
        } else {
            transmission_left -= 1;
            if transmission_left < 0 || hit.kind != 1u {
                break;
            }
            // Through the glass voxels to the far side, then out.
            let inside = p - hit.n * 2e-3;
            let exit = trace_exit(inside, s.dir);
            if exit.blocked == 1u {
                break;
            }
            throughput *= exp(-(vec3<f32>(1.0) - m.base) * exit.dist * 0.5);
            var out = refract(s.dir, -exit.n, m.ior);
            if dot(out, out) < 1e-6 {
                out = reflect(s.dir, -exit.n);
                ro = exit.p - exit.n * 2e-3;
                rd = out;
                // Bounced back inside: let the next pass treat it as glass.
                let again = trace_exit(ro, rd);
                ro = again.p + again.n * 2e-3;
                rd = refract(rd, -again.n, m.ior);
                if dot(rd, rd) < 1e-6 {
                    break;
                }
                rd = normalize(rd);
            } else {
                ro = exit.p + exit.n * 2e-3;
                rd = normalize(out);
            }
        }
        if params.bounces.w > 0.5 && bounce >= 2 {
            let q = clamp(max(throughput.x, max(throughput.y, throughput.z)), 0.05, 0.95);
            if rand() > q {
                break;
            }
            throughput /= q;
        }
    }
    return res;
}

@compute @workgroup_size(8, 8)
fn cs_trace(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = buffer_size();
    if id.x >= size.x || id.y >= size.y {
        return;
    }
    let pixel = vec2<f32>(id.xy) + params.image.zw;
    let index = pixel_index(id.xy);
    let seed = u32(params.frame.z);
    let base = pcg(u32(pixel.x) * 1973u + u32(pixel.y) * 9277u + seed * 26699u);
    rng = pcg(base);
    pixel_rot = vec4<f32>(rand(), rand(), rand(), rand());
    var acc = vec4<f32>(0.0);
    var bg = vec4<f32>(0.0);
    var alb = vec4<f32>(0.0);
    var nd = vec4<f32>(0.0);
    let first = u32(params.frame.x);
    if first > 0u {
        acc = accum[index];
        bg = aux[index];
        alb = albedo_sum[index];
        nd = normal_depth[index];
    }
    let clamp_max = params.misc.x;
    for (var s = 0u; s < u32(params.frame.y); s++) {
        sample_index = first + s;
        rng = pcg(base ^ (sample_index * 2654435761u));
        let r = trace_path(camera_ray(pixel));
        var radiance = r.radiance;
        let lum = luminance(radiance);
        if clamp_max > 0.0 && lum > clamp_max {
            radiance *= clamp_max / lum;
        }
        if r.coverage > 0.0 {
            acc += vec4<f32>(radiance, 1.0);
            alb += vec4<f32>(r.albedo, 1.0);
            nd += vec4<f32>(r.normal, min(r.depth, 1e6));
        } else {
            bg += vec4<f32>(r.background, 1.0);
            nd += vec4<f32>(0.0, 0.0, 0.0, 1e6);
        }
    }
    accum[index] = acc;
    aux[index] = bg;
    albedo_sum[index] = alb;
    normal_depth[index] = nd;
}

// ------------------------------------------------------------------ tests

struct RayIn {
    origin: vec4<f32>,
    dir: vec4<f32>,
};

@group(1) @binding(0) var<storage, read> test_rays: array<RayIn>;
// xyz = hit cell (or -1), w = t; second vec4: xyz = normal, w = stud flag.
@group(1) @binding(1) var<storage, read_write> test_hits: array<vec4<f32>>;

/// Traces rays given in voxel space (CPU vs GPU comparison tests).
@compute @workgroup_size(64)
fn cs_raycast_test(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= arrayLength(&test_rays) {
        return;
    }
    let r = test_rays[id.x];
    let h = trace_voxels(r.origin.xyz, normalize(r.dir.xyz), 0.0, 1e30);
    if h.kind == 0u {
        test_hits[id.x * 2u] = vec4<f32>(-1.0, -1.0, -1.0, -1.0);
        test_hits[id.x * 2u + 1u] = vec4<f32>(0.0, 0.0, f32(trace_steps), 0.0);
    } else {
        test_hits[id.x * 2u] = vec4<f32>(vec3<f32>(h.cell), h.t);
        test_hits[id.x * 2u + 1u] = vec4<f32>(h.n, f32(h.stud));
    }
}

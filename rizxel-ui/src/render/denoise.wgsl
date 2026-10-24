// Path tracer resolve (Phase 11b). Appended to pt_common.wgsl.
//
// cs_prepare  averages the accumulation buffers and divides the radiance
//             by the first-hit albedo (so the filter keeps texture detail);
// cs_atrous   one edge-aware à-trous wavelet pass (5×5 B3 spline at a
//             growing step), guided by normal, depth and luminance, run
//             five times (SVGF-lite);
// cs_finish   multiplies the albedo back, adds bloom, tone-maps, applies
//             exposure, vignette and the outline, blends the background
//             and writes the RGBA16F image.

// xyz = demodulated radiance, w = background multiplier (luminance).
@group(0) @binding(1) var<storage, read> src: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> dst: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read> albedo_sum: array<vec4<f32>>;
@group(0) @binding(4) var<storage, read> normal_depth: array<vec4<f32>>;
@group(0) @binding(5) var<storage, read> accum: array<vec4<f32>>;
@group(0) @binding(6) var<storage, read> aux: array<vec4<f32>>;
@group(0) @binding(7) var output: texture_storage_2d<rgba16float, write>;
// xyz = unit first-hit normal (0 for the background), w = mean depth.
// Written by cs_prepare so each filter tap reads one buffer.
@group(0) @binding(8) var<storage, read_write> guide: array<vec4<f32>>;

struct Step {
    // x = step in pixels, y = luminance sigma.
    value: vec4<f32>,
};

@group(1) @binding(0) var<uniform> step: Step;

fn samples() -> f32 {
    return max(params.bloom.w, 1.0);
}

fn guide_albedo(i: u32) -> vec3<f32> {
    let a = albedo_sum[i];
    return select(vec3<f32>(1.0), a.rgb / a.w, a.w > 0.0);
}

/// (unit normal, mean depth) of a pixel's first hits.
fn guide_normal_depth(i: u32) -> vec4<f32> {
    let g = normal_depth[i];
    let n = g.xyz;
    let len = length(n);
    return vec4<f32>(select(vec3<f32>(0.0), n / len, len > 1e-6), g.w / samples());
}

fn coverage(i: u32) -> f32 {
    return accum[i].w / samples();
}

@compute @workgroup_size(8, 8)
fn cs_prepare(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = buffer_size();
    if id.x >= size.x || id.y >= size.y {
        return;
    }
    let i = pixel_index(id.xy);
    let a = accum[i];
    var radiance = vec3<f32>(0.0);
    if a.w > 0.0 {
        radiance = a.rgb / a.w / max(guide_albedo(i), vec3<f32>(0.02));
    }
    let b = aux[i];
    let shade = select(1.0, luminance(b.rgb / b.w), b.w > 0.0);
    dst[i] = vec4<f32>(radiance, shade);
    // Mostly background pixels count as background for the filter.
    let g = guide_normal_depth(i);
    guide[i] = select(vec4<f32>(0.0, 0.0, 0.0, g.w), g, coverage(i) >= 0.5);
}

/// 1D B3-spline weights (1/16, 1/4, 3/8, 1/4, 1/16) without an array
/// (dynamically indexed arrays spill to slow memory).
fn b3(i: i32) -> f32 {
    return select(select(0.0625, 0.25, abs(i) == 1), 0.375, i == 0);
}

@compute @workgroup_size(8, 8)
fn cs_atrous(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = buffer_size();
    if id.x >= size.x || id.y >= size.y {
        return;
    }
    let i = pixel_index(id.xy);
    let center = src[i];
    let g = guide[i];
    let lum = luminance(center.rgb);
    let sigma_l = step.value.y;
    var sum = vec4<f32>(0.0);
    var weight = 0.0;
    let s = i32(step.value.x);
    for (var y = -2; y <= 2; y++) {
        for (var x = -2; x <= 2; x++) {
            let q = vec2<i32>(id.xy) + vec2<i32>(x, y) * s;
            if any(q < vec2<i32>(0)) || any(q >= vec2<i32>(size)) {
                continue;
            }
            let j = pixel_index(vec2<u32>(q));
            let c = src[j];
            let gj = guide[j];
            let k = b3(x) * b3(y);
            // Background pixels (normal 0) only mix with each other.
            let bg_i = dot(g.xyz, g.xyz) < 0.5;
            let bg_j = dot(gj.xyz, gj.xyz) < 0.5;
            let w_n = select(pow(max(dot(g.xyz, gj.xyz), 0.0), 32.0), f32(bg_j), bg_i || bg_j);
            let w_z = exp(-abs(g.w - gj.w) / max(0.02 * g.w * f32(s), 1e-3));
            let w_l = exp(-abs(lum - luminance(c.rgb)) / (sigma_l * (lum + 0.05) + 1e-4));
            let w = k * select(w_n, f32(bg_i == bg_j), bg_i) * w_z * w_l;
            sum += c * w;
            weight += w;
        }
    }
    dst[i] = select(center, sum / weight, weight > 1e-6);
}

fn aces(x: vec3<f32>) -> vec3<f32> {
    // Narkowicz's fit of the ACES filmic curve.
    let a = 2.51;
    let b = 0.03;
    let c = 2.43;
    let d = 0.59;
    let e = 0.14;
    return clamp((x * (a * x + b)) / (x * (c * x + d) + e), vec3<f32>(0.0), vec3<f32>(1.0));
}

fn hable(x: vec3<f32>) -> vec3<f32> {
    let a = 0.15;
    let b = 0.5;
    let c = 0.1;
    let d = 0.2;
    let e = 0.02;
    let f = 0.3;
    return ((x * (a * x + c * b) + d * e) / (x * (a * x + b) + d * f)) - e / f;
}

fn tone_map(c: vec3<f32>) -> vec3<f32> {
    let x = c * params.film.x;
    switch u32(params.film.y) {
        case 1u: {
            return aces(x);
        }
        case 2u: {
            let white = 11.2;
            return clamp(hable(x * 2.0) / hable(vec3<f32>(white)), vec3<f32>(0.0), vec3<f32>(1.0));
        }
        default: {
            return clamp(x, vec3<f32>(0.0), vec3<f32>(1.0));
        }
    }
}

/// Radiance of pixel `i` after the filter (albedo multiplied back).
fn radiance_at(i: u32) -> vec3<f32> {
    return src[i].rgb * max(guide_albedo(i), vec3<f32>(0.02));
}

@compute @workgroup_size(8, 8)
fn cs_finish(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = buffer_size();
    if id.x >= size.x || id.y >= size.y {
        return;
    }
    let i = pixel_index(id.xy);
    let image = params.image.xy;
    let pixel = vec2<f32>(id.xy) + params.image.zw + vec2<f32>(0.5);
    let cov = clamp(coverage(i), 0.0, 1.0);
    var hdr = radiance_at(i);

    // Bloom: bright pixels on two rings around this one.
    if params.bloom.x > 0.5 {
        let radius = image.y * 0.012;
        var glow = vec3<f32>(0.0);
        for (var k = 0; k < 12; k++) {
            let angle = f32(k) * PI / 6.0;
            let r = select(radius, radius * 0.45, (k & 1) == 1);
            let q = vec2<i32>(vec2<f32>(id.xy) + vec2<f32>(cos(angle), sin(angle)) * r);
            if all(q >= vec2<i32>(0)) && all(q < vec2<i32>(size)) {
                let j = pixel_index(vec2<u32>(q));
                glow += max(radiance_at(j) - vec3<f32>(params.bloom.y), vec3<f32>(0.0)) * coverage(j);
            }
        }
        hdr += glow / 12.0 * params.bloom.z * 4.0;
    }

    var color = pow(tone_map(hdr), vec3<f32>(params.film.z));

    // Background: color or screen-space gradient of the whole image,
    // darkened by the shadow catcher.
    let mode = u32(params.background.w);
    let t = clamp(1.0 - pixel.y / image.y, 0.0, 1.0);
    var bg = params.background.rgb;
    if mode == 1u {
        bg = mix(params.background.rgb, params.background_top.rgb, t);
    }
    let b = aux[i];
    var shade = vec3<f32>(1.0);
    if b.w > 0.0 {
        let raw = b.rgb / b.w;
        // Keep the filtered brightness, the raw tint.
        shade = raw / max(luminance(raw), 1e-4) * src[i].w;
    }
    var alpha = 1.0;
    var rgb = color * cov + bg * clamp(shade, vec3<f32>(0.0), vec3<f32>(1.2)) * (1.0 - cov);
    if mode == 3u {
        // Transparent: the shadow becomes black with alpha.
        let shadow_alpha = clamp(1.0 - luminance(shade), 0.0, 1.0);
        alpha = cov + (1.0 - cov) * shadow_alpha;
        rgb = select(vec3<f32>(0.0), color * cov / alpha, alpha > 1e-4);
    }

    // Vignette.
    let uv = pixel / image - vec2<f32>(0.5);
    rgb *= 1.0 - params.film.w * clamp(dot(uv, uv) * 2.0, 0.0, 1.0);

    // Outline where depth, normal or coverage jumps.
    let width = params.edges.w;
    if width > 0.0 {
        let g = guide_normal_depth(i);
        var edge = 0.0;
        for (var k = 0; k < 4; k++) {
            let sign = select(-1, 1, (k & 1) == 0);
            let o = select(vec2<i32>(0, sign), vec2<i32>(sign, 0), k < 2);
            let q = clamp(vec2<i32>(id.xy) + o * i32(ceil(width * 0.5)), vec2<i32>(0), vec2<i32>(size) - vec2<i32>(1));
            let j = pixel_index(vec2<u32>(q));
            let gj = guide_normal_depth(j);
            let dz = abs(g.w - gj.w) / max(min(g.w, gj.w), 1e-3);
            let dn = 1.0 - dot(g.xyz, gj.xyz);
            let dc = abs(cov - coverage(j));
            edge = max(edge, max(smoothstep(0.04, 0.1, dz), max(smoothstep(0.2, 0.5, dn), dc)));
        }
        rgb = mix(rgb, params.edges.rgb, edge);
        alpha = max(alpha, edge);
    }
    textureStore(output, vec2<i32>(id.xy), vec4<f32>(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)), alpha));
}

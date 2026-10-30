//! Golden-image checks of the *Voxel Icon* look (Phase 12e): voxel lines
//! on every visible face, the floor grid and its fade, and line smoothness,
//! path traced and raster. Images are small and compared by relation
//! (darker, brighter, smooth), not pixel by pixel, so they hold across GPUs.

use glam::{IVec3, Mat4, Vec3};
use rizxel_app::{Document, OrbitCamera, Projection, RgbaImage};
use rizxel_math::{BoundSize, RenderPreset, RenderSettings, Rgba8};

use super::offscreen::{headless_device, Headless, OffscreenRaster};
use super::pathtrace::{document_params, PathTracer, PhotoJob, PtInputs};
use crate::viewport::{output_frame, output_projection};

/// A small "R", front view, `#` = voxel, row 0 at the top.
const R: [&str; 7] = [
    "####.", //
    "#...#", //
    "#...#", //
    "####.", //
    "#.#..", //
    "#..#.", //
    "#...#",
];
/// Depth of the R in voxels.
const DEPTH: i32 = 3;

/// The small R in 7×8×5 bounds (one voxel of margin) with `settings`
/// applied to the *Voxel Icon* preset.
fn r_doc(settings: impl FnOnce(&mut RenderSettings)) -> Document {
    let mut doc = Document::new("r");
    doc.set_bounds(BoundSize::new(7, 8, DEPTH as u32 + 2), false);
    doc.set_palette_color(1, Rgba8::rgb(0x3a, 0x55, 0xf0), false);
    let mut cells = Vec::new();
    for (row, line) in R.iter().enumerate() {
        for (x, c) in line.chars().enumerate() {
            for z in 1..=DEPTH {
                if c == '#' {
                    let y = R.len() as i32 - 1 - row as i32;
                    cells.push((IVec3::new(x as i32 + 1, y, z), 1));
                }
            }
        }
    }
    doc.set_voxels(0, cells, false);
    let mut s = RenderPreset::VoxelIcon.apply(&doc.data.render);
    // Measured images: no vignette or bloom spreading into the samples,
    // and a bright sky so the faces turned from the sun are lit too.
    s.camera.vignette = 0.0;
    s.camera.bloom = false;
    s.light.sky_color = [1.0; 3];
    s.light.sky_intensity = 1.5;
    settings(&mut s);
    doc.set_render(s, false);
    doc
}

fn gpu() -> Option<Headless> {
    let gpu = headless_device();
    if gpu.is_none() {
        eprintln!("no GPU adapter: skipped");
    }
    gpu
}

/// Path-traced image of `doc` (fixed seed: the job's sample sequence).
/// `None` when the adapter can't run the path tracer (it needs Vulkan or
/// Metal; CI's Windows runner has only a software Direct3D 12 adapter): the
/// raster checks still run.
fn path_traced(
    gpu: &Headless,
    doc: &Document,
    camera: &OrbitCamera,
    size: [u32; 2],
    spp: u32,
) -> Option<RgbaImage> {
    if !gpu.path_tracing {
        eprintln!("{} can't run the path tracer: skipped", gpu.info.name);
        return None;
    }
    let inputs = PtInputs::from_document(doc, None);
    let mut pt = PathTracer::new(&gpu.device);
    inputs.upload(&mut pt, &gpu.device, &gpu.queue);
    let params = document_params(doc, camera, size, false, None);
    let mut job = PhotoJob::new(params, spp, None);
    Some(job.finish(&mut pt, &gpu.device, &gpu.queue).to_rgba8())
}

/// The raster *Image* output of `doc` (supersampled: lines are on).
fn raster(gpu: &Headless, doc: &Document, camera: &OrbitCamera, size: [u32; 2]) -> RgbaImage {
    let frame = output_frame(doc, camera, size, true);
    assert_eq!(super::offscreen::supersampling(&frame), 2);
    OffscreenRaster::new(&gpu.device).render(&gpu.device, &gpu.queue, &frame, size)
}

/// Writes `img` as `<name>.png` into `RIZXEL_TEST_IMAGES` when set, to
/// look at a failing check.
fn dump(img: &RgbaImage, name: &str) {
    if let Some(dir) = std::env::var_os("RIZXEL_TEST_IMAGES") {
        let path = std::path::Path::new(&dir).join(format!("{name}.png"));
        rizxel_app::render_job::save_png(&path, img).unwrap();
    }
}

fn luma(img: &RgbaImage, x: i32, y: i32) -> f32 {
    let x = x.clamp(0, img.width as i32 - 1) as u32;
    let y = y.clamp(0, img.height as i32 - 1) as u32;
    let [r, g, b, _] = img.pixel(x, y);
    0.2126 * f32::from(r) + 0.7152 * f32::from(g) + 0.0722 * f32::from(b)
}

/// Darkest and brightest luminance of the 3×3 pixels around `p`.
fn around(img: &RgbaImage, p: (i32, i32)) -> (f32, f32) {
    let mut lo = f32::MAX;
    let mut hi = f32::MIN;
    for dy in -1..=1 {
        for dx in -1..=1 {
            let l = luma(img, p.0 + dx, p.1 + dy);
            lo = lo.min(l);
            hi = hi.max(l);
        }
    }
    (lo, hi)
}

/// Pixel of world point `p`.
fn to_pixel(view_proj: Mat4, size: [u32; 2], p: Vec3) -> (i32, i32) {
    let c = view_proj * p.extend(1.0);
    let ndc = c.truncate() / c.w;
    (
        ((ndc.x * 0.5 + 0.5) * size[0] as f32) as i32,
        ((0.5 - ndc.y * 0.5) * size[1] as f32) as i32,
    )
}

/// The R from the front right, above: the front (+Z), left (-X) and top
/// faces show.
fn r_camera() -> OrbitCamera {
    OrbitCamera {
        yaw: 330.0,
        pitch: 36.0,
        distance: 34.0,
        ..Default::default()
    }
}

/// Pairs of (face center, point on a cell border) on the R's +Z, -X and
/// +Y faces, in world space.
fn face_samples(doc: &Document) -> [(&'static str, Vec3, Vec3, Vec3); 3] {
    let (bmin, _) = rizxel_app::world_bounds(doc.data.bounds);
    let top = R.len() as f32;
    let front = (DEPTH + 1) as f32;
    // The stem is column x = 1; rows y = 1 and 2 meet at y = 2.
    [
        (
            "front",
            Vec3::Z,
            bmin + Vec3::new(1.5, 1.5, front),
            bmin + Vec3::new(1.5, 2.0, front),
        ),
        (
            "left",
            Vec3::NEG_X,
            bmin + Vec3::new(1.0, 1.5, 1.5),
            bmin + Vec3::new(1.0, 1.5, 2.0),
        ),
        (
            "top",
            Vec3::Y,
            bmin + Vec3::new(2.5, top, 1.5),
            bmin + Vec3::new(2.0, top, 1.5),
        ),
    ]
}

/// Voxel lines darken every visible face's cell borders against the face
/// centers.
fn assert_voxel_lines(img: &RgbaImage, doc: &Document, camera: &OrbitCamera, what: &str) {
    let size = [img.width, img.height];
    let proj = output_projection(doc, camera, size);
    for (face, n, center, border) in face_samples(doc) {
        assert!(n.dot(proj.eye() - center) > 0.0, "{face} faces the camera");
        let c = luma(
            img,
            to_pixel(proj.view_proj(), size, center).0,
            to_pixel(proj.view_proj(), size, center).1,
        );
        let (b, _) = around(img, to_pixel(proj.view_proj(), size, border));
        eprintln!("{what} {face}: center {c:.1}, border {b:.1}");
        assert!(
            b < c * 0.85,
            "{what}: {face} border {b} not darker than center {c}"
        );
    }
}

#[test]
fn voxel_lines_outline_every_visible_face() {
    let Some(gpu) = gpu() else { return };
    let camera = r_camera();
    let size = [256, 256];
    // Lines a few px wide even on the left face, seen at a grazing angle.
    let doc = r_doc(|s| s.display.voxel_line_width = 0.08);
    if let Some(img) = path_traced(&gpu, &doc, &camera, size, 64) {
        dump(&img, "lines_pt");
        assert_voxel_lines(&img, &doc, &camera, "path traced");
    }
    let img = raster(&gpu, &doc, &camera, size);
    dump(&img, "lines_raster");
    assert_voxel_lines(&img, &doc, &camera, "raster");

    // Lines off: the border is about as bright as the center.
    let doc = r_doc(|s| s.display.voxel_lines = false);
    let img = raster(&gpu, &doc, &camera, size);
    let proj = output_projection(&doc, &camera, size);
    let (_, _, center, border) = face_samples(&doc)[0];
    let c = to_pixel(proj.view_proj(), size, center);
    let (b, _) = around(&img, to_pixel(proj.view_proj(), size, border));
    assert!(
        b > luma(&img, c.0, c.1) * 0.9,
        "no lines without the option"
    );
}

/// Grid contrast near the model and past the fade radius: brightest pixel
/// around a line minus a cell center.
fn floor_contrast(img: &RgbaImage, view_proj: Mat4, at: [Vec3; 2]) -> f32 {
    let size = [img.width, img.height];
    let (_, line) = around(img, to_pixel(view_proj, size, at[0]));
    let mid = to_pixel(view_proj, size, at[1]);
    line - luma(img, mid.0, mid.1)
}

#[test]
fn floor_grid_glows_near_the_model_and_fades_out() {
    let Some(gpu) = gpu() else { return };
    let camera = OrbitCamera {
        yaw: 330.0,
        pitch: 40.0,
        distance: 60.0,
        ..Default::default()
    };
    let size = [256, 256];
    // Fade radius 8 voxels (the longest side): full lines until 2.
    let doc = r_doc(|s| {
        s.display.floor_grid_fade = 1.0;
        s.display.floor_grid_width = 0.1;
        s.camera.fov = 45.0;
    });
    let proj = output_projection(&doc, &camera, size);
    let vp = proj.view_proj();
    // In front of the R (+Z, toward the camera), on the line through the
    // bounds' corner nearest x = 0 and a cell center beside it; near: 2.5
    // from the center, far: 11.
    let (bmin, _) = rizxel_app::world_bounds(doc.data.bounds);
    let x = bmin.x + 3.0;
    let near = [Vec3::new(x, 0.0, 2.5), Vec3::new(x + 0.5, 0.0, 2.5)];
    let far = [Vec3::new(x, 0.0, 11.5), Vec3::new(x + 0.5, 0.0, 11.5)];
    for p in near.iter().chain(&far) {
        let (x, y) = to_pixel(vp, size, *p);
        assert!(
            (2..254).contains(&x) && (2..254).contains(&y),
            "{p} in frame"
        );
    }
    let images = [
        ("path traced", path_traced(&gpu, &doc, &camera, size, 64)),
        ("raster", Some(raster(&gpu, &doc, &camera, size))),
    ];
    for (what, img) in images.into_iter().filter_map(|(w, i)| Some((w, i?))) {
        dump(&img, &format!("floor_{}", what.replace(' ', "_")));
        let n = floor_contrast(&img, vp, near);
        let f = floor_contrast(&img, vp, far);
        eprintln!("{what}: grid contrast near {n:.1}, far {f:.1}");
        assert!(
            n > 6.0,
            "{what}: lines brighter than the floor near the model ({n})"
        );
        assert!(
            f < n * 0.3,
            "{what}: lines fade out past the radius ({f} vs {n})"
        );
    }
}

/// Per-column profile of the one near-horizontal line in `img`: the line's
/// brightness over the floor summed down each column, and its centroid row.
fn line_profile(img: &RgbaImage) -> Vec<(f32, f32)> {
    let floor = {
        let mut v: Vec<f32> = (0..img.height as i32).map(|y| luma(img, 4, y)).collect();
        v.sort_by(f32::total_cmp);
        v[v.len() / 2]
    };
    (8..img.width as i32 - 8)
        .map(|x| {
            let (mut sum, mut moment) = (0.0, 0.0);
            for y in 0..img.height as i32 {
                let w = (luma(img, x, y) - floor).max(0.0);
                sum += w;
                moment += w * y as f32;
            }
            (sum, moment / sum.max(1e-3))
        })
        .collect()
}

/// A line at a shallow angle has no stair steps: its per-column ink stays
/// even and its centroid climbs steadily, with no jump much larger than the
/// slope between neighboring columns.
fn assert_smooth(profile: &[(f32, f32)], what: &str) {
    let n = profile.len() as f32;
    let mean = profile.iter().map(|p| p.0).sum::<f32>() / n;
    assert!(mean > 1.0, "{what}: the line shows ({mean})");
    let slope = (profile[profile.len() - 1].1 - profile[0].1) / (n - 1.0);
    let mut worst_ink = 0.0f32;
    let mut worst_step = 0.0f32;
    for w in profile.windows(2) {
        worst_ink = worst_ink.max((w[1].0 - w[0].0).abs() / mean);
        worst_step = worst_step.max((w[1].1 - w[0].1 - slope).abs());
    }
    eprintln!("{what}: slope {slope:.3} px/col, ink step {worst_ink:.3}, centroid step {worst_step:.3} px");
    assert!(
        worst_ink < 0.34,
        "{what}: ink jumps by {worst_ink} of the mean"
    );
    assert!(
        worst_step < 0.34,
        "{what}: centroid jumps by {worst_step} px"
    );
}

#[test]
fn floor_grid_lines_are_smooth_at_shallow_angles() {
    let Some(gpu) = gpu() else { return };
    // Empty 16×4×16 bounds, grid every 16 voxels: lines at x, z = ±8. Seen
    // from straight above, turned 4°, centered on the line z = -8 with the
    // x lines out of frame.
    let mut doc = Document::new("grid");
    doc.set_bounds(BoundSize::new(16, 4, 16), false);
    let mut s = RenderPreset::VoxelIcon.apply(&doc.data.render);
    s.camera.vignette = 0.0;
    s.camera.bloom = false;
    s.light.sun = false;
    s.display.floor_grid_spacing = 16;
    s.display.floor_grid_fade = 0.0;
    s.display.floor_grid_width = 0.05;
    doc.set_render(s, false);
    let camera = OrbitCamera {
        yaw: 4.0,
        pitch: 90.0,
        distance: 6.0,
        pan: Vec3::new(0.0, -rizxel_app::orbit_base(doc.data.bounds).y, -8.0),
        projection: Projection::Orthographic,
        ..Default::default()
    };
    let size = [192, 96];
    let proj = output_projection(&doc, &camera, size);
    dump(&raster(&gpu, &doc, &camera, size), "jaggy_raster");
    for x in [-8.0, 8.0] {
        let (px, _) = to_pixel(proj.view_proj(), size, Vec3::new(x, 0.0, -8.0));
        assert!(!(0..192).contains(&px), "the x = {x} line is out of frame");
    }
    // 4x the samples of a quick preview.
    if let Some(pt) = path_traced(&gpu, &doc, &camera, size, 64) {
        assert_smooth(&line_profile(&pt), "path traced");
    }
    let ras = raster(&gpu, &doc, &camera, size);
    assert_smooth(&line_profile(&ras), "raster");
}

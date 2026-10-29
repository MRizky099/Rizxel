//! Path tracer timing (Phase 11b budget: ≥ 10 samples per second at 720p
//! for a 64³ model, and 1920×1080 at 256 spp in about a minute):
//!
//! ```text
//! cargo run -p rizxel-ui --release --example render_bench
//! cargo run -p rizxel-ui --release --example render_bench -- --photo-spp 64 model.vox
//! ```
//!
//! Without a model it builds a 64³ test scene (a hollow dome on a base,
//! pillars and a sphere). Times are GPU-bound, so a debug build measures
//! about the same.

use std::time::Instant;

use glam::IVec3;
use rizxel_app::{AppState, Document, VoxelChange};
use rizxel_math::{BoundSize, RenderPreset, RenderSettings, VoxelShape};
use rizxel_ui::render::offscreen::headless_device;
use rizxel_ui::render::pathtrace::{document_params, PathTracer, PhotoJob, PtInputs};

fn fail(msg: impl std::fmt::Display) -> ! {
    eprintln!("{msg}");
    std::process::exit(2);
}

/// 64³ scene with about 40 % of its cells filled at the surface.
fn test_document() -> Document {
    let mut doc = Document::new("bench");
    doc.set_bounds(BoundSize::new(64, 64, 64), false);
    let mut changes = Vec::new();
    let c = 31.5f32;
    for z in 0..64 {
        for x in 0..64 {
            for y in 0..64 {
                let p = IVec3::new(x, y, z);
                let (dx, dz) = (x as f32 - c, z as f32 - c);
                let r = (dx * dx + dz * dz).sqrt();
                let dome = ((dx * dx + (y as f32 - 8.0).powi(2) + dz * dz).sqrt() - 26.0).abs()
                    < 1.5
                    && y >= 8;
                let base = y < 8 && r < 30.0;
                let pillar = y < 56
                    && [(-24.0, -24.0), (24.0, -24.0), (-24.0, 24.0), (24.0, 24.0)]
                        .iter()
                        .any(|(px, pz)| (dx - px).abs() < 2.5 && (dz - pz).abs() < 2.5);
                let ball = ((dx - 10.0).powi(2) + (y as f32 - 14.0).powi(2) + (dz + 10.0).powi(2))
                    .sqrt()
                    < 5.0;
                if dome || base || pillar || ball {
                    let color = 1 + ((x / 4 + y / 4 + z / 4) % 24) as u8 * 8;
                    changes.push(VoxelChange {
                        pos: p,
                        old: 0,
                        new: color,
                    });
                }
            }
        }
    }
    doc.begin_edit();
    doc.edit_voxels(0, &changes);
    doc.commit_edit();
    doc
}

fn main() {
    let mut photo_spp = 256u32;
    let mut input = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--photo-spp" => {
                photo_spp = args
                    .next()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or_else(|| fail("--photo-spp N"))
            }
            _ => input = Some(std::path::PathBuf::from(arg)),
        }
    }
    let mut doc = match input {
        Some(path) => {
            let mut state = AppState::default();
            let opened = if path.extension().is_some_and(|e| e == "rzxl") {
                state.open(&path).map(|_| String::new())
            } else {
                state.import(&path)
            };
            opened.unwrap_or_else(|e| fail(e));
            let i = state.active_doc;
            state.documents.swap_remove(i)
        }
        None => test_document(),
    };
    let Some(gpu) = headless_device() else {
        fail("no GPU adapter");
    };
    if !gpu.path_tracing {
        fail(format!("{} can't run the path tracer", gpu.info.name));
    }
    let (device, queue) = (gpu.device, gpu.queue);
    println!("adapter: {} ({:?})", gpu.info.name, gpu.info.backend);
    let b = doc.data.bounds.0;
    println!(
        "model: {}×{}×{}, {} voxels",
        b.x,
        b.y,
        b.z,
        doc.visible_voxel_count()
    );
    let camera = rizxel_app::OrbitCamera {
        yaw: 35.0,
        pitch: 30.0,
        distance: b.max_element() as f32 * 1.9,
        ..Default::default()
    };
    let mut pt = PathTracer::new(&device);

    let mut direct_only = RenderSettings::default();
    direct_only.sample.diffuse_bounces = 0;
    direct_only.sample.specular_bounces = 0;
    let mut configs: Vec<(&str, RenderSettings)> = vec![
        ("Direct light only", direct_only),
        ("Default", RenderSettings::default()),
    ];
    let mut studio = RenderPreset::StudioWhite.apply(&RenderSettings::default());
    studio.display.shape = VoxelShape::Brick;
    configs.push(("Studio White + Brick", studio));

    println!();
    println!("| Settings | Preview 1280×720 | Photo 1920×1080, {photo_spp} spp |");
    println!("|---|---|---|");
    for (name, settings) in configs {
        doc.set_render(settings, false);
        let started = Instant::now();
        let inputs = PtInputs::from_document(&doc, None);
        inputs.upload(&mut pt, &device, &queue);
        let scene_secs = started.elapsed().as_secs_f32();

        // Preview: one sample per frame, as the viewport does.
        let size = [1280, 720];
        let params = document_params(&doc, &camera, size, false, None);
        pt.resize(&device, size);
        let warm = 4;
        let frames = 60;
        let mut t0 = Instant::now();
        for i in 0..warm + frames {
            if i == warm {
                t0 = Instant::now();
            }
            let mut encoder = device.create_command_encoder(&Default::default());
            // The viewport's schedule: every sample up to 16, then every 4th.
            let resolve = i < 16 || (i + 1) % 4 == 0;
            pt.run(&device, &queue, &mut encoder, &params, i..i + 1, resolve);
            queue.submit([encoder.finish()]);
            let _ = device.poll(eframe::egui_wgpu::wgpu::PollType::wait_indefinitely());
        }
        let per_second = frames as f32 / t0.elapsed().as_secs_f32();
        // The same without the resolve (trace cost only).
        let t1 = Instant::now();
        for i in 0..frames {
            let mut encoder = device.create_command_encoder(&Default::default());
            pt.run(
                &device,
                &queue,
                &mut encoder,
                &params,
                warm + frames + i..warm + frames + i + 1,
                false,
            );
            queue.submit([encoder.finish()]);
            let _ = device.poll(eframe::egui_wgpu::wgpu::PollType::wait_indefinitely());
        }
        let trace_ms = t1.elapsed().as_secs_f32() * 1000.0 / frames as f32;
        eprintln!(
            "  trace {trace_ms:.1} ms, trace + resolve {:.1} ms per sample",
            1000.0 / per_second
        );

        // Photo: the tiled offline job.
        let params = document_params(&doc, &camera, [1920, 1080], false, None);
        let t0 = Instant::now();
        let mut job = PhotoJob::new(params, photo_spp, None);
        job.finish(&mut pt, &device, &queue);
        let photo = t0.elapsed().as_secs_f32();
        println!("| {name} | {per_second:.1} spp/s | {photo:.1} s |");
        eprintln!("  (scene build + upload {scene_secs:.2} s)");
    }
}

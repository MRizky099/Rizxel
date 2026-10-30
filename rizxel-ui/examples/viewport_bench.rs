//! Raster frame timing for the Phase 12 performance pass (target: a 60 fps
//! orbit on a 256³ model with about one million voxels):
//!
//! ```text
//! cargo run -p rizxel-ui --release --example viewport_bench
//! ```
//!
//! Builds the 256³ terrain of `mesh_bench --large`, meshes it once, then
//! draws it from 60 camera angles of an orbit at 1280×720 with 4× MSAA
//! through the offscreen raster renderer. Each frame includes the RGBA
//! readback, so the time is an upper bound of the window's frame.

use std::time::Instant;

use glam::IVec3;
use rizxel_app::{Document, OrbitCamera};
use rizxel_math::{BoundSize, Voxel};
use rizxel_ui::render::offscreen::{headless_device, OffscreenRaster};

fn height(x: i32, z: i32) -> i32 {
    let h = ((x as f32 * 0.11).sin() + (z as f32 * 0.07).cos()) * 6.0 + 12.0;
    h.round() as i32 + 16
}

fn main() {
    let Some(gpu) = headless_device() else {
        eprintln!("no GPU adapter");
        std::process::exit(2);
    };
    println!("adapter: {} ({:?})", gpu.info.name, gpu.info.backend);
    let n = 256;
    let mut doc = Document::new("bench");
    doc.set_bounds(BoundSize::new(n as u32, n as u32, n as u32), false);
    let mut writes: Vec<(IVec3, Voxel)> = Vec::new();
    for z in 0..n {
        for x in 0..n {
            let h = height(x, z);
            for y in (h - 14)..=h {
                writes.push((IVec3::new(x, y, z), (8 + (y / 4) * 8) as Voxel));
            }
        }
    }
    doc.set_voxels(0, writes, false);
    println!(
        "model: {} voxels in {n}³",
        doc.data.layers[0].volume.voxel_count()
    );

    let size = [1280, 720];
    let mut camera = OrbitCamera {
        distance: 420.0,
        ..Default::default()
    };
    let start = Instant::now();
    let frame = rizxel_ui::viewport::output_frame(&doc, &camera, size, true);
    println!(
        "mesh + upload: {:.0} ms",
        start.elapsed().as_secs_f64() * 1e3
    );
    let mut raster = OffscreenRaster::new(&gpu.device);
    raster.render(&gpu.device, &gpu.queue, &frame, size); // warm-up
    let frames = 60;
    let mut times = Vec::with_capacity(frames);
    for _ in 0..frames {
        camera.orbit(360.0 / frames as f32, 0.0);
        let mut f = frame.clone();
        let target = rizxel_app::orbit_base(doc.data.bounds) + camera.pan;
        let aspect = size[0] as f32 / size[1] as f32;
        f.view_proj = camera.view_proj(target, aspect);
        f.eye = camera.eye(target);
        let start = Instant::now();
        raster.render(&gpu.device, &gpu.queue, &f, size);
        times.push(start.elapsed().as_secs_f64() * 1e3);
    }
    times.sort_by(f64::total_cmp);
    let avg = times.iter().sum::<f64>() / frames as f64;
    println!(
        "orbit frame at {}×{}: avg {avg:.2} ms, median {:.2} ms, worst {:.2} ms ({:.0} fps avg)",
        size[0],
        size[1],
        times[frames / 2],
        times[frames - 1],
        1e3 / avg
    );
}

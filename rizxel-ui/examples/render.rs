//! Renders a model to PNG without the UI (needs a GPU adapter):
//!
//! ```text
//! cargo run -p rizxel-ui --example render -- in.rzxl out.png --spp 64
//! cargo run -p rizxel-ui --example render -- chr_knight.obj knight.png --preset studio --shape brick
//! ```
//!
//! Inputs: anything Rizxel opens or imports (`.rzxl`, `.vox`, `.obj`, ...).
//! Options:
//! - `--spp N` samples per pixel (default: the file's render settings)
//! - `--size WxH` (default 960x720)
//! - `--preset studio|outdoor|night|flat`
//! - `--shape cube|brick|rounded|sphere|cylinder`
//! - `--yaw DEG`, `--pitch DEG` camera angles (default 45, 30), framed on
//!   the model
//! - `--raster` the fast raster *Image* render instead of path tracing
//! - `--transparent` transparent background
//! - `--material diffuse|plastic|metal|glass|emit` gives every layer one
//!   material with that preset
//! - `--export-shaped FILE` also writes the shaped mesh (`.obj`, `.stl` or
//!   `.glb`, needs `--shape`)

use std::path::PathBuf;
use std::time::Instant;

use rizxel_app::{render_job, AppState};
use rizxel_math::{Material, MaterialPreset, RenderPreset, VoxelShape};
use rizxel_ui::render::offscreen::{headless_device, OffscreenRaster};
use rizxel_ui::render::pathtrace::{document_params, PathTracer, PhotoJob, PtInputs, SkyCache};

fn fail(msg: impl std::fmt::Display) -> ! {
    eprintln!("{msg}");
    std::process::exit(2);
}

fn main() {
    let mut paths: Vec<PathBuf> = Vec::new();
    let mut spp = None;
    let mut size = [960u32, 720];
    let mut preset = None;
    let mut shape = None;
    let mut yaw = 45.0f32;
    let mut pitch = 30.0f32;
    let mut raster = false;
    let mut transparent = false;
    let mut material = None;
    let mut export_shaped: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || {
            args.next()
                .unwrap_or_else(|| fail(format!("{arg} needs a value")))
        };
        match arg.as_str() {
            "--spp" => spp = Some(value().parse::<u32>().unwrap_or_else(|e| fail(e))),
            "--size" => {
                let v = value();
                let (w, h) = v.split_once('x').unwrap_or_else(|| fail("--size WxH"));
                size = [
                    w.parse().unwrap_or_else(|e| fail(e)),
                    h.parse().unwrap_or_else(|e| fail(e)),
                ];
            }
            "--preset" => {
                preset = Some(match value().as_str() {
                    "studio" => RenderPreset::StudioWhite,
                    "outdoor" => RenderPreset::OutdoorSun,
                    "night" => RenderPreset::NightEmissive,
                    "flat" => RenderPreset::FlatPixel,
                    other => fail(format!("unknown preset {other}")),
                })
            }
            "--shape" => {
                let v = value();
                shape = Some(
                    VoxelShape::ALL
                        .into_iter()
                        .find(|s| s.label().eq_ignore_ascii_case(&v))
                        .unwrap_or_else(|| fail(format!("unknown shape {v}"))),
                )
            }
            "--yaw" => yaw = value().parse().unwrap_or_else(|e| fail(e)),
            "--pitch" => pitch = value().parse().unwrap_or_else(|e| fail(e)),
            "--raster" => raster = true,
            "--transparent" => transparent = true,
            "--export-shaped" => export_shaped = Some(PathBuf::from(value())),
            "--material" => {
                let v = value();
                material = Some(
                    MaterialPreset::ALL
                        .into_iter()
                        .find(|p| p.label().eq_ignore_ascii_case(&v))
                        .unwrap_or_else(|| fail(format!("unknown material {v}"))),
                )
            }
            _ if arg.starts_with("--") => fail(format!("unknown option {arg}")),
            _ => paths.push(PathBuf::from(arg)),
        }
    }
    let [input, output] = paths.as_slice() else {
        fail("usage: render <input> <output.png> [options]");
    };

    let mut state = AppState::default();
    let opened = if input.extension().is_some_and(|e| e == "rzxl") {
        state.open(input).map(|_| String::new())
    } else {
        state.import(input)
    };
    if let Err(e) = opened {
        fail(format!("{}: {e}", input.display()));
    }
    let Some(gpu) = headless_device() else {
        fail("no GPU adapter");
    };
    let (device, queue) = (gpu.device, gpu.queue);
    eprintln!(
        "adapter: {} ({:?}), path tracer: {}",
        gpu.info.name, gpu.info.backend, gpu.path_tracing
    );
    if !raster && !gpu.path_tracing {
        fail("this adapter can't run the path tracer (GL, or no compute shaders): only --raster works");
    }

    state.viewport_aspect = size[0] as f32 / size[1] as f32;
    state.camera.yaw = yaw;
    state.camera.pitch = pitch;
    state.set_view_mode(rizxel_app::ViewMode::Render);
    let doc = state.doc_mut().unwrap_or_else(|| fail("nothing opened"));
    let mut s = doc.data.render.clone();
    if let Some(p) = preset {
        s = p.apply(&s);
    }
    if let Some(shape) = shape {
        s.display.shape = shape;
    }
    s.output.transparent = transparent;
    doc.set_render(s, false);
    if let Some(preset) = material {
        let mut m = Material::new(preset.label());
        m.apply_preset(preset);
        doc.set_materials(vec![m]);
        for i in 0..doc.data.layers.len() {
            let props = doc.data.layers[i].props.clone();
            doc.set_layer_props(
                i,
                rizxel_app::LayerProps {
                    material: Some(0),
                    ..props
                },
                false,
            );
        }
    }
    state.frame_selection();
    eprintln!("bounds {:?}", state.doc().map(|d| d.data.bounds));
    if let Some(path) = &export_shaped {
        let e = rizxel_app::ShapedExport::default();
        match state.export_shaped(path, &e) {
            Ok(m) => eprintln!("{m}"),
            Err(e) => fail(e),
        }
    }
    let doc = state.doc().unwrap_or_else(|| fail("nothing opened"));
    let started = Instant::now();
    let image = if raster {
        let frame = rizxel_ui::viewport::output_frame(doc, &state.camera, size, true);
        eprintln!("meshes: {:.2} s", started.elapsed().as_secs_f32());
        let mut raster = OffscreenRaster::new(&device);
        eprintln!("pipelines: {:.2} s", started.elapsed().as_secs_f32());
        raster.render(&device, &queue, &frame, size)
    } else {
        let mut sky = SkyCache::default();
        let sky = sky.get(&doc.data.render);
        let inputs = PtInputs::from_document(doc, sky.clone());
        eprintln!("scene: {:.2} s", started.elapsed().as_secs_f32());
        let mut pt = PathTracer::new(&device);
        inputs.upload(&mut pt, &device, &queue);
        eprintln!(
            "pipelines and upload: {:.2} s",
            started.elapsed().as_secs_f32()
        );
        let params = document_params(
            doc,
            &state.camera,
            size,
            transparent,
            sky.as_ref().map(|(_, i)| i.as_ref()),
        );
        let samples = spp.unwrap_or(doc.data.render.sample.samples);
        let mut job = PhotoJob::new(params, samples, None);
        job.finish(&mut pt, &device, &queue).to_rgba8()
    };
    let secs = started.elapsed().as_secs_f32();
    render_job::save_png(output, &image).unwrap_or_else(|e| fail(e));
    eprintln!(
        "{} ({}×{}) in {secs:.2} s",
        output.display(),
        size[0],
        size[1]
    );
}

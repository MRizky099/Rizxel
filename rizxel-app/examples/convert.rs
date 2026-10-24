//! Converts between the formats Rizxel imports and exports, headless:
//!
//! ```text
//! cargo run -p rizxel-app --example convert -- input.vox output.glb
//! cargo run -p rizxel-app --example convert -- --scale 1 --pivot corner input.vox output.obj
//! ```
//!
//! Inputs: `.rzxl`, `.vox`, `.gox`, `.qb`, `.qbt`, `.qef`, `.obj`, `.stl`,
//! `.ply`. Outputs: `.rzxl`, `.vox`, `.gox`, `.qb`, `.qbt`, `.qef`, `.obj`
//! (+ `.mtl` + `.png`), `.stl`, `.ply`, `.mtl` (+ `.png`), `.glb`, `.gltf`
//! (+ `.bin`).
//!
//! Mesh options (OBJ, STL, PLY, glTF):
//! - `--scale <units per voxel>` (default: the format's own)
//! - `--pivot bottom-center|center|corner`
//! - `--gltf-colors vertex|texture`

use std::path::PathBuf;

use rizxel_app::{AppState, ExportOptions, FileFormat, GltfColors, Pivot};

fn fail(msg: impl std::fmt::Display) -> ! {
    eprintln!("{msg}");
    std::process::exit(2);
}

fn main() {
    let mut options = ExportOptions::default();
    let mut paths: Vec<PathBuf> = Vec::new();
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        let flag = arg.to_string_lossy().into_owned();
        if !flag.starts_with("--") {
            paths.push(PathBuf::from(arg));
            continue;
        }
        let value = args
            .next()
            .map(|v| v.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_else(|| fail(format!("{flag} needs a value")));
        match flag.as_str() {
            "--scale" => {
                options.scale = Some(
                    value
                        .parse()
                        .ok()
                        .filter(|s: &f32| s.is_finite() && *s > 0.0)
                        .unwrap_or_else(|| fail("--scale needs a positive number")),
                )
            }
            "--pivot" => {
                options.pivot = match value.as_str() {
                    "bottom-center" => Pivot::BottomCenter,
                    "center" => Pivot::Center,
                    "corner" => Pivot::Corner,
                    _ => fail("--pivot is bottom-center, center or corner"),
                }
            }
            "--gltf-colors" => {
                options.gltf_colors = match value.as_str() {
                    "vertex" => GltfColors::Vertex,
                    "texture" => GltfColors::Texture,
                    _ => fail("--gltf-colors is vertex or texture"),
                }
            }
            _ => fail(format!("unknown option {flag}")),
        }
    }
    let [input, output] = paths.as_slice() else {
        fail("usage: convert [options] <input> <output>");
    };
    let Some(format) = FileFormat::from_path(output) else {
        fail(format!("unsupported output type: {}", output.display()));
    };
    let mut state = AppState::default();
    match state.import(input) {
        Ok(msg) => println!("{msg}"),
        Err(e) => {
            eprintln!("import failed: {e}");
            std::process::exit(1);
        }
    }
    if let Some(doc) = state.doc() {
        let b = doc.data.bounds;
        println!(
            "  bounds {}×{}×{}, {} layers, {} palette colors",
            b.x(),
            b.y(),
            b.z(),
            doc.data.layers.len(),
            doc.data.palette.len()
        );
    }
    match state.export_active_with(output, format, &options) {
        Ok(msg) => println!("{msg}"),
        Err(e) => {
            eprintln!("export failed: {e}");
            std::process::exit(1);
        }
    }
}

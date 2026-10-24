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
//! - `--obj-layout fast|magicavoxel` (default fast)
//!
//! Export Slices: `--slices x|y|z [--slice-scale N] input outdir` writes
//! one PNG per slice into the folder `outdir`.
//!
//! Imports: `--heightmap MAX` reads a PNG/JPEG input as a heightmap with
//! columns up to MAX voxels; `--resolution N` voxelizes OBJ/STL/PLY meshes
//! to N voxels on the longest side instead of detecting their grid;
//! `--turn x|y|z` turns a mesh a quarter turn about that axis (repeatable).

use std::path::PathBuf;

use rizxel_app::{
    AppState, ExportOptions, FileFormat, GltfColors, HeightmapOptions, ObjLayout, Pivot, SliceAxis,
    VoxelizerSettings,
};

fn fail(msg: impl std::fmt::Display) -> ! {
    eprintln!("{msg}");
    std::process::exit(2);
}

fn main() {
    let mut options = ExportOptions::default();
    let mut slices: Option<SliceAxis> = None;
    let mut slice_scale = 1u32;
    let mut heightmap: Option<u32> = None;
    let mut voxelizer = VoxelizerSettings::default();
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
            "--heightmap" => {
                heightmap = Some(
                    value
                        .parse()
                        .unwrap_or_else(|_| fail("--heightmap needs a height in voxels")),
                )
            }
            "--resolution" => {
                voxelizer.voxelize.detect_grid = false;
                voxelizer.voxelize.resolution = value
                    .parse()
                    .unwrap_or_else(|_| fail("--resolution needs a whole number"));
            }
            "--turn" => {
                let axis = match value.as_str() {
                    "x" => 0,
                    "y" => 1,
                    "z" => 2,
                    _ => fail("--turn is x, y or z"),
                };
                voxelizer.turns[axis] = (voxelizer.turns[axis] + 1) % 4;
            }
            "--slices" => {
                slices = Some(match value.as_str() {
                    "x" => SliceAxis::X,
                    "y" => SliceAxis::Y,
                    "z" => SliceAxis::Z,
                    _ => fail("--slices is x, y or z"),
                })
            }
            "--slice-scale" => {
                slice_scale = value
                    .parse()
                    .unwrap_or_else(|_| fail("--slice-scale needs a whole number"))
            }
            "--obj-layout" => {
                options.obj_layout = match value.as_str() {
                    "fast" | "fast-load" => ObjLayout::FastLoad,
                    "magicavoxel" | "magica" => ObjLayout::MagicaVoxel,
                    _ => fail("--obj-layout is fast or magicavoxel"),
                }
            }
            _ => fail(format!("unknown option {flag}")),
        }
    }
    let [input, output] = paths.as_slice() else {
        fail("usage: convert [options] <input> <output>");
    };
    let format = FileFormat::from_path(output);
    if slices.is_none() && format.is_none() {
        fail(format!("unsupported output type: {}", output.display()));
    }
    let mut state = AppState::default();
    let is_mesh = matches!(
        FileFormat::from_path(input),
        Some(FileFormat::Obj | FileFormat::Stl | FileFormat::Ply)
    );
    let imported = match heightmap {
        Some(max_height) => state.import_heightmap(input, None, HeightmapOptions { max_height }),
        None if is_mesh && voxelizer != VoxelizerSettings::default() => state
            .load_mesh(input)
            .and_then(|mesh| state.import_mesh(&mesh, &voxelizer)),
        None => state.import(input),
    };
    match imported {
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
    let result = match (slices, format) {
        (Some(axis), _) => state.export_slices(output, axis, slice_scale),
        (None, Some(format)) => state.export_active_with(output, format, &options),
        (None, None) => unreachable!(),
    };
    match result {
        Ok(msg) => println!("{msg}"),
        Err(e) => {
            eprintln!("export failed: {e}");
            std::process::exit(1);
        }
    }
}

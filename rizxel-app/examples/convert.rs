//! Converts between the formats Rizxel imports and exports, headless:
//!
//! ```text
//! cargo run -p rizxel-app --example convert -- input.vox output.obj
//! ```
//!
//! Inputs: `.rzxl`, `.vox`, `.gox`, `.obj`, `.stl`. Outputs: `.rzxl`,
//! `.vox`, `.gox`, `.obj` (+ `.mtl` + `.png`), `.stl`, `.mtl` (+ `.png`).

use std::path::PathBuf;

use rizxel_app::{AppState, FileFormat};

fn main() {
    let args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let [input, output] = args.as_slice() else {
        eprintln!("usage: convert <input> <output>");
        std::process::exit(2);
    };
    let Some(format) = FileFormat::from_path(output) else {
        eprintln!("unsupported output type: {}", output.display());
        std::process::exit(2);
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
    match state.export_active(output, format) {
        Ok(msg) => println!("{msg}"),
        Err(e) => {
            eprintln!("export failed: {e}");
            std::process::exit(1);
        }
    }
}

//! User-level commands that need OS interaction (file dialogs) on top of
//! [`AppState`]. Kept separate from panels so menus, buttons and shortcuts
//! share one implementation.

use rizxel_app::{AppState, FileFormat, RZXL_EXTENSION};

use crate::panels::export::{Dialogs, HeightmapDialog, VoxelizerDialog};

fn rzxl_dialog() -> rfd::FileDialog {
    rfd::FileDialog::new().add_filter("Rizxel Project", &[RZXL_EXTENSION])
}

fn format_filter(dialog: rfd::FileDialog, format: FileFormat) -> rfd::FileDialog {
    dialog.add_filter(format.label(), &[format.extension()])
}

pub fn new_document(state: &mut AppState) {
    state.new_document();
}

/// Open… accepts Rizxel projects and every importable model format.
pub fn open(state: &mut AppState) {
    let models: Vec<&str> = std::iter::once(FileFormat::Rzxl)
        .chain(
            FileFormat::IMPORTS
                .into_iter()
                .filter(|f| *f != FileFormat::Mtl),
        )
        .map(FileFormat::extension)
        .collect();
    let mut dialog = rfd::FileDialog::new()
        .set_title("Open")
        .add_filter("All Supported Models", &models);
    for f in std::iter::once(FileFormat::Rzxl).chain(FileFormat::IMPORTS) {
        dialog = format_filter(dialog, f);
    }
    let Some(path) = dialog.pick_file() else {
        return;
    };
    open_path(state, &path);
}

/// Opens or imports `path` by its extension; reports in the status bar.
pub fn open_path(state: &mut AppState, path: &std::path::Path) {
    match state.import(path) {
        Ok(msg) => state.status_message = Some(msg),
        Err(e) => {
            log::error!("open {}: {e}", path.display());
            state.status_message = Some(format!("Failed to open: {e}"));
        }
    }
}

/// File ▸ Import ▸ `format`. Meshes (OBJ, STL, PLY) open the Voxelizer
/// dialog first.
pub fn import(state: &mut AppState, dialogs: &mut Dialogs, format: FileFormat) {
    let title = format!("Import {}", format.label());
    let Some(path) = format_filter(rfd::FileDialog::new(), format)
        .set_title(title)
        .pick_file()
    else {
        return;
    };
    if !matches!(format, FileFormat::Obj | FileFormat::Stl | FileFormat::Ply) {
        open_path(state, &path);
        return;
    }
    match state.load_mesh(&path) {
        Ok(mesh) => {
            dialogs.voxelizer = Some(VoxelizerDialog::new(mesh, dialogs.voxelizer_settings));
        }
        Err(e) => {
            log::error!("import {}: {e}", path.display());
            state.status_message = Some(format!("Failed to import: {e}"));
        }
    }
}

/// File ▸ Import ▸ Heightmap: picks the image, then opens its dialog.
pub fn import_heightmap(dialogs: &mut Dialogs) {
    let Some(path) = rfd::FileDialog::new()
        .set_title("Import Heightmap")
        .add_filter("Image", &rizxel_app::IMAGE_EXTENSIONS)
        .pick_file()
    else {
        return;
    };
    dialogs.heightmap = Some(HeightmapDialog {
        path,
        color: None,
        options: rizxel_app::HeightmapOptions::default(),
    });
}

/// File ▸ Export ▸ `format`. Mesh formats open their options first.
pub fn export(state: &mut AppState, dialogs: &mut Dialogs, format: FileFormat) {
    if format.is_mesh() {
        dialogs.export = Some(format);
    } else {
        export_with(state, format, &rizxel_app::ExportOptions::default());
    }
}

/// Asks for the file name, then exports with `options`.
pub fn export_with(state: &mut AppState, format: FileFormat, options: &rizxel_app::ExportOptions) {
    let ext = format.extension();
    let file_name = state
        .doc()
        .map(|d| format!("{}.{ext}", d.name))
        .unwrap_or_default();
    let Some(mut path) = export_dialog(state, format)
        .set_title(format!("Export {}", format.label()))
        .set_file_name(file_name)
        .save_file()
    else {
        return;
    };
    if path.extension().is_none() {
        path.set_extension(ext);
    }
    match state.export_active_with(&path, format, options) {
        Ok(msg) => state.status_message = Some(msg),
        Err(e) => {
            log::error!("export {}: {e}", path.display());
            state.status_message = Some(format!("Failed to export: {e}"));
        }
    }
}

/// Save dialog for `format`, starting in the default export folder
/// (*Settings ▸ Paths*) when one is set.
pub fn export_dialog(state: &AppState, format: FileFormat) -> rfd::FileDialog {
    format_filter(export_folder_dialog(state), format)
}

/// File dialog starting in the default export folder, if one is set.
pub fn export_folder_dialog(state: &AppState) -> rfd::FileDialog {
    let dialog = rfd::FileDialog::new();
    match &state.settings.paths.export_folder {
        Some(dir) if dir.is_dir() => dialog.set_directory(dir),
        _ => dialog,
    }
}

fn palette_dialog() -> rfd::FileDialog {
    rfd::FileDialog::new()
        .add_filter(
            "Palette",
            &[rizxel_app::GPL_EXTENSION, rizxel_app::HEX_EXTENSION],
        )
        .add_filter("GIMP Palette", &[rizxel_app::GPL_EXTENSION])
        .add_filter("Hex Palette", &[rizxel_app::HEX_EXTENSION])
}

/// *Palette ▸ Load* (or *Load and Snap Model* with `snap`).
pub fn load_palette(state: &mut AppState, snap: bool) {
    let Some(path) = palette_dialog().set_title("Load Palette").pick_file() else {
        return;
    };
    if let Err(e) = state.load_palette_file(&path, snap) {
        log::error!("palette {}: {e}", path.display());
        state.status_message = Some(format!("Failed to load the palette: {e}"));
    }
}

/// *Palette ▸ Save*.
pub fn save_palette(state: &mut AppState) {
    let file_name = state
        .doc()
        .map(|d| format!("{}.{}", d.name, rizxel_app::GPL_EXTENSION))
        .unwrap_or_default();
    let Some(mut path) = palette_dialog()
        .set_title("Save Palette")
        .set_file_name(file_name)
        .save_file()
    else {
        return;
    };
    if path.extension().is_none() {
        path.set_extension(rizxel_app::GPL_EXTENSION);
    }
    if let Err(e) = state.save_palette_file(&path) {
        log::error!("palette {}: {e}", path.display());
        state.status_message = Some(format!("Failed to save the palette: {e}"));
    }
}

/// Saves to the existing path or falls back to "Save As".
pub fn save(state: &mut AppState) {
    match state.doc().and_then(|d| d.path.clone()) {
        Some(path) => save_to(state, &path),
        None => save_as(state),
    }
}

pub fn save_as(state: &mut AppState) {
    let file_name = state.doc().map(|d| d.title()).unwrap_or_default();
    let Some(mut path) = rzxl_dialog()
        .set_title("Save Rizxel Project")
        .set_file_name(file_name)
        .save_file()
    else {
        return;
    };
    if path.extension().is_none() {
        path.set_extension(RZXL_EXTENSION);
    }
    save_to(state, &path);
}

fn save_to(state: &mut AppState, path: &std::path::Path) {
    match state.save_active_as(path) {
        Ok(()) => state.status_message = Some(format!("Saved {}", path.display())),
        Err(e) => {
            log::error!("save {}: {e}", path.display());
            state.status_message = Some(format!("Failed to save: {e}"));
        }
    }
}

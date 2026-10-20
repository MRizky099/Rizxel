//! User-level commands that need OS interaction (file dialogs) on top of
//! [`AppState`]. Kept separate from panels so menus, buttons and shortcuts
//! share one implementation.

use rizxel_app::{AppState, FileFormat, RZXL_EXTENSION};

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

/// File ▸ Import ▸ `format`.
pub fn import(state: &mut AppState, format: FileFormat) {
    let title = format!("Import {}", format.label());
    let Some(path) = format_filter(rfd::FileDialog::new(), format)
        .set_title(title)
        .pick_file()
    else {
        return;
    };
    open_path(state, &path);
}

/// File ▸ Export ▸ `format`.
pub fn export(state: &mut AppState, format: FileFormat) {
    let ext = format.extension();
    let file_name = state
        .doc()
        .map(|d| format!("{}.{ext}", d.name))
        .unwrap_or_default();
    let Some(mut path) = format_filter(rfd::FileDialog::new(), format)
        .set_title(format!("Export {}", format.label()))
        .set_file_name(file_name)
        .save_file()
    else {
        return;
    };
    if path.extension().is_none() {
        path.set_extension(ext);
    }
    match state.export_active(&path, format) {
        Ok(msg) => state.status_message = Some(msg),
        Err(e) => {
            log::error!("export {}: {e}", path.display());
            state.status_message = Some(format!("Failed to export: {e}"));
        }
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

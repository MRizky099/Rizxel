//! User-level commands that need OS interaction (file dialogs) on top of
//! [`AppState`]. Kept separate from panels so menus, buttons and shortcuts
//! share one implementation.

use rizxel_app::{AppState, OBJ_EXTENSION, RZXL_EXTENSION};

fn rzxl_dialog() -> rfd::FileDialog {
    rfd::FileDialog::new().add_filter("Rizxel Project", &[RZXL_EXTENSION])
}

pub fn new_document(state: &mut AppState) {
    state.new_document();
}

pub fn open(state: &mut AppState) {
    let Some(path) = rzxl_dialog().set_title("Open Rizxel Project").pick_file() else {
        return;
    };
    match state.open(&path) {
        Ok(()) => state.status_message = Some(format!("Opened {}", path.display())),
        Err(e) => {
            log::error!("open {}: {e}", path.display());
            state.status_message = Some(format!("Failed to open: {e}"));
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

/// Exports the visible layers of the active document as OBJ.
pub fn export_obj(state: &mut AppState) {
    let file_name = state
        .doc()
        .map(|d| format!("{}.{OBJ_EXTENSION}", d.name))
        .unwrap_or_default();
    let Some(mut path) = rfd::FileDialog::new()
        .add_filter("Wavefront OBJ", &[OBJ_EXTENSION])
        .set_title("Export OBJ")
        .set_file_name(file_name)
        .save_file()
    else {
        return;
    };
    if path.extension().is_none() {
        path.set_extension(OBJ_EXTENSION);
    }
    match state.export_active_obj(&path) {
        Ok(triangles) => {
            state.status_message = Some(format!(
                "Exported {} ({triangles} triangles)",
                path.display()
            ))
        }
        Err(e) => {
            log::error!("export {}: {e}", path.display());
            state.status_message = Some(format!("Failed to export: {e}"));
        }
    }
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

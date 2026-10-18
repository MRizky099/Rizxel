//! User-level commands that need OS interaction (file dialogs) on top of
//! [`AppState`]. Kept separate from panels so menus, buttons and shortcuts
//! share one implementation.

use rizxel_app::{AppState, RZXL_EXTENSION};

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

fn save_to(state: &mut AppState, path: &std::path::Path) {
    match state.save_active_as(path) {
        Ok(()) => state.status_message = Some(format!("Saved {}", path.display())),
        Err(e) => {
            log::error!("save {}: {e}", path.display());
            state.status_message = Some(format!("Failed to save: {e}"));
        }
    }
}

//! UI panels, one module per dockable region of the workspace.

pub mod export;
pub mod history;
pub mod options;
pub mod references;
pub mod sidebar;
pub mod status;
pub mod tabs;
pub mod tools;
pub mod top_bar;

/// Transient, UI-only state (dialogs, inline edits). Never persisted.
#[derive(Default)]
pub struct UiState {
    pub show_about: bool,
    pub show_shortcuts: bool,
    /// Layer currently being renamed and its edit buffer.
    pub renaming_layer: Option<(usize, String)>,
    /// Zen mode: hide all panels except the viewport and status bar.
    pub zen_mode: bool,
    /// Autosaves offered for recovery at startup (dialog open while non-empty).
    pub recovery: Vec<rizxel_app::RecoveryEntry>,
    /// Color Palette panel: show only the colors the model uses.
    pub palette_used_only: bool,
    /// Second swatch for Gradient and Remap (`Shift+click`).
    pub palette_mark: Option<usize>,
    /// Reference image being renamed and its edit buffer.
    pub renaming_reference: Option<(usize, String)>,
    /// References panel: width and height change independently.
    pub reference_free_aspect: bool,
    /// References panel thumbnails.
    pub reference_thumbs: references::Thumbnails,
    /// Import and export dialogs.
    pub dialogs: export::Dialogs,
}

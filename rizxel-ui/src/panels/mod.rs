//! UI panels, one module per dockable region of the workspace.

pub mod export;
pub mod history;
pub mod options;
pub mod references;
pub mod render;
pub mod settings;
pub mod sidebar;
pub mod start;
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
    /// Render panel tabs, presets and the running output job.
    pub render: render::RenderUi,
    /// The open *Settings* dialog.
    pub settings: Option<settings::SettingsDialog>,
    /// The start screen (shown at start without a command-line file).
    pub start: Option<start::StartScreen>,
    /// Guide page a keyboard command asked to open (opened next frame,
    /// where the context is at hand).
    pub open_guide: Option<String>,
    /// The pointer was over the Layers panel last frame: `Ctrl+A` then
    /// selects every layer instead of every voxel. Cleared after the
    /// shortcuts run, set again by the panel.
    pub layers_hovered: bool,
}

impl UiState {
    /// Opens *Settings* on a copy of the current settings.
    pub fn open_settings(&mut self, state: &rizxel_app::AppState) {
        if self.settings.is_none() {
            self.settings = Some(settings::SettingsDialog::new(&state.settings));
        }
    }
}

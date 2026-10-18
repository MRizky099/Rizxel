//! UI panels, one module per region of the HTML prototype.

pub mod options;
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
    /// Collapsed state of sidebar sections.
    pub palette_collapsed: bool,
    pub bounds_collapsed: bool,
}

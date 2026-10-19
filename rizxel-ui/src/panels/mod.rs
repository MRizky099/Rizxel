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
    /// Zen mode: hide all panels except viewport, tabs, and status bar.
    pub zen_mode: bool,
    /// Sidebar section draw order, permuted by dragging headers.
    /// 0 = palette, 1 = layers, 2 = bounds.
    pub sidebar_order: SidebarOrder,
}

/// Sidebar draw order; defaults to palette, layers, bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SidebarOrder(pub [usize; 3]);

impl Default for SidebarOrder {
    fn default() -> Self {
        Self([0, 1, 2])
    }
}

impl std::ops::Deref for SidebarOrder {
    type Target = [usize; 3];
    fn deref(&self) -> &[usize; 3] {
        &self.0
    }
}

impl std::ops::DerefMut for SidebarOrder {
    fn deref_mut(&mut self) -> &mut [usize; 3] {
        &mut self.0
    }
}

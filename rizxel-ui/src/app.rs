//! eframe application: owns [`AppState`] and lays out all panels in the same
//! order as the HTML prototype.

use egui::{Frame, Key, KeyboardShortcut, Modifiers, Panel};
use rizxel_app::AppState;

use eframe::egui_wgpu::wgpu;

use crate::render::ViewportRenderer;
use crate::{actions, icons::Icons, panels, resize, theme, viewport};

pub struct RizxelApp {
    state: AppState,
    ui: panels::UiState,
    icons: Icons,
    /// Framebuffer format of the wgpu surface; `None` if wgpu is unavailable.
    target_format: Option<wgpu::TextureFormat>,
}

impl RizxelApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::install_fonts(&cc.egui_ctx);
        theme::apply(&cc.egui_ctx);
        let target_format = cc.wgpu_render_state.as_ref().map(|rs| {
            ViewportRenderer::install(rs);
            log::info!(
                "viewport renderer ready ({:?}, {:?})",
                rs.adapter.get_info().backend,
                rs.target_format
            );
            rs.target_format
        });
        Self {
            state: AppState::default(),
            ui: panels::UiState::default(),
            icons: Icons::load(&cc.egui_ctx),
            target_format,
        }
    }

    /// Global keyboard shortcuts. Skipped while a text field has focus so
    /// e.g. Ctrl+Z edits text instead of the document.
    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        const CMD: Modifiers = Modifiers::COMMAND;
        const CMD_SHIFT: Modifiers = Modifiers::COMMAND.plus(Modifiers::SHIFT);

        // Consume longer combos first (Ctrl+Shift+S before Ctrl+S).
        let pressed = |s: KeyboardShortcut| ctx.input_mut(|i| i.consume_shortcut(&s));
        if pressed(KeyboardShortcut::new(CMD_SHIFT, Key::Z))
            || pressed(KeyboardShortcut::new(CMD, Key::Y))
        {
            self.state.redo();
        }
        if pressed(KeyboardShortcut::new(CMD, Key::Z)) {
            self.state.undo();
        }
        if pressed(KeyboardShortcut::new(CMD_SHIFT, Key::S)) {
            actions::save_as(&mut self.state);
        }
        if pressed(KeyboardShortcut::new(CMD, Key::S)) {
            actions::save(&mut self.state);
        }
        if pressed(KeyboardShortcut::new(CMD, Key::O)) {
            actions::open(&mut self.state);
        }
        if pressed(KeyboardShortcut::new(CMD, Key::N)) {
            actions::new_document(&mut self.state);
        }
        if pressed(KeyboardShortcut::new(CMD_SHIFT, Key::F)) {
            self.ui.zen_mode = !self.ui.zen_mode;
        }
    }
}

impl eframe::App for RizxelApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        egui::Rgba::from(theme::BG_APP).to_array()
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_shortcuts(&ctx);

        let state = &mut self.state;
        let ui_state = &mut self.ui;
        let flat = |fill| Frame::NONE.fill(fill);

        // Panel order matters: outermost first, central panel last.
        Panel::top("title_bar")
            .exact_size(theme::TITLE_BAR_HEIGHT)
            .frame(flat(theme::BG_HEADER))
            .show_separator_line(false)
            .resizable(false)
            .show(ui, |ui| panels::top_bar::show(ui, state, ui_state));

        // Zen mode: only show tabs, viewport, and status bar.
        if !ui_state.zen_mode {
            Panel::top("option_bar")
                .exact_size(theme::OPTION_BAR_HEIGHT)
                .frame(flat(theme::BG_PANEL).inner_margin(egui::Margin::symmetric(8, 0)))
                .show_separator_line(false)
                .resizable(false)
                .show(ui, |ui| panels::options::show(ui, state, &self.icons));
        }

        Panel::top("doc_tabs")
            .exact_size(theme::TAB_BAR_HEIGHT)
            .frame(flat(theme::BG_APP))
            .show_separator_line(false)
            .resizable(false)
            .show(ui, |ui| panels::tabs::show(ui, state));

        Panel::bottom("status_bar")
            .exact_size(theme::STATUS_BAR_HEIGHT)
            .frame(flat(theme::BG_APP).inner_margin(egui::Margin::symmetric(12, 0)))
            .show_separator_line(false)
            .resizable(false)
            .show(ui, |ui| panels::status::show(ui, state));

        if !ui_state.zen_mode {
            Panel::left("tools")
                .default_size(theme::TOOLS_WIDTH)
                .size_range(48.0..=200.0)
                .frame(flat(theme::BG_PANEL))
                .show_separator_line(true)
                .resizable(true)
                .show(ui, |ui| panels::tools::show(ui, state, &self.icons));

            Panel::right("sidebar")
                .default_size(theme::SIDEBAR_WIDTH)
                .size_range(200.0..=400.0)
                .frame(flat(theme::BG_PANEL))
                .show_separator_line(true)
                .resizable(true)
                .show(ui, |ui| panels::sidebar::show(ui, state, ui_state));
        }

        egui::CentralPanel::no_frame().show(ui, |ui| viewport::show(ui, state, self.target_format));

        // Modal dialogs and frameless window chrome draw on top of everything.
        panels::top_bar::show_dialogs(&ctx, state, ui_state);
        resize::handle_edges(&ctx);
        resize::paint_border(&ctx);
    }
}

//! eframe application: owns [`AppState`] and lays out all panels in the same
//! order as the HTML prototype.

use egui::{Frame, Key, KeyboardShortcut, Modifiers, Panel, ViewportCommand};
use rizxel_app::AppState;

use eframe::egui_wgpu::wgpu;

use crate::render::ViewportRenderer;
use crate::tool_input::ToolInput;
use crate::{actions, dock, icons::Icons, panels, resize, theme, viewport};

pub struct RizxelApp {
    state: AppState,
    ui: panels::UiState,
    icons: Icons,
    /// Viewport meshes per open document (re-meshed once per revision).
    meshes: viewport::MeshCache,
    /// Left-drag tool state of the viewport.
    tools: ToolInput,
    /// Framebuffer format of the wgpu surface; `None` if wgpu is unavailable.
    target_format: Option<wgpu::TextureFormat>,
    /// Dockable panel layout of the editor workspace.
    dock: dock::Workspace,
    /// Every document is answered for: the next close request is let through.
    quitting: bool,
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
        let state = AppState::with_paths(rizxel_app::AppPaths::new());
        // Recovery files left by a session that did not exit cleanly.
        let ui = panels::UiState {
            recovery: state.recoverable(),
            ..Default::default()
        };
        Self {
            state,
            ui,
            icons: Icons::load(&cc.egui_ctx),
            meshes: viewport::MeshCache::default(),
            tools: ToolInput::default(),
            target_format,
            dock: dock::Workspace::default(),
            quitting: false,
        }
    }

    /// Opens a project or model passed on the command line (`.rzxl`,
    /// `.vox`, `.gox`, `.obj`, `.stl`); failures go to the status bar.
    pub fn open_at_startup(&mut self, path: &std::path::Path) {
        actions::open_path(&mut self.state, path);
    }

    /// Every exit path (title bar, *File ▸ Exit*, `Alt+F4`, the OS) ends in
    /// a close request. While documents are unsaved it is cancelled and the
    /// "Save changes?" prompt walks them; its last answer closes for real.
    fn handle_close(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) && !self.quitting {
            if self.state.request_exit() {
                self.quit();
            } else {
                ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            }
        }
        if panels::top_bar::close_prompt(ctx, &mut self.state) {
            self.quit();
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
    }

    /// Clean exit: lets the close through and deletes the recovery files.
    fn quit(&mut self) {
        self.quitting = true;
        self.state.clean_exit();
    }

    /// Global keyboard shortcuts. Skipped while a text field has focus so
    /// e.g. Ctrl+Z edits text instead of the document, and while a modal
    /// prompt is open.
    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let modal = self.state.closing().is_some() || !self.ui.recovery.is_empty();
        if ctx.egui_wants_keyboard_input() || modal {
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
        // `/`: show or hide the work plane (hiding it also ends Slice Mode).
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Slash)) {
            self.state.toggle_plane_visible();
        }
        // Tool shortcuts: plain letters (Ctrl/Shift/Alt combos are left to
        // the commands above and to the tools' own modifiers).
        for tool in rizxel_engine::Tool::ALL {
            let Some(key) = Key::from_name(&tool.shortcut().to_string()) else {
                continue;
            };
            if pressed(KeyboardShortcut::new(Modifiers::NONE, key)) {
                self.state.tool = tool;
            }
        }
    }
}

impl eframe::App for RizxelApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        egui::Rgba::from(theme::BG_APP).to_array()
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_close(&ctx);
        panels::top_bar::recovery_dialog(&ctx, &mut self.state, &mut self.ui);
        self.state.autosave_tick(std::time::Instant::now());
        // Keep ticking while idle so the time-based autosave still fires.
        ctx.request_repaint_after(std::time::Duration::from_secs(5));
        self.handle_shortcuts(&ctx);

        let state = &mut self.state;
        let ui_state = &mut self.ui;
        let icons = &self.icons;
        let meshes = &mut self.meshes;
        let tools = &mut self.tools;
        let target_format = self.target_format;
        let dock = &mut self.dock;
        let flat = |fill| Frame::NONE.fill(fill);

        // Panel order matters: outermost first, central panel last.
        Panel::top("title_bar")
            .exact_size(theme::TITLE_BAR_HEIGHT)
            .frame(flat(theme::BG_HEADER))
            .show_separator_line(false)
            .resizable(false)
            .show(ui, |ui| panels::top_bar::show(ui, state, ui_state, dock));

        Panel::bottom("status_bar")
            .exact_size(theme::STATUS_BAR_HEIGHT)
            .frame(flat(theme::BG_APP).inner_margin(egui::Margin::symmetric(12, 0)))
            .show_separator_line(false)
            .resizable(false)
            .show(ui, |ui| panels::status::show(ui, state));

        // Zen mode: only the viewport (with its document tabs) and status bar.
        egui::CentralPanel::no_frame().show(ui, |ui| {
            if ui_state.zen_mode {
                viewport_panel(ui, state, target_format, meshes, tools);
                return;
            }
            let error = dock.show(ui, |ui, panel| match panel {
                dock::Panel::Viewport => viewport_panel(ui, state, target_format, meshes, tools),
                dock::Panel::Tools => {
                    scroll_panel(ui, panel, |ui| panels::tools::show(ui, state, icons))
                }
                dock::Panel::Options => scroll_panel(ui, panel, |ui| {
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::symmetric(8, 0))
                        .show(ui, |ui| panels::options::show(ui, state, icons));
                }),
                dock::Panel::Palette => {
                    scroll_panel(ui, panel, |ui| panels::sidebar::palette(ui, state))
                }
                dock::Panel::Layers => {
                    scroll_panel(ui, panel, |ui| panels::sidebar::layers(ui, state, ui_state))
                }
                dock::Panel::Bounds => {
                    scroll_panel(ui, panel, |ui| panels::sidebar::bounds(ui, state))
                }
            });
            if let Some(error) = error {
                log::warn!("{error}");
                state.status_message = Some(error);
            }
        });

        // Modal dialogs and frameless window chrome draw on top of everything.
        panels::top_bar::show_dialogs(&ctx, state, ui_state);
        resize::handle_edges(&ctx);
        resize::paint_border(&ctx);
    }
}

/// Panel body that scrolls (both ways) when its content outgrows its dock
/// group or floating window.
fn scroll_panel(ui: &mut egui::Ui, panel: &dock::Panel, add: impl FnOnce(&mut egui::Ui)) {
    egui::ScrollArea::both()
        .id_salt(("dock_scroll", panel))
        .auto_shrink([false, false])
        .show(ui, add);
}

/// Viewport panel: document tabs above the 3D view, so they move with it.
fn viewport_panel(
    ui: &mut egui::Ui,
    state: &mut AppState,
    target_format: Option<wgpu::TextureFormat>,
    meshes: &mut viewport::MeshCache,
    tools: &mut ToolInput,
) {
    let rect = ui.max_rect();
    let (tabs_rect, view_rect) = rect.split_top_bottom_at_y(rect.top() + theme::TAB_BAR_HEIGHT);
    ui.painter().rect_filled(tabs_rect, 0.0, theme::BG_APP);
    ui.scope_builder(egui::UiBuilder::new().max_rect(tabs_rect), |ui| {
        panels::tabs::show(ui, state);
    });
    ui.scope_builder(egui::UiBuilder::new().max_rect(view_rect), |ui| {
        viewport::show(ui, state, target_format, meshes, tools);
    });
}

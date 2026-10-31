//! eframe application: owns [`AppState`] and lays out all panels in the same
//! order as the HTML prototype.

use egui::{Frame, Key, Panel, ViewportCommand};
use rizxel_app::{ActionId, AppState};

use eframe::egui_wgpu::wgpu;

use crate::render::ViewportRenderer;
use crate::tool_input::ToolInput;
use crate::{actions, dock, icons::Icons, keys, panels, resize, theme, viewport};

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
    /// Dockable panel layouts, one per view mode.
    docks: dock::Workspaces,
    /// Every document is answered for: the next close request is let through.
    quitting: bool,
    /// The window's wgpu device (renders outputs); `None` without wgpu.
    render_state: Option<eframe::egui_wgpu::RenderState>,
    /// Image, photo-realistic and turntable outputs.
    outputs: crate::output::Outputs,
    /// Renders the start-screen thumbnails (created on first use).
    thumbs: Option<crate::render::offscreen::OffscreenRaster>,
    /// The layouts and window state last written to `layout.json`.
    layout_saved: (dock::Workspaces, Option<dock::WindowState>),
    /// When the layout or window first differed from `layout_saved`.
    layout_changed: Option<std::time::Instant>,
    /// Current window state (size and position while not maximized).
    window: Option<dock::WindowState>,
    /// Receives files from later launches (`None`: no app data folder).
    instance: Option<crate::single_instance::Server>,
}

/// Unsaved layout changes are written after this quiet period.
const LAYOUT_SAVE_DELAY: std::time::Duration = std::time::Duration::from_secs(2);

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
        let mut state = AppState::with_paths(rizxel_app::AppPaths::new());
        // Compute shaders (not the GL backend) for the path tracer.
        state.render_view.path_tracing = cc
            .wgpu_render_state
            .as_ref()
            .is_some_and(|rs| crate::render::pathtrace::supported(&rs.adapter, &rs.device));
        log::info!("path tracer available: {}", state.render_view.path_tracing);
        // Recovery files left by a session that did not exit cleanly.
        let ui = panels::UiState {
            recovery: state.recoverable(),
            ..Default::default()
        };
        let (docks, window) = match state.paths() {
            Some(p) => dock::Workspaces::load(&p.layout_file()),
            None => (dock::Workspaces::default(), None),
        };
        Self {
            state,
            ui,
            icons: Icons::load(&cc.egui_ctx),
            meshes: viewport::MeshCache::default(),
            tools: ToolInput::default(),
            target_format,
            layout_saved: (docks.clone(), window),
            docks,
            quitting: false,
            render_state: cc.wgpu_render_state.clone(),
            outputs: Default::default(),
            thumbs: None,
            layout_changed: None,
            window,
            instance: None,
        }
    }

    /// Opens files handed over by later launches; `ctx` is woken when they
    /// arrive so they open while the window is idle.
    pub fn set_instance(&mut self, server: crate::single_instance::Server, ctx: &egui::Context) {
        let ctx = ctx.clone();
        server.set_waker(move || ctx.request_repaint());
        self.instance = Some(server);
    }

    /// Files from a second launch open as tabs and the window comes to the
    /// front (also for a launch without files).
    fn handle_instance(&mut self, ctx: &egui::Context) {
        let Some(server) = &self.instance else {
            return;
        };
        let Some(paths) = server.poll() else {
            return;
        };
        self.ui.start = None;
        for path in &paths {
            actions::open_path(&mut self.state, path);
        }
        ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(ViewportCommand::Focus);
        ctx.send_viewport_cmd(ViewportCommand::RequestUserAttention(
            egui::UserAttentionType::Informational,
        ));
    }

    /// Opens the start screen unless the settings turned it off.
    pub fn show_start_screen(&mut self) {
        if self.state.settings.general.start_screen {
            self.ui.start = Some(Default::default());
        }
    }

    /// Follows the window's size, position and maximized flag (in logical
    /// points, as the window builder takes them).
    fn track_window(&mut self, ctx: &egui::Context) {
        let zoom = ctx.zoom_factor();
        let (inner, outer, maximized, minimized) = ctx.input(|i| {
            let v = i.viewport();
            (
                v.inner_rect,
                v.outer_rect,
                v.maximized.unwrap_or(false),
                v.minimized.unwrap_or(false),
            )
        });
        if minimized {
            return;
        }
        let mut w = self.window.unwrap_or(dock::WindowState {
            size: [1080.0, 640.0],
            position: None,
            maximized: false,
        });
        w.maximized = maximized;
        // A maximized window keeps the size and spot it restores to.
        if !maximized {
            if let Some(r) = inner {
                w.size = (r.size() * zoom).into();
            }
            if let Some(r) = outer {
                w.position = Some((r.min.to_vec2() * zoom).into());
            }
        }
        self.window = Some(w);
    }

    /// Writes `layout.json` once the layout or window has been unchanged
    /// for [`LAYOUT_SAVE_DELAY`], or right away with `now`.
    fn save_layout(&mut self, now: bool) {
        let current = (&self.docks, &self.window);
        if (&self.layout_saved.0, &self.layout_saved.1) == current {
            self.layout_changed = None;
            return;
        }
        let changed = *self
            .layout_changed
            .get_or_insert_with(std::time::Instant::now);
        if !now && changed.elapsed() < LAYOUT_SAVE_DELAY {
            return;
        }
        let Some(paths) = self.state.paths() else {
            return;
        };
        if let Err(e) = self.docks.save(&paths.layout_file(), self.window) {
            log::warn!("layout: {e}");
        }
        self.layout_saved = (self.docks.clone(), self.window);
        self.layout_changed = None;
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

    /// Clean exit: lets the close through, deletes the recovery files and
    /// saves the layout.
    fn quit(&mut self) {
        self.quitting = true;
        self.state.clean_exit();
        self.save_layout(true);
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        dispatch_shortcuts(ctx, &mut self.state, &mut self.ui);
    }
}

/// Global keyboard shortcuts, dispatched through the keymap (*Settings ▸
/// Keymap*). Skipped while a text field has focus so e.g. Ctrl+Z edits text
/// instead of the document, and while a modal prompt or dialog is open.
pub fn dispatch_shortcuts(ctx: &egui::Context, state: &mut AppState, ui: &mut panels::UiState) {
    let modal = state.closing().is_some()
        || !ui.recovery.is_empty()
        || ui.dialogs.is_open()
        || ui.settings.is_some()
        || ui.start.is_some();
    if ctx.egui_wants_keyboard_input() || modal {
        return;
    }
    // Clipboard. Paste follows the cursor from the hovered voxel.
    match clipboard_command(ctx) {
        Some(ClipboardCommand::Copy) => {
            state.copy();
        }
        Some(ClipboardCommand::Cut) => {
            state.cut();
        }
        Some(ClipboardCommand::Paste) => {
            state.paste(state.cursor_voxel);
        }
        Some(ClipboardCommand::PasteInPlace) => {
            state.paste_in_place();
        }
        Some(ClipboardCommand::PasteToNewLayer) => {
            state.paste_to_new_layer();
        }
        Some(ClipboardCommand::FlipX) => state.flip(0),
        None => {}
    }
    let keymap = &state.settings.keymap;
    let actions = ctx.input_mut(|i| keys::take_actions(i, keymap));
    for action in actions {
        run_action(state, ui, action);
    }
    ui.layers_hovered = false;
}

/// Runs a keyboard command (the clipboard and the viewport's own keys are
/// handled where their input arrives).
pub fn run_action(state: &mut AppState, ui: &mut panels::UiState, action: ActionId) {
    use ActionId as A;
    match action {
        A::New => actions::new_document(state),
        A::Open => actions::open(state),
        A::Save => actions::save(state),
        A::SaveAs => actions::save_as(state),
        A::Settings => ui.open_settings(state),
        A::Undo => state.undo(),
        A::Redo => state.redo(),
        A::Cut | A::Copy | A::Paste | A::PasteInPlace | A::PasteToNewLayer => {}
        A::Delete => state.delete_selected(),
        A::SelectAll if ui.layers_hovered => {
            state.select_all_layers();
        }
        A::SelectAll => state.select_all(),
        A::Deselect => state.deselect(),
        A::InvertSelection => state.invert_selection(),
        A::SelectionToLayer => state.selection_to_new_layer(false),
        A::SelectionToLayerCut => state.selection_to_new_layer(true),
        A::MaskMode => state.toggle_mask_selection(),
        A::Group => state.group_selected_layers(),
        A::Ungroup => state.ungroup_active_layer(),
        // Rotation turns about the axis the camera faces.
        A::RotateCcw => state.rotate_facing(false),
        A::RotateCw => state.rotate_facing(true),
        A::FlipX => state.flip(0),
        A::FlipY => state.flip(1),
        A::FlipZ => state.flip(2),
        A::SliceMode => state.toggle_slice(None),
        // Hiding the work plane also ends Slice Mode.
        A::WorkPlane => state.toggle_plane_visible(),
        A::ZenMode => ui.zen_mode = !ui.zen_mode,
        A::RenderMode => state.toggle_view_mode(),
        A::Projection => state.camera.toggle_projection(),
        A::FrameSelection => state.frame_selection(),
        A::FrameWorkArea => state.frame_work_area(),
        A::HomeView => state.camera.reset(),
        A::FloorGrid => state.toggle_floor_grid(),
        A::VoxelGrid => state.view.voxel_grid = !state.view.voxel_grid,
        A::ModelBounds => state.view.model_bounds = !state.view.model_bounds,
        A::Ghosts => state.view.ghosts = !state.view.ghosts,
        A::References => state.toggle_all_references(),
        A::UserGuide => ui.open_guide = Some(String::new()),
        A::Tool(tool) => state.tool = tool,
        A::RecallFavorite(slot) => state.recall_favorite(slot.into()),
        A::StoreFavorite(slot) => state.store_favorite(slot.into()),
    }
}

impl RizxelApp {
    /// Image files dropped on the window become reference images on the
    /// plane the camera faces most.
    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let paths: Vec<std::path::PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        if paths.is_empty() {
            return;
        }
        let (images, others): (Vec<_>, Vec<_>) = paths
            .into_iter()
            .partition(|p| rizxel_app::is_image_path(p));
        panels::references::add_files(&mut self.state, &images);
        if images.is_empty() && !others.is_empty() {
            self.state.status_message =
                Some("Drop PNG or JPEG files to add reference images".to_owned());
        }
    }
}

impl eframe::App for RizxelApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        egui::Rgba::from(theme::BG_APP).to_array()
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let scale = self.state.settings.general.ui_scale;
        if (ctx.zoom_factor() - scale).abs() > f32::EPSILON {
            ctx.set_zoom_factor(scale);
        }
        panels::settings::capture_keys(&ctx, self.ui.settings.as_mut());
        self.handle_close(&ctx);
        panels::top_bar::recovery_dialog(&ctx, &mut self.state, &mut self.ui);
        // After the recovery prompt, which comes first.
        if self.ui.recovery.is_empty() {
            panels::start::show(&ctx, &mut self.state, &mut self.ui.start);
        }
        self.track_window(&ctx);
        self.handle_instance(&ctx);
        self.state.autosave_tick(std::time::Instant::now());
        // Keep ticking while idle so the time-based autosave still fires.
        ctx.request_repaint_after(std::time::Duration::from_secs(5));
        self.handle_shortcuts(&ctx);
        self.handle_dropped_files(&ctx);

        let state = &mut self.state;
        let ui_state = &mut self.ui;
        let icons = &self.icons;
        let meshes = &mut self.meshes;
        let tools = &mut self.tools;
        let target_format = self.target_format;
        let dock = self.docks.get_mut(state.view_mode);
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

        // Zen mode: only the title bar (with the document tabs), the viewport
        // and the status bar.
        egui::CentralPanel::no_frame().show(ui, |ui| {
            if ui_state.zen_mode {
                viewport_panel(ui, state, target_format, meshes, tools);
                return;
            }
            let error = dock.show(ui, |ui, panel| match panel {
                dock::Panel::Viewport => viewport_panel(ui, state, target_format, meshes, tools),
                dock::Panel::Tools => scroll_panel(ui, panel, |ui| {
                    panels::tools::show(ui, state, ui_state, icons)
                }),
                dock::Panel::Options => scroll_panel(ui, panel, |ui| {
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::symmetric(8, 0))
                        .show(ui, |ui| panels::options::show(ui, state, icons));
                }),
                dock::Panel::Palette => scroll_panel(ui, panel, |ui| {
                    panels::sidebar::palette(ui, state, ui_state)
                }),
                dock::Panel::Layers => {
                    scroll_panel(ui, panel, |ui| panels::sidebar::layers(ui, state, ui_state))
                }
                dock::Panel::Bounds => {
                    scroll_panel(ui, panel, |ui| panels::sidebar::bounds(ui, state))
                }
                dock::Panel::History => {
                    scroll_panel(ui, panel, |ui| panels::history::show(ui, state))
                }
                dock::Panel::References => scroll_panel(ui, panel, |ui| {
                    panels::references::show(ui, state, ui_state)
                }),
                dock::Panel::Render => panels::render::show(ui, state, ui_state),
            });
            if let Some(error) = error {
                log::warn!("{error}");
                state.status_message = Some(error);
            }
        });

        self.outputs.update(
            &ctx,
            self.render_state.as_ref(),
            state,
            &mut ui_state.render,
        );

        // Modal dialogs and frameless window chrome draw on top of everything.
        panels::top_bar::show_dialogs(&ctx, state, ui_state);
        panels::settings::show(&ctx, state, &mut ui_state.settings);
        panels::export::show(&ctx, state, &mut ui_state.dialogs);
        panels::render::shaped_dialog(&ctx, state, &mut ui_state.render);
        resize::handle_edges(&ctx);
        resize::paint_border(&ctx);

        crate::thumbnails::update(state, self.render_state.as_ref(), &mut self.thumbs);
        self.save_layout(false);
        if self.layout_changed.is_some() {
            ctx.request_repaint_after(LAYOUT_SAVE_DELAY);
        }
    }
}

enum ClipboardCommand {
    Copy,
    Cut,
    Paste,
    PasteInPlace,
    PasteToNewLayer,
    /// `Ctrl+Alt+X` arrives as a cut event too.
    FlipX,
}

/// `Ctrl+C`, `Ctrl+X`, `Ctrl+V`, `Ctrl+Alt+V`, `Ctrl+Shift+V` (and
/// `Ctrl+Alt+X`, which egui also reports as a cut).
/// egui-winit turns the first two into [`egui::Event::Copy`] / `Cut` and
/// swallows the `Ctrl+V` press (it only reports OS clipboard text), so
/// paste fires on the `V` release while `Ctrl` is held.
fn clipboard_command(ctx: &egui::Context) -> Option<ClipboardCommand> {
    ctx.input(|i| {
        i.events.iter().find_map(|e| match e {
            egui::Event::Copy => Some(ClipboardCommand::Copy),
            egui::Event::Cut if i.modifiers.alt => Some(ClipboardCommand::FlipX),
            egui::Event::Cut => Some(ClipboardCommand::Cut),
            egui::Event::Key {
                key: Key::V,
                pressed: false,
                modifiers,
                ..
            } if modifiers.command => Some(if modifiers.shift {
                ClipboardCommand::PasteToNewLayer
            } else if modifiers.alt {
                ClipboardCommand::PasteInPlace
            } else {
                ClipboardCommand::Paste
            }),
            _ => None,
        })
    })
}

/// Panel body that scrolls (both ways) when its content outgrows its dock
/// group or floating window.
fn scroll_panel(ui: &mut egui::Ui, panel: &dock::Panel, add: impl FnOnce(&mut egui::Ui)) {
    egui::ScrollArea::both()
        .id_salt(("dock_scroll", panel))
        .auto_shrink([false, false])
        .show(ui, add);
}

/// Viewport panel: the 3D view. The document tabs are in the title bar.
fn viewport_panel(
    ui: &mut egui::Ui,
    state: &mut AppState,
    target_format: Option<wgpu::TextureFormat>,
    meshes: &mut viewport::MeshCache,
    tools: &mut ToolInput,
) {
    viewport::show(ui, state, target_format, meshes, tools);
}

//! Headless UI smoke tests (egui_kittest, no GPU): the panels draw, the
//! menus open, shortcuts go through the keymap, and the Settings dialog
//! and start screen round-trip. The viewport needs wgpu and isn't drawn.
//! Clicks go through AccessKit actions: the frameless window's title bar
//! doesn't take the harness's synthetic pointer presses.

use egui::{Key, Modifiers};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use rizxel_app::{ActionId, AppPaths, AppState, KeyChord, Settings};
use rizxel_engine::Tool;
use rizxel_ui::dock::Workspace;
use rizxel_ui::icons::Icons;
use rizxel_ui::panels::{self, UiState};

struct World {
    state: AppState,
    ui: UiState,
    dock: Workspace,
    icons: Option<Icons>,
    /// The app's fonts are installed (they apply from the next frame).
    fonts: bool,
}

impl World {
    fn new(state: AppState) -> Self {
        Self {
            state,
            ui: UiState::default(),
            dock: Workspace::default(),
            icons: None,
            fonts: false,
        }
    }
}

fn temp_paths(name: &str) -> (std::path::PathBuf, AppPaths) {
    let dir = std::env::temp_dir().join(format!("rizxel_ui_smoke_{name}_{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    (dir.clone(), AppPaths::with_root(dir))
}

/// The title bar, tools rail, option bar and dialogs, with the app's
/// shortcut dispatch first (as in `RizxelApp::ui`).
fn editor(ui: &mut egui::Ui, w: &mut World) {
    let ctx = ui.ctx().clone();
    if !w.fonts {
        // The panels use the app's font families and theme.
        rizxel_ui::theme::install_fonts(&ctx);
        rizxel_ui::theme::apply(&ctx);
        w.fonts = true;
        ctx.request_repaint();
        return;
    }
    panels::settings::capture_keys(&ctx, w.ui.settings.as_mut());
    rizxel_ui::app::dispatch_shortcuts(&ctx, &mut w.state, &mut w.ui);
    let icons = w.icons.get_or_insert_with(|| Icons::load(&ctx));
    panels::top_bar::show(ui, &mut w.state, &mut w.ui, &mut w.dock);
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(64.0);
            panels::tools::show(ui, &mut w.state, &mut w.ui, icons);
        });
        ui.vertical(|ui| panels::options::show(ui, &mut w.state, icons));
    });
    panels::top_bar::show_dialogs(&ctx, &mut w.state, &mut w.ui);
    panels::settings::show(&ctx, &mut w.state, &mut w.ui.settings);
    panels::start::show(&ctx, &mut w.state, &mut w.ui.start);
}

fn harness(world: World) -> Harness<'static, World> {
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .build_ui_state(editor, world);
    h.run();
    h
}

#[test]
fn every_menu_opens() {
    let mut h = harness(World::new(AppState::default()));
    h.run();
    let buttons =
        |h: &Harness<'static, World>| h.query_all_by_role(egui::accesskit::Role::Button).count();
    let closed = buttons(&h);
    for menu in [
        "File",
        "Edit",
        "Selection",
        "Layer",
        "Model",
        "View",
        "Help",
    ] {
        // The title bar comes first ("Selection" is also an option label).
        h.get_all_by_label(menu).next().unwrap().click_accesskit();
        h.run();
        assert!(buttons(&h) > closed + 1, "{menu} shows its items");
        h.key_press(Key::Escape);
        h.run();
    }
    h.get_by_label("File").click_accesskit();
    h.run();
    h.get_by_label_contains("Settings...").click_accesskit();
    h.run();
    assert!(
        h.state().ui.settings.is_some(),
        "File > Settings opens the dialog"
    );
}

#[test]
fn tool_keys_go_through_the_keymap() {
    let mut h = harness(World::new(AppState::default()));
    h.run();
    for tool in Tool::ALL {
        let key = Key::from_name(&tool.shortcut().to_string()).unwrap();
        h.key_press(key);
        h.run();
        assert_eq!(h.state().state.tool, tool);
    }
    // Rebind the Brush to Q: Q selects it, B no longer does.
    let keymap = &mut h.state_mut().state.settings.keymap;
    keymap
        .set(
            ActionId::Tool(Tool::Brush),
            vec![KeyChord::parse("Q").unwrap()],
        )
        .unwrap();
    h.state_mut().state.tool = Tool::Cursor;
    h.key_press(Key::B);
    h.run();
    assert_eq!(h.state().state.tool, Tool::Cursor);
    h.key_press(Key::Q);
    h.run();
    assert_eq!(h.state().state.tool, Tool::Brush);
    // A command chord: Shift+G toggles the voxel grid.
    let before = h.state().state.view.voxel_grid;
    h.key_press_modifiers(Modifiers::SHIFT, Key::G);
    h.run();
    assert_ne!(h.state().state.view.voxel_grid, before);
}

#[test]
fn option_bar_renders_for_every_tool() {
    let mut h = harness(World::new(AppState::default()));
    for tool in Tool::ALL {
        h.state_mut().state.tool = tool;
        h.run();
        assert!(
            h.query_all_by_role(egui::accesskit::Role::Button).count() > 4,
            "{tool:?}"
        );
    }
}

#[test]
fn settings_dialog_round_trip() {
    let (dir, paths) = temp_paths("settings");
    let mut world = World::new(AppState::with_paths(Some(paths.clone())));
    world.ui.open_settings(&world.state);
    let mut h = harness(world);
    h.run();
    // Rebind Undo to Ctrl+U on the Keymap tab.
    h.get_by_label("Keymap").click_accesskit();
    h.run();
    if let Some(d) = h.state_mut().ui.settings.as_mut() {
        d.capture = Some(panels::settings::Capture {
            action: ActionId::Undo,
            slot: Some(0),
        });
    }
    h.key_press_modifiers(Modifiers::COMMAND, Key::U);
    h.run();
    // A conflicting chord blocks OK until it is resolved.
    if let Some(d) = h.state_mut().ui.settings.as_mut() {
        d.capture = Some(panels::settings::Capture {
            action: ActionId::Save,
            slot: Some(0),
        });
    }
    h.key_press_modifiers(Modifiers::COMMAND, Key::O);
    h.run();
    assert!(!h.state().ui.settings.as_ref().unwrap().can_save());
    if let Some(d) = h.state_mut().ui.settings.as_mut() {
        d.draft.keymap.reset(ActionId::Save);
    }
    h.run();
    h.get_by_label("OK").click_accesskit();
    h.run();
    assert!(h.state().ui.settings.is_none(), "OK closes the dialog");
    let saved = Settings::load(&paths.settings_file());
    let undo = KeyChord::parse("Ctrl+U").unwrap();
    assert_eq!(
        saved.keymap.chords(ActionId::Undo),
        std::slice::from_ref(&undo)
    );
    assert_eq!(
        h.state().state.settings.keymap.chords(ActionId::Undo),
        &[undo]
    );
    // Cancel drops the draft.
    h.state_mut()
        .ui
        .open_settings(&rizxel_app::AppState::default());
    if let Some(d) = h.state_mut().ui.settings.as_mut() {
        d.draft.general.undo_limit = 99;
    }
    h.run();
    h.get_by_label("Cancel").click_accesskit();
    h.run();
    assert_ne!(h.state().state.settings.general.undo_limit, 99);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn shortcuts_are_off_while_a_dialog_is_open() {
    let mut world = World::new(AppState::default());
    world.ui.open_settings(&world.state);
    let mut h = harness(world);
    h.run();
    h.key_press(Key::B);
    h.run();
    assert_eq!(h.state().state.tool, Tool::default());
}

#[test]
fn start_screen_lists_recent_files_and_closes() {
    let (dir, paths) = temp_paths("start");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("castle.rzxl");
    let mut doc = rizxel_app::Document::new("castle");
    doc.save_to(&file).unwrap();
    let mut state = AppState::with_paths(Some(paths));
    state.remember_recent(&file);
    let mut world = World::new(state);
    world.ui.start = Some(Default::default());
    let mut h = harness(world);
    h.run();
    h.get_by_label_contains("New Project");
    h.get_by_label_contains("castle.rzxl");
    h.get_by_label("Close").click_accesskit();
    h.run();
    assert!(h.state().ui.start.is_none());
    std::fs::remove_dir_all(&dir).ok();
}

/// URLs the last frame asked the system to open.
fn opened_urls(h: &Harness<'static, World>) -> Vec<String> {
    h.output()
        .platform_output
        .commands
        .iter()
        .filter_map(|c| match c {
            egui::OutputCommand::OpenUrl(open) => Some(open.url.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn help_opens_the_user_guide() {
    let mut h = harness(World::new(AppState::default()));
    h.run();
    // F1 (the keymap's default) opens the guide's front page.
    // `step` runs a frame per queued event: press only, then check.
    h.key_down(Key::F1);
    h.step();
    assert_eq!(opened_urls(&h), [rizxel_ui::help::GUIDE_URL]);
    h.key_up(Key::F1);
    h.run();
    // Help > User Guide does the same.
    h.get_all_by_label("Help").next().unwrap().click_accesskit();
    h.run();
    h.get_by_label_contains("User Guide").click_accesskit();
    h.step();
    assert_eq!(opened_urls(&h), [rizxel_ui::help::GUIDE_URL]);
    h.run();
    // The option bar's `?` opens the active tool's page.
    h.state_mut().state.tool = Tool::FuzzySelect;
    h.run();
    h.get_by_label(egui_phosphor::regular::QUESTION)
        .click_accesskit();
    h.step();
    assert_eq!(
        opened_urls(&h),
        [format!(
            "{}tools/fuzzy-select.html",
            rizxel_ui::help::GUIDE_URL
        )]
    );
}

/// The Layers panel with the app's shortcut dispatch first.
fn layers_panel(ui: &mut egui::Ui, w: &mut World) {
    let ctx = ui.ctx().clone();
    if !w.fonts {
        rizxel_ui::theme::install_fonts(&ctx);
        rizxel_ui::theme::apply(&ctx);
        w.fonts = true;
        ctx.request_repaint();
        return;
    }
    rizxel_ui::app::dispatch_shortcuts(&ctx, &mut w.state, &mut w.ui);
    panels::sidebar::layers(ui, &mut w.state, &mut w.ui);
}

#[test]
fn ctrl_click_two_layers_and_group_them() {
    let mut state = AppState::default();
    let d = state.doc_mut().unwrap();
    d.add_layer();
    d.add_layer();
    let names: Vec<String> = d.data.layers.iter().map(|l| l.name.clone()).collect();
    let mut h = Harness::builder()
        .with_size(egui::vec2(320.0, 700.0))
        .build_ui_state(layers_panel, World::new(state));
    h.run();
    h.get_by_label(&names[0]).click();
    h.run();
    h.get_by_label(&names[2])
        .click_modifiers(Modifiers::COMMAND);
    h.run();
    assert_eq!(h.state().state.doc().unwrap().selected_layer_count(), 2);
    h.key_press_modifiers(Modifiers::COMMAND, Key::G);
    h.run();
    let doc = h.state().state.doc().unwrap();
    let group = doc.active_layer().unwrap();
    assert!(group.kind().is_group());
    let parent = |name: &str| {
        let l = doc.data.layers.iter().find(|l| l.name == name).unwrap();
        l.props.parent
    };
    assert_eq!(parent(&names[0]), Some(group.id));
    assert_eq!(parent(&names[2]), Some(group.id));
    assert_eq!(parent(&names[1]), None, "the unselected layer stays out");
}

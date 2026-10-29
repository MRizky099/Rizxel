//! Rizxel — native voxel art editor (L4 frontend).

// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;

use rizxel_ui::app;
use rizxel_ui::single_instance::{self, Instance};

fn main() -> eframe::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    // 256 px is the largest size Windows/most Linux docks use for window icons.
    let window_icon =
        eframe::icon_data::from_png_bytes(include_bytes!("../../assets/Rizxel_icon_256x256.png"))
            .expect("embedded app icon is a valid PNG");

    // `rizxel path/to/model.rzxl` opens that project at startup.
    let open_paths: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let app_paths = rizxel_app::AppPaths::new();

    // One window per user: a second launch (e.g. double-clicking a `.rzxl`)
    // hands its files to the running window, which opens them as tabs.
    let instance = match &app_paths {
        Some(p) => match single_instance::acquire(&p.instance_file(), &open_paths) {
            Ok(Instance::HandedOff) => {
                log::info!(
                    "Rizxel is already running; handed over {} file(s)",
                    open_paths.len()
                );
                return Ok(());
            }
            Ok(Instance::Primary(server)) => Some(server),
            Err(e) => {
                log::warn!("single instance: {e}");
                None
            }
        },
        None => None,
    };

    // The window comes back where it was (layout.json); the first start
    // opens windowed at 1080x640, centered.
    let window = app_paths
        .as_ref()
        .and_then(|p| rizxel_ui::dock::load_window_state(&p.layout_file()));
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Rizxel - Voxel Editor")
        .with_icon(window_icon)
        .with_inner_size(window.map_or([1080.0, 640.0], |w| w.size))
        .with_min_inner_size(rizxel_ui::dock::WindowState::MIN_SIZE)
        .with_maximized(window.is_some_and(|w| w.maximized))
        // Frameless: title bar, window controls and resize edges are drawn by egui.
        .with_decorations(false);
    let position = window.and_then(|w| w.position);
    if let Some(pos) = position {
        viewport = viewport.with_position(pos);
    }
    let options = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Wgpu,
        centered: position.is_none(),
        ..Default::default()
    };

    eframe::run_native(
        "Rizxel",
        options,
        Box::new(move |cc| {
            let mut app = app::RizxelApp::new(cc);
            if open_paths.is_empty() {
                app.show_start_screen();
            }
            for path in &open_paths {
                app.open_at_startup(path);
            }
            if let Some(server) = instance {
                app.set_instance(server, &cc.egui_ctx);
            }
            Ok(Box::new(app))
        }),
    )
}

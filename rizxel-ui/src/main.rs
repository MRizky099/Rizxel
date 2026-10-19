//! Rizxel — native voxel art editor (L4 frontend).

// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod actions;
mod app;
mod icons;
mod panels;
mod render;
mod resize;
mod theme;
mod viewport;
mod widgets;

fn main() -> eframe::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    // 256 px is the largest size Windows/most Linux docks use for window icons.
    let window_icon =
        eframe::icon_data::from_png_bytes(include_bytes!("../../assets/Rizxel_icon_256x256.png"))
            .expect("embedded app icon is a valid PNG");

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Rizxel - Voxel Editor")
            .with_icon(window_icon)
            // Opens windowed (not maximized) at 1080x640.
            .with_inner_size([1080.0, 640.0])
            .with_min_inner_size([800.0, 480.0])
            .with_maximized(false)
            // Frameless: title bar, window controls and resize edges are drawn by egui.
            .with_decorations(false),
        renderer: eframe::Renderer::Wgpu,
        centered: true,
        ..Default::default()
    };

    eframe::run_native(
        "Rizxel",
        options,
        Box::new(|cc| Ok(Box::new(app::RizxelApp::new(cc)))),
    )
}

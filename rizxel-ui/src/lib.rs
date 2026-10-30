//! Rizxel L4 frontend: the egui/wgpu editor. The `rizxel` binary runs
//! [`app::RizxelApp`]; the library target also lets the `render` and
//! `render_bench` examples drive the renderers without a window.

pub mod actions;
pub mod app;
pub mod dock;
pub mod file_assoc;
pub mod help;
pub mod icons;
pub mod keys;
pub mod output;
pub mod panels;
pub mod render;
pub mod resize;
pub mod single_instance;
pub mod theme;
pub mod thumbnails;
pub mod tool_input;
pub mod viewport;
pub mod widgets;

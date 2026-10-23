//! Title bar: app icon, menus, centered title and window controls.
//! Doubles as the drag handle for the frameless window.

use egui::{Align2, Color32, Id, Sense, Ui, ViewportCommand};
use egui_phosphor::regular as icon;
use rizxel_app::{AppState, FileFormat, SliceStep};

use super::UiState;
use crate::dock::{self, Workspace};
use crate::widgets::{icon_button, icon_label, IconStyle};
use crate::{actions, theme};

const CONTROL_WIDTH: f32 = 46.0;

pub fn show(ui: &mut Ui, state: &mut AppState, ui_state: &mut UiState, dock: &mut Workspace) {
    let bar = ui.max_rect();

    // Background interaction: drag to move, double-click to maximize. Widgets
    // added afterwards take priority over this area.
    let drag = ui.interact(bar, Id::new("title_bar_drag"), Sense::click_and_drag());
    if drag.drag_started_by(egui::PointerButton::Primary) {
        ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
    }
    if drag.double_clicked() {
        toggle_maximize(ui.ctx());
    }

    // Centered title (painted, not interactive).
    let title = match state.doc() {
        Some(d) if d.is_dirty() => format!("{} \u{2022} Rizxel - Voxel Editor", d.title()),
        Some(d) => format!("{} - Rizxel - Voxel Editor", d.title()),
        None => "Rizxel - Voxel Editor".to_owned(),
    };
    ui.painter().text(
        bar.center(),
        Align2::CENTER_CENTER,
        title,
        theme::regular(12.0),
        theme::TEXT_MUTED,
    );

    ui.horizontal_centered(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;

        // App icon.
        let (r, _) = ui.allocate_exact_size(egui::vec2(32.0, bar.height()), Sense::hover());
        crate::widgets::paint_icon(ui, r, icon::CUBE, 15.0, theme::ACCENT);

        menus(ui, state, ui_state, dock);

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            window_controls(ui, bar.height());
        });
    });
}

fn menus(ui: &mut Ui, state: &mut AppState, ui_state: &mut UiState, dock: &mut Workspace) {
    ui.style_mut().spacing.button_padding = egui::vec2(10.0, 4.0);
    ui.style_mut()
        .text_styles
        .insert(egui::TextStyle::Button, theme::regular(12.0));

    egui::MenuBar::new().ui(ui, |ui| {
        ui.menu_button("File", |ui| {
            ui.set_min_width(220.0);
            if menu_item(ui, icon::FILE_PLUS, "New", "Ctrl+N", true) {
                actions::new_document(state);
            }
            if menu_item(ui, icon::FOLDER_OPEN, "Open...", "Ctrl+O", true) {
                actions::open(state);
            }
            ui.separator();
            if menu_item(ui, icon::FLOPPY_DISK, "Save", "Ctrl+S", true) {
                actions::save(state);
            }
            if menu_item(
                ui,
                icon::FLOPPY_DISK_BACK,
                "Save As...",
                "Ctrl+Shift+S",
                true,
            ) {
                actions::save_as(state);
            }
            ui.separator();
            ui.menu_button(
                icon_label(ui, icon::DOWNLOAD_SIMPLE, "Import", None),
                |ui| {
                    ui.set_min_width(240.0);
                    for format in FileFormat::IMPORTS {
                        let label = format!("{} (.{})...", format.label(), format.extension());
                        let hint = if format == FileFormat::Mtl {
                            "Replaces the palette and materials of the active document"
                        } else {
                            "Opens in a new tab"
                        };
                        if menu_item(ui, icon::FILE_ARROW_DOWN, &label, "", true) {
                            actions::import(state, format);
                        }
                        ui.label(egui::RichText::new(hint).size(10.0).color(theme::TEXT_DIM));
                    }
                },
            );
            ui.menu_button(icon_label(ui, icon::EXPORT, "Export", None), |ui| {
                ui.set_min_width(260.0);
                for format in FileFormat::EXPORTS {
                    let label = format!("{} (.{})...", format.label(), format.extension());
                    if menu_item(
                        ui,
                        icon::FILE_ARROW_UP,
                        &label,
                        "",
                        state.can_export(format),
                    ) {
                        actions::export(state, format);
                    }
                    ui.label(
                        egui::RichText::new(format.export_note())
                            .size(10.0)
                            .color(theme::TEXT_DIM),
                    );
                }
            });
            ui.separator();
            if menu_item(ui, icon::SIGN_OUT, "Exit", "Alt+F4", true) {
                ui.ctx().send_viewport_cmd(ViewportCommand::Close);
            }
        });

        ui.menu_button("Edit", |ui| {
            ui.set_min_width(200.0);
            let (can_undo, can_redo) = (state.can_undo(), state.can_redo());
            if menu_item(
                ui,
                icon::ARROW_COUNTER_CLOCKWISE,
                "Undo",
                "Ctrl+Z",
                can_undo,
            ) {
                state.undo();
            }
            if menu_item(ui, icon::ARROW_CLOCKWISE, "Redo", "Ctrl+Y", can_redo) {
                state.redo();
            }
            ui.separator();
            if menu_item(ui, icon::STACK_PLUS, "New Layer", "", true) {
                if let Some(d) = state.doc_mut() {
                    d.add_layer();
                }
            }
        });

        ui.menu_button("View", |ui| {
            ui.set_min_width(220.0);

            if menu_item(ui, icon::CUBE_FOCUS, "Zen Mode", "Ctrl+Shift+F", true) {
                ui_state.zen_mode = !ui_state.zen_mode;
            }
            slice_menu(ui, state);
            ui.separator();

            // Standard views: each family of targets is its own submenu.
            ui.menu_button(icon_label(ui, icon::SQUARE, "Faces", None), |ui| {
                ui.set_min_width(160.0);
                for face in rizxel_app::ViewFace::ALL {
                    if menu_item(ui, icon::SQUARE, face.label(), "", true) {
                        state.camera.snap(rizxel_app::ViewTarget::Face(face));
                    }
                }
            });

            ui.menu_button(icon_label(ui, icon::LINE_SEGMENT, "Edges", None), |ui| {
                ui.set_min_width(160.0);
                for edge in rizxel_app::ViewEdge::ALL {
                    if menu_item(ui, icon::LINE_SEGMENT, edge.label(), "", true) {
                        state.camera.snap(rizxel_app::ViewTarget::Edge(edge));
                    }
                }
            });

            ui.menu_button(icon_label(ui, icon::CUBE, "Corners", None), |ui| {
                ui.set_min_width(160.0);
                for corner in rizxel_app::ViewCorner::ALL {
                    if menu_item(ui, icon::CUBE, corner.label(), "", true) {
                        state.camera.snap(rizxel_app::ViewTarget::Corner(corner));
                    }
                }
            });

            ui.separator();

            if menu_item(ui, icon::ARROW_COUNTER_CLOCKWISE, "Reset Camera", "", true) {
                state.camera = rizxel_app::OrbitCamera::default();
            }

            ui.separator();
            panels_menu(ui, dock);
        });

        ui.menu_button("Help", |ui| {
            ui.set_min_width(200.0);
            if menu_item(ui, icon::KEYBOARD, "Keyboard Shortcuts", "", true) {
                ui_state.show_shortcuts = true;
            }
            if menu_item(ui, icon::INFO, "About Rizxel", "", true) {
                ui_state.show_about = true;
            }
        });
    });
}

/// View > Slice (Qubicle's *Tools ▸ Slice*): the Slice Mode toggle and the
/// slice navigation commands. Slice moves are not undo steps.
fn slice_menu(ui: &mut Ui, state: &mut AppState) {
    let has_doc = state.doc().is_some();
    let slicing = state.slice_counter().is_some();
    ui.menu_button(icon_label(ui, icon::STACK, "Slice", None), |ui| {
        ui.set_min_width(180.0);
        let glyph = if slicing {
            icon::CHECK_SQUARE
        } else {
            icon::SQUARE
        };
        if menu_item(ui, glyph, "Slice Mode", "Space", has_doc) {
            state.toggle_slice(None);
        }
        let shown = state.doc().is_some_and(|d| d.plane.active());
        let glyph = if shown { icon::EYE } else { icon::EYE_SLASH };
        if menu_item(ui, glyph, "Show Work Plane", "/", has_doc) {
            state.toggle_plane_visible();
        }
        ui.separator();
        let glyphs = [
            icon::CARET_DOWN,
            icon::CARET_UP,
            icon::CARET_DOUBLE_UP,
            icon::CARET_DOUBLE_DOWN,
        ];
        for (step, glyph) in SliceStep::ALL.into_iter().zip(glyphs) {
            if menu_item(ui, glyph, step.label(), "", slicing) {
                state.step_slice(step);
            }
        }
    });
}

/// Menu entry with leading icon and trailing shortcut. Returns true on click.
fn menu_item(ui: &mut Ui, glyph: &str, label: &str, shortcut: &str, enabled: bool) -> bool {
    let button = egui::Button::new(icon_label(ui, glyph, label, None)).shortcut_text(shortcut);
    let clicked = ui.add_enabled(enabled, button).clicked();
    if clicked {
        ui.close();
    }
    clicked
}

/// View > Panels: toggle visibility and restore the default workspace layout.
fn panels_menu(ui: &mut Ui, dock: &mut Workspace) {
    ui.menu_button(icon_label(ui, icon::SQUARES_FOUR, "Panels", None), |ui| {
        ui.set_min_width(200.0);
        for panel in dock::Panel::ALL {
            let closable = panel.permissions().closable;
            let mut open = dock.is_visible(panel);
            let toggle = ui.add_enabled(closable, egui::Checkbox::new(&mut open, panel.label()));
            if toggle.changed() {
                if let Err(e) = dock.set_visible(panel, open) {
                    log::warn!("toggle {panel:?}: {e}");
                }
            }
        }
        ui.separator();
        if menu_item(ui, icon::ARROWS_CLOCKWISE, "Reset Layout", "", true) {
            dock.reset(ui.ctx());
        }
    });
}

fn window_controls(ui: &mut Ui, height: f32) {
    let base = IconStyle::bar()
        .size(egui::vec2(CONTROL_WIDTH, height))
        .color(theme::TEXT);

    // Laid out right-to-left: close, maximize, minimize.
    let close = base
        .hover_fill(theme::CLOSE_HOVER)
        .hover_color(Color32::WHITE);
    if icon_button(ui, icon::X, close, true)
        .on_hover_text("Close")
        .clicked()
    {
        ui.ctx().send_viewport_cmd(ViewportCommand::Close);
    }

    let maximized = ui.input(|i| i.viewport().maximized.unwrap_or(false));
    let glyph = if maximized {
        icon::COPY_SIMPLE
    } else {
        icon::SQUARE
    };
    if icon_button(ui, glyph, base, true)
        .on_hover_text(if maximized { "Restore" } else { "Maximize" })
        .clicked()
    {
        toggle_maximize(ui.ctx());
    }

    if icon_button(ui, icon::MINUS, base, true)
        .on_hover_text("Minimize")
        .clicked()
    {
        ui.ctx().send_viewport_cmd(ViewportCommand::Minimized(true));
    }
}

fn toggle_maximize(ctx: &egui::Context) {
    let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
    ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
}

/// Help dialogs opened from the menu.
pub fn show_dialogs(ctx: &egui::Context, _state: &mut AppState, ui_state: &mut UiState) {
    if ui_state.show_about {
        egui::Window::new("About Rizxel")
            .open(&mut ui_state.show_about)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(icon::CUBE)
                            .font(theme::icon(28.0))
                            .color(theme::ACCENT),
                    );
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new("Rizxel").font(theme::semibold(16.0)));
                        ui.label(format!("Voxel Editor v{}", env!("CARGO_PKG_VERSION")));
                    });
                });
                ui.add_space(6.0);
                ui.label("Native voxel art editor built with Rust, egui and wgpu.");
                ui.label(egui::RichText::new("© Abati Technology").color(theme::TEXT_DIM));
            });
    }

    if ui_state.show_shortcuts {
        egui::Window::new("Keyboard Shortcuts")
            .open(&mut ui_state.show_shortcuts)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                egui::Grid::new("shortcuts").striped(true).show(ui, |ui| {
                    for (keys, action) in [
                        ("Ctrl+N", "New project"),
                        ("Ctrl+O", "Open project"),
                        ("Ctrl+S", "Save"),
                        ("Ctrl+Shift+S", "Save as"),
                        ("Ctrl+Z", "Undo"),
                        ("Ctrl+Y / Ctrl+Shift+Z", "Redo"),
                        ("Right drag (viewport)", "Orbit camera"),
                        ("Middle drag (viewport)", "Pan camera"),
                        ("Middle double-click", "Reset camera"),
                        ("Scroll (viewport)", "Zoom"),
                    ] {
                        ui.label(egui::RichText::new(keys).color(theme::TEXT_STRONG));
                        ui.label(action);
                        ui.end_row();
                    }
                });
            });
    }
}

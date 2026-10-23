//! Title bar: app icon, menus, centered title and window controls.
//! Doubles as the drag handle for the frameless window.

use egui::{Align2, Color32, Id, Sense, Ui, ViewportCommand};
use egui_phosphor::regular as icon;
use rizxel_app::{AppState, CloseAction, CloseChoice, CloseStep, FileFormat, SliceStep};

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
            recent_menu(ui, state);
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
            // Goes through the app's close request, which asks about
            // unsaved documents first.
            if menu_item(ui, icon::SIGN_OUT, "Exit", "Alt+F4", true) {
                ui.ctx().send_viewport_cmd(ViewportCommand::Close);
            }
        });

        ui.menu_button("Edit", |ui| edit_menu(ui, state));

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

/// Edit menu: history, clipboard, selection, layer and transform commands.
/// Every item shows its shortcut and is disabled when it can't apply.
fn edit_menu(ui: &mut Ui, state: &mut AppState) {
    ui.set_min_width(240.0);
    let (can_undo, can_redo) = (state.can_undo(), state.can_redo());
    let has_voxels = state.active_layer_has_voxels();
    let editable = state.active_layer_editable();
    let selected = state.selected_voxels() > 0;
    let clip = state.clipboard.is_some();
    let has_doc = state.doc().is_some();

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
    if menu_item(ui, icon::SCISSORS, "Cut", "Ctrl+X", has_voxels && editable) {
        state.cut();
    }
    if menu_item(ui, icon::COPY, "Copy", "Ctrl+C", has_voxels) {
        state.copy();
    }
    if menu_item(ui, icon::CLIPBOARD, "Paste", "Ctrl+V", clip && editable) {
        let at = state.cursor_voxel;
        state.paste(at);
    }
    if menu_item(
        ui,
        icon::CLIPBOARD_TEXT,
        "Paste in Place",
        "Ctrl+Alt+V",
        clip && editable,
    ) {
        state.paste_in_place();
    }
    if menu_item(
        ui,
        icon::STACK_PLUS,
        "Paste to New Layer",
        "Ctrl+Shift+V",
        clip && has_doc,
    ) {
        state.paste_to_new_layer();
    }
    ui.separator();
    if menu_item(ui, icon::SELECTION_ALL, "Select All", "Ctrl+A", has_voxels) {
        state.select_all();
    }
    if menu_item(ui, icon::SELECTION_SLASH, "Deselect", "Ctrl+D", selected) {
        state.deselect();
    }
    if menu_item(
        ui,
        icon::SELECTION_INVERSE,
        "Invert Selection",
        "Ctrl+I",
        has_voxels,
    ) {
        state.invert_selection();
    }
    if menu_item(ui, icon::TRASH, "Delete", "Delete", selected && editable) {
        state.delete_selected();
    }
    ui.separator();
    if menu_item(ui, icon::STACK_PLUS, "New Layer", "", has_doc) {
        if let Some(d) = state.doc_mut() {
            d.add_layer();
        }
    }
    if menu_item(ui, icon::STACK_SIMPLE, "Duplicate Layer", "", has_doc) {
        if let Some(d) = state.doc_mut() {
            d.duplicate_active_layer();
        }
    }
    ui.separator();
    let can_transform = has_voxels && editable;
    ui.add_enabled_ui(can_transform, |ui| transform_menus(ui, state));
}

/// Edit > Rotate 90°, Flip and Mirror. They act on the selection, or the
/// whole active layer without one.
fn transform_menus(ui: &mut Ui, state: &mut AppState) {
    const AXES: [&str; 3] = ["X", "Y", "Z"];
    ui.menu_button(
        icon_label(ui, icon::ARROW_ARC_LEFT, "Rotate 90°", None),
        |ui| {
            ui.set_min_width(240.0);
            let facing = AXES[state.facing_axis()];
            if menu_item(
                ui,
                icon::ARROW_ARC_LEFT,
                &format!("Counterclockwise (facing {facing})"),
                "Ctrl+R",
                true,
            ) {
                state.rotate_facing(false);
            }
            if menu_item(
                ui,
                icon::ARROW_ARC_RIGHT,
                &format!("Clockwise (facing {facing})"),
                "Ctrl+Shift+R",
                true,
            ) {
                state.rotate_facing(true);
            }
            ui.separator();
            for (axis, name) in AXES.iter().enumerate() {
                if menu_item(
                    ui,
                    icon::ARROW_ARC_LEFT,
                    &format!("+90° about {name}"),
                    "",
                    true,
                ) {
                    state.rotate(axis, 1);
                }
                if menu_item(
                    ui,
                    icon::ARROW_ARC_RIGHT,
                    &format!("-90° about {name}"),
                    "",
                    true,
                ) {
                    state.rotate(axis, -1);
                }
            }
        },
    );
    ui.menu_button(icon_label(ui, icon::FLIP_HORIZONTAL, "Flip", None), |ui| {
        ui.set_min_width(180.0);
        for (axis, name) in AXES.iter().enumerate() {
            let glyph = if axis == 1 {
                icon::FLIP_VERTICAL
            } else {
                icon::FLIP_HORIZONTAL
            };
            if menu_item(
                ui,
                glyph,
                &format!("Flip {name}"),
                &format!("Ctrl+Alt+{name}"),
                true,
            ) {
                state.flip(axis);
            }
        }
    });
    ui.menu_button(icon_label(ui, icon::SQUARE_HALF, "Mirror", None), |ui| {
        ui.set_min_width(240.0);
        for (axis, name) in AXES.iter().enumerate() {
            if menu_item(
                ui,
                icon::SQUARE_HALF,
                &format!("Mirror -{name} onto +{name}"),
                "",
                true,
            ) {
                state.mirror(axis);
            }
        }
    });
}

/// File > Open Recent: the last opened or saved files, newest first.
/// Missing files are dropped while the menu is open.
fn recent_menu(ui: &mut Ui, state: &mut AppState) {
    let enabled = !state.recent.is_empty();
    ui.add_enabled_ui(enabled, |ui| {
        ui.menu_button(
            icon_label(ui, icon::CLOCK_COUNTER_CLOCKWISE, "Open Recent", None),
            |ui| {
                ui.set_min_width(260.0);
                state.prune_recent();
                let mut open = None;
                for path in state.recent.paths() {
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.display().to_string());
                    let item = egui::Button::new(icon_label(ui, icon::FILE, &name, None));
                    if ui
                        .add(item)
                        .on_hover_text(path.display().to_string())
                        .clicked()
                    {
                        open = Some(path.clone());
                        ui.close();
                    }
                }
                if state.recent.is_empty() {
                    ui.label(egui::RichText::new("No recent files").color(theme::TEXT_DIM));
                }
                ui.separator();
                if menu_item(
                    ui,
                    icon::TRASH,
                    "Clear Recent",
                    "",
                    !state.recent.is_empty(),
                ) {
                    state.clear_recent();
                }
                if let Some(path) = open {
                    actions::open_path(state, &path);
                }
            },
        );
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

/// "Save changes to *name* before closing?" while a tab close or exit is
/// pending: Save (`Enter`), Don't Save, Cancel (`Esc`). Returns `true` when
/// every document is answered for and the app may quit.
pub fn close_prompt(ctx: &egui::Context, state: &mut AppState) -> bool {
    let Some(doc) = state.close_prompt() else {
        return false;
    };
    let title = doc.title();
    let exiting = state
        .closing()
        .is_some_and(|r| r.action == CloseAction::Exit);
    let mut choice = None;
    let modal = egui::Modal::new(Id::new("close_prompt")).show(ctx, |ui| {
        ui.set_width(360.0);
        ui.label(
            egui::RichText::new(format!("Save changes to {title} before closing?"))
                .font(theme::semibold(14.0))
                .color(theme::TEXT_STRONG),
        );
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new("Your changes will be lost if you don't save them.")
                .color(theme::TEXT_DIM),
        );
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            let save = if exiting { "Save and Exit" } else { "Save" };
            if ui.button(save).clicked() {
                choice = Some(CloseChoice::Save);
            }
            if ui.button("Don't Save").clicked() {
                choice = Some(CloseChoice::DontSave);
            }
            if ui.button("Cancel").clicked() {
                choice = Some(CloseChoice::Cancel);
            }
        });
    });
    let enter = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
    if enter {
        choice = Some(CloseChoice::Save);
    } else if modal.should_close() {
        choice = Some(CloseChoice::Cancel);
    }
    let Some(choice) = choice else {
        return false;
    };
    let mut step = state.answer_close(choice);
    if let CloseStep::Save(_) = step {
        // The prompt's document is the active one; untitled ones open Save As.
        state.status_message = None;
        actions::save(state);
        step = state.close_after_save();
        // A failed save already reports its error.
        let failed = state
            .status_message
            .as_deref()
            .is_some_and(|m| m.starts_with("Failed"));
        if step == CloseStep::Abort && !failed {
            state.status_message = Some("Close cancelled: the document was not saved".to_owned());
        }
    }
    step == CloseStep::Quit
}

/// Startup dialog listing autosaves left by a session that did not exit
/// cleanly: Restore or Discard each. "Decide Later" keeps the files for the
/// next start.
pub fn recovery_dialog(ctx: &egui::Context, state: &mut AppState, ui_state: &mut UiState) {
    if ui_state.recovery.is_empty() {
        return;
    }
    enum Pick {
        Restore(usize),
        Discard(usize),
        Later,
    }
    let mut pick = None;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    egui::Modal::new(Id::new("recovery_dialog")).show(ctx, |ui| {
        ui.set_width(440.0);
        ui.label(
            egui::RichText::new("Recover unsaved work")
                .font(theme::semibold(14.0))
                .color(theme::TEXT_STRONG),
        );
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new("Rizxel did not close normally. These documents were autosaved:")
                .color(theme::TEXT_DIM),
        );
        ui.add_space(8.0);
        for (i, entry) in ui_state.recovery.iter().enumerate() {
            let meta = &entry.meta;
            let minutes = now.saturating_sub(meta.saved_at) / 60;
            let when = match minutes {
                0 => "less than a minute ago".to_owned(),
                1 => "1 minute ago".to_owned(),
                m if m < 120 => format!("{m} minutes ago"),
                m => format!("{} hours ago", m / 60),
            };
            let place = meta
                .original
                .as_ref()
                .map_or("never saved".to_owned(), |p| p.display().to_string());
            ui.separator();
            ui.label(egui::RichText::new(&meta.title).color(theme::TEXT_STRONG));
            ui.label(
                egui::RichText::new(format!("{place}, autosaved {when}"))
                    .size(11.0)
                    .color(theme::TEXT_DIM),
            );
            ui.horizontal(|ui| {
                if ui.button("Restore").clicked() {
                    pick = Some(Pick::Restore(i));
                }
                if ui.button("Discard").clicked() {
                    pick = Some(Pick::Discard(i));
                }
            });
        }
        ui.separator();
        ui.add_space(8.0);
        if ui.button("Decide Later").clicked() {
            pick = Some(Pick::Later);
        }
    });
    match pick {
        Some(Pick::Restore(i)) => {
            let entry = ui_state.recovery.remove(i);
            state.status_message = Some(match state.restore(&entry) {
                Ok(()) => format!("Restored {}", entry.meta.title),
                Err(e) => {
                    log::error!("restore {}: {e}", entry.key);
                    format!("Failed to restore {}: {e}", entry.meta.title)
                }
            });
        }
        Some(Pick::Discard(i)) => {
            let entry = ui_state.recovery.remove(i);
            state.discard_recovery(&entry);
        }
        Some(Pick::Later) => ui_state.recovery.clear(),
        None => {}
    }
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
                egui::ScrollArea::vertical()
                    .max_height(440.0)
                    .show(ui, |ui| {
                        egui::Grid::new("shortcuts")
                            .striped(true)
                            .show(ui, shortcut_rows);
                    });
            });
    }
}

/// Rows of the Keyboard Shortcuts window: commands, then the tool keys.
fn shortcut_rows(ui: &mut Ui) {
    for (keys, action) in [
        ("Ctrl+N", "New project"),
        ("Ctrl+O", "Open project"),
        ("Ctrl+S", "Save"),
        ("Ctrl+Shift+S", "Save as"),
        ("Ctrl+Z", "Undo"),
        ("Ctrl+Y / Ctrl+Shift+Z", "Redo"),
        ("Ctrl+X", "Cut (selection or layer)"),
        ("Ctrl+C", "Copy (selection or layer)"),
        ("Ctrl+V", "Paste at the cursor"),
        ("Ctrl+Alt+V", "Paste in place"),
        ("Ctrl+Shift+V", "Paste to new layer"),
        ("Enter / click", "Place the paste"),
        ("Esc", "Cancel paste or drag, deselect"),
        ("Ctrl+A", "Select all"),
        ("Ctrl+D", "Deselect"),
        ("Ctrl+I", "Invert selection"),
        ("Delete", "Delete selected voxels"),
        ("Ctrl+R", "Rotate 90° counterclockwise (facing axis)"),
        ("Ctrl+Shift+R", "Rotate 90° clockwise (facing axis)"),
        ("Ctrl+Alt+X / Y / Z", "Flip X / Y / Z"),
        ("Space", "Slice Mode on / off"),
        ("/", "Show or hide the work plane"),
        ("Ctrl+Shift+F", "Zen mode"),
        ("Right drag (viewport)", "Orbit camera"),
        ("Middle drag (viewport)", "Pan camera"),
        ("Middle double-click", "Reset camera"),
        ("Scroll (viewport)", "Zoom"),
    ] {
        ui.label(egui::RichText::new(keys).color(theme::TEXT_STRONG));
        ui.label(action);
        ui.end_row();
    }
    for tool in rizxel_engine::Tool::ALL {
        ui.label(egui::RichText::new(tool.shortcut().to_string()).color(theme::TEXT_STRONG));
        ui.label(format!("{} tool", tool.label()));
        ui.end_row();
    }
}

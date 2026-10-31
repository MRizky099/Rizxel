//! *Settings* dialog: General, Viewport, Keymap and Paths tabs over a draft
//! copy of [`Settings`]; OK applies and saves it, Cancel drops it.

use egui::{Color32, RichText, Ui};
use rizxel_app::settings::{
    GRID_OPACITY_RANGE, GRID_WIDTH_RANGE, MAX_AUTOSAVE_MINUTES, UI_SCALE_RANGE, UNDO_LIMIT_RANGE,
};
use rizxel_app::{ActionGroup, ActionId, AppState, CameraControls, KeyChord, Settings, Theme};

use crate::{keys, theme};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    General,
    Viewport,
    Keymap,
    Paths,
}

impl Tab {
    const ALL: [Tab; 4] = [Tab::General, Tab::Viewport, Tab::Keymap, Tab::Paths];

    fn label(self) -> &'static str {
        match self {
            Tab::General => "General",
            Tab::Viewport => "Viewport",
            Tab::Keymap => "Keymap",
            Tab::Paths => "Paths",
        }
    }
}

/// Waiting for a key press for `action`: replace chord `slot`, or add one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capture {
    pub action: ActionId,
    pub slot: Option<usize>,
}

/// The open dialog.
#[derive(Debug, Clone)]
pub struct SettingsDialog {
    pub draft: Settings,
    pub tab: Tab,
    pub capture: Option<Capture>,
    /// Keymap filter text.
    pub filter: String,
    /// Last refused rebinding.
    pub error: Option<String>,
    /// Result of the last *Associate .rzxl files* click.
    pub assoc_status: Option<String>,
}

impl SettingsDialog {
    pub fn new(settings: &Settings) -> Self {
        Self {
            draft: settings.clone(),
            tab: Tab::General,
            capture: None,
            filter: String::new(),
            error: None,
            assoc_status: None,
        }
    }

    /// Settings can be saved only without keymap conflicts.
    pub fn can_save(&self) -> bool {
        self.draft.keymap.conflicts().is_empty()
    }

    /// Applies a captured key press (`Esc` cancels the capture).
    pub fn captured(&mut self, key: egui::Key, modifiers: egui::Modifiers) {
        let Some(capture) = self.capture.take() else {
            return;
        };
        if key == egui::Key::Escape && modifiers.is_none() {
            return;
        }
        let Some(chord) = keys::chord_of(key, modifiers) else {
            self.error = Some(format!("{} can't be used in a shortcut", key.name()));
            return;
        };
        let mut chords = self.draft.keymap.chords(capture.action).to_vec();
        match capture.slot {
            Some(i) if i < chords.len() => chords[i] = chord,
            _ => chords.push(chord),
        }
        self.error = self
            .draft
            .keymap
            .set(capture.action, chords)
            .err()
            .map(|e| e.to_string());
    }
}

/// Feeds the next key press to a waiting rebinding before anything else
/// sees it. Call at the start of the frame.
pub fn capture_keys(ctx: &egui::Context, dialog: Option<&mut SettingsDialog>) {
    let Some(dialog) = dialog.filter(|d| d.capture.is_some()) else {
        return;
    };
    let pressed = ctx.input_mut(|i| {
        let pressed = keys::captured_chord(i);
        if pressed.is_some() {
            i.events
                .retain(|e| !matches!(e, egui::Event::Key { pressed: true, .. }));
        }
        pressed
    });
    if let Some((key, modifiers)) = pressed {
        dialog.captured(key, modifiers);
    }
}

/// Draws the dialog while it is open; OK applies and saves the draft.
pub fn show(ctx: &egui::Context, state: &mut AppState, dialog: &mut Option<SettingsDialog>) {
    let Some(d) = dialog else {
        return;
    };
    let mut close = false;
    let mut ok = false;
    let modal = egui::Modal::new(egui::Id::new("settings_dialog")).show(ctx, |ui| {
        ui.set_width(560.0);
        ui.label(RichText::new("Settings").font(theme::semibold(15.0)));
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            for tab in Tab::ALL {
                if ui.selectable_label(d.tab == tab, tab.label()).clicked() {
                    d.tab = tab;
                    d.capture = None;
                }
            }
        });
        ui.separator();
        // One height for every tab, so the dialog doesn't jump when the tab
        // changes; less in a small window.
        let height = (ctx.content_rect().height() - 200.0).clamp(160.0, 420.0);
        egui::ScrollArea::vertical()
            .max_height(height)
            .auto_shrink([false, false])
            .show(ui, |ui| match d.tab {
                Tab::General => general(ui, d),
                Tab::Viewport => viewport(ui, d),
                Tab::Keymap => keymap(ui, d),
                Tab::Paths => paths(ui, d),
            });
        ui.separator();
        ui.horizontal(|ui| {
            let can_save = d.can_save();
            if ui
                .add_enabled(can_save, egui::Button::new("OK"))
                .on_disabled_hover_text("Resolve the keymap conflicts first")
                .clicked()
            {
                ok = true;
            }
            if ui.button("Cancel").clicked() {
                close = true;
            }
            if !can_save {
                ui.label(
                    RichText::new("Keymap conflicts must be resolved").color(theme::p().danger),
                );
            }
        });
    });
    if modal.should_close() && d.capture.is_none() {
        close = true;
    }
    if ok {
        let viewport_changed = d.draft.viewport != state.settings.viewport;
        state.apply_settings(d.draft.clone());
        if viewport_changed {
            state.apply_view_defaults();
        }
        state.status_message = Some(match state.save_settings() {
            Ok(()) => "Settings saved".to_owned(),
            Err(e) => {
                log::warn!("settings: {e}");
                format!("Failed to save the settings: {e}")
            }
        });
        close = true;
    }
    if close {
        *dialog = None;
    }
}

fn row(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    ui.label(label);
    ui.horizontal(add);
    ui.end_row();
}

fn hint(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(11.0).color(theme::p().text_dim));
}

fn general(ui: &mut Ui, d: &mut SettingsDialog) {
    let g = &mut d.draft.general;
    egui::Grid::new("settings_general")
        .num_columns(2)
        .spacing([16.0, 8.0])
        .show(ui, |ui| {
            row(ui, "Theme", |ui| {
                egui::ComboBox::from_id_salt("theme")
                    .selected_text(g.theme.label())
                    .show_ui(ui, |ui| {
                        for t in Theme::ALL {
                            ui.selectable_value(&mut g.theme, t, t.label());
                        }
                    });
            });
            row(ui, "UI scale", |ui| {
                ui.add(
                    egui::Slider::new(&mut g.ui_scale, UI_SCALE_RANGE)
                        .step_by(0.05)
                        .suffix("×"),
                );
            });
            row(ui, "Undo steps per document", |ui| {
                ui.add(egui::DragValue::new(&mut g.undo_limit).range(UNDO_LIMIT_RANGE));
            });
            row(ui, "Autosave every", |ui| {
                ui.add(
                    egui::DragValue::new(&mut g.autosave_minutes)
                        .range(0..=MAX_AUTOSAVE_MINUTES)
                        .suffix(" min"),
                );
                if g.autosave_minutes == 0 {
                    ui.label(RichText::new("off: no crash recovery").color(theme::p().danger));
                }
            });
            row(ui, "Start screen", |ui| {
                ui.checkbox(&mut g.start_screen, "Show when no file was opened");
            });
            row(ui, "Layers", |ui| {
                ui.checkbox(
                    &mut g.layer_click_selects_voxels,
                    "Clicking a layer selects its voxels",
                )
                .on_hover_text(
                    "Move, Delete, Copy and the Transform tool then act on the whole \
                     layer. Painting new voxels still works unless the tool's Mask \
                     selection option is on, which limits brushes to the selection.",
                );
            });
        });
    crate::file_assoc::settings_section(ui, &mut d.assoc_status);
}

fn viewport(ui: &mut Ui, d: &mut SettingsDialog) {
    let v = &mut d.draft.viewport;
    hint(ui, "Applied when you press OK and at every start.");
    egui::Grid::new("settings_viewport")
        .num_columns(2)
        .spacing([16.0, 8.0])
        .show(ui, |ui| {
            row(ui, "Floor grid", |ui| {
                ui.checkbox(&mut v.floor_grid, "Shown");
                grid_width(ui, &mut v.floor_grid_width);
            });
            row(ui, "Voxel grid", |ui| {
                ui.checkbox(&mut v.voxel_grid, "Shown on the model");
            });
            row(ui, "", |ui| {
                grid_width(ui, &mut v.voxel_grid_width);
                ui.add(
                    egui::Slider::new(&mut v.voxel_grid_opacity, GRID_OPACITY_RANGE)
                        .step_by(0.05)
                        .text("opacity"),
                );
            });
            row(ui, "Projection", |ui| {
                ui.radio_value(&mut v.orthographic, false, "Perspective");
                ui.radio_value(&mut v.orthographic, true, "Orthographic");
            });
            row(ui, "Background (Model mode)", |ui| {
                let mut custom = v.background.is_some();
                if ui.checkbox(&mut custom, "Custom").changed() {
                    let c = theme::p().workspace;
                    v.background = custom.then_some([c.r(), c.g(), c.b()]);
                }
                if let Some(rgb) = &mut v.background {
                    ui.color_edit_button_srgb(rgb);
                }
            });
            row(ui, "Camera controls", |ui| {
                egui::ComboBox::from_id_salt("camera_controls")
                    .selected_text(v.camera_controls.label())
                    .width(300.0)
                    .show_ui(ui, |ui| {
                        for c in CameraControls::ALL {
                            ui.selectable_value(&mut v.camera_controls, c, c.label());
                        }
                    });
            });
        });
    if v.camera_controls == CameraControls::AltMouse {
        hint(
            ui,
            "Alt + right drag zooms. Alt no longer picks colors in the tools; \
             use the Color Picker tool (C).",
        );
    }
}

/// Line width slider of a grid, in pixels.
fn grid_width(ui: &mut Ui, width: &mut f32) {
    ui.add(
        egui::Slider::new(width, GRID_WIDTH_RANGE)
            .step_by(0.25)
            .suffix(" px")
            .text("width"),
    );
}

fn paths(ui: &mut Ui, d: &mut SettingsDialog) {
    let p = &mut d.draft.paths;
    egui::Grid::new("settings_paths")
        .num_columns(2)
        .spacing([16.0, 8.0])
        .show(ui, |ui| {
            row(ui, "Default export folder", |ui| {
                let text = p
                    .export_folder
                    .as_ref()
                    .map_or("System default".to_owned(), |f| f.display().to_string());
                ui.label(RichText::new(text).color(theme::p().text_strong));
            });
            row(ui, "", |ui| {
                if ui.button("Browse...").clicked() {
                    if let Some(f) = rfd::FileDialog::new()
                        .set_title("Default Export Folder")
                        .pick_folder()
                    {
                        p.export_folder = Some(f);
                    }
                }
                if ui
                    .add_enabled(p.export_folder.is_some(), egui::Button::new("Clear"))
                    .clicked()
                {
                    p.export_folder = None;
                }
            });
        });
}

fn keymap(ui: &mut Ui, d: &mut SettingsDialog) {
    ui.horizontal(|ui| {
        ui.label("Filter");
        ui.add(egui::TextEdit::singleline(&mut d.filter).desired_width(200.0));
        if ui.button("Reset All").clicked() {
            d.draft.keymap.reset_all();
            d.capture = None;
            d.error = None;
        }
    });
    hint(
        ui,
        "Click a shortcut and press the new keys (Esc cancels). + adds another one, \
         × removes it.",
    );
    if let Some(e) = &d.error {
        ui.label(RichText::new(e).color(theme::p().danger));
    }
    for c in d.draft.keymap.conflicts() {
        let names: Vec<String> = c.actions.iter().map(|a| a.label()).collect();
        ui.label(
            RichText::new(format!(
                "{} is used by: {}",
                c.chord.label(),
                names.join(", ")
            ))
            .color(theme::p().danger),
        );
    }
    let filter = d.filter.to_lowercase();
    for group in ActionGroup::ALL {
        let actions: Vec<ActionId> = ActionId::all()
            .into_iter()
            .filter(|a| a.group() == group)
            .filter(|a| {
                filter.is_empty()
                    || a.label().to_lowercase().contains(&filter)
                    || d.draft.keymap.labels(*a).to_lowercase().contains(&filter)
            })
            .collect();
        if actions.is_empty() {
            continue;
        }
        ui.add_space(6.0);
        ui.label(RichText::new(group.label()).font(theme::semibold(12.0)));
        egui::Grid::new(("keymap", group.label()))
            .num_columns(2)
            .striped(true)
            .min_col_width(240.0)
            .show(ui, |ui| {
                for action in actions {
                    keymap_row(ui, d, action);
                    ui.end_row();
                }
            });
    }
}

fn keymap_row(ui: &mut Ui, d: &mut SettingsDialog, action: ActionId) {
    let conflict = d.draft.keymap.in_conflict(action);
    let color = if conflict {
        theme::p().danger
    } else {
        theme::p().text
    };
    ui.label(RichText::new(action.label()).color(color));
    ui.horizontal(|ui| {
        let chords: Vec<KeyChord> = d.draft.keymap.chords(action).to_vec();
        if action.is_fixed() {
            for c in &chords {
                chip(ui, &c.label(), false, theme::p().text_dim)
                    .on_hover_text("Clipboard shortcuts can't be changed");
            }
            return;
        }
        let mut remove = None;
        for (i, c) in chords.iter().enumerate() {
            let waiting = d.capture
                == Some(Capture {
                    action,
                    slot: Some(i),
                });
            let text = if waiting { "Press keys..." } else { &c.label() };
            let in_conflict = d.draft.keymap.conflicts().iter().any(|k| &k.chord == c);
            let color = if in_conflict {
                theme::p().danger
            } else {
                theme::p().text_strong
            };
            if chip(ui, text, waiting, color).clicked() {
                d.capture = Some(Capture {
                    action,
                    slot: Some(i),
                });
                d.error = None;
            }
            if ui.small_button("×").on_hover_text("Remove").clicked() {
                remove = Some(i);
            }
        }
        let adding = d.capture == Some(Capture { action, slot: None });
        if adding {
            chip(ui, "Press keys...", true, theme::p().text_strong);
        } else if ui
            .small_button("+")
            .on_hover_text("Add a shortcut")
            .clicked()
        {
            d.capture = Some(Capture { action, slot: None });
            d.error = None;
        }
        if let Some(i) = remove {
            let mut chords = chords;
            chords.remove(i);
            d.error = d
                .draft
                .keymap
                .set(action, chords)
                .err()
                .map(|e| e.to_string());
            d.capture = None;
        }
        let changed = d.draft.keymap.chords(action) != action.default_chords().as_slice();
        if changed
            && ui
                .small_button("↺")
                .on_hover_text("Reset to default")
                .clicked()
        {
            d.draft.keymap.reset(action);
            d.capture = None;
        }
    });
}

fn chip(ui: &mut Ui, text: &str, active: bool, color: Color32) -> egui::Response {
    let button = egui::Button::new(RichText::new(text).monospace().color(color))
        .selected(active)
        .min_size(egui::vec2(64.0, 0.0));
    ui.add(button)
}

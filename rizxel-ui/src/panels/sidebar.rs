//! Inspector dock panels: Color Palette, Layers and Bound Model.

use egui::{Color32, Rect, RichText, Sense, Stroke, StrokeKind, Ui};
use egui_phosphor::regular as icon;
use rizxel_app::{AppState, Document, Favorites, PaletteSort};
use rizxel_math::BoundSize;

use super::UiState;
use crate::theme;
use crate::widgets::{icon_button, icon_label, IconStyle};

const PALETTE_COLUMNS: usize = 8;
const SWATCH_GAP: f32 = 4.0;

/// Color Palette dock panel.
pub fn palette(ui: &mut Ui, state: &mut AppState, ui_state: &mut UiState) {
    if state.doc().is_none() {
        return;
    }
    palette_tools(ui, state, ui_state);
    favorites_row(ui, state);
    if let Some(doc) = state.doc_mut() {
        palette_section(ui, doc, ui_state);
    }
}

/// Palette menu, used-colors toggle and the Shift+click marker.
fn palette_tools(ui: &mut Ui, state: &mut AppState, ui_state: &mut UiState) {
    let Some(doc) = state.doc() else { return };
    let selected = doc.selected_color;
    let mark = ui_state
        .palette_mark
        .filter(|&m| doc.data.palette.is_selectable(m) && m != selected);
    egui::Frame::NONE
        .inner_margin(egui::Margin::symmetric(12, 6))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.menu_button(icon_label(ui, icon::LIST, "Palette", None), |ui| {
                    ui.set_min_width(260.0);
                    palette_menu(ui, state, mark);
                });
                ui.toggle_value(&mut ui_state.palette_used_only, "Used only")
                    .on_hover_text("Show only the colors the model uses");
                if let Some(m) = mark {
                    ui.label(
                        RichText::new(format!("Marked: {m}"))
                            .size(11.0)
                            .color(theme::TEXT_DIM),
                    )
                    .on_hover_text("Shift+click a swatch to mark it for Gradient and Remap");
                }
            });
        });
}

fn palette_menu(ui: &mut Ui, state: &mut AppState, mark: Option<usize>) {
    if ui.button("Load .gpl / .hex...").clicked() {
        crate::actions::load_palette(state, false);
    }
    if ui
        .button("Load and Snap Model...")
        .on_hover_text("Replace the palette and give every voxel the nearest new color")
        .clicked()
    {
        crate::actions::load_palette(state, true);
    }
    if ui.button("Save .gpl / .hex...").clicked() {
        crate::actions::save_palette(state);
    }
    ui.separator();
    let Some(doc) = state.doc_mut() else { return };
    let selected = doc.selected_color;
    let mut message = None;
    if ui.button("Sort by Hue").clicked() && doc.sort_palette(PaletteSort::Hue) {
        message = Some("Palette sorted by hue".to_owned());
    }
    if ui.button("Sort by Luminance").clicked() && doc.sort_palette(PaletteSort::Luminance) {
        message = Some("Palette sorted by luminance".to_owned());
    }
    if ui.button("Remove Duplicates").clicked() {
        let n = doc.remove_duplicate_colors();
        message = Some(format!("Removed {n} duplicate colors"));
    }
    ui.separator();
    match mark {
        None => {
            ui.label(
                RichText::new("Gradient and Remap: Shift+click a swatch to mark it first")
                    .size(10.0)
                    .color(theme::TEXT_DIM),
            );
        }
        Some(m) => {
            if ui
                .button(format!("Gradient {m} → {selected}"))
                .on_hover_text("Blend the slots between the marked and the selected color")
                .clicked()
            {
                message = Some(if doc.palette_gradient(m, selected) {
                    format!("Gradient from color {m} to {selected}")
                } else {
                    "Gradient: the two colors need a slot between them".to_owned()
                });
            }
            if ui
                .button(format!("Remap {m} → {selected}"))
                .on_hover_text("Recolor every voxel of the marked color with the selected one")
                .clicked()
            {
                let n = doc.remap_color(m, selected);
                message = Some(format!("Remapped {n} voxels from color {m} to {selected}"));
            }
        }
    }
    if message.is_some() {
        state.status_message = message;
        ui.close();
    }
}

/// Favorite colors `0`-`9`: click selects, right-click stores the selected
/// color (also `Ctrl+digit` / `Ctrl+Shift+digit`).
fn favorites_row(ui: &mut Ui, state: &mut AppState) {
    let Some(palette) = state.doc().map(|d| d.data.palette.clone()) else {
        return;
    };
    egui::Frame::NONE
        .inner_margin(egui::Margin::symmetric(12, 2))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                let size = ((ui.available_width() - 27.0) / 10.0).clamp(12.0, 22.0);
                for slot in 0..Favorites::SLOTS {
                    let (rect, response) =
                        ui.allocate_exact_size(egui::vec2(size, size), Sense::click());
                    let color = state.favorites.get(slot).and_then(|i| palette.get(i));
                    match color {
                        Some(c) => ui.painter().rect_filled(rect, 2.0, theme::to_color32(c)),
                        None => ui.painter().rect_stroke(
                            rect.shrink(0.5),
                            2.0,
                            Stroke::new(1.0, theme::TEXT_DIM),
                            StrokeKind::Inside,
                        ),
                    };
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        slot.to_string(),
                        theme::regular(9.0),
                        Color32::from_white_alpha(170),
                    );
                    let response = response.on_hover_text(format!(
                        "Favorite {slot}: click or Ctrl+{slot} selects it, right-click or Ctrl+Shift+{slot} stores the selected color"
                    ));
                    if response.clicked() {
                        state.recall_favorite(slot);
                    }
                    if response.secondary_clicked() {
                        state.store_favorite(slot);
                    }
                }
            });
        });
}

/// Layers dock panel.
pub fn layers(ui: &mut Ui, state: &mut AppState, ui_state: &mut UiState) {
    if let Some(doc) = state.doc_mut() {
        layers_section(ui, doc, ui_state);
    }
}

/// Bound Model dock panel.
pub fn bounds(ui: &mut Ui, state: &mut AppState) {
    if let Some(doc) = state.doc_mut() {
        bounds_section(ui, doc);
    }
}

// ------------------------------------------------------------------ palette

fn palette_section(ui: &mut Ui, doc: &mut Document, ui_state: &mut UiState) {
    // Selected color preview with an editable color picker.
    egui::Frame::NONE
        .inner_margin(egui::Margin {
            left: 12,
            right: 12,
            top: 12,
            bottom: 8,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                let rgba = doc.selected_rgba();
                let mut color = theme::to_color32(rgba);

                let (rect, response) =
                    ui.allocate_exact_size(egui::vec2(48.0, 48.0), Sense::click());
                ui.painter().rect_filled(rect, theme::RADIUS, color);
                ui.painter().rect_stroke(
                    rect,
                    theme::RADIUS,
                    Stroke::new(2.0, Color32::WHITE),
                    StrokeKind::Inside,
                );
                // Click the preview to edit the palette entry.
                let popup_id = ui.id().with("palette_color_edit");
                let response = response.on_hover_text("Click to edit this palette color");
                egui::Popup::from_response(&response)
                    .id(popup_id)
                    .open_memory(response.clicked().then_some(egui::SetOpenCommand::Toggle))
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                    .show(|ui| {
                        if egui::color_picker::color_picker_color32(
                            ui,
                            &mut color,
                            egui::color_picker::Alpha::Opaque,
                        ) {
                            let [r, g, b, _] = color.to_array();
                            let new = rizxel_math::Rgba8::rgb(r, g, b);
                            let index = doc.selected_color;
                            // Merge continuous picker drags into one undo step.
                            doc.set_palette_color(index, new, true);
                        }
                    });

                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 6.0;
                    ui.add_space(6.0);
                    kv_row(ui, "HEX", &rgba.to_hex_string());
                    kv_row(ui, "RGB", &format!("{}, {}, {}", rgba.r, rgba.g, rgba.b));
                });
            });
        });
    let r = ui.cursor();
    ui.painter().hline(
        r.x_range(),
        r.top(),
        Stroke::new(1.0, Color32::from_rgb(0x33, 0x33, 0x33)),
    );

    // Swatch grid.
    egui::Frame::NONE
        .inner_margin(12)
        .show(ui, |ui| swatch_grid(ui, doc, ui_state));
}

fn kv_row(ui: &mut Ui, key: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(key).size(12.0).color(theme::TEXT));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new(value).size(12.0).color(theme::TEXT_STRONG));
        });
    });
}

fn swatch_grid(ui: &mut Ui, doc: &mut Document, ui_state: &mut UiState) {
    let colors = doc.data.palette.colors().to_vec();
    // Palette indices shown, in order ("Used only" hides unused colors).
    let shown: Vec<usize> = if ui_state.palette_used_only {
        let used = doc.used_colors();
        (0..colors.len())
            .filter(|&i| used.get(i).copied().unwrap_or(false))
            .collect()
    } else {
        (0..colors.len()).collect()
    };
    let width = ui.available_width();
    let size =
        ((width - SWATCH_GAP * (PALETTE_COLUMNS as f32 - 1.0)) / PALETTE_COLUMNS as f32).floor();
    let rows = shown.len().div_ceil(PALETTE_COLUMNS);
    let height = rows as f32 * (size + SWATCH_GAP) - SWATCH_GAP;
    let (area, _) = ui.allocate_exact_size(egui::vec2(width, height.max(0.0)), Sense::hover());

    let mut hovered: Option<(usize, Rect)> = None;
    for (n, &i) in shown.iter().enumerate() {
        let c = &colors[i];
        let (row, col) = (n / PALETTE_COLUMNS, n % PALETTE_COLUMNS);
        let rect = Rect::from_min_size(
            area.min
                + egui::vec2(
                    col as f32 * (size + SWATCH_GAP),
                    row as f32 * (size + SWATCH_GAP),
                ),
            egui::vec2(size, size),
        );
        // Slot 0 is the reserved "empty" voxel: drawn as an empty swatch
        // with a diagonal slash, and never selectable as a paint color.
        if !doc.data.palette.is_selectable(i) {
            ui.interact(rect, ui.id().with(("swatch", i)), Sense::hover())
                .on_hover_text("Empty (reserved)");
            let painter = ui.painter();
            painter.rect_stroke(
                rect.shrink(0.5),
                2.0,
                Stroke::new(1.0, theme::TEXT_DIM),
                StrokeKind::Inside,
            );
            painter.line_segment(
                [rect.left_bottom(), rect.right_top()],
                Stroke::new(1.0, theme::AXIS_X),
            );
            continue;
        }
        let response = ui
            .interact(rect, ui.id().with(("swatch", i)), Sense::click())
            .on_hover_text(format!("{i}: {} (Shift+click: mark)", c.to_hex_string()));
        if response.clicked() {
            if ui.input(|inp| inp.modifiers.shift) {
                ui_state.palette_mark = Some(i);
            } else {
                doc.select_color(i);
            }
        }
        if ui_state.palette_mark == Some(i) {
            ui.painter().rect_stroke(
                rect.expand(1.0),
                2.0,
                Stroke::new(1.0, theme::ACCENT),
                StrokeKind::Outside,
            );
        }
        if response.hovered() {
            hovered = Some((i, rect));
        }
        ui.painter().rect_filled(rect, 2.0, theme::to_color32(*c));
        if i == doc.selected_color {
            ui.painter().rect_stroke(
                rect,
                2.0,
                Stroke::new(1.0, Color32::WHITE),
                StrokeKind::Outside,
            );
        }
    }

    // Hovered swatch is drawn last, scaled up 10% like the prototype.
    if let Some((i, rect)) = hovered {
        let grown = rect.expand(size * 0.05);
        ui.painter()
            .rect_filled(grown, 2.0, theme::to_color32(colors[i]));
        ui.painter().rect_stroke(
            grown,
            2.0,
            Stroke::new(1.0, Color32::WHITE),
            StrokeKind::Outside,
        );
    }
}

// ------------------------------------------------------------------- layers

/// Height of one layer row.
const LAYER_ROW_HEIGHT: f32 = 32.0;

/// Drag payload for reordering layer rows.
#[derive(Clone, Copy)]
struct LayerDrag(usize);

enum LayerAction {
    Select(usize),
    ToggleVisible(usize),
    Solo(usize),
    ToggleLocked(usize),
    StartRename(usize),
    Duplicate(usize),
    MergeDown(usize),
    Move { from: usize, to: usize },
    Delete(usize),
}

/// Goxel-style layer panel: toolbar, reorderable list (top layer first),
/// then whole-document layer operations.
fn layers_section(ui: &mut Ui, doc: &mut Document, ui_state: &mut UiState) {
    let mut action = None;
    egui::Frame::NONE.inner_margin(8).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        layer_toolbar(ui, doc, ui_state);
        ui.add_space(2.0);

        // Higher indices draw on top, so the list runs from the last layer.
        for i in (0..doc.data.layers.len()).rev() {
            if let Some(a) = layer_row(ui, doc, i, ui_state) {
                action = Some(a);
            }
        }

        ui.add_space(6.0);
        layer_properties(ui, doc);
        ui.add_space(6.0);
        layer_operations(ui, doc, ui_state);
    });

    match action {
        Some(LayerAction::Select(i)) => doc.active_layer = i,
        Some(LayerAction::ToggleVisible(i)) => doc.toggle_layer_visible(i),
        Some(LayerAction::Solo(i)) => doc.solo_layer(i),
        Some(LayerAction::ToggleLocked(i)) => doc.toggle_layer_locked(i),
        Some(LayerAction::StartRename(i)) => {
            doc.active_layer = i;
            ui_state.renaming_layer = Some((i, doc.data.layers[i].name.clone()));
        }
        Some(LayerAction::Duplicate(i)) => {
            doc.active_layer = i;
            doc.duplicate_active_layer();
        }
        Some(LayerAction::MergeDown(i)) => {
            doc.active_layer = i;
            doc.merge_active_layer_down();
            ui_state.renaming_layer = None;
        }
        Some(LayerAction::Move { from, to }) => {
            doc.move_layer(from, to);
            ui_state.renaming_layer = None;
        }
        Some(LayerAction::Delete(i)) => {
            doc.active_layer = i;
            doc.delete_active_layer();
            ui_state.renaming_layer = None;
        }
        None => {}
    }
}

/// New / delete / merge down / move up / move down / duplicate for the
/// active layer.
fn layer_toolbar(ui: &mut Ui, doc: &mut Document, ui_state: &mut UiState) {
    let count = doc.data.layers.len();
    let active = doc.active_layer;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        let style = IconStyle::small();
        if icon_button(ui, icon::PLUS, style, true)
            .on_hover_text("New layer")
            .clicked()
        {
            doc.add_layer();
        }
        let del = style.hover_color(theme::DANGER);
        // Disabled for the last layer and for layers linked clones point at.
        if icon_button(ui, icon::TRASH, del, doc.can_delete_active_layer())
            .on_hover_text("Delete layer")
            .clicked()
        {
            doc.delete_active_layer();
            ui_state.renaming_layer = None;
        }
        if icon_button(ui, icon::ARROW_LINE_DOWN, style, active > 0)
            .on_hover_text("Merge down")
            .clicked()
        {
            doc.merge_active_layer_down();
            ui_state.renaming_layer = None;
        }
        if icon_button(ui, icon::ARROW_UP, style, active + 1 < count)
            .on_hover_text("Move layer up")
            .clicked()
        {
            doc.move_active_layer(true);
            ui_state.renaming_layer = None;
        }
        if icon_button(ui, icon::ARROW_DOWN, style, active > 0)
            .on_hover_text("Move layer down")
            .clicked()
        {
            doc.move_active_layer(false);
            ui_state.renaming_layer = None;
        }
        if icon_button(ui, icon::COPY, style, true)
            .on_hover_text("Duplicate layer")
            .clicked()
        {
            doc.duplicate_active_layer();
        }
    });
}

/// Opacity of the active layer. One undo step per drag; the viewport
/// updates the same frame without re-meshing.
fn layer_properties(ui: &mut Ui, doc: &mut Document) {
    let index = doc.active_layer;
    let Some(props) = doc.data.layers.get(index).map(|l| l.props.clone()) else {
        return;
    };
    ui.spacing_mut().slider_width = (ui.available_width() - 130.0).max(60.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("Opacity").size(12.0).color(theme::TEXT));
        let mut percent = props.opacity * 100.0;
        let response = ui.add(
            egui::Slider::new(&mut percent, 0.0..=100.0)
                .suffix("%")
                .fixed_decimals(0),
        );
        if response.changed() {
            let new = rizxel_app::LayerProps {
                opacity: (percent / 100.0).clamp(0.0, 1.0),
                ..props
            };
            doc.set_layer_props(index, new, !response.drag_started());
        }
    });
    layer_material(ui, doc, index);
}

/// Material of the active layer: pick one, add one, and edit the picked
/// material. Visible in Render mode (emission, roughness, metallic, tint);
/// the tint's alpha also acts as opacity in both modes.
fn layer_material(ui: &mut Ui, doc: &mut Document, index: usize) {
    let Some(props) = doc.data.layers.get(index).map(|l| l.props.clone()) else {
        return;
    };
    let name =
        |doc: &Document, m: Option<u32>| match m.and_then(|m| doc.data.materials.get(m as usize)) {
            Some(mat) => mat.name.clone(),
            None => "Default".to_owned(),
        };
    let mut picked = props.material;
    ui.horizontal(|ui| {
        ui.label(RichText::new("Material").size(12.0).color(theme::TEXT));
        egui::ComboBox::from_id_salt(("layer_material", index))
            .selected_text(name(doc, picked))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut picked, None, "Default");
                for (i, m) in doc.data.materials.iter().enumerate() {
                    ui.selectable_value(&mut picked, Some(i as u32), &m.name);
                }
            });
        if icon_button(ui, icon::PLUS, IconStyle::small(), true)
            .on_hover_text("New material for this layer")
            .clicked()
        {
            picked = doc.add_material();
        }
    });
    if picked != props.material {
        let new = rizxel_app::LayerProps {
            material: picked,
            ..props
        };
        doc.set_layer_props(index, new, false);
    }

    let Some(m) = picked else {
        return;
    };
    let Some(mut mat) = doc.data.materials.get(m as usize).cloned() else {
        return;
    };
    let mut changed = false;
    let mut started = false;
    egui::Grid::new(("material_editor", m))
        .num_columns(2)
        .show(ui, |ui| {
            let mut slider = |ui: &mut Ui, label: &str, value: &mut f32| {
                ui.label(RichText::new(label).size(12.0).color(theme::TEXT));
                let r = ui.add(egui::Slider::new(value, 0.0..=1.0).fixed_decimals(2));
                changed |= r.changed();
                started |= r.drag_started();
                ui.end_row();
            };
            slider(ui, "Metallic", &mut mat.metallic);
            slider(ui, "Roughness", &mut mat.roughness);
            ui.label(RichText::new("Emission").size(12.0).color(theme::TEXT));
            changed |= ui.color_edit_button_rgb(&mut mat.emission).changed();
            ui.end_row();
            ui.label(RichText::new("Tint").size(12.0).color(theme::TEXT));
            let [r, g, b, a] = mat.tint;
            let mut tint = egui::Rgba::from_rgba_unmultiplied(r, g, b, a);
            let alpha = egui::color_picker::Alpha::OnlyBlend;
            if egui::color_picker::color_edit_button_rgba(ui, &mut tint, alpha).changed() {
                mat.tint = tint.to_rgba_unmultiplied().map(|v| v.clamp(0.0, 1.0));
                changed = true;
            }
            ui.end_row();
        });
    if changed {
        doc.set_material(m as usize, mat, !started);
    }
}

/// Full-width button for an operation spanning several layers.
fn wide_button(ui: &mut Ui, glyph: &str, label: &str, enabled: bool, tip: &str) -> bool {
    let text = icon_label(ui, glyph, label, Some(12.0));
    ui.add_enabled(
        enabled,
        egui::Button::new(text).min_size(egui::vec2(ui.available_width(), 26.0)),
    )
    .on_hover_text(tip)
    .on_disabled_hover_text(tip)
    .clicked()
}

fn layer_operations(ui: &mut Ui, doc: &mut Document, ui_state: &mut UiState) {
    let visible = doc.data.layers.iter().filter(|l| l.visible).count();
    let hidden = doc.data.layers.len() - visible;
    let all_locked = doc.data.layers.iter().all(|l| l.locked);

    if wide_button(
        ui,
        icon::STACK,
        "Merge visible",
        visible > 1,
        "Merge every visible layer into the lowest visible one",
    ) {
        doc.merge_visible_layers();
        ui_state.renaming_layer = None;
    }
    if wide_button(
        ui,
        icon::EYE_SLASH,
        "Delete hidden layers",
        hidden > 0 && visible > 0,
        "Delete every hidden layer",
    ) {
        doc.delete_hidden_layers();
        ui_state.renaming_layer = None;
    }
    let (glyph, label) = if all_locked {
        (icon::LOCK_OPEN, "Unlock all layers")
    } else {
        (icon::LOCK, "Lock all layers")
    };
    if wide_button(ui, glyph, label, true, "Lock or unlock every layer") {
        doc.set_all_locked(!all_locked);
    }

    if let Some(layer) = doc.active_layer() {
        ui.add_space(4.0);
        ui.label(
            RichText::new(format!(
                "Active: {} ({} voxels)",
                layer.name,
                layer.volume.voxel_count()
            ))
            .size(11.0)
            .color(theme::TEXT_MUTED),
        );
    }
}

/// Final index of a layer dragged from `from` and dropped above (`true`) or
/// below row `target`. Rows run top layer first, so "above" is a higher index.
fn drop_index(from: usize, target: usize, above: bool) -> usize {
    match (above, from > target) {
        (true, true) => target + 1,
        (true, false) | (false, true) => target,
        (false, false) => target - 1,
    }
}

fn layer_row(
    ui: &mut Ui,
    doc: &mut Document,
    i: usize,
    ui_state: &mut UiState,
) -> Option<LayerAction> {
    let layer = &doc.data.layers[i];
    let (visible, locked) = (layer.visible, layer.locked);
    let name = layer.name.clone();
    let voxels = layer.volume.voxel_count();
    let selected_cells = layer.selection.len();
    let count = doc.data.layers.len();
    let selected = i == doc.active_layer;
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), LAYER_ROW_HEIGHT),
        Sense::click_and_drag(),
    );
    response.dnd_set_drag_payload(LayerDrag(i));

    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, theme::RADIUS, theme::BG_SELECTED);
        painter.rect_stroke(
            rect,
            theme::RADIUS,
            Stroke::new(1.0, theme::ACCENT),
            StrokeKind::Inside,
        );
    } else if response.hovered() {
        painter.rect_filled(rect, theme::RADIUS, theme::BG_HEADER);
    }

    let mut action = None;

    // Eye toggle; Ctrl+click shows only this layer.
    let eye_rect = Rect::from_center_size(
        rect.left_center() + egui::vec2(16.0, 0.0),
        egui::vec2(18.0, 18.0),
    );
    let (eye_glyph, eye_color) = if visible {
        let color = if selected {
            theme::TEXT
        } else {
            theme::TEXT_DIM
        };
        (icon::EYE, color)
    } else {
        (icon::EYE_SLASH, Color32::from_rgb(0x4b, 0x55, 0x63))
    };
    let eye = ui
        .put(eye_rect, |ui: &mut Ui| {
            icon_button(ui, eye_glyph, IconStyle::small().color(eye_color), true)
        })
        .on_hover_text(if visible {
            "Hide layer (Ctrl+click: show only this layer)"
        } else {
            "Show layer (Ctrl+click: show only this layer)"
        });
    if eye.clicked() {
        let solo = ui.input(|inp| inp.modifiers.command);
        action = Some(if solo {
            LayerAction::Solo(i)
        } else {
            LayerAction::ToggleVisible(i)
        });
    }

    // Lock toggle.
    let lock_rect = Rect::from_center_size(
        rect.right_center() - egui::vec2(16.0, 0.0),
        egui::vec2(18.0, 18.0),
    );
    let lock_glyph = if locked { icon::LOCK } else { icon::LOCK_OPEN };
    let lock_color = if selected && !locked {
        theme::TEXT_MUTED
    } else {
        theme::TEXT_DIM
    };
    let lock = ui
        .put(lock_rect, |ui: &mut Ui| {
            icon_button(ui, lock_glyph, IconStyle::small().color(lock_color), true)
        })
        .on_hover_text(if locked { "Unlock layer" } else { "Lock layer" });
    if lock.clicked() {
        action = Some(LayerAction::ToggleLocked(i));
    }

    // Voxel count, right-aligned before the lock.
    let count_galley =
        ui.painter()
            .layout_no_wrap(voxels.to_string(), theme::regular(10.0), theme::TEXT_MUTED);
    let count_pos = egui::pos2(
        lock_rect.left() - 6.0 - count_galley.size().x,
        rect.center().y - count_galley.size().y / 2.0,
    );
    ui.painter()
        .galley(count_pos, count_galley, theme::TEXT_MUTED);

    // Selection badge: selected cell count in a pill before the voxel count.
    let mut count_left = count_pos.x;
    if selected_cells > 0 {
        let galley = ui.painter().layout_no_wrap(
            selected_cells.to_string(),
            theme::regular(10.0),
            theme::TEXT_STRONG,
        );
        let size = galley.size() + egui::vec2(8.0, 2.0);
        let badge = Rect::from_min_size(
            egui::pos2(count_left - 6.0 - size.x, rect.center().y - size.y / 2.0),
            size,
        );
        ui.painter()
            .rect_filled(badge, egui::CornerRadius::same(6), theme::ACCENT);
        ui.painter()
            .galley(badge.min + egui::vec2(4.0, 1.0), galley, theme::TEXT_STRONG);
        ui.interact(badge, ui.id().with(("sel_badge", i)), Sense::hover())
            .on_hover_text(format!("{selected_cells} selected cells"));
        count_left = badge.left();
    }

    // Name (inline rename on double-click).
    let name_rect = Rect::from_min_max(
        egui::pos2(eye_rect.right() + 10.0, rect.top() + 4.0),
        egui::pos2(count_left - 6.0, rect.bottom() - 4.0),
    );
    let renaming = matches!(&ui_state.renaming_layer, Some((r, _)) if *r == i);
    if renaming {
        if let Some((_, buffer)) = ui_state.renaming_layer.as_mut() {
            let edit = ui.put(
                name_rect,
                egui::TextEdit::singleline(buffer)
                    .font(theme::regular(12.0))
                    .margin(egui::vec2(4.0, 2.0)),
            );
            if !edit.has_focus() && !edit.lost_focus() {
                edit.request_focus();
            }
            let cancel = ui.input(|inp| inp.key_pressed(egui::Key::Escape));
            if edit.lost_focus() || cancel {
                let (_, name) = ui_state.renaming_layer.take().expect("renaming");
                if !cancel {
                    doc.rename_layer(i, name);
                }
            }
        }
    } else if name_rect.width() > 0.0 {
        let mut text = RichText::new(&name).size(12.0);
        text = match (selected, visible) {
            (true, _) => text.color(theme::TEXT_STRONG).font(theme::semibold(12.0)),
            (false, false) => text.color(theme::TEXT_DIM).italics(),
            (false, true) => text.color(theme::TEXT_DIM),
        };
        let galley = egui::WidgetText::from(text).into_galley(
            ui,
            Some(egui::TextWrapMode::Truncate),
            name_rect.width(),
            egui::TextStyle::Body,
        );
        let pos = egui::pos2(name_rect.left(), rect.center().y - galley.size().y / 2.0);
        ui.painter().galley(pos, galley, theme::TEXT);
    }

    // Drag a row onto another to reorder: the upper half drops above it.
    if let Some(from) = response.dnd_hover_payload::<LayerDrag>().map(|p| p.0) {
        if from != i {
            let above = ui
                .input(|inp| inp.pointer.interact_pos())
                .is_some_and(|p| p.y < rect.center().y);
            let y = if above {
                rect.top() - 2.0
            } else {
                rect.bottom() + 2.0
            };
            ui.painter()
                .hline(rect.x_range(), y, Stroke::new(2.0, theme::ACCENT));
        }
    }
    if let Some(from) = response.dnd_release_payload::<LayerDrag>().map(|p| p.0) {
        if from != i {
            let above = ui
                .input(|inp| inp.pointer.interact_pos())
                .is_some_and(|p| p.y < rect.center().y);
            action = Some(LayerAction::Move {
                from,
                to: drop_index(from, i, above),
            });
        }
    }

    if action.is_none() && !renaming {
        if response.double_clicked() {
            action = Some(LayerAction::StartRename(i));
        } else if response.clicked() {
            action = Some(LayerAction::Select(i));
        }
    }

    response
        .on_hover_text("Double-click to rename, drag to reorder, right-click for more")
        .context_menu(|ui| {
            ui.set_min_width(180.0);
            let item = |ui: &mut Ui, glyph: &str, label: &str, enabled: bool| {
                let button = egui::Button::new(icon_label(ui, glyph, label, None));
                let clicked = ui.add_enabled(enabled, button).clicked();
                if clicked {
                    ui.close();
                }
                clicked
            };
            if item(ui, icon::PENCIL_SIMPLE, "Rename", true) {
                action = Some(LayerAction::StartRename(i));
            }
            if item(ui, icon::COPY, "Duplicate", true) {
                action = Some(LayerAction::Duplicate(i));
            }
            if item(ui, icon::ARROW_LINE_DOWN, "Merge Down", i > 0) {
                action = Some(LayerAction::MergeDown(i));
            }
            ui.separator();
            if item(ui, icon::ARROW_UP, "Move Up", i + 1 < count) {
                action = Some(LayerAction::Move { from: i, to: i + 1 });
            }
            if item(ui, icon::ARROW_DOWN, "Move Down", i > 0) {
                action = Some(LayerAction::Move { from: i, to: i - 1 });
            }
            if item(ui, icon::EYE, "Show Only This", true) {
                action = Some(LayerAction::Solo(i));
            }
            ui.separator();
            if item(ui, icon::TRASH, "Delete", count > 1) {
                action = Some(LayerAction::Delete(i));
            }
        });
    action
}

// ------------------------------------------------------------------- bounds

fn bounds_section(ui: &mut Ui, doc: &mut Document) {
    egui::Frame::NONE.inner_margin(12).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 8.0;
        ui.label(
            RichText::new("Adjust 3D bounds area size:")
                .size(12.0)
                .color(theme::TEXT_DIM),
        );

        let current = doc.data.bounds;
        let mut values = [current.x(), current.y(), current.z()];
        let mut changed = false;
        let mut dragging = false;

        for (axis, (label, color)) in [
            ("X", theme::AXIS_X),
            ("Y", theme::AXIS_Y),
            ("Z", theme::AXIS_Z),
        ]
        .into_iter()
        .enumerate()
        {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.add_sized(
                    [16.0, 24.0],
                    egui::Label::new(
                        RichText::new(label)
                            .font(theme::semibold(12.0))
                            .color(color),
                    ),
                );
                // Yellow focus ring like the prototype.
                ui.visuals_mut().widgets.active.bg_stroke = Stroke::new(1.0, theme::BOUND);
                ui.visuals_mut().selection.stroke = Stroke::new(1.0, theme::BOUND);
                let resp = ui.add_sized(
                    [ui.available_width(), 24.0],
                    egui::DragValue::new(&mut values[axis])
                        .range(BoundSize::MIN..=BoundSize::MAX)
                        .speed(0.25),
                );
                changed |= resp.changed();
                // Merge drags (and keyboard edits) of one widget into one undo step.
                dragging |= resp.dragged() || resp.has_focus();
            });
        }

        if changed {
            doc.set_bounds(BoundSize::new(values[0], values[1], values[2]), dragging);
        }

        ui.label(
            RichText::new(format!("Volume: {} voxels", current.volume()))
                .size(11.0)
                .color(theme::TEXT_MUTED),
        );
    });
}

#[cfg(test)]
mod tests {
    use super::drop_index;

    #[test]
    fn drop_index_places_layer_next_to_target() {
        // Layers [0, 1, 2, 3], list shows 3 at the top.
        assert_eq!(drop_index(0, 2, true), 2, "0 above 2 -> [1,2,0,3]");
        assert_eq!(drop_index(0, 2, false), 1, "0 below 2 -> [1,0,2,3]");
        assert_eq!(drop_index(3, 1, true), 2, "3 above 1 -> [0,1,3,2]");
        assert_eq!(drop_index(3, 1, false), 1, "3 below 1 -> [0,3,1,2]");
    }
}

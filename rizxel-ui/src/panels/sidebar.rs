//! Inspector dock panels: Color Palette, Layers and Bound Model.

use egui::{Color32, Rect, RichText, Sense, Stroke, StrokeKind, Ui};
use egui_phosphor::regular as icon;
use rizxel_app::{AppState, Document, Favorites, LayerDrop, PaletteSort};
use rizxel_math::{BoundSize, LayerKind};

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
    if ui.button("Sort by Hue").clicked()
        && doc.labeled("Sort Palette by Hue", |d| d.sort_palette(PaletteSort::Hue))
    {
        message = Some("Palette sorted by hue".to_owned());
    }
    if ui.button("Sort by Luminance").clicked()
        && doc.labeled("Sort Palette by Luminance", |d| {
            d.sort_palette(PaletteSort::Luminance)
        })
    {
        message = Some("Palette sorted by luminance".to_owned());
    }
    if ui.button("Remove Duplicates").clicked() {
        let n = doc.labeled("Remove Duplicate Colors", Document::remove_duplicate_colors);
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
                message = Some(
                    if doc.labeled("Palette Gradient", |d| d.palette_gradient(m, selected)) {
                        format!("Gradient from color {m} to {selected}")
                    } else {
                        "Gradient: the two colors need a slot between them".to_owned()
                    },
                );
            }
            if ui
                .button(format!("Remap {m} → {selected}"))
                .on_hover_text("Recolor every voxel of the marked color with the selected one")
                .clicked()
            {
                let n = doc.labeled(format!("Remap Color {m} to {selected}"), |d| {
                    d.remap_color(m, selected)
                });
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
    let render = state.view_mode == rizxel_app::ViewMode::Render;
    if let Some(doc) = state.doc_mut() {
        layers_section(ui, doc, ui_state, render);
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
/// Indent per nesting level.
const LAYER_INDENT: f32 = 14.0;

/// Drag payload for reordering layer rows.
#[derive(Clone, Copy)]
struct LayerDrag(usize);

enum LayerAction {
    Select(usize),
    ToggleVisible(usize),
    Solo(usize),
    ToggleLocked(usize),
    ToggleCollapsed(usize),
    StartRename(usize),
    Duplicate(usize),
    /// Selection to a new layer (`cut` or copy).
    ToNewLayer {
        index: usize,
        cut: bool,
    },
    MergeDown(usize),
    Step {
        index: usize,
        up: bool,
    },
    Drop {
        from: usize,
        target: LayerDrop,
    },
    Group(usize),
    Ungroup(usize),
    MergeGroup(usize),
    DeleteGroup(usize),
    Delete(usize),
}

/// What a row shows besides the layer itself.
struct RowInfo {
    depth: usize,
    /// Hidden or locked through a group.
    hidden_by_group: bool,
    locked_by_group: bool,
    has_children: bool,
}

/// Goxel-style layer panel: toolbar, reorderable tree (top layer first),
/// then whole-document layer operations. In Render mode each row shows its
/// material, and the material is edited in the Render panel.
fn layers_section(ui: &mut Ui, doc: &mut Document, ui_state: &mut UiState, render: bool) {
    let mut action = None;
    egui::Frame::NONE.inner_margin(8).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        layer_toolbar(ui, doc, ui_state);
        ui.add_space(2.0);

        // Higher indices draw on top, so the list runs from the last layer;
        // a group's subtree sits right under its row.
        let tree = doc.data.tree();
        for i in (0..doc.data.layers.len()).rev() {
            let folded = tree.ancestors(i).any(|a| {
                matches!(
                    doc.data.layers[a].kind(),
                    LayerKind::Group { collapsed: true }
                )
            });
            if folded {
                continue;
            }
            let info = RowInfo {
                depth: tree.depth(i),
                hidden_by_group: tree.ancestors(i).any(|a| !doc.data.layers[a].visible),
                locked_by_group: tree.ancestors(i).any(|a| doc.data.layers[a].locked),
                has_children: tree.has_children(i),
            };
            if let Some(a) = layer_row(ui, doc, i, &info, ui_state, render) {
                action = Some(a);
            }
        }

        ui.add_space(6.0);
        layer_properties(ui, doc, render);
        ui.add_space(6.0);
        layer_operations(ui, doc, ui_state);
    });

    let Some(action) = action else { return };
    // Structure changes shift indices: stop any rename in progress.
    if !matches!(
        action,
        LayerAction::Select(_)
            | LayerAction::ToggleVisible(_)
            | LayerAction::ToggleLocked(_)
            | LayerAction::Solo(_)
            | LayerAction::ToggleCollapsed(_)
            | LayerAction::StartRename(_)
    ) {
        ui_state.renaming_layer = None;
    }
    match action {
        LayerAction::Select(i) => doc.active_layer = i,
        LayerAction::ToggleVisible(i) => doc.toggle_layer_visible(i),
        LayerAction::Solo(i) => doc.solo_layer(i),
        LayerAction::ToggleLocked(i) => doc.toggle_layer_locked(i),
        LayerAction::ToggleCollapsed(i) => {
            let collapsed = matches!(
                doc.data.layers[i].kind(),
                LayerKind::Group { collapsed: true }
            );
            doc.set_group_collapsed(i, !collapsed);
        }
        LayerAction::StartRename(i) => {
            doc.active_layer = i;
            ui_state.renaming_layer = Some((i, doc.data.layers[i].name.clone()));
        }
        LayerAction::Duplicate(i) => {
            doc.active_layer = i;
            doc.duplicate_active_layer();
        }
        LayerAction::ToNewLayer { index, cut } => {
            doc.active_layer = index;
            if let Err(e) = doc.selection_to_new_layer(cut) {
                log::info!("Selection to new layer: {}", e.message());
            }
        }
        LayerAction::MergeDown(i) => {
            doc.active_layer = i;
            doc.merge_active_layer_down();
        }
        LayerAction::Step { index, up } => {
            doc.active_layer = index;
            doc.move_active_layer(up);
        }
        LayerAction::Drop { from, target } => {
            doc.labeled("Move Layer", |d| d.drop_layer(from, target));
        }
        LayerAction::Group(i) => {
            doc.group_layer(i);
        }
        LayerAction::Ungroup(i) => {
            doc.ungroup(i);
        }
        LayerAction::MergeGroup(i) => {
            doc.merge_group(i);
        }
        LayerAction::DeleteGroup(i) => {
            doc.delete_group_with_contents(i);
        }
        LayerAction::Delete(i) => {
            doc.active_layer = i;
            doc.delete_active_layer();
        }
    }
}

/// New layer / new group / delete / merge down / move up / move down /
/// duplicate for the active layer.
fn layer_toolbar(ui: &mut Ui, doc: &mut Document, ui_state: &mut UiState) {
    let active = doc.active_layer;
    let is_group = doc.active_layer().is_some_and(|l| l.kind().is_group());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        let style = IconStyle::small();
        if icon_button(ui, icon::PLUS, style, true)
            .on_hover_text("New layer")
            .clicked()
        {
            doc.add_layer();
        }
        if icon_button(ui, icon::FOLDER_PLUS, style, true)
            .on_hover_text("New group")
            .clicked()
        {
            doc.add_group();
        }
        let del = style.hover_color(theme::DANGER);
        // Disabled for the last layer and for layers linked clones point at.
        if icon_button(ui, icon::TRASH, del, doc.can_delete_active_layer())
            .on_hover_text("Delete layer (a group's layers move up a level)")
            .clicked()
        {
            doc.delete_active_layer();
            ui_state.renaming_layer = None;
        }
        if icon_button(ui, icon::ARROW_LINE_DOWN, style, active > 0 && !is_group)
            .on_hover_text("Merge down")
            .clicked()
        {
            doc.merge_active_layer_down();
            ui_state.renaming_layer = None;
        }
        if icon_button(ui, icon::ARROW_UP, style, true)
            .on_hover_text("Move layer up (inside its group)")
            .clicked()
        {
            doc.move_active_layer(true);
            ui_state.renaming_layer = None;
        }
        if icon_button(ui, icon::ARROW_DOWN, style, true)
            .on_hover_text("Move layer down (inside its group)")
            .clicked()
        {
            doc.move_active_layer(false);
            ui_state.renaming_layer = None;
        }
        if icon_button(ui, icon::COPY, style, true)
            .on_hover_text("Duplicate layer (a group with its contents)")
            .clicked()
        {
            doc.duplicate_active_layer();
            ui_state.renaming_layer = None;
        }
    });
}

/// Opacity of the active layer. One undo step per drag; the viewport
/// updates the same frame without re-meshing.
fn layer_properties(ui: &mut Ui, doc: &mut Document, render: bool) {
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
    if !render {
        layer_material(ui, doc, index);
    }
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

/// Drop zone of the pointer at height `y` over a row: the upper third
/// drops above and the lower third below; the middle of a group row drops
/// into it (other rows split at the middle). The lower third of an
/// expanded group with children also drops into it, as its top child,
/// since that is where the row below it is.
fn drop_zone(row: Rect, y: f32, i: usize, group: bool, open_children: bool) -> LayerDrop {
    let t = (y - row.top()) / row.height().max(1.0);
    match (group, t) {
        (false, t) if t < 0.5 => LayerDrop::Above(i),
        (false, _) => LayerDrop::Below(i),
        (true, t) if t < 1.0 / 3.0 => LayerDrop::Above(i),
        (true, t) if t < 2.0 / 3.0 || open_children => LayerDrop::Into(i),
        (true, _) => LayerDrop::Below(i),
    }
}

fn layer_row(
    ui: &mut Ui,
    doc: &mut Document,
    i: usize,
    info: &RowInfo,
    ui_state: &mut UiState,
    render: bool,
) -> Option<LayerAction> {
    let layer = &doc.data.layers[i];
    let (visible, locked) = (layer.visible, layer.locked);
    let name = layer.name.clone();
    let group = match layer.kind() {
        LayerKind::Group { collapsed } => Some(*collapsed),
        _ => None,
    };
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
    let dim = Color32::from_rgb(0x4b, 0x55, 0x63);
    let (eye_glyph, eye_color) = match (visible, info.hidden_by_group) {
        (false, _) => (icon::EYE_SLASH, dim),
        (true, true) => (icon::EYE, dim),
        (true, false) if selected => (icon::EYE, theme::TEXT),
        (true, false) => (icon::EYE, theme::TEXT_DIM),
    };
    let eye = ui
        .put(eye_rect, |ui: &mut Ui| {
            icon_button(ui, eye_glyph, IconStyle::small().color(eye_color), true)
        })
        .on_hover_text(match (visible, info.hidden_by_group) {
            (true, true) => "Hidden by its group (Ctrl+click: show only this layer)",
            (true, false) => "Hide layer (Ctrl+click: show only this layer)",
            (false, _) => "Show layer (Ctrl+click: show only this layer)",
        });
    if eye.clicked() {
        let solo = ui.input(|inp| inp.modifiers.command);
        action = Some(if solo {
            LayerAction::Solo(i)
        } else {
            LayerAction::ToggleVisible(i)
        });
    }

    // Lock toggle; a lock inherited from a group shows dimmed.
    let lock_rect = Rect::from_center_size(
        rect.right_center() - egui::vec2(16.0, 0.0),
        egui::vec2(18.0, 18.0),
    );
    let lock_glyph = if locked || info.locked_by_group {
        icon::LOCK
    } else {
        icon::LOCK_OPEN
    };
    let lock_color = if !locked && info.locked_by_group {
        dim
    } else if selected && !locked {
        theme::TEXT_MUTED
    } else {
        theme::TEXT_DIM
    };
    let lock = ui
        .put(lock_rect, |ui: &mut Ui| {
            icon_button(ui, lock_glyph, IconStyle::small().color(lock_color), true)
        })
        .on_hover_text(match (locked, info.locked_by_group) {
            (true, _) => "Unlock layer",
            (false, true) => "Locked by its group (click to lock this layer too)",
            (false, false) => "Lock layer",
        });
    if lock.clicked() {
        action = Some(LayerAction::ToggleLocked(i));
    }

    // Voxel count (voxel layers), right-aligned before the lock. Render
    // mode shows the layer's material and its swatch instead.
    let count_text = if group.is_some() {
        String::new()
    } else if render {
        doc.layer_material(i)
            .map_or_else(|| "Default".to_owned(), |m| m.name.clone())
    } else {
        voxels.to_string()
    };
    if render && group.is_none() {
        let swatch = Rect::from_center_size(
            lock_rect.left_center() - egui::vec2(10.0, 0.0),
            egui::vec2(10.0, 10.0),
        );
        let fill = super::render::material_swatch(doc.layer_material(i));
        ui.painter().circle_filled(swatch.center(), 5.0, fill);
        ui.painter()
            .circle_stroke(swatch.center(), 5.0, Stroke::new(1.0, theme::BORDER));
    }
    let lock_rect = if render && group.is_none() {
        lock_rect.translate(egui::vec2(-16.0, 0.0))
    } else {
        lock_rect
    };
    let count_galley =
        ui.painter()
            .layout_no_wrap(count_text, theme::regular(10.0), theme::TEXT_MUTED);
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

    // Indent, then the collapse caret and folder icon of a group.
    let mut name_left = eye_rect.right() + 10.0 + info.depth as f32 * LAYER_INDENT;
    if let Some(collapsed) = group {
        let caret_rect = Rect::from_center_size(
            egui::pos2(name_left + 7.0, rect.center().y),
            egui::vec2(16.0, 18.0),
        );
        let caret = if collapsed {
            icon::CARET_RIGHT
        } else {
            icon::CARET_DOWN
        };
        let toggle = ui
            .put(caret_rect, |ui: &mut Ui| {
                icon_button(ui, caret, IconStyle::small().color(theme::TEXT_DIM), true)
            })
            .on_hover_text(if collapsed {
                "Expand group"
            } else {
                "Collapse group"
            });
        if toggle.clicked() {
            action = Some(LayerAction::ToggleCollapsed(i));
        }
        let folder = if collapsed || !info.has_children {
            icon::FOLDER
        } else {
            icon::FOLDER_OPEN
        };
        ui.painter().text(
            egui::pos2(caret_rect.right() + 8.0, rect.center().y),
            egui::Align2::CENTER_CENTER,
            folder,
            theme::icon(14.0),
            if selected {
                theme::TEXT
            } else {
                theme::TEXT_DIM
            },
        );
        name_left = caret_rect.right() + 18.0;
    }

    // Name (inline rename on double-click).
    let name_rect = Rect::from_min_max(
        egui::pos2(name_left, rect.top() + 4.0),
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
        let shown = visible && !info.hidden_by_group;
        let mut text = RichText::new(&name).size(12.0);
        text = match (selected, shown) {
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

    // Drag a row onto another: above, below or into a group. Drops the
    // document refuses (into its own subtree) show no marker.
    let zone = |ui: &Ui| {
        ui.input(|inp| inp.pointer.interact_pos()).map(|p| {
            let open = group == Some(false) && info.has_children;
            drop_zone(rect, p.y, i, group.is_some(), open)
        })
    };
    if let Some(from) = response.dnd_hover_payload::<LayerDrag>().map(|p| p.0) {
        if let Some(target) = zone(ui).filter(|&t| doc.drop_destination(from, t).is_some()) {
            let stroke = Stroke::new(2.0, theme::ACCENT);
            let indented = (name_left - 4.0)..=rect.right();
            match target {
                LayerDrop::Above(_) => {
                    ui.painter().hline(indented, rect.top() - 2.0, stroke);
                }
                LayerDrop::Below(_) => {
                    ui.painter().hline(indented, rect.bottom() + 2.0, stroke);
                }
                LayerDrop::Into(_) => {
                    ui.painter()
                        .rect_stroke(rect, theme::RADIUS, stroke, StrokeKind::Inside);
                }
            }
        }
    }
    if let Some(from) = response.dnd_release_payload::<LayerDrag>().map(|p| p.0) {
        if let Some(target) = zone(ui) {
            action = Some(LayerAction::Drop { from, target });
        }
    }

    if action.is_none() && !renaming {
        if response.double_clicked() {
            action = Some(LayerAction::StartRename(i));
        } else if response.clicked() {
            action = Some(LayerAction::Select(i));
        }
    }

    let can_delete_contents = doc.can_delete_group_with_contents(i);
    response
        .on_hover_text(
            "Double-click to rename, drag to reorder or into a group, right-click for more",
        )
        .context_menu(|ui| {
            ui.set_min_width(200.0);
            let item = |ui: &mut Ui, glyph: &str, label: &str, shortcut: &str, enabled: bool| {
                let button =
                    egui::Button::new(icon_label(ui, glyph, label, None)).shortcut_text(shortcut);
                let clicked = ui.add_enabled(enabled, button).clicked();
                if clicked {
                    ui.close();
                }
                clicked
            };
            if item(ui, icon::PENCIL_SIMPLE, "Rename", "", true) {
                action = Some(LayerAction::StartRename(i));
            }
            if item(ui, icon::COPY, "Duplicate", "", true) {
                action = Some(LayerAction::Duplicate(i));
            }
            let selected = !doc.data.layers[i].selection.is_empty();
            if item(
                ui,
                icon::SCISSORS,
                "Selection to New Layer (Cut)",
                "Ctrl+Shift+J",
                selected,
            ) {
                action = Some(LayerAction::ToNewLayer {
                    index: i,
                    cut: true,
                });
            }
            if item(
                ui,
                icon::STACK_PLUS,
                "Selection to New Layer (Copy)",
                "Ctrl+J",
                selected,
            ) {
                action = Some(LayerAction::ToNewLayer {
                    index: i,
                    cut: false,
                });
            }
            if item(
                ui,
                icon::ARROW_LINE_DOWN,
                "Merge Down",
                "",
                i > 0 && group.is_none(),
            ) {
                action = Some(LayerAction::MergeDown(i));
            }
            ui.separator();
            if item(ui, icon::FOLDER_PLUS, "Group", "Ctrl+G", true) {
                action = Some(LayerAction::Group(i));
            }
            if group.is_some() {
                if item(ui, icon::FOLDER_MINUS, "Ungroup", "Ctrl+Shift+G", count > 1) {
                    action = Some(LayerAction::Ungroup(i));
                }
                if item(ui, icon::STACK, "Merge Group", "", info.has_children) {
                    action = Some(LayerAction::MergeGroup(i));
                }
            }
            ui.separator();
            if item(ui, icon::ARROW_UP, "Move Up", "", true) {
                action = Some(LayerAction::Step { index: i, up: true });
            }
            if item(ui, icon::ARROW_DOWN, "Move Down", "", true) {
                action = Some(LayerAction::Step {
                    index: i,
                    up: false,
                });
            }
            if item(ui, icon::EYE, "Show Only This", "", true) {
                action = Some(LayerAction::Solo(i));
            }
            ui.separator();
            if item(ui, icon::TRASH, "Delete", "", count > 1) {
                action = Some(LayerAction::Delete(i));
            }
            if group.is_some()
                && item(
                    ui,
                    icon::TRASH,
                    "Delete Group and Contents",
                    "",
                    can_delete_contents,
                )
            {
                action = Some(LayerAction::DeleteGroup(i));
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
    use super::drop_zone;
    use egui::{pos2, Rect};
    use rizxel_app::LayerDrop;

    #[test]
    fn drop_zones_split_rows() {
        let r = Rect::from_min_max(pos2(0.0, 100.0), pos2(200.0, 130.0));
        // Plain rows split at the middle.
        assert_eq!(drop_zone(r, 105.0, 3, false, false), LayerDrop::Above(3));
        assert_eq!(drop_zone(r, 125.0, 3, false, false), LayerDrop::Below(3));
        // Group rows: thirds, the middle drops into the group.
        assert_eq!(drop_zone(r, 105.0, 3, true, false), LayerDrop::Above(3));
        assert_eq!(drop_zone(r, 115.0, 3, true, false), LayerDrop::Into(3));
        assert_eq!(drop_zone(r, 125.0, 3, true, false), LayerDrop::Below(3));
        // An expanded group's lower third leads to its first child.
        assert_eq!(drop_zone(r, 125.0, 3, true, true), LayerDrop::Into(3));
    }
}

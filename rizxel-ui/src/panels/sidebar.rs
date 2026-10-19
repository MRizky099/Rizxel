//! Right inspector: Color Palette, Layers and Bound Model sections.

use egui::{Color32, Id, Rect, RichText, Sense, Stroke, StrokeKind, Ui};
use egui_phosphor::regular as icon;
use rizxel_app::{AppState, Document};
use rizxel_math::BoundSize;

use super::UiState;
use crate::theme;
use crate::widgets::{icon_button, panel_header, IconStyle};

const PALETTE_COLUMNS: usize = 8;
const SWATCH_GAP: f32 = 4.0;

pub fn show(ui: &mut Ui, state: &mut AppState, ui_state: &mut UiState) {
    let Some(doc) = state.doc_mut() else {
        return;
    };

    ui.spacing_mut().item_spacing.y = 0.0;
    let total = ui.available_height();
    let collapsed = [ui_state.palette_collapsed, false, ui_state.bounds_collapsed];

    // Collapsed sections keep a header-only height; the rest share the rest
    // of the space evenly (matching the prototype's flex ratios).
    let order = ui_state.sidebar_order;
    let n_open = order.iter().filter(|&&s| !collapsed[s]).count().max(1) as f32;
    let used = order.iter().filter(|&&s| collapsed[s]).count() as f32 * 32.0;
    let flex = ((total - used) / n_open).max(120.0);

    // Draw each section; register its header rect for drag-to-reorder.
    for (slot, &s) in order.iter().enumerate() {
        let h = if collapsed[s] { 32.0 } else { flex };
        let header_rect =
            Rect::from_min_size(ui.cursor().min, egui::vec2(ui.available_width(), 32.0));
        register_header(ui, ui_state, s, slot, header_rect);
        match s {
            0 => section(ui, h, |ui| palette_section(ui, doc, ui_state)),
            1 => section(ui, h, |ui| layers_section(ui, doc, ui_state)),
            _ => section(ui, h, |ui| bounds_section(ui, doc, ui_state)),
        }
    }
}

/// Tracks header drags so sections can be reordered. A drag that moves
/// beyond the midpoint of an adjacent slot swaps the two sections.
fn register_header(ui: &mut Ui, ui_state: &mut UiState, section: usize, slot: usize, rect: Rect) {
    let resp = ui.interact(rect, Id::new(("sidebar_header", section)), Sense::drag());
    let drag = resp.drag_delta().y;
    if drag == 0.0 || ui_state.sidebar_order[slot] != section {
        return;
    }
    let target = if drag < 0.0 {
        slot.checked_sub(1)
    } else {
        (slot + 1 < 3).then_some(slot + 1)
    };
    let Some(target) = target else {
        return;
    };
    let order = &mut ui_state.sidebar_order;
    order.swap(slot, target);
}

/// Allocates a fixed-height vertical region with a dark bottom separator.
fn section(ui: &mut Ui, height: f32, add: impl FnOnce(&mut Ui)) {
    let rect = Rect::from_min_size(ui.cursor().min, egui::vec2(ui.available_width(), height));
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
        |ui| {
            ui.set_clip_rect(rect.intersect(ui.clip_rect()));
            ui.set_min_size(rect.size());
            add(ui);
        },
    );
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        Stroke::new(1.0, theme::BG_APP),
    );
}

fn collapse_icon(collapsed: bool) -> &'static str {
    if collapsed {
        icon::CARET_RIGHT
    } else {
        icon::CARET_DOWN
    }
}

// ------------------------------------------------------------------ palette

fn palette_section(ui: &mut Ui, doc: &mut Document, ui_state: &mut UiState) {
    let collapsed = ui_state.palette_collapsed;
    panel_header(
        ui,
        "COLOR PALETTE",
        None,
        theme::TEXT,
        theme::BORDER,
        |ui| {
            if icon_button(ui, collapse_icon(collapsed), IconStyle::small(), true).clicked() {
                ui_state.palette_collapsed = !collapsed;
            }
        },
    );
    if collapsed {
        return;
    }

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
    egui::ScrollArea::vertical()
        .id_salt("palette_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Frame::NONE
                .inner_margin(12)
                .show(ui, |ui| swatch_grid(ui, doc));
        });
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

fn swatch_grid(ui: &mut Ui, doc: &mut Document) {
    let colors = doc.data.palette.colors().to_vec();
    let width = ui.available_width();
    let size =
        ((width - SWATCH_GAP * (PALETTE_COLUMNS as f32 - 1.0)) / PALETTE_COLUMNS as f32).floor();
    let rows = colors.len().div_ceil(PALETTE_COLUMNS);
    let height = rows as f32 * (size + SWATCH_GAP) - SWATCH_GAP;
    let (area, _) = ui.allocate_exact_size(egui::vec2(width, height.max(0.0)), Sense::hover());

    let mut hovered: Option<(usize, Rect)> = None;
    for (i, c) in colors.iter().enumerate() {
        let (row, col) = (i / PALETTE_COLUMNS, i % PALETTE_COLUMNS);
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
            .on_hover_text(c.to_hex_string());
        if response.clicked() {
            doc.select_color(i);
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

enum LayerAction {
    Select(usize),
    ToggleVisible(usize),
    ToggleLocked(usize),
    StartRename(usize),
}

fn layers_section(ui: &mut Ui, doc: &mut Document, ui_state: &mut UiState) {
    let can_delete = doc.data.layers.len() > 1;
    let mut add = false;
    let mut delete = false;
    panel_header(ui, "LAYERS", None, theme::TEXT, theme::BORDER, |ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let del_style = IconStyle::small().hover_color(theme::DANGER);
        delete = icon_button(ui, icon::TRASH, del_style, can_delete)
            .on_hover_text("Delete Layer")
            .clicked();
        add = icon_button(ui, icon::PLUS, IconStyle::small(), true)
            .on_hover_text("New Layer")
            .clicked();
    });
    if add {
        doc.add_layer();
    }
    if delete {
        doc.delete_active_layer();
        ui_state.renaming_layer = None;
    }

    let mut action = None;
    egui::ScrollArea::vertical()
        .id_salt("layers_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Frame::NONE.inner_margin(8).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                for i in 0..doc.data.layers.len() {
                    if let Some(a) = layer_row(ui, doc, i, ui_state) {
                        action = Some(a);
                    }
                }
            });
        });

    match action {
        Some(LayerAction::Select(i)) => doc.active_layer = i,
        Some(LayerAction::ToggleVisible(i)) => doc.toggle_layer_visible(i),
        Some(LayerAction::ToggleLocked(i)) => doc.toggle_layer_locked(i),
        Some(LayerAction::StartRename(i)) => {
            doc.active_layer = i;
            ui_state.renaming_layer = Some((i, doc.data.layers[i].name.clone()));
        }
        None => {}
    }
}

fn layer_row(
    ui: &mut Ui,
    doc: &mut Document,
    i: usize,
    ui_state: &mut UiState,
) -> Option<LayerAction> {
    let layer = doc.data.layers[i].clone();
    let selected = i == doc.active_layer;
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 32.0), Sense::click());

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

    // Eye toggle.
    let eye_rect = Rect::from_center_size(
        rect.left_center() + egui::vec2(16.0, 0.0),
        egui::vec2(18.0, 18.0),
    );
    let (eye_glyph, eye_color) = if layer.visible {
        (
            icon::EYE,
            if selected {
                theme::TEXT
            } else {
                theme::TEXT_DIM
            },
        )
    } else {
        (icon::EYE_SLASH, Color32::from_rgb(0x4b, 0x55, 0x63))
    };
    let eye = ui
        .put(eye_rect, |ui: &mut Ui| {
            icon_button(ui, eye_glyph, IconStyle::small().color(eye_color), true)
        })
        .on_hover_text(if layer.visible {
            "Hide layer"
        } else {
            "Show layer"
        });
    if eye.clicked() {
        action = Some(LayerAction::ToggleVisible(i));
    }

    // Lock toggle.
    let lock_rect = Rect::from_center_size(
        rect.right_center() - egui::vec2(16.0, 0.0),
        egui::vec2(18.0, 18.0),
    );
    let lock_glyph = if layer.locked {
        icon::LOCK
    } else {
        icon::LOCK_OPEN
    };
    let lock_color = if selected && !layer.locked {
        theme::TEXT_MUTED
    } else {
        theme::TEXT_DIM
    };
    let lock = ui
        .put(lock_rect, |ui: &mut Ui| {
            icon_button(ui, lock_glyph, IconStyle::small().color(lock_color), true)
        })
        .on_hover_text(if layer.locked {
            "Unlock layer"
        } else {
            "Lock layer"
        });
    if lock.clicked() {
        action = Some(LayerAction::ToggleLocked(i));
    }

    // Name (inline rename on double-click).
    let name_rect = Rect::from_min_max(
        egui::pos2(eye_rect.right() + 10.0, rect.top() + 4.0),
        egui::pos2(lock_rect.left() - 6.0, rect.bottom() - 4.0),
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
    } else {
        let mut text = RichText::new(&layer.name).size(12.0);
        text = match (selected, layer.visible) {
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

    if action.is_none() && !renaming {
        if response.double_clicked() {
            action = Some(LayerAction::StartRename(i));
        } else if response.clicked() {
            action = Some(LayerAction::Select(i));
        }
    }
    response.on_hover_text("Double-click to rename");
    action
}

// ------------------------------------------------------------------- bounds

fn bounds_section(ui: &mut Ui, doc: &mut Document, ui_state: &mut UiState) {
    let collapsed = ui_state.bounds_collapsed;
    panel_header(
        ui,
        "BOUND MODEL",
        Some(icon::PACKAGE),
        theme::BOUND,
        theme::BOUND,
        |ui| {
            let style = IconStyle::small().hover_color(theme::BOUND);
            if icon_button(ui, collapse_icon(collapsed), style, true).clicked() {
                ui_state.bounds_collapsed = !collapsed;
            }
        },
    );
    if collapsed {
        return;
    }

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

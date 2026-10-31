//! Left vertical toolbar.

use egui::{Align2, Color32, Rect, Response, Sense, Stroke, TextureHandle, Ui, Vec2};
use egui_phosphor::regular as icon;
use rizxel_app::{ActionId, AppState, Keymap};
use rizxel_engine::Tool;

use crate::icons::{self, Icon, Icons};
use crate::panels::UiState;
use crate::widgets::{horizontal_divider, IconStyle};
use crate::{theme, tool_input};

/// Drawn size of the pixel-art icons inside the 40 px buttons.
const ICON_SIZE: f32 = 26.0;
/// Inactive icons are slightly dimmed so the active/hovered one stands out.
const INACTIVE_TINT: Color32 = Color32::from_gray(185);

/// `Name (Key)`, or just the name when the action has no shortcut.
fn shortcut_tip(name: &str, keymap: &Keymap, action: ActionId) -> String {
    match keymap.label(action) {
        k if k.is_empty() => name.to_owned(),
        k => format!("{name} ({k})"),
    }
}

pub fn show(ui: &mut Ui, state: &mut AppState, ui_state: &mut UiState, icons: &Icons) {
    // Subtle right border + shadow line toward the viewport.
    let panel = ui.max_rect();
    ui.painter().vline(
        panel.right() - 0.5,
        panel.y_range(),
        Stroke::new(1.0, theme::BORDER),
    );

    ui.vertical_centered(|ui| {
        ui.spacing_mut().item_spacing.y = 8.0;
        ui.add_space(8.0);

        for tool in Tool::PRIMARY {
            tool_button(ui, state, icons, tool);
        }
        horizontal_divider(ui, 24.0);
        for tool in Tool::SECONDARY {
            tool_button(ui, state, icons, tool);
        }
        horizontal_divider(ui, 24.0);
        slice_toggle(ui, state);
        mask_toggle(ui, state);

        // Settings pinned to the bottom.
        let remaining = ui.available_height() - 48.0;
        if remaining > 0.0 {
            ui.add_space(remaining);
        }
        let settings = ui_state.settings.is_some();
        let tip = shortcut_tip("Settings", &state.settings.keymap, ActionId::Settings);
        if textured_button(ui, icons.get(Icon::Settings), settings)
            .on_hover_text(tip)
            .clicked()
        {
            ui_state.open_settings(state);
        }
    });
}

fn tool_button(ui: &mut Ui, state: &mut AppState, icons: &Icons, tool: Tool) {
    let active = state.tool == tool;
    if textured_button(ui, icons.get(tool), active)
        .on_hover_text(shortcut_tip(
            tool.label(),
            &state.settings.keymap,
            ActionId::Tool(tool),
        ))
        .clicked()
    {
        state.tool = tool;
    }
}

/// Slice Mode (Qubicle's `2D` button): one slice of the model from any
/// tool, without switching tools. While slicing, the label is the axis and
/// the 1-based `n/N` slice number in the axis colour.
fn slice_toggle(ui: &mut Ui, state: &mut AppState) {
    let enabled = state.doc().is_some();
    let (label, label_color) = match state.slice_counter() {
        Some((axis, n, count)) => (
            format!("{} {n}/{count}", ["X", "Y", "Z"][axis]),
            Some(tool_input::axis_color(axis)),
        ),
        None => ("Slice".to_owned(), None),
    };
    let active = state.slice_counter().is_some();
    let response = mode_toggle(ui, icon::STACK_SIMPLE, &label, label_color, active, enabled)
        .on_hover_text(
            "Slice Mode (Space): edit one slice of the model with any tool. Space over a voxel slices at that voxel.",
        );
    if response.clicked() && enabled {
        response.surrender_focus();
        state.toggle_slice(None);
    }
}

/// Mask Mode button (`Shift+M`): protects the selection from every tool.
fn mask_toggle(ui: &mut Ui, state: &mut AppState) {
    let active = state.options.mask_selection;
    let response = mode_toggle(ui, icon::SHIELD_CHECK, "Mask", None, active, true).on_hover_text(
        "Mask Mode (Shift+M): protect the selected voxels. Every tool then edits only the rest of the layer.",
    );
    if response.clicked() {
        response.surrender_focus();
        state.toggle_mask_selection();
    }
}

/// A rail toggle for a mode: an icon over a short label, in a tool-sized
/// button with the active accent of the tool buttons.
fn mode_toggle(
    ui: &mut Ui,
    glyph: &str,
    label: &str,
    label_color: Option<Color32>,
    active: bool,
    enabled: bool,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(IconStyle::tool().size, Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, enabled, active, label)
    });
    let painter = ui.painter();
    let hovered = response.hovered() && enabled;
    if active {
        painter.rect_filled(rect, theme::RADIUS, theme::HOVER);
        let accent = Rect::from_min_max(rect.min, egui::pos2(rect.left() + 2.0, rect.bottom()));
        painter.rect_filled(accent, 0.0, theme::ACCENT_LINE);
    } else if hovered {
        painter.rect_filled(rect, theme::RADIUS, theme::HOVER);
    }
    let color = if !enabled {
        theme::TEXT_MUTED
    } else if active || hovered {
        theme::TEXT_STRONG
    } else {
        INACTIVE_TINT
    };
    painter.text(
        rect.center() - Vec2::new(0.0, 7.0),
        Align2::CENTER_CENTER,
        glyph,
        theme::icon(16.0),
        color,
    );
    painter.text(
        rect.center() + Vec2::new(0.0, 11.0),
        Align2::CENTER_CENTER,
        label,
        theme::regular(11.0),
        label_color.unwrap_or(color),
    );
    response
}

/// 40x40 toolbar button showing a pixel-art texture. Active buttons get a
/// highlighted background and a blue accent line on the left edge.
fn textured_button(ui: &mut Ui, texture: &TextureHandle, active: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(IconStyle::tool().size, Sense::click());
    let painter = ui.painter();

    if active {
        painter.rect_filled(rect, theme::RADIUS, theme::HOVER);
        let accent = Rect::from_min_max(rect.min, egui::pos2(rect.left() + 2.0, rect.bottom()));
        painter.rect_filled(accent, 0.0, theme::ACCENT_LINE);
    } else if response.hovered() {
        painter.rect_filled(rect, theme::RADIUS, theme::HOVER);
    }

    let tint = if active || response.hovered() {
        Color32::WHITE
    } else {
        INACTIVE_TINT
    };
    icons::paint(ui, rect, texture, ICON_SIZE, tint);
    response
}

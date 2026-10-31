//! Left vertical toolbar.

use egui::{Align2, Color32, Rect, Response, RichText, Sense, Stroke, TextureHandle, Ui, Vec2};
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

/// Qubicle's `2D` button and slice counter: Slice Mode from any tool,
/// without switching tools. The counter shows the axis and the 1-based
/// `n/N` slice number while slicing.
fn slice_toggle(ui: &mut Ui, state: &mut AppState) {
    let counter = state.slice_counter();
    let enabled = state.doc().is_some();
    let size = Vec2::new(IconStyle::tool().size.x, 26.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let painter = ui.painter();
    let active = counter.is_some();
    if active {
        painter.rect_filled(rect, theme::RADIUS, theme::HOVER);
        let accent = Rect::from_min_max(rect.min, egui::pos2(rect.left() + 2.0, rect.bottom()));
        painter.rect_filled(accent, 0.0, theme::ACCENT_LINE);
    } else if response.hovered() && enabled {
        painter.rect_filled(rect, theme::RADIUS, theme::HOVER);
    }
    let text = if active || (response.hovered() && enabled) {
        theme::TEXT_STRONG
    } else if enabled {
        INACTIVE_TINT
    } else {
        theme::TEXT_MUTED
    };
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        "2D",
        theme::semibold(13.0),
        text,
    );
    let response = response.on_hover_text(
        "Slice Mode (Space): edit one slice of the model with any tool.          Space over a voxel slices at that voxel.",
    );
    if response.clicked() && enabled {
        response.surrender_focus();
        state.toggle_slice(None);
    }
    let (label, color) = match state.slice_counter() {
        Some((axis, n, count)) => (
            format!("{} {n}/{count}", ["X", "Y", "Z"][axis]),
            tool_input::axis_color(axis),
        ),
        None => ("–".to_owned(), theme::TEXT_MUTED),
    };
    ui.label(RichText::new(label).size(10.0).color(color));
}

/// Mask Mode button (`Shift+M`): protects the selection from every tool.
fn mask_toggle(ui: &mut Ui, state: &mut AppState) {
    let active = state.options.mask_selection;
    let size = Vec2::new(IconStyle::tool().size.x, 26.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let painter = ui.painter();
    if active {
        painter.rect_filled(rect, theme::RADIUS, theme::HOVER);
        let accent = Rect::from_min_max(rect.min, egui::pos2(rect.left() + 2.0, rect.bottom()));
        painter.rect_filled(accent, 0.0, theme::ACCENT_LINE);
    } else if response.hovered() {
        painter.rect_filled(rect, theme::RADIUS, theme::HOVER);
    }
    let color = if active || response.hovered() {
        theme::TEXT_STRONG
    } else {
        INACTIVE_TINT
    };
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        egui_phosphor::regular::SHIELD_CHECK,
        theme::icon(16.0),
        color,
    );
    let response = response.on_hover_text(
        "Mask Mode (Shift+M): protect the selected voxels. Every tool then edits only the rest of the layer.",
    );
    if response.clicked() {
        response.surrender_focus();
        state.toggle_mask_selection();
    }
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

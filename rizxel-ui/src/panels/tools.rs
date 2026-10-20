//! Left vertical toolbar.

use egui::{Color32, Rect, Response, Sense, Stroke, TextureHandle, Ui};
use rizxel_app::AppState;
use rizxel_engine::Tool;

use crate::icons::{self, Icon, Icons};
use crate::theme;
use crate::widgets::{horizontal_divider, IconStyle};

/// Drawn size of the pixel-art icons inside the 40 px buttons.
const ICON_SIZE: f32 = 26.0;
/// Inactive icons are slightly dimmed so the active/hovered one stands out.
const INACTIVE_TINT: Color32 = Color32::from_gray(185);

pub fn show(ui: &mut Ui, state: &mut AppState, icons: &Icons) {
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

        // Settings pinned to the bottom.
        let remaining = ui.available_height() - 48.0;
        if remaining > 0.0 {
            ui.add_space(remaining);
        }
        // Settings dialog arrives with preferences in a later phase.
        textured_button(ui, icons.get(Icon::Settings), false).on_hover_text("Settings");
    });
}

fn tool_button(ui: &mut Ui, state: &mut AppState, icons: &Icons, tool: Tool) {
    let active = state.tool == tool;
    if textured_button(ui, icons.get(tool), active)
        .on_hover_text(format!("{} ({})", tool.label(), tool.shortcut()))
        .clicked()
    {
        state.tool = tool;
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
        painter.rect_filled(accent, 0.0, theme::ACCENT);
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

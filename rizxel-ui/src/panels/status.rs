//! Bottom information bar.

use egui::{RichText, Stroke, Ui};
use egui_phosphor::regular as icon;
use rizxel_app::AppState;

use crate::theme;
use crate::tool_input::mask_mode;

pub fn show(ui: &mut Ui, state: &mut AppState) {
    let bar = ui.max_rect();
    ui.painter().hline(
        bar.x_range(),
        bar.top() + 0.5,
        Stroke::new(1.0, theme::BG_HEADER),
    );

    let small = |s: String| RichText::new(s).size(11.0).color(theme::TEXT);

    ui.horizontal_centered(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;

        let coords = match state.cursor_voxel {
            Some(v) => format!("X: {} Y: {} Z: {}", v.x, v.y, v.z),
            None => "X: - Y: - Z: -".to_owned(),
        };
        ui.label(
            RichText::new(icon::CROSSHAIR)
                .font(theme::icon(12.0))
                .color(theme::TEXT_DIM),
        );
        ui.label(small(coords));

        separator(ui);

        ui.label(
            RichText::new(icon::CUBE)
                .font(theme::icon(12.0))
                .color(theme::TEXT_DIM),
        );
        ui.label(small(format!(
            "Selected: {} voxels",
            state.selected_voxels()
        )));
        // Mask mode of the selection tools, following the held modifiers.
        let option = match state.tool {
            rizxel_engine::Tool::Selection => Some(state.options.selection.mask),
            rizxel_engine::Tool::FuzzySelect => Some(state.options.fuzzy_select.mask),
            _ => None,
        };
        if let Some(option) = option {
            let mode = mask_mode(ui.input(|i| i.modifiers), option);
            ui.label(
                RichText::new(format!("Mask: {}", mode.label()))
                    .size(11.0)
                    .color(theme::ACCENT_LIGHT),
            );
        }

        if let Some(doc) = state.doc() {
            separator(ui);
            ui.label(
                RichText::new(icon::CUBE_TRANSPARENT)
                    .font(theme::icon(12.0))
                    .color(theme::TEXT_DIM),
            );
            ui.label(small(format!("Voxels: {}", doc.visible_voxel_count())));

            separator(ui);
            let b = doc.data.bounds;
            ui.label(
                RichText::new(icon::BOUNDING_BOX)
                    .font(theme::icon(12.0))
                    .color(theme::TEXT_DIM),
            );
            ui.label(small(format!("Bounds: {}×{}×{}", b.x(), b.y(), b.z())));
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Transient messages (save/open results) take precedence over hints.
            // Transient messages first, then live tool feedback, then the hint.
            let (text, color) = match (&state.status_message, &state.tool_feedback) {
                (Some(msg), _) => (msg.clone(), theme::TEXT),
                (None, Some(fb)) => (fb.clone(), theme::ACCENT_LIGHT),
                (None, None) => (state.hint(), theme::BOUND),
            };
            ui.label(RichText::new(text).size(11.0).color(color));
            ui.label(
                RichText::new(icon::INFO)
                    .font(theme::icon(12.0))
                    .color(color),
            );
        });
    });

    // Clear the message once the user starts interacting with something else.
    if state.status_message.is_some() && ui.input(|i| i.pointer.any_pressed()) {
        state.status_message = None;
    }
}

fn separator(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(9.0, 12.0), egui::Sense::hover());
    ui.painter().vline(
        rect.center().x,
        rect.y_range(),
        Stroke::new(1.0, egui::Color32::from_white_alpha(51)),
    );
}

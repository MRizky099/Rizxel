//! History dock panel: the undo steps of the active document by name.
//! Clicking a step undoes or redoes to the state right after it.

use egui::{Sense, Ui};
use egui_phosphor::regular as icon;
use rizxel_app::AppState;

use crate::theme;

/// Height of one history row.
const ROW_HEIGHT: f32 = 22.0;

pub fn show(ui: &mut Ui, state: &mut AppState) {
    let Some(doc) = state.doc() else { return };
    let (entries, position) = doc.history_entries();
    let mut target = None;
    egui::Frame::NONE
        .inner_margin(egui::Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            // Row 0 is the state before the oldest step kept.
            let rows = std::iter::once("Start".to_owned()).chain(entries);
            for (row, label) in rows.enumerate() {
                if let Some(clicked) = history_row(ui, row, &label, position) {
                    target = Some(clicked);
                }
            }
        });
    if let Some(to) = target {
        if let Some(doc) = state.doc_mut() {
            doc.jump_to(to);
            state.status_message = Some(format!("History: {to} steps done"));
        }
    }
}

/// One row; returns `row` when clicked. Undone steps (after `position`)
/// are dimmed, the current state is highlighted.
fn history_row(ui: &mut Ui, row: usize, label: &str, position: usize) -> Option<usize> {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, ROW_HEIGHT), Sense::click());
    let current = row == position;
    if current {
        ui.painter()
            .rect_filled(rect, theme::RADIUS, theme::p().hover);
        let accent =
            egui::Rect::from_min_max(rect.min, egui::pos2(rect.left() + 2.0, rect.bottom()));
        ui.painter()
            .rect_filled(accent, 0.0, theme::p().accent_line);
    } else if response.hovered() {
        ui.painter()
            .rect_filled(rect, theme::RADIUS, theme::p().hover);
    }
    let color = if row > position {
        theme::p().text_dim
    } else if current {
        theme::p().text_strong
    } else {
        theme::p().text
    };
    let glyph = if row == 0 {
        icon::FILE
    } else {
        icon::ARROW_BEND_DOWN_RIGHT
    };
    // Painted, not labels: labels are selectable and would take the click.
    let painter = ui.painter();
    let mid = rect.left_center();
    painter.text(
        mid + egui::vec2(8.0, 0.0),
        egui::Align2::LEFT_CENTER,
        glyph,
        theme::icon(12.0),
        color,
    );
    painter.text(
        mid + egui::vec2(26.0, 0.0),
        egui::Align2::LEFT_CENTER,
        label,
        theme::regular(12.0),
        color,
    );
    let response = response.on_hover_text(if row > position {
        "Undone: click to redo up to here"
    } else {
        "Click to go back to this state"
    });
    response.clicked().then_some(row)
}

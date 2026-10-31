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
        if state.options.mask_selection {
            ui.label(
                RichText::new(icon::SHIELD_CHECK)
                    .font(theme::icon(12.0))
                    .color(theme::ACCENT_LINE),
            );
            ui.label(
                RichText::new("Mask Mode")
                    .size(11.0)
                    .color(theme::ACCENT_LINE),
            );
        }
        if let Some(option) = option {
            let mode = mask_mode(ui.input(|i| i.modifiers), option);
            ui.label(
                RichText::new(format!("Mask: {}", mode.label()))
                    .size(11.0)
                    .color(theme::ACCENT_LINE),
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
            // A hint is neither a warning nor live state: it stays neutral, with
            // its key names drawn as caps.
            let (text, color, icon_color) = match (&state.status_message, &state.tool_feedback) {
                (Some(msg), _) => (msg.clone(), theme::TEXT_STRONG, theme::TEXT_DIM),
                (None, Some(fb)) => (fb.clone(), theme::ACCENT_LINE, theme::ACCENT_LINE),
                (None, None) => (state.hint(), theme::TEXT, theme::TEXT_DIM),
            };
            ui.label(hint_job(&text, color));
            ui.label(
                RichText::new(icon::INFO)
                    .font(theme::icon(12.0))
                    .color(icon_color),
            );
        });
    });

    // Clear the message once the user starts interacting with something else.
    if state.status_message.is_some() && ui.input(|i| i.pointer.any_pressed()) {
        state.status_message = None;
    }
}

/// Key names a hint may start a word with (`Ctrl+click`, `Shift/Ctrl:`).
const KEYS: [&str; 6] = ["Ctrl", "Shift", "Alt", "Esc", "Enter", "Space"];

/// Splits `text` into plain runs and key names (`true`). A key name is one
/// of [`KEYS`] or `F1`-`F12`, at the start of a word or after `+` or `/`,
/// and followed by `+`, `/`, `:`, `,`, `.`, `)`, a space or the end.
fn hint_spans(text: &str) -> Vec<(bool, &str)> {
    let mut spans = Vec::new();
    let mut plain = 0;
    let mut i = 0;
    while i < text.len() {
        let at_boundary = i == 0
            || text[..i]
                .chars()
                .next_back()
                .is_some_and(|c| c == ' ' || c == '+' || c == '/' || c == '(');
        let key_len = at_boundary.then(|| key_at(&text[i..])).flatten();
        match key_len {
            Some(len) => {
                if plain < i {
                    spans.push((false, &text[plain..i]));
                }
                spans.push((true, &text[i..i + len]));
                i += len;
                plain = i;
            }
            None => i += text[i..].chars().next().map_or(1, char::len_utf8),
        }
    }
    if plain < text.len() {
        spans.push((false, &text[plain..]));
    }
    spans
}

/// Length of the key name `s` starts with, if any.
fn key_at(s: &str) -> Option<usize> {
    let function_key = s.strip_prefix('F').and_then(|rest| {
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        let n: u32 = rest[..digits].parse().ok()?;
        (1..=12).contains(&n).then_some(1 + digits)
    });
    let len = function_key.or_else(|| KEYS.iter().find(|k| s.starts_with(**k)).map(|k| k.len()))?;
    let ends_word = s[len..]
        .chars()
        .next()
        .is_none_or(|c| matches!(c, '+' | '/' | ':' | ',' | '.' | ')' | ' '));
    ends_word.then_some(len)
}

/// The status text with each key name on a raised cap.
fn hint_job(text: &str, color: egui::Color32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    for (is_key, run) in hint_spans(text) {
        if is_key {
            let format = egui::TextFormat {
                font_id: theme::regular(11.0),
                color: theme::TEXT_STRONG,
                background: theme::BG_BUTTON,
                ..Default::default()
            };
            job.append(&format!(" {run} "), 0.0, format);
        } else {
            job.append(
                run,
                0.0,
                egui::TextFormat::simple(theme::regular(11.0), color),
            );
        }
    }
    job
}

fn separator(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(9.0, 12.0), egui::Sense::hover());
    ui.painter().vline(
        rect.center().x,
        rect.y_range(),
        Stroke::new(1.0, egui::Color32::from_white_alpha(51)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(text: &str) -> Vec<&str> {
        hint_spans(text)
            .into_iter()
            .filter_map(|(key, run)| key.then_some(run))
            .collect()
    }

    #[test]
    fn hint_keys_become_caps() {
        let hint =
            "Drag to paint. Ctrl+click: line from the last click. Shift: erase, Alt: pick color.";
        assert_eq!(keys(hint), ["Ctrl", "Shift", "Alt"]);
        assert_eq!(
            keys("Shift/Ctrl: finer steps. Enter: apply."),
            ["Shift", "Ctrl", "Enter"]
        );
        assert_eq!(
            keys("Alt+click picks a layer. F5: Model mode"),
            ["Alt", "F5"]
        );
    }

    #[test]
    fn hint_spans_keep_every_character() {
        let hint = "Shift: carve, Esc: cancel.";
        let joined: String = hint_spans(hint).into_iter().map(|(_, run)| run).collect();
        assert_eq!(joined, hint);
    }

    #[test]
    fn words_that_only_start_like_keys_stay_plain() {
        assert!(keys("Altitude, Shifted, Escape, Opened F:/models/a.rzxl, F13").is_empty());
    }
}

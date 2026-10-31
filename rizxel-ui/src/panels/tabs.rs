//! Document tabs.

use egui::{Align, Align2, Id, Rect, Sense, Stroke, Ui};
use egui_phosphor::regular as icon;
use rizxel_app::AppState;

use crate::theme;
use crate::widgets::{hscroll_strip, icon_button, IconStyle};

const TAB_MIN_WIDTH: f32 = 120.0;
const TAB_MAX_WIDTH: f32 = 220.0;
const NEW_BUTTON_WIDTH: f32 = 36.0;

enum TabAction {
    Activate(usize),
    Close(usize),
}

pub fn show(ui: &mut Ui, state: &mut AppState) {
    let height = ui.max_rect().height();
    let mut action = None;

    // Bottom border of the whole bar.
    let bar = ui.max_rect();
    ui.painter().hline(
        bar.x_range(),
        bar.bottom() - 0.5,
        Stroke::new(1.0, theme::BG_PANEL),
    );

    // `(active_doc, doc_count)` last frame; a change scrolls the active tab into view.
    let seen_id = Id::new("doc_tabs_seen");
    let seen = (state.active_doc, state.documents.len());
    let reveal_active = ui.data(|d| d.get_temp::<(usize, usize)>(seen_id)) != Some(seen);
    ui.data_mut(|d| d.insert_temp(seen_id, seen));

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        hscroll_strip(
            ui,
            "doc_tabs",
            height,
            NEW_BUTTON_WIDTH,
            theme::BG_TAB_HOVER,
            |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                for (i, doc) in state.documents.iter().enumerate() {
                    let active = i == state.active_doc;
                    // `name*.rzxl` while unsaved.
                    let title = doc.tab_title();
                    if let Some(a) = tab(
                        ui,
                        i,
                        &title,
                        doc.is_dirty(),
                        active,
                        active && reveal_active,
                        height,
                    ) {
                        action = Some(a);
                    }
                }
            },
        );

        // Pinned outside the scroll area so it is always reachable.
        let new_style = IconStyle::small()
            .size(egui::vec2(NEW_BUTTON_WIDTH, height))
            .hover_fill(theme::BG_TAB_HOVER);
        if icon_button(ui, icon::PLUS, new_style, true)
            .on_hover_text("New project (Ctrl+N)")
            .clicked()
        {
            state.new_document();
        }
    });

    match action {
        Some(TabAction::Activate(i)) => state.activate(i),
        // Unsaved documents ask first (the prompt is drawn by the app).
        Some(TabAction::Close(i)) => {
            state.close_document_checked(i);
        }
        None => {}
    }
}

fn tab(
    ui: &mut Ui,
    index: usize,
    title: &str,
    dirty: bool,
    active: bool,
    reveal: bool,
    height: f32,
) -> Option<TabAction> {
    let font = theme::regular(13.0);
    let text_width = ui
        .painter()
        .layout_no_wrap(title.to_owned(), font.clone(), theme::TEXT)
        .size()
        .x;
    // Padding (16) + icon (18) + text + gap (12) + close (14) + padding (16).
    let width = (text_width + 76.0).clamp(TAB_MIN_WIDTH, TAB_MAX_WIDTH);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), Sense::click());
    if reveal {
        response.scroll_to_me(Some(Align::Center));
    }

    let close_rect = Rect::from_center_size(
        egui::pos2(rect.right() - 20.0, rect.center().y),
        egui::vec2(18.0, 18.0),
    );
    let close = ui.interact(
        close_rect,
        ui.id().with(("tab_close", index)),
        Sense::click(),
    );

    let hovered = response.hovered() || close.hovered();
    let painter = ui.painter();
    if active {
        painter.rect_filled(rect, 0.0, theme::BG_PANEL);
        painter.hline(
            rect.x_range(),
            rect.top() + 1.0,
            Stroke::new(2.0, theme::ACCENT_LINE),
        );
    } else {
        if hovered {
            painter.rect_filled(rect, 0.0, theme::BG_TAB_HOVER);
        }
        painter.vline(
            rect.right() - 0.5,
            rect.y_range(),
            Stroke::new(1.0, theme::BORDER),
        );
    }

    let text_color = if active {
        theme::TEXT_STRONG
    } else {
        theme::TEXT_DIM
    };
    let logo_rect = Rect::from_center_size(
        rect.left_center() + egui::vec2(23.0, 0.0),
        egui::Vec2::splat(14.0),
    );
    crate::icons::paint_logo(ui, logo_rect, 14.0, active || hovered);

    // Title, clipped before the close button.
    let text_clip =
        Rect::from_min_max(rect.min, egui::pos2(close_rect.left() - 4.0, rect.bottom()));
    painter.with_clip_rect(text_clip).text(
        rect.left_center() + egui::vec2(34.0, 0.0),
        Align2::LEFT_CENTER,
        title,
        font,
        text_color,
    );

    // Dirty documents show a dot that turns into × on hover (VS Code style).
    let (glyph, color) = match (dirty, close.hovered() || hovered) {
        (true, false) => (icon::CIRCLE, theme::TEXT_DIM),
        _ if close.hovered() => (icon::X, theme::TEXT_STRONG),
        _ if active || hovered => (icon::X, theme::TEXT_DIM),
        _ => ("", theme::TEXT_DIM),
    };
    let glyph_size = if glyph == icon::CIRCLE { 8.0 } else { 11.0 };
    painter.text(
        close_rect.center(),
        Align2::CENTER_CENTER,
        glyph,
        theme::icon(glyph_size),
        color,
    );

    if close.clicked() || response.middle_clicked() {
        Some(TabAction::Close(index))
    } else if response.clicked() {
        Some(TabAction::Activate(index))
    } else {
        None
    }
}

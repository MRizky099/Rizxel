//! Small custom widgets shared by several panels.

use egui::{Color32, Rect, Response, Sense, Stroke, Ui, Vec2};

use crate::theme;

/// Visual style of an [`icon_button`].
#[derive(Clone, Copy)]
pub struct IconStyle {
    pub size: Vec2,
    pub icon_size: f32,
    pub color: Color32,
    pub hover_color: Color32,
    pub hover_fill: Color32,
}

impl IconStyle {
    /// 32x32 option-bar button.
    pub fn bar() -> Self {
        Self {
            size: Vec2::splat(32.0),
            icon_size: 15.0,
            color: theme::p().text,
            hover_color: theme::p().text_strong,
            hover_fill: theme::p().hover,
        }
    }

    /// 40x40 vertical-toolbar button.
    pub fn tool() -> Self {
        Self {
            size: Vec2::splat(40.0),
            icon_size: 17.0,
            color: theme::p().text_dim,
            ..Self::bar()
        }
    }

    /// Small inline icon (panel headers, layer rows).
    pub fn small() -> Self {
        Self {
            size: Vec2::splat(18.0),
            icon_size: 13.0,
            color: theme::p().text_dim,
            hover_color: theme::p().text_strong,
            hover_fill: Color32::TRANSPARENT,
        }
    }

    pub fn color(mut self, color: Color32) -> Self {
        self.color = color;
        self
    }

    pub fn hover_color(mut self, color: Color32) -> Self {
        self.hover_color = color;
        self
    }

    pub fn hover_fill(mut self, fill: Color32) -> Self {
        self.hover_fill = fill;
        self
    }

    pub fn size(mut self, size: Vec2) -> Self {
        self.size = size;
        self
    }
}

/// Frameless icon button with a hover background. Disabled buttons are dimmed
/// and do not react to clicks.
pub fn icon_button(ui: &mut Ui, icon: &str, style: IconStyle, enabled: bool) -> Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(style.size, sense);

    if ui.is_rect_visible(rect) {
        let hovered = enabled && response.hovered();
        if hovered && style.hover_fill != Color32::TRANSPARENT {
            ui.painter()
                .rect_filled(rect, theme::RADIUS, style.hover_fill);
        }
        let color = match (enabled, hovered) {
            (false, _) => style.color.gamma_multiply(0.35),
            (true, true) => style.hover_color,
            (true, false) => style.color,
        };
        paint_icon(ui, rect, icon, style.icon_size, color);
    }
    response
}

/// Checkbox in the redesign's style: a 14 px box, filled with the accent
/// under a light tick when on, a recessed well when off. Same use as
/// [`Ui::checkbox`].
pub fn check_box(ui: &mut Ui, checked: &mut bool, label: &str) -> Response {
    const BOX: f32 = 14.0;
    const GAP: f32 = 6.0;
    let galley = egui::WidgetText::from(label).into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        egui::TextStyle::Body,
    );
    let height = ui.spacing().interact_size.y.max(galley.size().y);
    let width = BOX
        + if label.is_empty() {
            0.0
        } else {
            GAP + galley.size().x
        };
    let (rect, mut response) = ui.allocate_exact_size(egui::vec2(width, height), Sense::click());
    if response.clicked() {
        *checked = !*checked;
        response.mark_changed();
    }
    let on = *checked;
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), on, label)
    });
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let enabled = ui.is_enabled();
    let dim = |c: Color32| if enabled { c } else { c.gamma_multiply(0.45) };
    let box_rect = Rect::from_center_size(
        egui::pos2(rect.left() + BOX / 2.0, rect.center().y),
        Vec2::splat(BOX),
    );
    let painter = ui.painter();
    let radius = 3.0;
    if on {
        painter.rect_filled(box_rect, radius, dim(theme::ACCENT));
        let tick = [
            box_rect.left_center() + egui::vec2(3.2, 0.2),
            box_rect.center_bottom() + egui::vec2(-1.2, -3.6),
            box_rect.right_top() + egui::vec2(-3.0, 3.6),
        ];
        painter.add(egui::Shape::line(
            tick.to_vec(),
            Stroke::new(1.8, dim(theme::ACCENT_INK)),
        ));
    } else {
        let edge = if enabled && response.hovered() {
            theme::p().text_dim
        } else {
            theme::p().separator
        };
        painter.rect_filled(box_rect, radius, theme::p().bg_app);
        painter.rect_stroke(
            box_rect,
            radius,
            Stroke::new(1.0, dim(edge)),
            egui::StrokeKind::Inside,
        );
    }
    if !label.is_empty() {
        let text_color = if enabled && response.hovered() {
            theme::p().text_strong
        } else {
            dim(theme::p().text)
        };
        let pos = egui::pos2(
            box_rect.right() + GAP,
            rect.center().y - galley.size().y / 2.0,
        );
        painter.galley(pos, galley, text_color);
    }
    response
}

/// Drop-down in the redesign's style: the field shows a chevron, and in its
/// list the current item sits on a grey highlight rather than the accent.
/// Use in place of [`egui::ComboBox::from_id_salt`].
pub fn dropdown(id_salt: impl std::hash::Hash + std::fmt::Debug) -> egui::ComboBox {
    egui::ComboBox::from_id_salt(id_salt)
        .icon(|ui, rect, visuals, _open| {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                egui_phosphor::regular::CARET_DOWN,
                theme::icon(12.0),
                visuals.fg_stroke.color,
            );
        })
        .popup_style(egui::style::StyleModifier::new(
            |style: &mut egui::Style| {
                let c = theme::p();
                style.visuals.selection.bg_fill = c.bg_selected;
                style.visuals.selection.stroke = Stroke::new(1.0, c.text_strong);
                style.spacing.button_padding = egui::vec2(8.0, 4.0);
            },
        ))
}

/// Paints a centered icon glyph inside `rect`.
/// `glyph` (in the icon font) followed by `label`, for buttons and menus.
/// Uses the widget's own text color, so hover/disabled styling still applies.
pub fn icon_label(ui: &Ui, glyph: &str, label: &str, size: Option<f32>) -> egui::text::LayoutJob {
    let size = size.unwrap_or_else(|| egui::TextStyle::Button.resolve(ui.style()).size);
    let mut job = egui::text::LayoutJob::default();
    let format = |font_id| egui::TextFormat {
        font_id,
        color: Color32::PLACEHOLDER,
        valign: egui::Align::Center,
        ..Default::default()
    };
    job.append(glyph, 0.0, format(theme::icon(size)));
    job.append(label, size * 0.6, format(theme::regular(size)));
    job
}

pub fn paint_icon(ui: &Ui, rect: Rect, icon: &str, size: f32, color: Color32) {
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        icon,
        theme::icon(size),
        color,
    );
}

/// Thin vertical divider used in horizontal bars.
pub fn vertical_divider(ui: &mut Ui, height: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(17.0, height), Sense::hover());
    ui.painter().vline(
        rect.center().x,
        rect.y_range(),
        Stroke::new(1.0, theme::p().separator),
    );
}

/// Horizontal divider used in vertical bars.
pub fn horizontal_divider(ui: &mut Ui, width: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 9.0), Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        Stroke::new(1.0, theme::p().separator),
    );
}

/// Scroll bookkeeping for [`hscroll_strip`], carried across frames in egui's
/// temp memory.
#[derive(Clone, Copy, Default)]
struct StripMemo {
    offset: f32,
    max_offset: f32,
    /// Width of the content last frame.
    content_width: f32,
    /// Offset requested by a chevron click, applied on the next frame.
    pending: Option<f32>,
}

/// Width of the chevron buttons shown when a strip overflows.
const STRIP_ARROW_WIDTH: f32 = 24.0;
/// Distance scrolled by one chevron click.
const STRIP_ARROW_STEP: f32 = 160.0;

/// Horizontal strip that scrolls when its content is wider than the space
/// left after `reserved_right` px. Scrolls with the mouse wheel; left/right
/// chevrons appear only while the content overflows.
pub fn hscroll_strip<R>(
    ui: &mut Ui,
    id_salt: &str,
    height: f32,
    reserved_right: f32,
    arrow_fill: Color32,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> R {
    let memo_id = ui.id().with(("hscroll_strip", id_salt));
    let mut memo: StripMemo = ui.data(|d| d.get_temp(memo_id)).unwrap_or_default();
    // Measured against the whole width, not the one left beside the
    // arrows: otherwise the arrows' own width keeps the strip overflowing
    // after the content shrinks back (closed tabs).
    let overflowing = memo.content_width > ui.available_width() - reserved_right + 0.5;
    let arrow_style = IconStyle::small()
        .size(egui::vec2(STRIP_ARROW_WIDTH, height))
        .hover_fill(arrow_fill);

    let saved_spacing = ui.spacing().item_spacing.x;
    ui.spacing_mut().item_spacing.x = 0.0;

    if overflowing
        && icon_button(
            ui,
            egui_phosphor::regular::CARET_LEFT,
            arrow_style,
            memo.offset > 0.5,
        )
        .on_hover_text("Scroll left")
        .clicked()
    {
        memo.pending = Some((memo.offset - STRIP_ARROW_STEP).max(0.0));
    }

    let reserved = reserved_right + if overflowing { STRIP_ARROW_WIDTH } else { 0.0 };
    let width = (ui.available_width() - reserved).max(0.0);

    let inner = ui
        .allocate_ui(egui::vec2(width, height), |ui| {
            // The vertical wheel scrolls this horizontal-only area.
            ui.style_mut().always_scroll_the_only_direction = true;
            let mut area = egui::ScrollArea::horizontal()
                .id_salt(id_salt)
                .max_width(width)
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden);
            match memo.pending.take() {
                Some(offset) => area = area.horizontal_scroll_offset(offset),
                // Everything fits again: back to the start.
                None if !overflowing && memo.offset > 0.0 => {
                    area = area.horizontal_scroll_offset(0.0);
                }
                None => {}
            }
            let output = area.show(ui, |ui| {
                ui.spacing_mut().item_spacing.x = saved_spacing;
                ui.set_min_height(height);
                ui.horizontal_centered(add_contents).inner
            });
            memo.offset = output.state.offset.x;
            memo.max_offset = (output.content_size.x - output.inner_rect.width()).max(0.0);
            memo.content_width = output.content_size.x;
            output.inner
        })
        .inner;

    if overflowing
        && icon_button(
            ui,
            egui_phosphor::regular::CARET_RIGHT,
            arrow_style,
            memo.offset < memo.max_offset - 0.5,
        )
        .on_hover_text("Scroll right")
        .clicked()
    {
        memo.pending = Some((memo.offset + STRIP_ARROW_STEP).min(memo.max_offset));
    }
    ui.spacing_mut().item_spacing.x = saved_spacing;

    if memo.pending.is_some() {
        ui.ctx().request_repaint();
    }
    ui.data_mut(|d| d.insert_temp(memo_id, memo));
    inner
}

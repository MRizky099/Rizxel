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
            color: theme::TEXT,
            hover_color: theme::TEXT_STRONG,
            hover_fill: theme::HOVER,
        }
    }

    /// 40x40 vertical-toolbar button.
    pub fn tool() -> Self {
        Self {
            size: Vec2::splat(40.0),
            icon_size: 17.0,
            color: theme::TEXT_DIM,
            ..Self::bar()
        }
    }

    /// Small inline icon (panel headers, layer rows).
    pub fn small() -> Self {
        Self {
            size: Vec2::splat(18.0),
            icon_size: 13.0,
            color: theme::TEXT_DIM,
            hover_color: theme::TEXT_STRONG,
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

/// Paints a centered icon glyph inside `rect`.
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
        Stroke::new(1.0, theme::SEPARATOR),
    );
}

/// Horizontal divider used in vertical bars.
pub fn horizontal_divider(ui: &mut Ui, width: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 9.0), Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        Stroke::new(1.0, theme::SEPARATOR),
    );
}

/// Panel header row (`COLOR PALETTE`, `LAYERS`, ...) with optional bottom accent.
pub fn panel_header(
    ui: &mut Ui,
    title: &str,
    icon: Option<&str>,
    text_color: Color32,
    border: Color32,
    add_right: impl FnOnce(&mut Ui),
) {
    let height = 32.0;
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), height), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 0.0, theme::BG_HEADER);
    painter.hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        Stroke::new(1.0, border),
    );

    let label = match icon {
        Some(icon) => format!("{icon}  {title}"),
        None => title.to_owned(),
    };
    painter.text(
        rect.left_center() + egui::vec2(12.0, 0.0),
        egui::Align2::LEFT_CENTER,
        label,
        theme::semibold(11.0),
        text_color,
    );

    let right = Rect::from_min_max(
        egui::pos2(rect.right() - 80.0, rect.top()),
        egui::pos2(rect.right() - 8.0, rect.bottom()),
    );
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(right)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
        add_right,
    );
}

/// Scroll bookkeeping for [`hscroll_strip`], carried across frames in egui's
/// temp memory.
#[derive(Clone, Copy, Default)]
struct StripMemo {
    offset: f32,
    max_offset: f32,
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
    let overflowing = memo.max_offset > 0.5;
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
            if let Some(offset) = memo.pending.take() {
                area = area.horizontal_scroll_offset(offset);
            }
            let output = area.show(ui, |ui| {
                ui.spacing_mut().item_spacing.x = saved_spacing;
                ui.set_min_height(height);
                ui.horizontal_centered(add_contents).inner
            });
            memo.offset = output.state.offset.x;
            memo.max_offset = (output.content_size.x - output.inner_rect.width()).max(0.0);
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

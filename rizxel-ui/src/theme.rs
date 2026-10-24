//! Dark theme, color tokens and font setup mirroring `docs/rizxel_ui.html`.

use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle,
};
use std::sync::Arc;

// Background layers (darkest -> lightest).
pub const BG_APP: Color32 = Color32::from_rgb(0x1e, 0x1e, 0x1e);
pub const BG_PANEL: Color32 = Color32::from_rgb(0x25, 0x25, 0x26);
pub const BG_HEADER: Color32 = Color32::from_rgb(0x2d, 0x2d, 0x30);
pub const BG_BUTTON: Color32 = Color32::from_rgb(0x33, 0x33, 0x37);
pub const BG_SELECTED: Color32 = Color32::from_rgb(0x37, 0x37, 0x3d);
pub const BG_TAB_HOVER: Color32 = Color32::from_rgb(0x2a, 0x2a, 0x2d);
pub const HOVER: Color32 = Color32::from_rgb(0x3f, 0x3f, 0x46);
pub const BORDER: Color32 = Color32::from_rgb(0x3c, 0x3c, 0x3c);
pub const SEPARATOR: Color32 = Color32::from_rgb(0x44, 0x44, 0x44);
pub const WORKSPACE: Color32 = Color32::from_rgb(0x4b, 0x4b, 0x4b);
pub const GRID_LINE: Color32 = Color32::from_rgba_unmultiplied_const(0xae, 0xae, 0xae, 102);

// Text.
pub const TEXT: Color32 = Color32::from_rgb(0xcc, 0xcc, 0xcc);
pub const TEXT_DIM: Color32 = Color32::from_rgb(0x99, 0x99, 0x99);
pub const TEXT_MUTED: Color32 = Color32::from_rgb(0x88, 0x88, 0x88);
pub const TEXT_STRONG: Color32 = Color32::WHITE;

// Accents.
pub const ACCENT: Color32 = Color32::from_rgb(0x00, 0x7a, 0xcc);
pub const ACCENT_LIGHT: Color32 = Color32::from_rgb(0x38, 0xbd, 0xf8);
pub const BOUND: Color32 = Color32::from_rgb(0xfb, 0xbf, 0x24);
pub const PAINT: Color32 = Color32::from_rgb(0xea, 0xb3, 0x08);
pub const CLOSE_HOVER: Color32 = Color32::from_rgb(0xe8, 0x11, 0x23);
pub const DANGER: Color32 = Color32::from_rgb(0xf8, 0x71, 0x71);
/// Box of the Selection tool's Box mode (distinct from the yellow model
/// bounds and the light-blue selection outline).
pub const SELECTION_BOX: Color32 = Color32::from_rgb(0x5b, 0x6c, 0xff);

// Axis colors.
pub const AXIS_X: Color32 = Color32::from_rgb(0xff, 0x47, 0x57);
pub const AXIS_Y: Color32 = Color32::from_rgb(0x2e, 0xd5, 0x73);
pub const AXIS_Z: Color32 = Color32::from_rgb(0x1e, 0x90, 0xff);

// Layout metrics (px).
pub const TITLE_BAR_HEIGHT: f32 = 32.0;
pub const OPTION_BAR_HEIGHT: f32 = 48.0;
pub const TAB_BAR_HEIGHT: f32 = 36.0;
pub const STATUS_BAR_HEIGHT: f32 = 24.0;
pub const RADIUS: CornerRadius = CornerRadius::same(4);

/// Font family for the SemiBold weight (panel headers, active labels).
pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("inter-semibold".into()))
}

pub fn regular(size: f32) -> FontId {
    FontId::proportional(size)
}

/// Phosphor icon font at `size`. Icons use the Phosphor family directly:
/// Inter defines glyphs in the same Private Use Area and would otherwise
/// shadow some icons in the proportional fallback chain.
pub fn icon(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("phosphor".into()))
}

/// Registers Inter (Regular + SemiBold) and Phosphor icons.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();

    fonts.font_data.insert(
        "inter".into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../assets/fonts/Inter-Regular.ttf"
        ))),
    );
    fonts.font_data.insert(
        "inter-semibold".into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../assets/fonts/Inter-SemiBold.ttf"
        ))),
    );

    // Inter first so it overrides egui's default proportional font.
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "inter".into());

    // SemiBold family falls back to the full proportional chain (icons, emoji).
    let mut semibold_chain = vec!["inter-semibold".to_owned()];
    semibold_chain.extend(
        fonts
            .families
            .get(&FontFamily::Proportional)
            .cloned()
            .unwrap_or_default(),
    );
    fonts
        .families
        .insert(FontFamily::Name("inter-semibold".into()), semibold_chain);

    // Phosphor is inserted as a proportional fallback (after Inter) and also
    // appended to the semibold chain so icons render in headers too.
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    // The "phosphor" family [`icon`] draws with holds Phosphor alone: Inter has
    // its own glyphs at many of the same Private Use Area codepoints, and
    // would shadow icons like the eye or arrows if it came first.
    fonts
        .families
        .insert(FontFamily::Name("phosphor".into()), vec!["phosphor".into()]);
    if let Some(chain) = fonts
        .families
        .get_mut(&FontFamily::Name("inter-semibold".into()))
    {
        chain.push("phosphor".into());
    }

    ctx.set_fonts(fonts);
}

/// Applies the Rizxel dark visuals and spacing.
pub fn apply(ctx: &egui::Context) {
    ctx.set_visuals(egui::Visuals::dark());
    ctx.global_style_mut(|style| {
        style.text_styles = [
            (TextStyle::Small, regular(11.0)),
            (TextStyle::Body, regular(13.0)),
            (TextStyle::Button, regular(13.0)),
            (TextStyle::Monospace, FontId::monospace(12.0)),
            (TextStyle::Heading, semibold(16.0)),
        ]
        .into();

        style.spacing.item_spacing = egui::vec2(6.0, 4.0);
        style.spacing.button_padding = egui::vec2(8.0, 3.0);
        style.spacing.interact_size.y = 22.0;
        style.spacing.menu_margin = egui::Margin::same(4);

        let v = &mut style.visuals;
        v.dark_mode = true;
        v.override_text_color = None;
        v.panel_fill = BG_PANEL;
        v.window_fill = BG_HEADER;
        v.extreme_bg_color = BG_APP;
        v.faint_bg_color = BG_HEADER;
        v.window_stroke = Stroke::new(1.0, BORDER);
        v.window_corner_radius = RADIUS;
        v.menu_corner_radius = RADIUS;
        v.selection.bg_fill = ACCENT;
        v.selection.stroke = Stroke::new(1.0, TEXT_STRONG);
        v.hyperlink_color = ACCENT_LIGHT;

        let w = &mut v.widgets;
        w.noninteractive.bg_fill = BG_PANEL;
        w.noninteractive.weak_bg_fill = BG_PANEL;
        w.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
        w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);

        w.inactive.bg_fill = BG_APP;
        w.inactive.weak_bg_fill = Color32::TRANSPARENT;
        w.inactive.bg_stroke = Stroke::new(1.0, BORDER);
        w.inactive.fg_stroke = Stroke::new(1.0, TEXT);
        w.inactive.corner_radius = RADIUS;

        w.hovered.bg_fill = HOVER;
        w.hovered.weak_bg_fill = HOVER;
        w.hovered.bg_stroke = Stroke::new(1.0, BORDER);
        w.hovered.fg_stroke = Stroke::new(1.0, TEXT_STRONG);
        w.hovered.corner_radius = RADIUS;
        w.hovered.expansion = 0.0;

        w.active.bg_fill = HOVER;
        w.active.weak_bg_fill = HOVER;
        w.active.bg_stroke = Stroke::new(1.0, ACCENT);
        w.active.fg_stroke = Stroke::new(1.0, TEXT_STRONG);
        w.active.corner_radius = RADIUS;
        w.active.expansion = 0.0;

        w.open.bg_fill = HOVER;
        w.open.weak_bg_fill = HOVER;
        w.open.bg_stroke = Stroke::new(1.0, BORDER);
        w.open.fg_stroke = Stroke::new(1.0, TEXT_STRONG);
        w.open.corner_radius = RADIUS;
    });
}

/// Converts an engine color into an egui color.
pub fn to_color32(c: rizxel_math::Rgba8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a)
}

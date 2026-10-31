//! Dark theme, color tokens and font setup mirroring `docs/rizxel_ui_redesign.html`.
//!
//! The colors are OKLCH tokens converted to sRGB: neutrals sit on hue 280
//! at chroma 0.012 to 0.015, close enough to grey that palette colors read
//! true, and the one accent is the ultramarine of the Rizxel icon.

use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle,
};
use std::sync::Arc;

// Background layers. BG_APP is the darkest (input wells, the window clear
// color); panels sit above the header strips, popups above the panels.
pub const BG_APP: Color32 = Color32::from_rgb(0x0f, 0x10, 0x15); // oklch(17.5% 0.011 280)
pub const BG_HEADER: Color32 = Color32::from_rgb(0x16, 0x17, 0x1d); // oklch(20.5% 0.012 280)
pub const BG_TAB_HOVER: Color32 = Color32::from_rgb(0x1a, 0x1b, 0x21); // oklch(22.25% 0.0125 280)
pub const BG_PANEL: Color32 = Color32::from_rgb(0x1e, 0x1f, 0x25); // oklch(24% 0.013 280)
pub const BG_BUTTON: Color32 = Color32::from_rgb(0x29, 0x29, 0x31); // oklch(28.5% 0.014 280)
pub const BG_SELECTED: Color32 = Color32::from_rgb(0x2f, 0x30, 0x38); // oklch(31% 0.015 280)
pub const HOVER: Color32 = Color32::from_rgb(0x34, 0x35, 0x3d); // oklch(33% 0.015 280)
pub const BORDER: Color32 = Color32::from_rgb(0x31, 0x32, 0x3b); // oklch(32% 0.015 280)
pub const SEPARATOR: Color32 = Color32::from_rgb(0x48, 0x49, 0x54); // oklch(41% 0.018 280)
/// Viewport ground: a mid grey, the neutral reference colors are judged on.
pub const WORKSPACE: Color32 = Color32::from_rgb(0x4a, 0x4a, 0x4e); // oklch(41% 0.006 280)
pub const GRID_LINE: Color32 = Color32::from_rgba_unmultiplied_const(0xae, 0xae, 0xae, 102);

// Text. Every pair stays at 4.5:1 or better on BG_PANEL and BG_HEADER.
pub const TEXT: Color32 = Color32::from_rgb(0xc9, 0xca, 0xd2); // oklch(84% 0.011 280)
pub const TEXT_DIM: Color32 = Color32::from_rgb(0x9f, 0xa1, 0xab); // oklch(71% 0.015 280)
pub const TEXT_MUTED: Color32 = Color32::from_rgb(0x90, 0x91, 0x9c); // oklch(66% 0.015 280)
pub const TEXT_STRONG: Color32 = Color32::from_rgb(0xea, 0xeb, 0xf1); // oklch(94% 0.008 280)

// Accents.
/// Brand ultramarine, for fills: text on it is TEXT_STRONG.
pub const ACCENT: Color32 = Color32::from_rgb(0x29, 0x41, 0xd9); // oklch(47% 0.23 268)
/// The accent as a line or text on the dark panels (outlines, underlines,
/// the active-tool bar).
pub const ACCENT_LINE: Color32 = Color32::from_rgb(0x86, 0x9e, 0xfc); // oklch(72% 0.14 272)
/// Viewport selection outline and tool previews (not used for chrome).
pub const ACCENT_LIGHT: Color32 = Color32::from_rgb(0x38, 0xbd, 0xf8);
pub const BOUND: Color32 = Color32::from_rgb(0xfb, 0xbf, 0x24);
pub const PAINT: Color32 = Color32::from_rgb(0xea, 0xb3, 0x08);
pub const CLOSE_HOVER: Color32 = Color32::from_rgb(0xe8, 0x11, 0x23);
pub const DANGER: Color32 = Color32::from_rgb(0xf9, 0x77, 0x70); // oklch(72% 0.16 25)
/// Box of the Selection tool's Box mode (distinct from the yellow model
/// bounds and the light-blue selection outline).
pub const SELECTION_BOX: Color32 = Color32::from_rgb(0x5b, 0x6c, 0xff);

// Axis colors.
pub const AXIS_X: Color32 = Color32::from_rgb(0xff, 0x47, 0x57);
pub const AXIS_Y: Color32 = Color32::from_rgb(0x2e, 0xd5, 0x73);
pub const AXIS_Z: Color32 = Color32::from_rgb(0x1e, 0x90, 0xff);

// Layout metrics (px).
/// Tall enough for the document tabs it holds.
pub const TITLE_BAR_HEIGHT: f32 = 36.0;
pub const OPTION_BAR_HEIGHT: f32 = 48.0;
pub const STATUS_BAR_HEIGHT: f32 = 24.0;
pub const RADIUS: CornerRadius = CornerRadius::same(4);

/// Font family for the SemiBold weight (panel headers, active labels).
pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("geist-semibold".into()))
}

pub fn regular(size: f32) -> FontId {
    FontId::proportional(size)
}

/// Geist Mono at `size`, for measured values: coordinates, counts, hex
/// colors, sample counts.
pub fn mono(size: f32) -> FontId {
    FontId::monospace(size)
}

/// Phosphor icon font at `size`. Icons use the Phosphor family directly:
/// a text face may define glyphs in the same Private Use Area and would
/// otherwise shadow some icons in the proportional fallback chain.
pub fn icon(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("phosphor".into()))
}

/// Registers Geist (Regular + SemiBold), Geist Mono and Phosphor icons.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();

    fonts.font_data.insert(
        "geist".into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../assets/fonts/Geist-Regular.ttf"
        ))),
    );
    fonts.font_data.insert(
        "geist-semibold".into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../assets/fonts/Geist-SemiBold.ttf"
        ))),
    );
    fonts.font_data.insert(
        "geist-mono".into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../assets/fonts/GeistMono-Regular.ttf"
        ))),
    );

    // Geist first so it overrides egui's default proportional font.
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "geist".into());
    // Geist Mono first in the monospace chain; egui's own font stays behind
    // it for any glyph Geist Mono lacks.
    fonts
        .families
        .entry(FontFamily::Monospace)
        .or_default()
        .insert(0, "geist-mono".into());

    // SemiBold family falls back to the full proportional chain (icons, emoji).
    let mut semibold_chain = vec!["geist-semibold".to_owned()];
    semibold_chain.extend(
        fonts
            .families
            .get(&FontFamily::Proportional)
            .cloned()
            .unwrap_or_default(),
    );
    fonts
        .families
        .insert(FontFamily::Name("geist-semibold".into()), semibold_chain);

    // Phosphor is inserted as a proportional fallback (after Geist) and also
    // appended to the semibold chain so icons render in headers too.
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    // The "phosphor" family [`icon`] draws with holds Phosphor alone, so no
    // text face ahead of it can shadow an icon codepoint.
    fonts
        .families
        .insert(FontFamily::Name("phosphor".into()), vec!["phosphor".into()]);
    if let Some(chain) = fonts
        .families
        .get_mut(&FontFamily::Name("geist-semibold".into()))
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
            (TextStyle::Monospace, mono(12.0)),
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
        v.window_fill = BG_BUTTON;
        v.extreme_bg_color = BG_APP;
        v.faint_bg_color = BG_HEADER;
        v.window_stroke = Stroke::new(1.0, BORDER);
        v.window_corner_radius = RADIUS;
        v.menu_corner_radius = RADIUS;
        v.selection.bg_fill = ACCENT;
        v.selection.stroke = Stroke::new(1.0, TEXT_STRONG);
        v.hyperlink_color = ACCENT_LINE;

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
        w.active.bg_stroke = Stroke::new(1.0, ACCENT_LINE);
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

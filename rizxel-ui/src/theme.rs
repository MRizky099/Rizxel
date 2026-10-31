//! Dark and light themes, color tokens and font setup mirroring
//! `docs/rizxel_ui_redesign.html`.
//!
//! The colors are OKLCH tokens converted to sRGB: neutrals sit on hue 280
//! at chroma 0.003 to 0.016, close enough to grey that palette colors read
//! true, and the one accent is the ultramarine of the Rizxel icon. The
//! chrome reads its colors from [`p`], the palette of the active theme;
//! colors that mean the same in both themes (the accent fill, the axes, the
//! model bounds, viewport overlays) stay constants.

use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// The chrome colors of one theme.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    /// Input wells, the status bar and the window clear color.
    pub bg_app: Color32,
    /// The title bar and tab strips.
    pub bg_header: Color32,
    pub bg_tab_hover: Color32,
    /// Panel bodies.
    pub bg_panel: Color32,
    /// Buttons and popups.
    pub bg_button: Color32,
    pub bg_selected: Color32,
    pub hover: Color32,
    pub border: Color32,
    pub separator: Color32,
    /// Viewport ground: a mid grey in both themes, the neutral reference
    /// colors are judged on.
    pub workspace: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub text_muted: Color32,
    pub text_strong: Color32,
    /// The accent as a line or text on the panels (outlines, underlines,
    /// the active-tool bar).
    pub accent_line: Color32,
    pub danger: Color32,
}

/// Panels above the header strips, popups above the panels. Every text pair
/// stays at 4.5:1 or better on `bg_panel` and `bg_header`.
pub const DARK: Palette = Palette {
    bg_app: Color32::from_rgb(0x0f, 0x10, 0x15), // oklch(17.5% 0.011 280)
    bg_header: Color32::from_rgb(0x16, 0x17, 0x1d), // oklch(20.5% 0.012 280)
    bg_tab_hover: Color32::from_rgb(0x1a, 0x1b, 0x21), // oklch(22.25% 0.0125 280)
    bg_panel: Color32::from_rgb(0x1e, 0x1f, 0x25), // oklch(24% 0.013 280)
    bg_button: Color32::from_rgb(0x29, 0x29, 0x31), // oklch(28.5% 0.014 280)
    bg_selected: Color32::from_rgb(0x2f, 0x30, 0x38), // oklch(31% 0.015 280)
    hover: Color32::from_rgb(0x34, 0x35, 0x3d),  // oklch(33% 0.015 280)
    border: Color32::from_rgb(0x31, 0x32, 0x3b), // oklch(32% 0.015 280)
    separator: Color32::from_rgb(0x48, 0x49, 0x54), // oklch(41% 0.018 280)
    workspace: Color32::from_rgb(0x4a, 0x4a, 0x4e), // oklch(41% 0.006 280)
    text: Color32::from_rgb(0xc9, 0xca, 0xd2),   // oklch(84% 0.011 280)
    text_dim: Color32::from_rgb(0x9f, 0xa1, 0xab), // oklch(71% 0.015 280)
    text_muted: Color32::from_rgb(0x90, 0x91, 0x9c), // oklch(66% 0.015 280)
    text_strong: Color32::from_rgb(0xea, 0xeb, 0xf1), // oklch(94% 0.008 280)
    accent_line: Color32::from_rgb(0x86, 0x9e, 0xfc), // oklch(72% 0.14 272)
    danger: Color32::from_rgb(0xf9, 0x77, 0x70), // oklch(72% 0.16 25)
};

/// The same hue and accent with lightness re-derived: panels lighter than
/// the header strips, input wells a step darker than the panels.
pub const LIGHT: Palette = Palette {
    bg_app: Color32::from_rgb(0xf3, 0xf3, 0xf6), // oklch(96.5% 0.004 280)
    bg_header: Color32::from_rgb(0xef, 0xf0, 0xf3), // oklch(95.5% 0.005 280)
    bg_tab_hover: Color32::from_rgb(0xf4, 0xf5, 0xf8), // oklch(97% 0.004 280)
    bg_panel: Color32::from_rgb(0xfa, 0xfa, 0xfc), // oklch(98.5% 0.003 280)
    bg_button: Color32::from_rgb(0xe7, 0xe7, 0xec), // oklch(93% 0.006 280)
    bg_selected: Color32::from_rgb(0xe2, 0xe2, 0xe8), // oklch(91.5% 0.008 280)
    hover: Color32::from_rgb(0xdb, 0xdc, 0xe2),  // oklch(89.5% 0.008 280)
    border: Color32::from_rgb(0xda, 0xda, 0xdf), // oklch(89% 0.007 280)
    separator: Color32::from_rgb(0xbc, 0xbd, 0xc4), // oklch(80% 0.01 280)
    workspace: Color32::from_rgb(0x62, 0x63, 0x66), // oklch(50% 0.005 280)
    text: Color32::from_rgb(0x39, 0x3a, 0x43),   // oklch(35% 0.016 280)
    text_dim: Color32::from_rgb(0x5e, 0x60, 0x6a), // oklch(49% 0.016 280)
    text_muted: Color32::from_rgb(0x6a, 0x6b, 0x75), // oklch(53% 0.016 280)
    text_strong: Color32::from_rgb(0x19, 0x1a, 0x22), // oklch(22% 0.015 280)
    accent_line: Color32::from_rgb(0x39, 0x4f, 0xd8), // oklch(50% 0.21 270)
    danger: Color32::from_rgb(0xbe, 0x22, 0x2a), // oklch(52% 0.19 25)
};

static LIGHT_THEME: AtomicBool = AtomicBool::new(false);

/// The palette of the active theme.
pub fn p() -> &'static Palette {
    if is_light() {
        &LIGHT
    } else {
        &DARK
    }
}

pub fn is_light() -> bool {
    LIGHT_THEME.load(Ordering::Relaxed)
}

/// Switches the palette [`p`] returns and egui's theme to match (both
/// egui styles are set up by [`apply`]). Returns whether anything changed.
pub fn set_light(ctx: &egui::Context, light: bool) -> bool {
    if LIGHT_THEME.swap(light, Ordering::Relaxed) == light {
        return false;
    }
    ctx.set_theme(egui_theme());
    true
}

fn egui_theme() -> egui::Theme {
    if is_light() {
        egui::Theme::Light
    } else {
        egui::Theme::Dark
    }
}

pub const GRID_LINE: Color32 = Color32::from_rgba_unmultiplied_const(0xae, 0xae, 0xae, 102);

// Accents.
/// Brand ultramarine, for fills in both themes: text on it is ACCENT_INK.
pub const ACCENT: Color32 = Color32::from_rgb(0x29, 0x41, 0xd9); // oklch(47% 0.23 268)
pub const ACCENT_INK: Color32 = Color32::from_rgb(0xf3, 0xf4, 0xfc); // oklch(97% 0.01 280)
/// Viewport selection outline and tool previews (not used for chrome).
pub const ACCENT_LIGHT: Color32 = Color32::from_rgb(0x38, 0xbd, 0xf8);
pub const BOUND: Color32 = Color32::from_rgb(0xfb, 0xbf, 0x24);
pub const PAINT: Color32 = Color32::from_rgb(0xea, 0xb3, 0x08);
pub const CLOSE_HOVER: Color32 = Color32::from_rgb(0xe8, 0x11, 0x23);
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

/// Sets up egui's dark and light styles from [`DARK`] and [`LIGHT`], with
/// Rizxel's text sizes and spacing, and selects the active theme's.
pub fn apply(ctx: &egui::Context) {
    for (theme, palette) in [(egui::Theme::Dark, &DARK), (egui::Theme::Light, &LIGHT)] {
        ctx.set_visuals_of(theme, theme.default_visuals());
        ctx.style_mut_of(theme, |style| style_with(style, palette, theme));
    }
    ctx.set_theme(egui_theme());
}

fn style_with(style: &mut egui::Style, c: &Palette, theme: egui::Theme) {
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

    let light = theme == egui::Theme::Light;
    let v = &mut style.visuals;
    v.dark_mode = !light;
    v.override_text_color = None;
    v.panel_fill = c.bg_panel;
    v.window_fill = c.bg_button;
    v.extreme_bg_color = c.bg_app;
    v.faint_bg_color = c.bg_header;
    v.window_stroke = Stroke::new(1.0, c.border);
    v.window_corner_radius = RADIUS;
    v.menu_corner_radius = RADIUS;
    v.selection.bg_fill = ACCENT;
    v.selection.stroke = Stroke::new(1.0, ACCENT_INK);
    v.hyperlink_color = c.accent_line;
    // Field edges: the border is enough on dark, too faint on white.
    let edge = if light { c.separator } else { c.border };

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = c.bg_panel;
    w.noninteractive.weak_bg_fill = c.bg_panel;
    w.noninteractive.bg_stroke = Stroke::new(1.0, c.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0, c.text);

    // Also the slider rail: the ground is too close to a white panel.
    w.inactive.bg_fill = if light { c.hover } else { c.bg_app };
    w.inactive.weak_bg_fill = Color32::TRANSPARENT;
    w.inactive.bg_stroke = Stroke::new(1.0, edge);
    w.inactive.fg_stroke = Stroke::new(1.0, c.text);
    w.inactive.corner_radius = RADIUS;

    w.hovered.bg_fill = c.hover;
    w.hovered.weak_bg_fill = c.hover;
    w.hovered.bg_stroke = Stroke::new(1.0, edge);
    w.hovered.fg_stroke = Stroke::new(1.0, c.text_strong);
    w.hovered.corner_radius = RADIUS;
    w.hovered.expansion = 0.0;

    w.active.bg_fill = c.hover;
    w.active.weak_bg_fill = c.hover;
    w.active.bg_stroke = Stroke::new(1.0, c.accent_line);
    w.active.fg_stroke = Stroke::new(1.0, c.text_strong);
    w.active.corner_radius = RADIUS;
    w.active.expansion = 0.0;

    w.open.bg_fill = c.hover;
    w.open.weak_bg_fill = c.hover;
    w.open.bg_stroke = Stroke::new(1.0, edge);
    w.open.fg_stroke = Stroke::new(1.0, c.text_strong);
    w.open.corner_radius = RADIUS;
}

/// Text or a mark drawn on a user color (a palette swatch): light on dark
/// colors, dark on light ones, whatever the theme.
pub fn ink_on(bg: Color32) -> Color32 {
    let luma = 0.299 * f32::from(bg.r()) + 0.587 * f32::from(bg.g()) + 0.114 * f32::from(bg.b());
    if luma > 150.0 {
        Color32::from_black_alpha(180)
    } else {
        Color32::from_white_alpha(200)
    }
}

/// Converts an engine color into an egui color.
pub fn to_color32(c: rizxel_math::Rgba8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance(c: Color32) -> f32 {
        let lin = |v: u8| {
            let v = f32::from(v) / 255.0;
            if v <= 0.040_45 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
    }

    fn contrast(a: Color32, b: Color32) -> f32 {
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    #[test]
    fn text_reads_on_panels_and_headers_in_both_themes() {
        for (name, c) in [("dark", DARK), ("light", LIGHT)] {
            for bg in [c.bg_panel, c.bg_header, c.bg_app] {
                for (role, fg) in [
                    ("text", c.text),
                    ("text_dim", c.text_dim),
                    ("text_muted", c.text_muted),
                    ("text_strong", c.text_strong),
                    ("accent_line", c.accent_line),
                    ("danger", c.danger),
                ] {
                    let ratio = contrast(fg, bg);
                    assert!(ratio >= 4.5, "{name} {role} on {bg:?}: {ratio:.2}");
                }
            }
        }
        assert!(contrast(ACCENT_INK, ACCENT) >= 4.5);
    }

    #[test]
    fn ink_on_a_swatch_contrasts_with_it() {
        for c in [
            Color32::WHITE,
            Color32::BLACK,
            ACCENT,
            BOUND,
            DARK.bg_panel,
            LIGHT.bg_panel,
        ] {
            let ink = ink_on(c);
            // Color32 is premultiplied: over = ink + bg * (1 - alpha).
            let over = |i: u8, b: u8| {
                (u16::from(i) + u16::from(b) * (255 - u16::from(ink.a())) / 255).min(255) as u8
            };
            let shown = Color32::from_rgb(
                over(ink.r(), c.r()),
                over(ink.g(), c.g()),
                over(ink.b(), c.b()),
            );
            assert!(contrast(shown, c) >= 3.0, "{c:?}");
        }
    }
}

//! Color picker of the Color Palette panel (a popup under its color chip):
//! edits the selected palette color with a saturation / value square, a hue
//! bar, RGB or HSV sliders and a hex field. Its eyedropper switches to the
//! Color Picker tool, which picks a color off the model instead.

use egui::{Color32, Mesh, Pos2, Rect, Response, RichText, Sense, Stroke, StrokeKind, Ui};
use egui_phosphor::regular as icon;
use rizxel_app::Document;
use rizxel_math::Rgba8;

use crate::theme;
use crate::widgets::{icon_button, IconStyle};

/// Which sliders are shown under the square.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SliderMode {
    #[default]
    Rgb,
    Hsv,
}

/// Picker state kept between frames.
#[derive(Debug, Default)]
pub struct PickerState {
    pub mode: SliderMode,
    /// Hue, saturation and value of the color being edited, and the palette
    /// slot and color they belong to. Kept so the hue survives greys (where
    /// RGB has none) while you drag.
    hsv: Option<(usize, Rgba8, [f32; 3])>,
    /// Text of the hex field while it is being typed in.
    hex: Option<String>,
}

/// What the picker asks of the app beyond the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerAction {
    /// The eyedropper: pick a color off the model with the Color Picker tool.
    Eyedropper,
}

/// Height of the RGB / HSV slider tracks.
const TRACK_HEIGHT: f32 = 12.0;
/// Width of a slider's value field.
const FIELD_WIDTH: f32 = 44.0;

pub fn show(ui: &mut Ui, doc: &mut Document, state: &mut PickerState) -> Option<PickerAction> {
    let index = doc.selected_color;
    let current = doc.selected_rgba();
    // Resync when another slot is selected or the color changed elsewhere
    // (undo, palette load).
    let hsv = match state.hsv {
        Some((i, c, hsv)) if i == index && c == current => hsv,
        _ => rgb_to_hsv(current),
    };
    let mut next = Edit::default();
    let mut action = None;

    ui.spacing_mut().item_spacing.y = 8.0;
    sv_and_hue(ui, hsv, &mut next);

    ui.horizontal(|ui| {
        let style = IconStyle::small().size(egui::vec2(24.0, 22.0));
        if icon_button(ui, icon::EYEDROPPER, style, true)
            .on_hover_text("Pick a color off the model (Color Picker tool)")
            .clicked()
        {
            action = Some(PickerAction::Eyedropper);
        }
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 22.0), Sense::hover());
        ui.painter()
            .rect_filled(rect, theme::RADIUS, theme::to_color32(current));
        ui.painter().rect_stroke(
            rect,
            theme::RADIUS,
            Stroke::new(1.0, theme::p().border),
            StrokeKind::Inside,
        );
    });

    super::options::segmented_fill(
        ui,
        "picker_mode",
        &mut state.mode,
        &[(SliderMode::Rgb, "RGB"), (SliderMode::Hsv, "HSV")],
    );

    match state.mode {
        SliderMode::Rgb => rgb_sliders(ui, current, &mut next),
        SliderMode::Hsv => hsv_sliders(ui, hsv, &mut next),
    }
    hex_row(ui, current, state, &mut next);

    // Apply: an edit merges into the previous one while a drag goes on, so a
    // drag is one undo step.
    let (new, new_hsv) = match (next.rgb, next.hsv) {
        (_, Some(h)) => (hsv_to_rgb(h), h),
        (Some(c), None) => (c, rgb_to_hsv(c)),
        (None, None) => (current, hsv),
    };
    if new != current {
        doc.set_palette_color(index, new, next.merge);
    }
    state.hsv = Some((index, new, new_hsv));
    action
}

/// The change asked for this frame.
#[derive(Default)]
struct Edit {
    rgb: Option<Rgba8>,
    hsv: Option<[f32; 3]>,
    /// Merge into the previous undo step (a drag in progress).
    merge: bool,
}

impl Edit {
    fn merge_while_dragging(&mut self, response: &Response) {
        self.merge = response.dragged() && !response.drag_started();
    }
}

// ------------------------------------------------------------ square + hue

fn sv_and_hue(ui: &mut Ui, [h, s, v]: [f32; 3], next: &mut Edit) {
    const HUE_WIDTH: f32 = 14.0;
    const GAP: f32 = 6.0;
    let width = ui.available_width();
    let side = (width - HUE_WIDTH - GAP).clamp(80.0, 220.0);
    let (row, _) = ui.allocate_exact_size(egui::vec2(width, side * 0.75), Sense::hover());
    let square = Rect::from_min_size(row.min, egui::vec2(side, row.height()));
    let hue_rect = Rect::from_min_size(
        egui::pos2(square.right() + GAP, row.top()),
        egui::vec2(HUE_WIDTH, row.height()),
    );

    // Saturation across, value up.
    let response = ui.interact(square, ui.id().with("picker_sv"), Sense::click_and_drag());
    paint_grid(ui, square, 16, 12, |x, y| hsv_color32([h, x, 1.0 - y]));
    let p = Pos2::new(
        square.left() + s * square.width(),
        square.top() + (1.0 - v) * square.height(),
    );
    ui.painter()
        .circle_stroke(p, 5.0, Stroke::new(2.0, Color32::BLACK));
    ui.painter()
        .circle_stroke(p, 4.0, Stroke::new(1.5, Color32::WHITE));
    if let Some(pos) = response.interact_pointer_pos() {
        if response.is_pointer_button_down_on() || response.clicked() {
            let ns = ((pos.x - square.left()) / square.width()).clamp(0.0, 1.0);
            let nv = 1.0 - ((pos.y - square.top()) / square.height()).clamp(0.0, 1.0);
            next.hsv = Some([h, ns, nv]);
            next.merge_while_dragging(&response);
        }
    }
    response.on_hover_text("Saturation (across) and value (up)");

    // Hue, red at the top.
    let response = ui.interact(
        hue_rect,
        ui.id().with("picker_hue"),
        Sense::click_and_drag(),
    );
    paint_grid(ui, hue_rect, 1, 24, |_, y| hsv_color32([y, 1.0, 1.0]));
    let y = hue_rect.top() + h * hue_rect.height();
    ui.painter().rect_stroke(
        Rect::from_center_size(
            egui::pos2(hue_rect.center().x, y),
            egui::vec2(HUE_WIDTH + 4.0, 4.0),
        ),
        1.0,
        Stroke::new(1.5, Color32::WHITE),
        StrokeKind::Middle,
    );
    if let Some(pos) = response.interact_pointer_pos() {
        if response.is_pointer_button_down_on() || response.clicked() {
            let nh = ((pos.y - hue_rect.top()) / hue_rect.height()).clamp(0.0, 0.9999);
            next.hsv = Some([nh, s, v]);
            next.merge_while_dragging(&response);
        }
    }
    response.on_hover_text("Hue");
}

/// Fills `rect` with a `cols` x `rows` grid of vertex colors from
/// `color(x, y)`, `x` and `y` in `0..=1`.
fn paint_grid(ui: &Ui, rect: Rect, cols: usize, rows: usize, color: impl Fn(f32, f32) -> Color32) {
    let mut mesh = Mesh::default();
    for j in 0..=rows {
        for i in 0..=cols {
            let (x, y) = (i as f32 / cols as f32, j as f32 / rows as f32);
            let pos = egui::pos2(
                rect.left() + x * rect.width(),
                rect.top() + y * rect.height(),
            );
            mesh.colored_vertex(pos, color(x, y));
        }
    }
    let stride = cols as u32 + 1;
    for j in 0..rows as u32 {
        for i in 0..cols as u32 {
            let a = j * stride + i;
            mesh.add_triangle(a, a + 1, a + stride);
            mesh.add_triangle(a + 1, a + stride + 1, a + stride);
        }
    }
    ui.painter().add(mesh);
    ui.painter().rect_stroke(
        rect,
        0.0,
        Stroke::new(1.0, theme::p().border),
        StrokeKind::Outside,
    );
}

// ------------------------------------------------------------ sliders

fn rgb_sliders(ui: &mut Ui, c: Rgba8, next: &mut Edit) {
    let channels = [("R", c.r), ("G", c.g), ("B", c.b)];
    for (i, (name, value)) in channels.into_iter().enumerate() {
        let with = |v: u8| {
            let mut out = [c.r, c.g, c.b];
            out[i] = v;
            Color32::from_rgb(out[0], out[1], out[2])
        };
        let mut x = value as f32 / 255.0;
        let mut field = value as u32;
        let changed = slider_row(
            ui,
            name,
            &mut x,
            |t| with((t * 255.0).round() as u8),
            |ui| ui.add(egui::DragValue::new(&mut field).range(0..=255).speed(0.5)),
            next,
        );
        if changed.track {
            field = (x * 255.0).round() as u32;
        }
        if changed.track || changed.field {
            let mut out = [c.r, c.g, c.b];
            out[i] = field.min(255) as u8;
            next.rgb = Some(Rgba8::rgb(out[0], out[1], out[2]));
        }
    }
}

fn hsv_sliders(ui: &mut Ui, hsv: [f32; 3], next: &mut Edit) {
    let names = ["H", "S", "V"];
    let scales = [360.0, 100.0, 100.0];
    for i in 0..3 {
        let with = |t: f32| {
            let mut out = hsv;
            out[i] = t;
            if i == 0 {
                // The hue track shows every hue at full strength.
                out = [t, 1.0, 1.0];
            }
            hsv_color32(out)
        };
        let mut x = hsv[i];
        let mut field = (hsv[i] * scales[i]).round() as u32;
        let max = scales[i] as u32 - u32::from(i == 0);
        let changed = slider_row(
            ui,
            names[i],
            &mut x,
            with,
            |ui| {
                let suffix = if i == 0 { "°" } else { "%" };
                ui.add(
                    egui::DragValue::new(&mut field)
                        .range(0..=max)
                        .speed(0.5)
                        .suffix(suffix),
                )
            },
            next,
        );
        if changed.track || changed.field {
            let mut out = hsv;
            out[i] = if changed.track {
                x
            } else {
                field.min(max) as f32 / scales[i]
            };
            if i == 0 {
                out[0] = out[0].min(0.9999);
            }
            next.hsv = Some(out);
        }
    }
}

struct Changed {
    track: bool,
    field: bool,
}

/// A label, a gradient track with a marker under it, and a value field.
fn slider_row(
    ui: &mut Ui,
    name: &str,
    value: &mut f32,
    color: impl Fn(f32) -> Color32,
    field: impl FnOnce(&mut Ui) -> Response,
    next: &mut Edit,
) -> Changed {
    let mut out = Changed {
        track: false,
        field: false,
    };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.add_sized(
            [12.0, 20.0],
            egui::Label::new(RichText::new(name).color(theme::p().text_dim)),
        );
        let width = (ui.available_width() - FIELD_WIDTH - 8.0).max(40.0);
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(width, 20.0), Sense::click_and_drag());
        let track = Rect::from_min_size(
            rect.left_top() + egui::vec2(0.0, 2.0),
            egui::vec2(width, TRACK_HEIGHT),
        );
        paint_grid(ui, track, 24, 1, |x, _| color(x));
        let x = track.left() + *value * track.width();
        let tip = egui::pos2(x, track.bottom() + 1.0);
        ui.painter().add(egui::Shape::convex_polygon(
            vec![tip, tip + egui::vec2(4.0, 5.0), tip + egui::vec2(-4.0, 5.0)],
            theme::p().text_strong,
            Stroke::NONE,
        ));
        if let Some(pos) = response.interact_pointer_pos() {
            if response.is_pointer_button_down_on() || response.clicked() {
                *value = ((pos.x - track.left()) / track.width()).clamp(0.0, 1.0);
                out.track = true;
                next.merge_while_dragging(&response);
            }
        }
        let response = ui
            .add_sized([FIELD_WIDTH, 20.0], |ui: &mut Ui| field(ui))
            .on_hover_text("Drag or type a value");
        if response.changed() {
            out.field = true;
            next.merge = response.dragged() && !response.drag_started();
        }
    });
    out
}

// ------------------------------------------------------------ hex

fn hex_row(ui: &mut Ui, c: Rgba8, state: &mut PickerState, next: &mut Edit) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.label(RichText::new("Hex").color(theme::p().text_dim));
        ui.label(
            RichText::new("#")
                .font(theme::mono(12.0))
                .color(theme::p().text_dim),
        );
        let shown = format!("{:02X}{:02X}{:02X}", c.r, c.g, c.b);
        let mut text = state.hex.clone().unwrap_or(shown.clone());
        let width = (ui.available_width() - 32.0).max(60.0);
        let response = ui.add(
            egui::TextEdit::singleline(&mut text)
                .font(theme::mono(12.0))
                .char_limit(6)
                .desired_width(width),
        );
        if response.has_focus() {
            if let Some(color) = parse_hex(&text) {
                next.rgb = Some(color);
            }
            state.hex = Some(text);
        } else {
            state.hex = None;
        }
        let style = IconStyle::small().size(egui::vec2(24.0, 22.0));
        if icon_button(ui, icon::COPY, style, true)
            .on_hover_text("Copy the hex code")
            .clicked()
        {
            ui.ctx().copy_text(format!("#{shown}"));
        }
    });
}

/// `RRGGBB` or `RGB` (with or without `#`) to a color.
fn parse_hex(text: &str) -> Option<Rgba8> {
    let t = text.trim().trim_start_matches('#');
    let digits: Vec<u8> = t
        .chars()
        .map(|ch| ch.to_digit(16).map(|d| d as u8))
        .collect::<Option<_>>()?;
    match digits.as_slice() {
        [r, g, b] => Some(Rgba8::rgb(r * 17, g * 17, b * 17)),
        [r1, r0, g1, g0, b1, b0] => Some(Rgba8::rgb(r1 * 16 + r0, g1 * 16 + g0, b1 * 16 + b0)),
        _ => None,
    }
}

// ------------------------------------------------------------ color math

/// sRGB to hue, saturation and value, each in `0..=1` (gamma space, as
/// paint programs show them).
fn rgb_to_hsv(c: Rgba8) -> [f32; 3] {
    let [r, g, b] = [c.r, c.g, c.b].map(|x| x as f32 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        ((g - b) / d).rem_euclid(6.0) / 6.0
    } else if max == g {
        ((b - r) / d + 2.0) / 6.0
    } else {
        ((r - g) / d + 4.0) / 6.0
    };
    let s = if max == 0.0 { 0.0 } else { d / max };
    [h, s, max]
}

fn hsv_to_rgb([h, s, v]: [f32; 3]) -> Rgba8 {
    let h6 = (h.rem_euclid(1.0)) * 6.0;
    let c = v * s;
    let x = c * (1.0 - (h6 % 2.0 - 1.0).abs());
    let (r, g, b) = match h6 as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    let to = |x: f32| ((x + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    Rgba8::rgb(to(r), to(g), to(b))
}

fn hsv_color32(hsv: [f32; 3]) -> Color32 {
    let c = hsv_to_rgb(hsv);
    Color32::from_rgb(c.r, c.g, c.b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsv_round_trips_every_primary_and_a_grey() {
        for c in [
            Rgba8::rgb(255, 0, 0),
            Rgba8::rgb(0, 255, 0),
            Rgba8::rgb(0, 0, 255),
            Rgba8::rgb(91, 42, 138),
            Rgba8::rgb(128, 128, 128),
            Rgba8::BLACK,
            Rgba8::WHITE,
        ] {
            assert_eq!(hsv_to_rgb(rgb_to_hsv(c)), c, "{c:?}");
        }
        assert_eq!(rgb_to_hsv(Rgba8::rgb(0, 0, 255))[0], 2.0 / 3.0);
    }

    #[test]
    fn hex_accepts_six_or_three_digits() {
        assert_eq!(parse_hex("5B2A8A"), Some(Rgba8::rgb(0x5b, 0x2a, 0x8a)));
        assert_eq!(parse_hex("#fff"), Some(Rgba8::WHITE));
        assert_eq!(parse_hex("12345"), None);
        assert_eq!(parse_hex("zz0000"), None);
    }
}

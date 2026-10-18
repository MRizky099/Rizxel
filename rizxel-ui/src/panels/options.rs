//! Option bar: history, voxel operations, per-tool parameters and view mode.
//!
//! The parameters after the Paint button follow the active tool. Their set
//! mirrors Goxel's tool panels (`src/tools/*.c`). The whole left part scrolls
//! horizontally when the window is too narrow; the Model/Render toggle stays
//! pinned to the right.

use egui::{Color32, Sense, Stroke, StrokeKind, Ui};
use rizxel_app::{AppState, ViewMode};
use rizxel_engine::{
    BrushShape, DragMode, MaskMode, NamesMode, PlaneMode, Symmetry, Tool, VoxelOp,
};

use crate::icons::{self, Icon, Icons};
use crate::theme;
use crate::widgets::{hscroll_strip, vertical_divider};

const BUTTON_SIZE: f32 = 32.0;
const ICON_SIZE: f32 = 20.0;
const DIVIDER_HEIGHT: f32 = 20.0;
/// Gap between the scrolling strip and the pinned view toggle.
const TOGGLE_GAP: f32 = 8.0;
const VIEW_MODES: [(ViewMode, &str); 2] =
    [(ViewMode::Model, "Model"), (ViewMode::Render, "Render")];

pub fn show(ui: &mut Ui, state: &mut AppState, icons: &Icons) {
    let height = ui.max_rect().height();
    let toggle_width = segmented_width(ui, &VIEW_MODES);

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        hscroll_strip(
            ui,
            "option_bar",
            height,
            toggle_width + TOGGLE_GAP,
            theme::HOVER,
            |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                common_controls(ui, state, icons);
                vertical_divider(ui, DIVIDER_HEIGHT);
                tool_parameters(ui, state);
                ui.add_space(8.0);
            },
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            segmented(ui, "view_mode", &mut state.view_mode, &VIEW_MODES);
        });
    });
}

/// Undo/Redo, Add/Subtract/Join and Paint.
fn common_controls(ui: &mut Ui, state: &mut AppState, icons: &Icons) {
    let (can_undo, can_redo) = (state.can_undo(), state.can_redo());
    if icon_texture_button(ui, icons.get(Icon::Undo), false, can_undo, None)
        .on_hover_text("Undo (Ctrl+Z)")
        .clicked()
    {
        state.undo();
    }
    if icon_texture_button(ui, icons.get(Icon::Redo), false, can_redo, None)
        .on_hover_text("Redo (Ctrl+Y)")
        .clicked()
    {
        state.redo();
    }

    vertical_divider(ui, DIVIDER_HEIGHT);

    // Operations only affect the drawing tools; others show them dimmed.
    let ops_enabled = state.tool.uses_voxel_op();
    for op in [VoxelOp::Add, VoxelOp::Subtract, VoxelOp::Join] {
        op_button(ui, state, icons, op, ops_enabled, theme::ACCENT);
    }

    vertical_divider(ui, DIVIDER_HEIGHT);

    op_button(ui, state, icons, VoxelOp::Paint, ops_enabled, theme::PAINT);
}

fn op_button(
    ui: &mut Ui,
    state: &mut AppState,
    icons: &Icons,
    op: VoxelOp,
    enabled: bool,
    accent: Color32,
) {
    let active = state.op == op;
    let response = icon_texture_button(ui, icons.get(op), active, enabled, Some(accent));
    let tip = if enabled {
        op.label().to_owned()
    } else {
        format!(
            "{} (not used by the {} tool)",
            op.label(),
            state.tool.label()
        )
    };
    if response.on_hover_text(tip).clicked() {
        state.op = op;
    }
}

/// 32x32 option-bar button with a pixel-art icon. `active` draws an accent
/// border (Paint uses yellow, the others blue).
fn icon_texture_button(
    ui: &mut Ui,
    texture: &egui::TextureHandle,
    active: bool,
    enabled: bool,
    accent: Option<Color32>,
) -> egui::Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(egui::Vec2::splat(BUTTON_SIZE), sense);
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let painter = ui.painter();
    let hovered = enabled && response.hovered();

    if active {
        painter.rect_filled(rect, theme::RADIUS, theme::BG_BUTTON);
        let stroke = accent.unwrap_or(theme::ACCENT);
        let stroke = if enabled {
            stroke
        } else {
            stroke.gamma_multiply(0.4)
        };
        painter.rect_stroke(
            rect,
            theme::RADIUS,
            Stroke::new(1.0, stroke),
            StrokeKind::Inside,
        );
    } else if hovered {
        painter.rect_filled(rect, theme::RADIUS, theme::HOVER);
    }

    let tint = match (enabled, active || hovered) {
        (false, _) => Color32::from_white_alpha(70),
        (true, true) => Color32::WHITE,
        (true, false) => Color32::from_gray(185),
    };
    icons::paint(ui, rect, texture, ICON_SIZE, tint);
    response
}

// ------------------------------------------------------------ tool params

fn tool_parameters(ui: &mut Ui, state: &mut AppState) {
    ui.label(
        egui::RichText::new(state.tool.label())
            .size(12.0)
            .strong()
            .color(theme::TEXT_STRONG),
    );
    ui.add_space(6.0);

    let opts = &mut state.options;
    match state.tool {
        Tool::Cursor => {
            caption(ui, "Show names");
            let modes = NamesMode::ALL.map(|m| (m, m.label()));
            segmented(ui, "cursor_names", &mut opts.cursor.show_names, &modes);
        }
        Tool::Brush => {
            let b = &mut opts.brush;
            shape_combo(ui, "brush_shape", &mut b.shape);
            gap(ui);
            caption(ui, "Size");
            for (axis, value) in ["X", "Y", "Z"].into_iter().zip(b.size.iter_mut()) {
                labeled_drag(ui, axis, value, 1..=rizxel_engine::BrushOptions::MAX_SIZE);
            }
            gap(ui);
            ui.checkbox(&mut b.surface_paint, "Surface paint")
                .on_hover_text("Only paint voxels on the surface under the cursor");
            gap(ui);
            antialiasing(ui, &mut b.antialiasing);
            gap(ui);
            symmetry(ui, "brush_sym", &mut b.symmetry);
        }
        Tool::Shape => {
            let s = &mut opts.shape;
            shape_combo(ui, "shape_shape", &mut s.shape);
            gap(ui);
            ui.checkbox(&mut s.planar, "Planar")
                .on_hover_text("Stay on the plane where the drag started");
            gap(ui);
            antialiasing(ui, &mut s.antialiasing);
            gap(ui);
            symmetry(ui, "shape_sym", &mut s.symmetry);
        }
        Tool::Plane => {
            let p = &mut opts.plane;
            ui.checkbox(&mut p.slice, "Slice");
            gap(ui);
            egui::ComboBox::from_id_salt("plane_mode")
                .width(88.0)
                .selected_text(p.mode.label())
                .show_ui(ui, |ui| {
                    for mode in PlaneMode::ALL {
                        ui.selectable_value(&mut p.mode, mode, mode.label());
                    }
                });
            gap(ui);
            caption(ui, "Offset");
            ui.add(
                egui::DragValue::new(&mut p.offset)
                    .speed(0.1)
                    .range(-256..=256),
            );
        }
        Tool::Move => {
            caption(ui, "Drag mode");
            let modes = DragMode::ALL.map(|m| (m, m.label()));
            segmented(ui, "move_drag", &mut opts.moving.drag_mode, &modes);
        }
        Tool::ColorPicker => {
            ui.checkbox(
                &mut opts.color_picker.sample_all_layers,
                "Sample all layers",
            )
            .on_hover_text("Pick the topmost visible voxel instead of the active layer's");
        }
        Tool::Selection => {
            mask_mode(ui, "sel_mask", &mut opts.selection.mask);
        }
        Tool::FuzzySelect => {
            let f = &mut opts.fuzzy_select;
            ui.checkbox(&mut f.entire_layer, "Entire layer")
                .on_hover_text(
                    "Select matching voxels anywhere in the layer, not only connected ones",
                );
            gap(ui);
            color_threshold(ui, &mut f.use_color, &mut f.threshold);
            gap(ui);
            mask_mode(ui, "fuzzy_mask", &mut f.mask);
        }
        Tool::Extrude => {
            let e = &mut opts.extrude;
            ui.checkbox(&mut e.confine_to_layer, "Confine to layer");
            gap(ui);
            color_threshold(ui, &mut e.use_color, &mut e.threshold);
        }
        Tool::Fill => {
            let f = &mut opts.fill;
            ui.checkbox(&mut f.current_layer_only, "Current layer only")
                .on_hover_text("Restrict the flood fill to voxels of the active layer");
            gap(ui);
            caption(ui, "Threshold");
            ui.add(egui::DragValue::new(&mut f.threshold).range(0..=255))
                .on_hover_text("How close colors must be to be filled");
        }
    }
}

fn gap(ui: &mut Ui) {
    ui.add_space(8.0);
}

fn caption(ui: &mut Ui, text: &str) {
    ui.label(egui::RichText::new(text).size(12.0).color(theme::TEXT_DIM));
}

fn labeled_drag(ui: &mut Ui, label: &str, value: &mut u32, range: std::ops::RangeInclusive<u32>) {
    ui.label(
        egui::RichText::new(label)
            .size(11.0)
            .color(theme::TEXT_MUTED),
    );
    ui.add(egui::DragValue::new(value).speed(0.1).range(range));
}

fn shape_combo(ui: &mut Ui, id: &str, shape: &mut BrushShape) {
    egui::ComboBox::from_id_salt(id)
        .width(80.0)
        .selected_text(shape.label())
        .show_ui(ui, |ui| {
            for s in BrushShape::ALL {
                ui.selectable_value(shape, s, s.label());
            }
        });
}

fn antialiasing(ui: &mut Ui, value: &mut u32) {
    caption(ui, "Antialias");
    ui.add(
        egui::DragValue::new(value)
            .speed(0.05)
            .range(0..=rizxel_engine::BrushOptions::MAX_ANTIALIASING),
    )
    .on_hover_text("0 = hard edges, 1 = soft edge (~1 voxel), 2+ = wider blur");
}

fn symmetry(ui: &mut Ui, id: &str, sym: &mut Symmetry) {
    caption(ui, "Symmetry");
    let labels = ["X", "Y", "Z"];
    let mut axes = [sym.x, sym.y, sym.z];
    toggle_group(ui, id, &labels, &mut axes);
    [sym.x, sym.y, sym.z] = axes;
}

fn mask_mode(ui: &mut Ui, id: &str, mode: &mut MaskMode) {
    caption(ui, "Mask");
    let modes = MaskMode::ALL.map(|m| (m, m.label()));
    segmented(ui, id, mode, &modes);
}

fn color_threshold(ui: &mut Ui, use_color: &mut bool, threshold: &mut u8) {
    ui.checkbox(use_color, "Use color")
        .on_hover_text("Stop at voxels of a different color");
    ui.add_enabled_ui(*use_color, |ui| {
        caption(ui, "Threshold");
        ui.add(egui::DragValue::new(threshold).range(0..=254))
            .on_hover_text("How close colors must be to count as the same");
    });
}

// ------------------------------------------------------------ segmented

const SEG_PAD: egui::Vec2 = egui::vec2(12.0, 4.0);

fn seg_font() -> egui::FontId {
    theme::regular(12.0)
}

fn segment_widths<T>(ui: &Ui, items: &[(T, &str)]) -> (Vec<f32>, f32) {
    let mut height = 0.0_f32;
    let widths = items
        .iter()
        .map(|(_, label)| {
            let size = ui
                .painter()
                .layout_no_wrap((*label).to_owned(), seg_font(), theme::TEXT)
                .size();
            height = height.max(size.y);
            size.x + SEG_PAD.x * 2.0
        })
        .collect();
    (widths, height + SEG_PAD.y * 2.0)
}

fn segmented_width<T>(ui: &Ui, items: &[(T, &str)]) -> f32 {
    segment_widths(ui, items).0.iter().sum()
}

/// Segmented control sized to its labels (`px-3 py-1` in the HTML), used for
/// Model/Render and the single-choice tool options.
fn segmented<T: PartialEq + Copy>(ui: &mut Ui, id: &str, value: &mut T, items: &[(T, &str)]) {
    let labels: Vec<&str> = items.iter().map(|(_, l)| *l).collect();
    let active: Vec<bool> = items.iter().map(|(item, _)| item == value).collect();
    if let Some(i) = segments(ui, id, &labels, &active) {
        *value = items[i].0;
    }
}

/// Like [`segmented`] but every segment toggles independently (multi-select).
fn toggle_group(ui: &mut Ui, id: &str, labels: &[&str], values: &mut [bool]) {
    if let Some(i) = segments(ui, id, labels, values) {
        values[i] = !values[i];
    }
}

/// Paints a row of joined segments and returns the clicked index.
fn segments(ui: &mut Ui, id: &str, labels: &[&str], active: &[bool]) -> Option<usize> {
    let items: Vec<((), &str)> = labels.iter().map(|l| ((), *l)).collect();
    let (widths, height) = segment_widths(ui, &items);
    let (outer, _) =
        ui.allocate_exact_size(egui::vec2(widths.iter().sum(), height), Sense::hover());
    let painter = ui.painter().clone();
    painter.rect_filled(outer, theme::RADIUS, theme::BG_APP);

    let radius = theme::RADIUS.nw;
    let last = labels.len() - 1;
    let mut clicked = None;
    let mut x = outer.left();
    for (i, label) in labels.iter().enumerate() {
        let rect =
            egui::Rect::from_min_size(egui::pos2(x, outer.top()), egui::vec2(widths[i], height));
        x += widths[i];

        let response = ui.interact(rect, ui.id().with((id, i)), Sense::click());
        let active = active[i];
        let fill = if active {
            theme::HOVER
        } else if response.hovered() {
            theme::BG_BUTTON
        } else {
            Color32::TRANSPARENT
        };
        // Round only the outer corners of the group.
        let corners = egui::CornerRadius {
            nw: if i == 0 { radius } else { 0 },
            sw: if i == 0 { radius } else { 0 },
            ne: if i == last { radius } else { 0 },
            se: if i == last { radius } else { 0 },
        };
        painter.rect_filled(rect, corners, fill);
        let color = if active || response.hovered() {
            theme::TEXT_STRONG
        } else {
            theme::TEXT_DIM
        };
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            *label,
            seg_font(),
            color,
        );

        if response.clicked() {
            clicked = Some(i);
        }
    }
    painter.rect_stroke(
        outer,
        theme::RADIUS,
        Stroke::new(1.0, theme::BORDER),
        StrokeKind::Inside,
    );
    clicked
}

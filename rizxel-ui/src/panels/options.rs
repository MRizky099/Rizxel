//! Option bar: history, voxel operations, per-tool parameters and view mode.
//!
//! The parameters after the Paint button follow the active tool. Their set
//! mirrors Goxel's tool panels (`src/tools/*.c`). The whole left part scrolls
//! horizontally when the window is too narrow; the Model/Render toggle stays
//! pinned to the right.

use egui::{Color32, Sense, Stroke, StrokeKind, Ui};
use rizxel_app::{AppState, ViewMode};
use rizxel_engine::{
    BrushShape, DragMode, ExtrudeOptions, FillMode, MaskMode, NamesMode, Symmetry, Tool, VoxelOp,
    WorkPlane,
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
    tool_rows(ui, state);
}

fn tool_rows(ui: &mut Ui, state: &mut AppState) {
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
        Tool::Plane => plane_rows(ui, state),
        Tool::Move => {
            let m = &mut opts.moving;
            caption(ui, "Drag mode");
            let modes = DragMode::ALL.map(|m| (m, m.label()));
            segmented(ui, "move_drag", &mut m.drag_mode, &modes);
            gap(ui);
            ui.add_enabled_ui(m.drag_mode == DragMode::Move, |ui| {
                ui.checkbox(&mut m.overwrite, "Overwrite").on_hover_text(
                    "A moved selection replaces the voxels it lands on; \
                         otherwise the move stops before them",
                );
            });
        }
        Tool::ColorPicker => {
            ui.checkbox(
                &mut opts.color_picker.sample_all_layers,
                "Sample all layers",
            )
            .on_hover_text("Pick the topmost visible voxel instead of the active layer's");
        }
        Tool::Selection => {
            let s = &mut opts.selection;
            mask_mode(ui, "sel_mask", &mut s.mask);
            gap(ui);
            ui.checkbox(&mut s.all_layers, "All layers")
                .on_hover_text("Select the voxels of every visible layer inside the box");
            selection_actions(ui, state);
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
            ui.checkbox(&mut f.all_layers, "All layers")
                .on_hover_text("Flood through the voxels of every visible layer");
            gap(ui);
            mask_mode(ui, "fuzzy_mask", &mut f.mask);
            selection_actions(ui, state);
        }
        Tool::Extrude => {
            let e = &mut opts.extrude;
            caption(ui, "Depth");
            ui.add(
                egui::DragValue::new(&mut e.depth)
                    .speed(0.1)
                    .range(1..=ExtrudeOptions::MAX_DEPTH),
            )
            .on_hover_text("Depth of a click; dragging adds to it");
            gap(ui);
            ui.checkbox(&mut e.confine_to_layer, "Confine to layer")
                .on_hover_text("Find the face region in the active layer only");
            gap(ui);
            color_threshold(ui, &mut e.use_color, &mut e.threshold);
        }
        Tool::Fill => {
            let f = &mut opts.fill;
            let modes = FillMode::ALL.map(|m| (m, m.label()));
            segmented(ui, "fill_mode", &mut f.mode, &modes);
            gap(ui);
            let recolor = f.mode == FillMode::Recolor;
            ui.add_enabled_ui(recolor, |ui| {
                ui.checkbox(&mut f.connected, "Connected").on_hover_text(
                    "Only voxels connected to the clicked one (off: the whole layer)",
                );
            });
            gap(ui);
            ui.checkbox(&mut f.current_layer_only, "Current layer only")
                .on_hover_text("Find the region in the active layer only");
            gap(ui);
            ui.add_enabled_ui(recolor, |ui| {
                caption(ui, "Threshold");
                ui.add(egui::DragValue::new(&mut f.threshold).range(0..=255))
                    .on_hover_text("How close colors must be to be filled");
            });
            gap(ui);
            caption(ui, "Color");
            if let Some(doc) = state.doc() {
                let c = doc.selected_rgba();
                let (rect, response) =
                    ui.allocate_exact_size(egui::vec2(18.0, 18.0), Sense::hover());
                ui.painter()
                    .rect_filled(rect, theme::RADIUS, Color32::from_rgb(c.r, c.g, c.b));
                ui.painter().rect_stroke(
                    rect,
                    theme::RADIUS,
                    Stroke::new(1.0, theme::BORDER),
                    StrokeKind::Inside,
                );
                response.on_hover_text(format!(
                    "Fills with palette color {} ({})",
                    doc.selected_color,
                    c.to_hex_string()
                ));
            }
        }
    }
}

/// Deselect / Delete buttons of the selection tools.
fn selection_actions(ui: &mut Ui, state: &mut AppState) {
    vertical_divider(ui, DIVIDER_HEIGHT);
    let count = state.selected_voxels();
    let mut message = None;
    ui.add_enabled_ui(count > 0, |ui| {
        let Some(doc) = state.doc_mut() else { return };
        if ui
            .button("Deselect")
            .on_hover_text("Clear the active layer's selection (Esc)")
            .clicked()
            && doc.clear_selection()
        {
            message = Some("Selection cleared".to_owned());
        }
        if ui
            .button("Delete")
            .on_hover_text("Delete the selected voxels (Del)")
            .clicked()
        {
            let removed = doc.delete_selected();
            message = Some(match doc.editable_layer(doc.active_layer) {
                Err(e) if removed == 0 => e.message().to_owned(),
                _ => format!("Deleted {removed} selected voxels"),
            });
        }
    });
    if message.is_some() {
        state.status_message = message;
    }
}

/// Plane tool: the document's work plane (Goxel's plane panel, Qubicle's
/// slice mode) plus the destructive Cut.
fn plane_rows(ui: &mut Ui, state: &mut AppState) {
    let mut cap_value = state.options.plane.cap;
    let mut message = None;
    if let Some(doc) = state.doc_mut() {
        let bounds = doc.data.bounds;
        let mut p = doc.plane.clamped(bounds);
        ui.checkbox(&mut p.visible, "Visible")
            .on_hover_text("Show the plane; drawing tools snap onto it");
        gap(ui);
        caption(ui, "Axis");
        let axes = [(0usize, "X"), (1, "Y"), (2, "Z")];
        segmented(ui, "plane_axis", &mut p.axis, &axes);
        gap(ui);
        if p.slice {
            // Qubicle's slice counter: the visible, editable layer.
            caption(ui, "Layer");
            let mut layer = p.slice_layer(bounds);
            if ui.small_button("-").clicked() {
                layer -= 1;
            }
            ui.add(
                egui::DragValue::new(&mut layer)
                    .speed(0.1)
                    .range(0..=bounds.0[p.axis] as i32 - 1),
            )
            .on_hover_text("Slice layer: shown and editable; layers beyond it are hidden");
            if ui.small_button("+").clicked() {
                layer += 1;
            }
            if layer != p.slice_layer(bounds) {
                p.set_slice_layer(layer, bounds);
            }
        } else {
            caption(ui, "Position");
            ui.add(
                egui::DragValue::new(&mut p.coord)
                    .speed(0.1)
                    .range(0..=bounds.0[p.axis] as i32),
            )
            .on_hover_text("Plane coordinate along its axis");
        }
        vertical_divider(ui, DIVIDER_HEIGHT);
        ui.checkbox(&mut p.slice, "Slice")
            .on_hover_text("Hide the model beyond the plane (view only) and edit up to it");
        ui.add_enabled_ui(p.slice, |ui| {
            if ui
                .button("Flip")
                .on_hover_text("Hide the other side instead")
                .clicked()
            {
                p.flip_side(bounds);
            }
        });
        if ui
            .button("Reset")
            .on_hover_text("Hide the plane and turn Slice off")
            .clicked()
        {
            p = WorkPlane::default();
        }
        vertical_divider(ui, DIVIDER_HEIGHT);
        ui.add_enabled_ui(p.slice, |ui| {
            if ui
                .button("Cut")
                .on_hover_text("Delete the hidden side from every visible, unlocked layer")
                .clicked()
            {
                let out = doc.cut_hidden_side(cap_value, doc.selected_color as rizxel_math::Voxel);
                message = Some(if out.layers == 0 {
                    "Cut: nothing on the hidden side".to_owned()
                } else {
                    let mut m = format!(
                        "Cut: removed {} voxels in {} layer(s)",
                        out.removed, out.layers
                    );
                    if out.capped > 0 {
                        m.push_str(&format!(", capped {}", out.capped));
                    }
                    if out.locked_skipped > 0 {
                        m.push_str(&format!(", {} locked layer(s) skipped", out.locked_skipped));
                    }
                    m
                });
            }
            ui.checkbox(&mut cap_value, "Cap")
                .on_hover_text("Close the cut with the current color");
        });
        // Switching axis keeps the plane inside the bounds.
        doc.plane = p.clamped(bounds);
    }
    state.options.plane.cap = cap_value;
    if message.is_some() {
        state.status_message = message;
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

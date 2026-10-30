//! Option bar: history, voxel operations and per-tool parameters.
//!
//! The parameters after the Paint button follow the active tool. Their set
//! mirrors Goxel's tool panels (`src/tools/*.c`). The bar scrolls
//! horizontally when the window is too narrow. The Model/Render switch lives
//! in the title bar.

use egui::{Color32, Sense, Stroke, StrokeKind, Ui};
use rizxel_app::{AppState, ViewMode};
use rizxel_engine::{
    BrushShape, Connectivity, DragMode, ExtrudeOptions, FillMode, LineMode, MaskMode, NamesMode,
    SelectionMode, Symmetry, Tool, VoxelOp, WorkPlane,
};

use crate::icons::{self, Icon, Icons};
use crate::theme;
use crate::widgets::{hscroll_strip, vertical_divider};

const BUTTON_SIZE: f32 = 32.0;
const ICON_SIZE: f32 = 20.0;
const DIVIDER_HEIGHT: f32 = 20.0;

pub fn show(ui: &mut Ui, state: &mut AppState, icons: &Icons) {
    let height = ui.max_rect().height();
    // The strip's arrows sit beside it only in a horizontal layout: in the
    // panel's vertical one an overflowing bar pushed its content below the
    // left arrow, out of view.
    ui.horizontal(|ui| {
        hscroll_strip(ui, "option_bar", height, 0.0, theme::HOVER, |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            if state.view_mode == ViewMode::Render {
                render_bar(ui, state, icons);
            } else {
                common_controls(ui, state, icons);
                vertical_divider(ui, DIVIDER_HEIGHT);
                tool_parameters(ui, state);
            }
            ui.add_space(8.0);
        });
    });
}

/// Render mode's bar: undo/redo, pause, raster or path-traced preview, its
/// resolution, sample progress, output size and projection.
fn render_bar(ui: &mut Ui, state: &mut AppState, icons: &Icons) {
    undo_redo(ui, state, icons);
    vertical_divider(ui, DIVIDER_HEIGHT);

    let view = &mut state.render_view;
    let traced = view.path_traced && view.path_tracing;
    let (glyph, tip) = if view.paused {
        (egui_phosphor::regular::PLAY, "Resume the preview")
    } else {
        (egui_phosphor::regular::PAUSE, "Pause the preview")
    };
    if ui
        .add_enabled(
            traced,
            egui::Button::new(egui::RichText::new(glyph).font(theme::icon(16.0)))
                .min_size(egui::vec2(28.0, 26.0)),
        )
        .on_hover_text(tip)
        .clicked()
    {
        view.paused = !view.paused;
    }
    let mut mode = traced;
    ui.add_enabled_ui(view.path_tracing, |ui| {
        segmented(
            ui,
            "preview_renderer",
            &mut mode,
            &[(false, "Raster"), (true, "Path traced")],
        );
    })
    .response
    .on_disabled_hover_text(
        "The path tracer needs Vulkan or Metal with compute shaders: raster preview only",
    );
    if view.path_tracing {
        view.path_traced = mode;
    }
    let scale_text = |s: f32| match s {
        s if s >= 1.0 => "Full",
        s if s >= 0.5 => "1/2",
        _ => "1/4",
    };
    egui::ComboBox::from_id_salt("preview_scale")
        .selected_text(scale_text(view.scale))
        .width(56.0)
        .show_ui(ui, |ui| {
            for s in [1.0, 0.5, 0.25] {
                ui.selectable_value(&mut view.scale, s, scale_text(s));
            }
        })
        .response
        .on_hover_text("Path-traced preview resolution");

    vertical_divider(ui, DIVIDER_HEIGHT);
    let target = state.doc().map_or(1, |d| d.data.render.sample.samples);
    let done = state.render_view.samples_done.min(target);
    let text = if traced {
        format!("{done} / {target}")
    } else {
        "Raster".to_owned()
    };
    let fraction = if traced {
        done as f32 / target.max(1) as f32
    } else {
        1.0
    };
    ui.add(
        egui::ProgressBar::new(fraction)
            .text(egui::RichText::new(text).size(11.0))
            .desired_width(120.0),
    )
    .on_hover_text("Samples per pixel of the preview");

    vertical_divider(ui, DIVIDER_HEIGHT);
    if let Some(doc) = state.doc_mut() {
        let mut s = doc.data.render.clone();
        ui.label(
            egui::RichText::new("Output")
                .size(12.0)
                .color(theme::TEXT_DIM),
        );
        let max = rizxel_math::render::MAX_OUTPUT_SIDE;
        ui.add(egui::DragValue::new(&mut s.output.width).range(1..=max));
        ui.label("×");
        ui.add(egui::DragValue::new(&mut s.output.height).range(1..=max));
        if s != doc.data.render {
            let merge = ui.input(|i| i.pointer.any_down());
            doc.set_render(s, merge);
        }
    }

    vertical_divider(ui, DIVIDER_HEIGHT);
    super::render::projection_buttons(ui, state);
    help_button(ui, crate::help::RENDER_PAGE, "Render mode");
}

/// `?` button opening a guide page.
fn help_button(ui: &mut Ui, page: &str, topic: &str) {
    let button = egui::Button::new(
        egui::RichText::new(egui_phosphor::regular::QUESTION)
            .font(theme::icon(14.0))
            .color(theme::TEXT_DIM),
    )
    .frame(false);
    if ui
        .add(button)
        .on_hover_text(format!("{topic} in the user guide"))
        .clicked()
    {
        crate::help::open(ui.ctx(), page);
    }
}

fn undo_redo(ui: &mut Ui, state: &mut AppState, icons: &Icons) {
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
}

/// Undo/Redo, Add/Subtract/Join and Paint.
fn common_controls(ui: &mut Ui, state: &mut AppState, icons: &Icons) {
    undo_redo(ui, state, icons);

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
    let tool = state.tool;
    help_button(
        ui,
        &crate::help::tool_page(tool),
        &format!("The {} tool", tool.label()),
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
            gap(ui);
            ui.add_enabled_ui(b.single_voxel(), |ui| {
                connectivity(ui, "brush_connect", &mut b.connect)
                    .on_disabled_hover_text("Line connectivity applies to size 1 strokes");
            });
        }
        Tool::Line => {
            let modes = LineMode::ALL.map(|m| (m, m.label()));
            segmented(ui, "line_mode", &mut opts.line.mode, &modes);
            gap(ui);
            let b = &mut opts.brush;
            connectivity(ui, "line_connect", &mut b.connect);
            gap(ui);
            caption(ui, "Brush");
            shape_combo(ui, "line_shape", &mut b.shape);
            for (axis, value) in ["X", "Y", "Z"].into_iter().zip(b.size.iter_mut()) {
                labeled_drag(ui, axis, value, 1..=rizxel_engine::BrushOptions::MAX_SIZE);
            }
            gap(ui);
            symmetry(ui, "line_sym", &mut b.symmetry);
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
        Tool::Transform => transform_rows(ui, state),
        Tool::ColorPicker => {
            ui.checkbox(
                &mut opts.color_picker.sample_all_layers,
                "Sample all layers",
            )
            .on_hover_text("Pick the topmost visible voxel instead of the active layer's");
        }
        Tool::Selection => {
            let s = &mut opts.selection;
            let modes = SelectionMode::ALL.map(|m| (m, m.label()));
            segmented(ui, "sel_mode", &mut s.mode, &modes);
            gap(ui);
            mask_mode(ui, "sel_mask", &mut s.mask);
            gap(ui);
            ui.checkbox(&mut s.all_layers, "All layers")
                .on_hover_text("Select the voxels of every visible layer inside the box");
            let all_layers = s.all_layers;
            if s.mode == SelectionMode::Box {
                box_rows(ui, state, all_layers);
            }
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

/// Transform tool: typed rotation, scale (or size), move and pivot, live on
/// the open transform (the first change opens one); Apply / Cancel.
fn transform_rows(ui: &mut Ui, state: &mut AppState) {
    use rizxel_engine::tools::{TransformParams, MAX_SCALE, MIN_SCALE};
    let Some(doc) = state.doc() else { return };
    let session = doc.transform_session();
    let target = session
        .map(|s| s.size)
        .or_else(|| doc.transform_target_box().map(|b| b.size()));
    let Some(source_size) = target else {
        caption(ui, "Select voxels, or pick a layer that has some");
        return;
    };
    let open = session.is_some();
    let old = session.map(|s| s.params).unwrap_or_default();
    let mut p = old;
    let opts = &mut state.options.transform;

    caption(ui, "Rotate");
    for axis in 0..3 {
        caption_small(ui, ["X", "Y", "Z"][axis]);
        ui.add(
            egui::DragValue::new(&mut p.rotate[axis])
                .speed(1.0)
                .range(-360.0..=360.0)
                .suffix("°"),
        );
    }
    gap(ui);
    let modes = [(false, "%"), (true, "Size")];
    segmented(ui, "transform_size_mode", &mut opts.size_mode, &modes);
    for axis in 0..3 {
        caption_small(ui, ["X", "Y", "Z"][axis]);
        if opts.size_mode {
            let src = source_size[axis].max(1);
            let mut size = (src as f32 * p.scale[axis]).round() as i32;
            let max = (src as f32 * MAX_SCALE) as i32;
            if ui
                .add(egui::DragValue::new(&mut size).speed(0.1).range(1..=max))
                .changed()
            {
                p.scale[axis] = size as f32 / src as f32;
            }
        } else {
            let mut percent = p.scale[axis] * 100.0;
            if ui
                .add(
                    egui::DragValue::new(&mut percent)
                        .speed(1.0)
                        .range(MIN_SCALE * 100.0..=MAX_SCALE * 100.0)
                        .suffix("%"),
                )
                .changed()
            {
                p.scale[axis] = percent / 100.0;
            }
        }
    }
    gap(ui);
    caption(ui, "Move");
    for axis in 0..3 {
        caption_small(ui, ["X", "Y", "Z"][axis]);
        ui.add(egui::DragValue::new(&mut p.offset[axis]).speed(0.1));
    }
    gap(ui);
    caption(ui, "Pivot");
    let old_pivot = (opts.pivot, opts.pivot_cell);
    egui::ComboBox::from_id_salt("transform_pivot")
        .width(90.0)
        .selected_text(opts.pivot.label())
        .show_ui(ui, |ui| {
            for pv in rizxel_engine::Pivot::ALL {
                ui.selectable_value(&mut opts.pivot, pv, pv.label());
            }
        })
        .response
        .on_hover_text("Cursor: click a voxel off the gizmo to place it");
    if opts.pivot == rizxel_engine::Pivot::Cursor {
        for axis in 0..3 {
            caption_small(ui, ["X", "Y", "Z"][axis]);
            ui.add(egui::DragValue::new(&mut opts.pivot_cell[axis]).speed(0.1));
        }
    }
    gap(ui);
    ui.checkbox(&mut opts.clip_to_bounds, "Clip to bounds")
        .on_hover_text("Drop the voxels that end up outside the bounds instead of refusing Apply");
    let pivot_changed = (opts.pivot, opts.pivot_cell) != old_pivot;
    vertical_divider(ui, DIVIDER_HEIGHT);
    let mut apply = false;
    let mut cancel = false;
    ui.add_enabled_ui(open, |ui| {
        apply = ui
            .button("Apply")
            .on_hover_text("Apply the transform (Enter)")
            .clicked();
        cancel = ui
            .button("Cancel")
            .on_hover_text("Restore the voxels (Esc)")
            .clicked();
    });
    if ui
        .button("Reset")
        .on_hover_text("Back to no rotation, 100% and no move")
        .clicked()
    {
        p = TransformParams::default();
    }
    p.scale = p
        .scale
        .clamp(glam::Vec3::splat(MIN_SCALE), glam::Vec3::splat(MAX_SCALE));
    if p != old || (open && pivot_changed) {
        crate::tool_input::set_transform_params(state, p);
    }
    if apply {
        crate::tool_input::apply_transform(state);
    } else if cancel {
        if let Some(doc) = state.doc_mut() {
            doc.cancel_transform();
        }
        state.status_message = Some("Transform cancelled".to_owned());
    }
}

fn caption_small(ui: &mut Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(11.0)
            .color(theme::TEXT_MUTED),
    );
}

/// Box mode: position (min corner) and size of the open box, typed values
/// move or resize it live; Apply / Cancel.
fn box_rows(ui: &mut Ui, state: &mut AppState, all_layers: bool) {
    let mask = state.options.selection.mask;
    let Some(doc) = state.doc_mut() else { return };
    doc.set_box_all_layers(all_layers);
    vertical_divider(ui, DIVIDER_HEIGHT);
    let Some(area) = doc.box_select().map(|b| b.area) else {
        caption(ui, "Click a voxel to place a box");
        return;
    };
    let limit = doc.box_limit();
    let (mut min, mut size) = (area.min, area.size());
    caption(ui, "Position");
    for axis in 0..3 {
        axis_drag(
            ui,
            ["X", "Y", "Z"][axis],
            &mut min[axis],
            0..=limit.max[axis] - 1,
        );
    }
    gap(ui);
    caption(ui, "Size");
    for axis in 0..3 {
        axis_drag(
            ui,
            ["X", "Y", "Z"][axis],
            &mut size[axis],
            1..=limit.size()[axis].max(1),
        );
    }
    if (min, size) != (area.min, area.size()) {
        doc.set_box_area(
            rizxel_math::VoxelBox {
                min,
                max: min + size,
            },
            false,
        );
    }
    gap(ui);
    let mut message = None;
    if ui
        .button("Apply")
        .on_hover_text("Select the voxels in the box (Enter)")
        .clicked()
    {
        doc.set_box_mode(mask);
        doc.apply_box_select();
        message = Some(format!(
            "Selected: {} voxels",
            doc.selection().map_or(0, |s| s.len())
        ));
    }
    if ui
        .button("Cancel")
        .on_hover_text("Close the box and keep the selection (Esc)")
        .clicked()
    {
        doc.cancel_box_select();
        message = Some("Box selection cancelled".to_owned());
    }
    if message.is_some() {
        state.status_message = message;
    }
}

fn axis_drag(ui: &mut Ui, label: &str, value: &mut i32, range: std::ops::RangeInclusive<i32>) {
    ui.label(
        egui::RichText::new(label)
            .size(11.0)
            .color(theme::TEXT_MUTED),
    );
    ui.add(egui::DragValue::new(value).speed(0.1).range(range));
}

/// Deselect / Delete / To New Layer buttons of the selection tools.
fn selection_actions(ui: &mut Ui, state: &mut AppState) {
    vertical_divider(ui, DIVIDER_HEIGHT);
    let count = state.selected_voxels();
    let mut to_layer = None;
    ui.add_enabled_ui(count > 0, |ui| {
        if ui
            .button("Cut to Layer")
            .on_hover_text("Move the selected voxels into a new layer above (Ctrl+Shift+J)")
            .clicked()
        {
            to_layer = Some(true);
        }
        if ui
            .button("Copy to Layer")
            .on_hover_text("Copy the selected voxels into a new layer above (Ctrl+J)")
            .clicked()
        {
            to_layer = Some(false);
        }
    });
    if let Some(cut) = to_layer {
        state.selection_to_new_layer(cut);
    }
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

/// Plane tool: the document's work plane (Goxel's plane panel), the Slice
/// Mode toggle (also `Space` and the Tools panel's `2D` button) and the
/// destructive Cut. The hidden side follows the camera, so there is no
/// Flip.
fn plane_rows(ui: &mut Ui, state: &mut AppState) {
    let mut cap_value = state.options.plane.cap;
    let mut message = None;
    if let Some(doc) = state.doc_mut() {
        let bounds = doc.data.bounds;
        let mut p = doc.plane.clamped(bounds);
        let mut visible = p.visible;
        if ui
            .checkbox(&mut visible, "Visible")
            .on_hover_text(
                "Show the plane (/); drawing tools snap onto it. Hiding it ends Slice Mode                  until it is shown again",
            )
            .changed()
        {
            p.set_visible(visible, bounds);
        }
        gap(ui);
        caption(ui, "Axis");
        let axes = [(0usize, "X"), (1, "Y"), (2, "Z")];
        segmented(ui, "plane_axis", &mut p.axis, &axes);
        gap(ui);
        if p.slice {
            // Qubicle's slice counter (1-based): the only editable layer.
            caption(ui, "Slice");
            let mut number = p.slice_layer(bounds) + 1;
            if ui.small_button("-").clicked() {
                number -= 1;
            }
            ui.add(
                egui::DragValue::new(&mut number)
                    .speed(0.1)
                    .range(1..=p.slice_count(bounds))
                    .suffix(format!(" / {}", p.slice_count(bounds))),
            )
            .on_hover_text("Slice: the only editable layer; the camera's side is hidden");
            if ui.small_button("+").clicked() {
                number += 1;
            }
            if number != p.slice_layer(bounds) + 1 {
                p.set_slice_layer(number - 1, bounds);
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
        let mut slicing = p.slice;
        if ui
            .checkbox(&mut slicing, "Slice")
            .on_hover_text(
                "Slice Mode (Space): edit one layer; the camera's side is hidden (view only)",
            )
            .changed()
        {
            p.toggle_slice(None, bounds);
        }
        if ui
            .button("Reset")
            .on_hover_text("Hide the plane and turn Slice off")
            .clicked()
        {
            p = WorkPlane {
                last_slice: p.last_slice,
                ..WorkPlane::default()
            };
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
                    m.push_str(". Fit shrinks the bounds to what is left");
                    m
                });
            }
            ui.checkbox(&mut cap_value, "Cap")
                .on_hover_text("Close the cut with the current color");
        });
        if ui
            .button("Fit")
            .on_hover_text("Fit Bounds to Content: move the model to the origin and shrink the bounds around it")
            .clicked()
        {
            message = Some(match doc.fit_bounds_to_content() {
                Ok(b) => format!("Fit Bounds: {}×{}×{}", b.x(), b.y(), b.z()),
                Err(e) => format!("Fit Bounds: {}", e.message()),
            });
        }
        // Switching axis keeps the plane inside the (maybe fitted) bounds.
        doc.plane = p.clamped(doc.data.bounds);
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

fn connectivity(ui: &mut Ui, id: &str, value: &mut Connectivity) -> egui::Response {
    egui::ComboBox::from_id_salt(id)
        .width(96.0)
        .selected_text(value.label())
        .show_ui(ui, |ui| {
            for c in Connectivity::ALL {
                ui.selectable_value(value, c, c.label());
            }
        })
        .response
        .on_hover_text(
            "Pixel perfect: no L-corners. Solid: every step along one axis, no diagonal gaps",
        )
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

pub(crate) fn segmented_width<T>(ui: &Ui, items: &[(T, &str)]) -> f32 {
    segment_widths(ui, items).0.iter().sum()
}

/// Segmented control sized to its labels (`px-3 py-1` in the HTML), used for
/// Model/Render and the single-choice tool options.
pub(crate) fn segmented<T: PartialEq + Copy>(
    ui: &mut Ui,
    id: &str,
    value: &mut T,
    items: &[(T, &str)],
) {
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

/// [`segmented`] whose segments share the available width (Render panel tabs).
pub(crate) fn segmented_fill<T: PartialEq + Copy>(
    ui: &mut Ui,
    id: &str,
    value: &mut T,
    items: &[(T, &str)],
) {
    let labels: Vec<&str> = items.iter().map(|(_, l)| *l).collect();
    let active: Vec<bool> = items.iter().map(|(item, _)| item == value).collect();
    let width = ui.available_width() / items.len().max(1) as f32;
    if let Some(i) = segments_sized(ui, id, &labels, &active, Some(width)) {
        *value = items[i].0;
    }
}

/// Paints a row of joined segments and returns the clicked index.
fn segments(ui: &mut Ui, id: &str, labels: &[&str], active: &[bool]) -> Option<usize> {
    segments_sized(ui, id, labels, active, None)
}

/// [`segments`] with every segment `width` wide (`None`: sized to its label).
fn segments_sized(
    ui: &mut Ui,
    id: &str,
    labels: &[&str],
    active: &[bool],
    width: Option<f32>,
) -> Option<usize> {
    let items: Vec<((), &str)> = labels.iter().map(|l| ((), *l)).collect();
    let (mut widths, height) = segment_widths(ui, &items);
    if let Some(w) = width {
        widths.iter_mut().for_each(|x| *x = w);
    }
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

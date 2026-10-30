//! Title bar: app icon, menus, centered title, the Model/Render switch and
//! window controls.
//! Doubles as the drag handle for the frameless window.

use egui::{Align2, Color32, Id, Sense, Ui, ViewportCommand};
use egui_phosphor::regular as icon;
use rizxel_app::{
    ActionId, AppState, CloseAction, CloseChoice, CloseStep, CoreOp, FileFormat, SliceStep,
    ViewMode,
};
use rizxel_engine::tools::BooleanOp;

use super::{options, UiState};
use crate::dock::{self, Workspace};
use crate::widgets::{icon_button, icon_label, IconStyle};
use crate::{actions, theme};

use ActionId as A;

const CONTROL_WIDTH: f32 = 46.0;
/// Shown in *Help ▸ About Rizxel*.
const AUTHOR_NAME: &str = "Muhammad Rizky Abdillah";
const AUTHOR_HANDLE: &str = "MRizky099";
const REPO_URL: &str = "https://github.com/MRizky099/Rizxel";

pub fn show(ui: &mut Ui, state: &mut AppState, ui_state: &mut UiState, dock: &mut Workspace) {
    let bar = ui.max_rect();

    // Background interaction: drag to move, double-click to maximize. Widgets
    // added afterwards take priority over this area.
    let drag = ui.interact(bar, Id::new("title_bar_drag"), Sense::click_and_drag());
    if drag.drag_started_by(egui::PointerButton::Primary) {
        ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
    }
    if drag.double_clicked() {
        toggle_maximize(ui.ctx());
    }

    let title = match state.doc() {
        Some(d) if d.is_dirty() => format!("{} \u{2022} Rizxel - Voxel Editor", d.title()),
        Some(d) => format!("{} - Rizxel - Voxel Editor", d.title()),
        None => "Rizxel - Voxel Editor".to_owned(),
    };
    let font = theme::regular(12.0);
    let title_width = ui
        .painter()
        .layout_no_wrap(title.clone(), font.clone(), theme::TEXT_MUTED)
        .size()
        .x;
    let menus_id = Id::new("title_bar_menus_width");
    let menus_width = ui.data(|d| d.get_temp(menus_id)).unwrap_or(MENUS_ESTIMATE);
    let layout = bar_layout(
        bar.width(),
        menus_width,
        title_width,
        options::segmented_width(ui, &VIEW_MODES),
    );
    let span = |(left, right): (f32, f32)| {
        egui::Rect::from_x_y_ranges(bar.left() + left..=bar.left() + right, bar.y_range())
    };

    // Title (painted, not interactive), centered on the bar while it fits,
    // otherwise in the free space between the menus and the mode switch.
    if let Some(range) = layout.title {
        let rect = span(range);
        let mut job = egui::text::LayoutJob::single_section(
            title,
            egui::TextFormat::simple(font, theme::TEXT_MUTED),
        );
        job.wrap = egui::text::TextWrapping::truncate_at_width(rect.width());
        let galley = ui.fonts_mut(|f| f.layout_job(job));
        let pos = Align2::CENTER_CENTER
            .align_size_within_rect(galley.size(), rect)
            .min;
        ui.painter()
            .with_clip_rect(rect)
            .galley(pos, galley, theme::TEXT_MUTED);
    }

    // App icon.
    let icon_rect = span((0.0, ICON_WIDTH));
    crate::widgets::paint_icon(ui, icon_rect, icon::CUBE, 15.0, theme::ACCENT);

    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(span(layout.menus))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    child.spacing_mut().item_spacing.x = 0.0;
    menu_style(&mut child);
    if layout.collapsed {
        child.menu_button(
            egui::RichText::new(icon::LIST).font(theme::icon(16.0)),
            |ui| menu_entries(ui, state, ui_state, dock),
        );
    } else {
        let width = egui::MenuBar::new()
            .ui(&mut child, |ui| menu_entries(ui, state, ui_state, dock))
            .response
            .rect
            .width();
        ui.data_mut(|d| d.insert_temp(menus_id, width));
    }

    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(span(layout.switch))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    let mut mode = state.view_mode;
    options::segmented(&mut child, "view_mode", &mut mode, &VIEW_MODES);
    state.set_view_mode(mode);
    child
        .interact(child.min_rect(), Id::new("view_mode_hint"), Sense::hover())
        .on_hover_text("View mode: Model for editing, Render for the lit preview");

    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(span(layout.controls))
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    child.spacing_mut().item_spacing.x = 0.0;
    window_controls(&mut child, bar.height());
}

const VIEW_MODES: [(ViewMode, &str); 2] =
    [(ViewMode::Model, "Model"), (ViewMode::Render, "Render")];
const ICON_WIDTH: f32 = 32.0;
const CONTROLS_WIDTH: f32 = CONTROL_WIDTH * 3.0;
/// Space between the mode switch and its neighbours.
const SWITCH_GAP: f32 = 8.0;
/// Width of the ☰ button that replaces the menus in narrow windows.
const COLLAPSED_MENUS_WIDTH: f32 = 36.0;
/// Narrower than this, the title is hidden rather than squeezed.
const MIN_TITLE_WIDTH: f32 = 48.0;
/// Menu bar width before it has been measured once.
const MENUS_ESTIMATE: f32 = 420.0;

/// Horizontal spans of the title bar parts, as `(left, right)` offsets from
/// the bar's left edge.
#[derive(Debug, Clone, Copy, PartialEq)]
struct BarLayout {
    menus: (f32, f32),
    /// Menus folded into one ☰ button.
    collapsed: bool,
    title: Option<(f32, f32)>,
    switch: (f32, f32),
    controls: (f32, f32),
}

/// Lays the title bar out right to left: window controls at the far edge,
/// the mode switch beside them, then the menus and title share the rest. In
/// narrow bars the title shrinks first, then the menus fold into ☰.
fn bar_layout(width: f32, menus: f32, title: f32, switch: f32) -> BarLayout {
    let width = width.max(0.0);
    let controls = ((width - CONTROLS_WIDTH).max(0.0), width);
    let switch_right = (controls.0 - SWITCH_GAP).max(0.0);
    let switch = ((switch_right - switch).max(0.0), switch_right);
    let free_right = (switch.0 - SWITCH_GAP).max(ICON_WIDTH);
    let collapsed = ICON_WIDTH + menus > free_right;
    let menus_right = (ICON_WIDTH
        + if collapsed {
            COLLAPSED_MENUS_WIDTH
        } else {
            menus
        })
    .min(free_right);
    let room = free_right - menus_right;
    let title = if room < MIN_TITLE_WIDTH {
        None
    } else {
        let centered = (width - title) / 2.0;
        Some(
            if centered >= menus_right && centered + title <= free_right {
                (centered, centered + title)
            } else if title <= room {
                let left = menus_right + (room - title) / 2.0;
                (left, left + title)
            } else {
                (menus_right, free_right)
            },
        )
    };
    BarLayout {
        menus: (ICON_WIDTH, menus_right),
        collapsed,
        title,
        switch,
        controls,
    }
}

fn menu_style(ui: &mut Ui) {
    ui.style_mut().spacing.button_padding = egui::vec2(10.0, 4.0);
    ui.style_mut()
        .text_styles
        .insert(egui::TextStyle::Button, theme::regular(12.0));
}

/// The top-level menus, laid out in the menu bar or, when the bar is narrow,
/// as submenus of the ☰ button.
fn menu_entries(ui: &mut Ui, state: &mut AppState, ui_state: &mut UiState, dock: &mut Workspace) {
    ui.menu_button("File", |ui| {
        ui.set_min_width(220.0);
        if menu_item(ui, icon::FILE_PLUS, "New", &sc(state, A::New), true) {
            actions::new_document(state);
        }
        if menu_item(ui, icon::FOLDER_OPEN, "Open...", &sc(state, A::Open), true) {
            actions::open(state);
        }
        recent_menu(ui, state);
        ui.separator();
        if menu_item(ui, icon::FLOPPY_DISK, "Save", &sc(state, A::Save), true) {
            actions::save(state);
        }
        if menu_item(
            ui,
            icon::FLOPPY_DISK_BACK,
            "Save As...",
            &sc(state, A::SaveAs),
            true,
        ) {
            actions::save_as(state);
        }
        ui.separator();
        ui.menu_button(
            icon_label(ui, icon::DOWNLOAD_SIMPLE, "Import", None),
            |ui| {
                ui.set_min_width(240.0);
                for format in FileFormat::IMPORTS {
                    let label = format!("{} (.{})...", format.label(), format.extension());
                    let hint = match format {
                        FileFormat::Mtl => {
                            "Replaces the palette and materials of the active document"
                        }
                        FileFormat::Obj | FileFormat::Stl | FileFormat::Ply => {
                            "Voxelizer: size, rotation, then a new tab"
                        }
                        _ => "Opens in a new tab",
                    };
                    if menu_item(ui, icon::FILE_ARROW_DOWN, &label, "", true) {
                        actions::import(state, &mut ui_state.dialogs, format);
                    }
                    ui.label(egui::RichText::new(hint).size(10.0).color(theme::TEXT_DIM));
                }
                ui.separator();
                if menu_item(ui, icon::MOUNTAINS, "Heightmap (.png, .jpg)...", "", true) {
                    actions::import_heightmap(&mut ui_state.dialogs);
                }
                ui.label(
                    egui::RichText::new("Grayscale image to terrain, in a new tab")
                        .size(10.0)
                        .color(theme::TEXT_DIM),
                );
            },
        );
        ui.menu_button(icon_label(ui, icon::EXPORT, "Export", None), |ui| {
            ui.set_min_width(260.0);
            for format in FileFormat::EXPORTS {
                let label = format!("{} (.{})...", format.label(), format.extension());
                if menu_item(
                    ui,
                    icon::FILE_ARROW_UP,
                    &label,
                    "",
                    state.can_export(format),
                ) {
                    actions::export(state, &mut ui_state.dialogs, format);
                }
                ui.label(
                    egui::RichText::new(format.export_note())
                        .size(10.0)
                        .color(theme::TEXT_DIM),
                );
            }
            ui.separator();
            if menu_item(
                ui,
                icon::STACK,
                "Slices (PNG)...",
                "",
                state.can_export(FileFormat::Obj),
            ) {
                ui_state.dialogs.slices = true;
            }
            ui.label(
                egui::RichText::new("one image per voxel slice, into a folder")
                    .size(10.0)
                    .color(theme::TEXT_DIM),
            );
            ui.separator();
            if menu_item(
                ui,
                icon::CUBE,
                "Shaped Mesh...",
                "",
                state.can_export_shaped(),
            ) {
                ui_state.render.export_shape = true;
            }
            ui.label(
                egui::RichText::new(
                    "Render mode: the model as bricks, spheres, ... (OBJ, STL, GLB)",
                )
                .size(10.0)
                .color(theme::TEXT_DIM),
            );
        });
        ui.separator();
        if menu_item(ui, icon::GEAR, "Settings...", &sc(state, A::Settings), true) {
            ui_state.open_settings(state);
        }
        ui.separator();
        // Goes through the app's close request, which asks about
        // unsaved documents first.
        if menu_item(ui, icon::SIGN_OUT, "Exit", "Alt+F4", true) {
            ui.ctx().send_viewport_cmd(ViewportCommand::Close);
        }
    });

    ui.menu_button("Edit", |ui| edit_menu(ui, state));
    ui.menu_button("Selection", |ui| selection_menu(ui, state));
    ui.menu_button("Layer", |ui| layer_menu(ui, state));
    ui.menu_button("Model", |ui| model_menu(ui, state));

    ui.menu_button("View", |ui| {
        ui.set_min_width(220.0);

        if menu_item(
            ui,
            icon::CUBE_FOCUS,
            "Zen Mode",
            &sc(state, A::ZenMode),
            true,
        ) {
            ui_state.zen_mode = !ui_state.zen_mode;
        }
        slice_menu(ui, state);
        show_menu(ui, state);
        ui.separator();

        // Standard views: each family of targets is its own submenu.
        ui.menu_button(icon_label(ui, icon::SQUARE, "Faces", None), |ui| {
            ui.set_min_width(160.0);
            for face in rizxel_app::ViewFace::ALL {
                if menu_item(ui, icon::SQUARE, face.label(), "", true) {
                    state.snap_view(rizxel_app::ViewTarget::Face(face));
                }
            }
        });

        ui.menu_button(icon_label(ui, icon::LINE_SEGMENT, "Edges", None), |ui| {
            ui.set_min_width(160.0);
            for edge in rizxel_app::ViewEdge::ALL {
                if menu_item(ui, icon::LINE_SEGMENT, edge.label(), "", true) {
                    state.snap_view(rizxel_app::ViewTarget::Edge(edge));
                }
            }
        });

        ui.menu_button(icon_label(ui, icon::CUBE, "Corners", None), |ui| {
            ui.set_min_width(160.0);
            for corner in rizxel_app::ViewCorner::ALL {
                if menu_item(ui, icon::CUBE, corner.label(), "", true) {
                    state.snap_view(rizxel_app::ViewTarget::Corner(corner));
                }
            }
        });

        ui.separator();
        camera_items(ui, state);

        ui.separator();
        let refs = state.doc().map_or((0, false), |d| {
            let r = &d.data.references;
            (r.len(), r.iter().any(|r| r.visible))
        });
        if menu_item(
            ui,
            check_glyph(refs.1),
            "Reference Images",
            &sc(state, A::References),
            refs.0 > 0,
        ) {
            state.toggle_all_references();
        }
        panels_menu(ui, dock);
    });

    ui.menu_button("Help", |ui| {
        ui.set_min_width(200.0);
        if menu_item(ui, icon::KEYBOARD, "Keyboard Shortcuts", "", true) {
            ui_state.show_shortcuts = true;
        }
        if menu_item(ui, icon::INFO, "About Rizxel", "", true) {
            ui_state.show_about = true;
        }
    });
}

/// View menu: projection and the camera framing commands.
fn camera_items(ui: &mut Ui, state: &mut AppState) {
    let has_doc = state.doc().is_some();
    let ortho = state.camera.is_ortho();
    if menu_item(
        ui,
        check_glyph(ortho),
        "Orthographic",
        &sc(state, A::Projection),
        true,
    ) {
        state.camera.toggle_projection();
    }
    if menu_item(
        ui,
        check_glyph(state.view.auto_ortho),
        "Auto Ortho on Face Snap",
        "",
        true,
    ) {
        state.view.auto_ortho = !state.view.auto_ortho;
    }
    ui.separator();
    if menu_item(
        ui,
        icon::FRAME_CORNERS,
        "Frame Selection",
        &sc(state, A::FrameSelection),
        has_doc,
    ) {
        state.frame_selection();
    }
    if menu_item(
        ui,
        icon::BOUNDING_BOX,
        "Frame Work Area",
        &sc(state, A::FrameWorkArea),
        has_doc,
    ) {
        state.frame_work_area();
    }
    if menu_item(ui, icon::HOUSE, "Home View", &sc(state, A::HomeView), true) {
        state.camera.reset();
    }
}

/// View > Show: what the viewport draws besides the model (also on the
/// viewport's toggle buttons).
fn show_menu(ui: &mut Ui, state: &mut AppState) {
    ui.menu_button(icon_label(ui, icon::EYE, "Show", None), |ui| {
        ui.set_min_width(200.0);
        let keys = [A::FloorGrid, A::VoxelGrid, A::ModelBounds].map(|a| sc(state, a));
        let view = &mut state.view;
        for ((on, label), shortcut) in [
            (&mut view.floor_grid, "Floor Grid"),
            (&mut view.voxel_grid, "Voxel Grid"),
            (&mut view.model_bounds, "Model Bounds"),
        ]
        .into_iter()
        .zip(&keys)
        {
            if menu_item(ui, check_glyph(*on), label, shortcut, true) {
                *on = !*on;
            }
        }
    });
    let ghosts = state.view.ghosts;
    if menu_item(
        ui,
        check_glyph(ghosts),
        "Ghosts",
        &sc(state, A::Ghosts),
        true,
    ) {
        state.view.ghosts = !ghosts;
    }
    // Per view mode: on in Render, off in Model by default.
    let mode = state.view_mode;
    let label = match mode {
        rizxel_app::ViewMode::Model => "Ambient Occlusion (Model)",
        rizxel_app::ViewMode::Render => "Ambient Occlusion (Render)",
    };
    if menu_item(ui, check_glyph(state.view.ao(mode)), label, "", true) {
        state.view.toggle_ao(mode);
    }
    let render = mode == rizxel_app::ViewMode::Render;
    if menu_item(
        ui,
        check_glyph(render),
        "Render Mode",
        &sc(state, A::RenderMode),
        true,
    ) {
        state.toggle_view_mode();
    }
}

/// Leading glyph of a menu toggle.
fn check_glyph(on: bool) -> &'static str {
    if on {
        icon::CHECK_SQUARE
    } else {
        icon::SQUARE
    }
}

/// Edit menu: history, clipboard, selection, layer and transform commands.
/// Every item shows its shortcut and is disabled when it can't apply.
fn edit_menu(ui: &mut Ui, state: &mut AppState) {
    ui.set_min_width(240.0);
    let (can_undo, can_redo) = (state.can_undo(), state.can_redo());
    let has_voxels = state.active_layer_has_voxels();
    let editable = state.active_layer_editable();
    let selected = state.selected_voxels() > 0;
    let clip = state.clipboard.is_some();
    let has_doc = state.doc().is_some();

    if menu_item(
        ui,
        icon::ARROW_COUNTER_CLOCKWISE,
        "Undo",
        &sc(state, A::Undo),
        can_undo,
    ) {
        state.undo();
    }
    if menu_item(
        ui,
        icon::ARROW_CLOCKWISE,
        "Redo",
        &sc(state, A::Redo),
        can_redo,
    ) {
        state.redo();
    }
    ui.separator();
    if menu_item(
        ui,
        icon::SCISSORS,
        "Cut",
        &sc(state, A::Cut),
        has_voxels && editable,
    ) {
        state.cut();
    }
    if menu_item(ui, icon::COPY, "Copy", &sc(state, A::Copy), has_voxels) {
        state.copy();
    }
    if menu_item(
        ui,
        icon::CLIPBOARD,
        "Paste",
        &sc(state, A::Paste),
        clip && editable,
    ) {
        let at = state.cursor_voxel;
        state.paste(at);
    }
    if menu_item(
        ui,
        icon::CLIPBOARD_TEXT,
        "Paste in Place",
        &sc(state, A::PasteInPlace),
        clip && editable,
    ) {
        state.paste_in_place();
    }
    if menu_item(
        ui,
        icon::STACK_PLUS,
        "Paste to New Layer",
        &sc(state, A::PasteToNewLayer),
        clip && has_doc,
    ) {
        state.paste_to_new_layer();
    }
    if menu_item(
        ui,
        icon::TRASH,
        "Delete",
        &sc(state, A::Delete),
        selected && editable,
    ) {
        state.delete_selected();
    }
    ui.separator();
    if menu_item(ui, icon::STACK_PLUS, "New Layer", "", has_doc) {
        if let Some(d) = state.doc_mut() {
            d.add_layer();
        }
    }
    if menu_item(ui, icon::STACK_SIMPLE, "Duplicate Layer", "", has_doc) {
        if let Some(d) = state.doc_mut() {
            d.duplicate_active_layer();
        }
    }
    ui.separator();
    let can_transform = has_voxels && editable;
    ui.add_enabled_ui(can_transform, |ui| transform_menus(ui, state));
}

/// Selection menu: build and refine the active layer's selection, and Mask
/// Mode, which protects it from every tool.
fn selection_menu(ui: &mut Ui, state: &mut AppState) {
    ui.set_min_width(240.0);
    let has_voxels = state.active_layer_has_voxels();
    let selected = state.selected_voxels() > 0;
    if menu_item(
        ui,
        icon::SELECTION_ALL,
        "Select All",
        &sc(state, A::SelectAll),
        has_voxels,
    ) {
        state.select_all();
    }
    if menu_item(
        ui,
        icon::SELECTION_SLASH,
        "Deselect",
        &sc(state, A::Deselect),
        selected,
    ) {
        state.deselect();
    }
    if menu_item(
        ui,
        icon::SELECTION_INVERSE,
        "Invert",
        &sc(state, A::InvertSelection),
        has_voxels,
    ) {
        state.invert_selection();
    }
    ui.separator();
    if menu_item(ui, icon::ARROWS_OUT, "Grow", "", selected) {
        state.grow_selection(1);
    }
    if menu_item(ui, icon::ARROWS_IN, "Shrink", "", selected) {
        state.shrink_selection(1);
    }
    if menu_item(ui, icon::PALETTE, "By Color", "", has_voxels) {
        state.select_by_color();
    }
    ui.label(
        egui::RichText::new("By Color: the selected swatch, within the Fuzzy Select threshold")
            .size(10.0)
            .color(theme::TEXT_DIM),
    );
    ui.separator();
    if menu_item(
        ui,
        icon::SCISSORS,
        "To New Layer (Cut)",
        &sc(state, A::SelectionToLayerCut),
        selected,
    ) {
        state.selection_to_new_layer(true);
    }
    if menu_item(
        ui,
        icon::STACK_PLUS,
        "To New Layer (Copy)",
        &sc(state, A::SelectionToLayer),
        selected,
    ) {
        state.selection_to_new_layer(false);
    }
    ui.separator();
    let mask = state.options.mask_selection;
    if menu_item(
        ui,
        check_glyph(mask),
        "Mask Mode",
        &sc(state, A::MaskMode),
        true,
    ) {
        state.toggle_mask_selection();
    }
}

/// Layer menu: structure operations on the active layer.
fn layer_menu(ui: &mut Ui, state: &mut AppState) {
    ui.set_min_width(260.0);
    let has_voxels = state.active_layer_has_voxels();
    let editable = has_voxels && state.active_layer_editable();
    let has_doc = state.doc().is_some();
    let in_group = state.doc().is_some_and(|d| {
        d.active_layer()
            .is_some_and(|l| l.kind().is_group() || l.props.parent.is_some())
    });
    if menu_item(ui, icon::FOLDER_PLUS, "New Group", "", has_doc) {
        if let Some(d) = state.doc_mut() {
            d.add_group();
        }
    }
    if menu_item(
        ui,
        icon::FOLDER_SIMPLE_PLUS,
        "Group Layer",
        &sc(state, A::Group),
        has_doc,
    ) {
        state.group_active_layer();
    }
    if menu_item(
        ui,
        icon::FOLDER_MINUS,
        "Ungroup",
        &sc(state, A::Ungroup),
        in_group,
    ) {
        state.ungroup_active_layer();
    }
    ui.label(
        egui::RichText::new("Drag rows in the Layers panel into and out of groups")
            .size(10.0)
            .color(theme::TEXT_DIM),
    );
    ui.separator();
    if menu_item(ui, icon::PUZZLE_PIECE, "Separate Parts", "", editable) {
        state.separate_parts();
    }
    ui.horizontal(|ui| {
        ui.add_space(28.0);
        ui.label(
            egui::RichText::new("Keep parts under")
                .size(11.0)
                .color(theme::TEXT_DIM),
        );
        ui.add(
            egui::DragValue::new(&mut state.structure.min_part_voxels)
                .range(1..=1_000_000)
                .suffix(" vox"),
        )
        .on_hover_text("Parts smaller than this stay in the layer");
    });
    if menu_item(
        ui,
        icon::SQUARE_SPLIT_VERTICAL,
        "Split at Work Plane",
        "",
        editable,
    ) {
        state.split_at_plane();
    }
    ui.label(
        egui::RichText::new("Voxels on the plane's + side move to a new layer")
            .size(10.0)
            .color(theme::TEXT_DIM),
    );
    ui.separator();
    // Boolean ▸ op ▸ other layer.
    let others: Vec<(usize, String)> = state
        .doc()
        .map(|d| {
            d.data
                .layers
                .iter()
                .enumerate()
                .filter(|(i, l)| *i != d.active_layer && !l.kind().is_group())
                .map(|(i, l)| (i, l.name.clone()))
                .rev()
                .collect()
        })
        .unwrap_or_default();
    ui.add_enabled_ui(state.active_layer_editable() && !others.is_empty(), |ui| {
        ui.menu_button(icon_label(ui, icon::INTERSECT, "Boolean", None), |ui| {
            ui.set_min_width(200.0);
            for op in BooleanOp::ALL {
                ui.menu_button(op.label(), |ui| {
                    ui.label(
                        egui::RichText::new("Active layer with:")
                            .size(10.0)
                            .color(theme::TEXT_DIM),
                    );
                    for (index, name) in &others {
                        if ui.button(name).clicked() {
                            state.boolean(*index, op);
                            ui.close();
                        }
                    }
                });
            }
        });
    });
}

/// Model menu: bounds and core operations.
fn model_menu(ui: &mut Ui, state: &mut AppState) {
    ui.set_min_width(260.0);
    let has_doc = state.doc().is_some();
    let editable = state.active_layer_has_voxels() && state.active_layer_editable();
    if menu_item(ui, icon::BOUNDING_BOX, "Fit Bounds to Content", "", has_doc) {
        state.fit_bounds();
    }
    let fit = &mut state.structure.fit_on_import;
    if menu_item(ui, check_glyph(*fit), "Fit Bounds on Import", "", true) {
        *fit = !*fit;
    }
    ui.separator();
    ui.menu_button(icon_label(ui, icon::CUBE_TRANSPARENT, "Core", None), |ui| {
        ui.set_min_width(240.0);
        if menu_item(ui, icon::PAINT_BUCKET, "Fill Hollows", "", editable) {
            state.core_op(CoreOp::FillHollows);
        }
        ui.label(
            egui::RichText::new("Closed cavities get the selected color")
                .size(10.0)
                .color(theme::TEXT_DIM),
        );
        if menu_item(ui, icon::CUBE_TRANSPARENT, "Hollow", "", editable) {
            state.core_op(CoreOp::Hollow);
        }
        let thickness = state.structure.hollow_thickness;
        if menu_item(ui, icon::CUBE_TRANSPARENT, "Advanced Hollow", "", editable) {
            state.core_op(CoreOp::HollowAdvanced(thickness));
        }
        ui.horizontal(|ui| {
            ui.add_space(28.0);
            ui.label(
                egui::RichText::new("Shell")
                    .size(11.0)
                    .color(theme::TEXT_DIM),
            );
            ui.add(
                egui::DragValue::new(&mut state.structure.hollow_thickness)
                    .range(1..=64)
                    .suffix(" vox"),
            );
        });
    });
}

/// Edit > Rotate 90°, Flip and Mirror. They act on the selection, or the
/// whole active layer without one.
fn transform_menus(ui: &mut Ui, state: &mut AppState) {
    const AXES: [&str; 3] = ["X", "Y", "Z"];
    ui.menu_button(
        icon_label(ui, icon::ARROW_ARC_LEFT, "Rotate 90°", None),
        |ui| {
            ui.set_min_width(240.0);
            let facing = AXES[state.facing_axis()];
            if menu_item(
                ui,
                icon::ARROW_ARC_LEFT,
                &format!("Counterclockwise (facing {facing})"),
                &sc(state, A::RotateCcw),
                true,
            ) {
                state.rotate_facing(false);
            }
            if menu_item(
                ui,
                icon::ARROW_ARC_RIGHT,
                &format!("Clockwise (facing {facing})"),
                &sc(state, A::RotateCw),
                true,
            ) {
                state.rotate_facing(true);
            }
            ui.separator();
            for (axis, name) in AXES.iter().enumerate() {
                if menu_item(
                    ui,
                    icon::ARROW_ARC_LEFT,
                    &format!("+90° about {name}"),
                    "",
                    true,
                ) {
                    state.rotate(axis, 1);
                }
                if menu_item(
                    ui,
                    icon::ARROW_ARC_RIGHT,
                    &format!("-90° about {name}"),
                    "",
                    true,
                ) {
                    state.rotate(axis, -1);
                }
            }
        },
    );
    ui.menu_button(icon_label(ui, icon::FLIP_HORIZONTAL, "Flip", None), |ui| {
        ui.set_min_width(180.0);
        for (axis, name) in AXES.iter().enumerate() {
            let glyph = if axis == 1 {
                icon::FLIP_VERTICAL
            } else {
                icon::FLIP_HORIZONTAL
            };
            if menu_item(
                ui,
                glyph,
                &format!("Flip {name}"),
                &sc(state, [A::FlipX, A::FlipY, A::FlipZ][axis]),
                true,
            ) {
                state.flip(axis);
            }
        }
    });
    ui.menu_button(icon_label(ui, icon::SQUARE_HALF, "Mirror", None), |ui| {
        ui.set_min_width(240.0);
        for (axis, name) in AXES.iter().enumerate() {
            if menu_item(
                ui,
                icon::SQUARE_HALF,
                &format!("Mirror -{name} onto +{name}"),
                "",
                true,
            ) {
                state.mirror(axis);
            }
        }
    });
}

/// File > Open Recent: the last opened or saved files, newest first.
/// Missing files are dropped while the menu is open.
fn recent_menu(ui: &mut Ui, state: &mut AppState) {
    let enabled = !state.recent.is_empty();
    ui.add_enabled_ui(enabled, |ui| {
        ui.menu_button(
            icon_label(ui, icon::CLOCK_COUNTER_CLOCKWISE, "Open Recent", None),
            |ui| {
                ui.set_min_width(260.0);
                state.prune_recent();
                let mut open = None;
                for path in state.recent.paths() {
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.display().to_string());
                    let item = egui::Button::new(icon_label(ui, icon::FILE, &name, None));
                    if ui
                        .add(item)
                        .on_hover_text(path.display().to_string())
                        .clicked()
                    {
                        open = Some(path.clone());
                        ui.close();
                    }
                }
                if state.recent.is_empty() {
                    ui.label(egui::RichText::new("No recent files").color(theme::TEXT_DIM));
                }
                ui.separator();
                if menu_item(
                    ui,
                    icon::TRASH,
                    "Clear Recent",
                    "",
                    !state.recent.is_empty(),
                ) {
                    state.clear_recent();
                }
                if let Some(path) = open {
                    actions::open_path(state, &path);
                }
            },
        );
    });
}

/// View > Slice (Qubicle's *Tools ▸ Slice*): the Slice Mode toggle and the
/// slice navigation commands. Slice moves are not undo steps.
fn slice_menu(ui: &mut Ui, state: &mut AppState) {
    let has_doc = state.doc().is_some();
    let slicing = state.slice_counter().is_some();
    ui.menu_button(icon_label(ui, icon::STACK, "Slice", None), |ui| {
        ui.set_min_width(180.0);
        let glyph = if slicing {
            icon::CHECK_SQUARE
        } else {
            icon::SQUARE
        };
        if menu_item(ui, glyph, "Slice Mode", &sc(state, A::SliceMode), has_doc) {
            state.toggle_slice(None);
        }
        let shown = state.doc().is_some_and(|d| d.plane.active());
        let glyph = if shown { icon::EYE } else { icon::EYE_SLASH };
        if menu_item(
            ui,
            glyph,
            "Show Work Plane",
            &sc(state, A::WorkPlane),
            has_doc,
        ) {
            state.toggle_plane_visible();
        }
        ui.separator();
        let glyphs = [
            icon::CARET_DOWN,
            icon::CARET_UP,
            icon::CARET_DOUBLE_UP,
            icon::CARET_DOUBLE_DOWN,
        ];
        for (step, glyph) in SliceStep::ALL.into_iter().zip(glyphs) {
            if menu_item(ui, glyph, step.label(), "", slicing) {
                state.step_slice(step);
            }
        }
    });
}

/// Menu text of `action`'s shortcut (*Settings ▸ Keymap*).
fn sc(state: &AppState, action: ActionId) -> String {
    state.settings.keymap.label(action)
}

/// Menu entry with leading icon and trailing shortcut. Returns true on click.
fn menu_item(ui: &mut Ui, glyph: &str, label: &str, shortcut: &str, enabled: bool) -> bool {
    let button = egui::Button::new(icon_label(ui, glyph, label, None)).shortcut_text(shortcut);
    let clicked = ui.add_enabled(enabled, button).clicked();
    if clicked {
        ui.close();
    }
    clicked
}

/// View > Panels: toggle visibility and restore the default workspace layout.
fn panels_menu(ui: &mut Ui, dock: &mut Workspace) {
    ui.menu_button(icon_label(ui, icon::SQUARES_FOUR, "Panels", None), |ui| {
        ui.set_min_width(200.0);
        for panel in dock::Panel::offered(dock.mode()) {
            let closable = panel.permissions().closable;
            let mut open = dock.is_visible(panel);
            let toggle = ui.add_enabled(closable, egui::Checkbox::new(&mut open, panel.label()));
            if toggle.changed() {
                if let Err(e) = dock.set_visible(panel, open) {
                    log::warn!("toggle {panel:?}: {e}");
                }
            }
        }
        ui.separator();
        if menu_item(ui, icon::ARROWS_CLOCKWISE, "Reset Layout", "", true) {
            dock.reset(ui.ctx());
        }
    });
}

fn window_controls(ui: &mut Ui, height: f32) {
    let base = IconStyle::bar()
        .size(egui::vec2(CONTROL_WIDTH, height))
        .color(theme::TEXT);

    // Laid out right-to-left: close, maximize, minimize.
    let close = base
        .hover_fill(theme::CLOSE_HOVER)
        .hover_color(Color32::WHITE);
    if icon_button(ui, icon::X, close, true)
        .on_hover_text("Close")
        .clicked()
    {
        ui.ctx().send_viewport_cmd(ViewportCommand::Close);
    }

    let maximized = ui.input(|i| i.viewport().maximized.unwrap_or(false));
    let glyph = if maximized {
        icon::COPY_SIMPLE
    } else {
        icon::SQUARE
    };
    if icon_button(ui, glyph, base, true)
        .on_hover_text(if maximized { "Restore" } else { "Maximize" })
        .clicked()
    {
        toggle_maximize(ui.ctx());
    }

    if icon_button(ui, icon::MINUS, base, true)
        .on_hover_text("Minimize")
        .clicked()
    {
        ui.ctx().send_viewport_cmd(ViewportCommand::Minimized(true));
    }
}

fn toggle_maximize(ctx: &egui::Context) {
    let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
    ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
}

/// "Save changes to *name* before closing?" while a tab close or exit is
/// pending: Save (`Enter`), Don't Save, Cancel (`Esc`). Returns `true` when
/// every document is answered for and the app may quit.
pub fn close_prompt(ctx: &egui::Context, state: &mut AppState) -> bool {
    let Some(doc) = state.close_prompt() else {
        return false;
    };
    let title = doc.title();
    let exiting = state
        .closing()
        .is_some_and(|r| r.action == CloseAction::Exit);
    let mut choice = None;
    let modal = egui::Modal::new(Id::new("close_prompt")).show(ctx, |ui| {
        ui.set_width(360.0);
        ui.label(
            egui::RichText::new(format!("Save changes to {title} before closing?"))
                .font(theme::semibold(14.0))
                .color(theme::TEXT_STRONG),
        );
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new("Your changes will be lost if you don't save them.")
                .color(theme::TEXT_DIM),
        );
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            let save = if exiting { "Save and Exit" } else { "Save" };
            if ui.button(save).clicked() {
                choice = Some(CloseChoice::Save);
            }
            if ui.button("Don't Save").clicked() {
                choice = Some(CloseChoice::DontSave);
            }
            if ui.button("Cancel").clicked() {
                choice = Some(CloseChoice::Cancel);
            }
        });
    });
    let enter = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
    if enter {
        choice = Some(CloseChoice::Save);
    } else if modal.should_close() {
        choice = Some(CloseChoice::Cancel);
    }
    let Some(choice) = choice else {
        return false;
    };
    let mut step = state.answer_close(choice);
    if let CloseStep::Save(_) = step {
        // The prompt's document is the active one; untitled ones open Save As.
        state.status_message = None;
        actions::save(state);
        step = state.close_after_save();
        // A failed save already reports its error.
        let failed = state
            .status_message
            .as_deref()
            .is_some_and(|m| m.starts_with("Failed"));
        if step == CloseStep::Abort && !failed {
            state.status_message = Some("Close cancelled: the document was not saved".to_owned());
        }
    }
    step == CloseStep::Quit
}

/// Startup dialog listing autosaves left by a session that did not exit
/// cleanly: Restore or Discard each. "Decide Later" keeps the files for the
/// next start.
pub fn recovery_dialog(ctx: &egui::Context, state: &mut AppState, ui_state: &mut UiState) {
    if ui_state.recovery.is_empty() {
        return;
    }
    enum Pick {
        Restore(usize),
        Discard(usize),
        Later,
    }
    let mut pick = None;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    egui::Modal::new(Id::new("recovery_dialog")).show(ctx, |ui| {
        ui.set_width(440.0);
        ui.label(
            egui::RichText::new("Recover unsaved work")
                .font(theme::semibold(14.0))
                .color(theme::TEXT_STRONG),
        );
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new("Rizxel did not close normally. These documents were autosaved:")
                .color(theme::TEXT_DIM),
        );
        ui.add_space(8.0);
        for (i, entry) in ui_state.recovery.iter().enumerate() {
            let meta = &entry.meta;
            let minutes = now.saturating_sub(meta.saved_at) / 60;
            let when = match minutes {
                0 => "less than a minute ago".to_owned(),
                1 => "1 minute ago".to_owned(),
                m if m < 120 => format!("{m} minutes ago"),
                m => format!("{} hours ago", m / 60),
            };
            let place = meta
                .original
                .as_ref()
                .map_or("never saved".to_owned(), |p| p.display().to_string());
            ui.separator();
            ui.label(egui::RichText::new(&meta.title).color(theme::TEXT_STRONG));
            ui.label(
                egui::RichText::new(format!("{place}, autosaved {when}"))
                    .size(11.0)
                    .color(theme::TEXT_DIM),
            );
            ui.horizontal(|ui| {
                if ui.button("Restore").clicked() {
                    pick = Some(Pick::Restore(i));
                }
                if ui.button("Discard").clicked() {
                    pick = Some(Pick::Discard(i));
                }
            });
        }
        ui.separator();
        ui.add_space(8.0);
        if ui.button("Decide Later").clicked() {
            pick = Some(Pick::Later);
        }
    });
    match pick {
        Some(Pick::Restore(i)) => {
            let entry = ui_state.recovery.remove(i);
            state.status_message = Some(match state.restore(&entry) {
                Ok(()) => format!("Restored {}", entry.meta.title),
                Err(e) => {
                    log::error!("restore {}: {e}", entry.key);
                    format!("Failed to restore {}: {e}", entry.meta.title)
                }
            });
        }
        Some(Pick::Discard(i)) => {
            let entry = ui_state.recovery.remove(i);
            state.discard_recovery(&entry);
        }
        Some(Pick::Later) => ui_state.recovery.clear(),
        None => {}
    }
}

/// Help dialogs opened from the menu.
pub fn show_dialogs(ctx: &egui::Context, state: &mut AppState, ui_state: &mut UiState) {
    if ui_state.show_about {
        egui::Window::new("About Rizxel")
            .open(&mut ui_state.show_about)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(icon::CUBE)
                            .font(theme::icon(28.0))
                            .color(theme::ACCENT),
                    );
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new("Rizxel").font(theme::semibold(16.0)));
                        ui.label(format!("Voxel Editor v{}", env!("CARGO_PKG_VERSION")));
                    });
                });
                ui.add_space(6.0);
                ui.label("Native voxel art editor built with Rust, egui and wgpu.");
                ui.label(format!("Created by {AUTHOR_NAME} ({AUTHOR_HANDLE})"));
                ui.label(
                    egui::RichText::new(format!("© 2026 {AUTHOR_NAME}")).color(theme::TEXT_DIM),
                );
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(
                        "Free software under the GNU General Public License v3 or later.\n\
                         This program comes with ABSOLUTELY NO WARRANTY.",
                    )
                    .color(theme::TEXT_DIM),
                );
                ui.hyperlink_to("Source code and license", REPO_URL);
            });
    }

    if ui_state.show_shortcuts {
        egui::Window::new("Keyboard Shortcuts")
            .open(&mut ui_state.show_shortcuts)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(440.0)
                    .show(ui, |ui| {
                        egui::Grid::new("shortcuts")
                            .striped(true)
                            .show(ui, |ui| shortcut_rows(ui, &state.settings.keymap));
                    });
            });
    }
}

/// Rows of the Keyboard Shortcuts window: the keymap by group, then the
/// mouse and the fixed keys.
fn shortcut_rows(ui: &mut Ui, keymap: &rizxel_app::Keymap) {
    let strong = |ui: &mut Ui, text: &str| {
        ui.label(egui::RichText::new(text).color(theme::TEXT_STRONG));
    };
    for group in rizxel_app::ActionGroup::ALL {
        ui.label(egui::RichText::new(group.label()).font(theme::semibold(12.0)));
        ui.end_row();
        for action in ActionId::all().into_iter().filter(|a| a.group() == group) {
            let keys = keymap.labels(action);
            strong(ui, if keys.is_empty() { "—" } else { &keys });
            ui.label(action.label());
            ui.end_row();
        }
    }
    ui.label(egui::RichText::new("Mouse and fixed keys").font(theme::semibold(12.0)));
    ui.end_row();
    for (keys, action) in [
        ("Enter / click", "Place the paste"),
        (
            "Esc",
            "Cancel paste, drag, box, line or transform; deselect",
        ),
        ("Enter", "Apply the open box, polyline, curve or transform"),
        ("Ctrl+click (Brush)", "Line from the last click"),
        (
            "Right-click / double-click",
            "Finish a polyline (Line tool)",
        ),
        ("Shift+click swatch", "Mark a color for Gradient and Remap"),
        ("Right drag (viewport)", "Orbit camera"),
        ("Middle drag (viewport)", "Pan camera"),
        ("Middle double-click", "Reset camera"),
        ("Scroll (viewport)", "Zoom"),
    ] {
        strong(ui, keys);
        ui.label(action);
        ui.end_row();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SWITCH: f32 = 120.0;

    #[test]
    fn mode_switch_never_overlaps_window_controls() {
        for width in (640..=3840).step_by(16) {
            let width = width as f32;
            for menus in [300.0, 420.0, 600.0] {
                let l = bar_layout(width, menus, 260.0, SWITCH);
                assert_eq!(l.controls, (width - CONTROLS_WIDTH, width));
                assert!(l.switch.1 + SWITCH_GAP <= l.controls.0, "{width}");
                assert!((l.switch.1 - l.switch.0 - SWITCH).abs() < 1e-3, "{width}");
                assert!(l.menus.1 <= l.switch.0, "{width}");
                if let Some((left, right)) = l.title {
                    assert!(left >= l.menus.1 && right <= l.switch.0, "{width}");
                }
            }
        }
    }

    #[test]
    fn title_shrinks_before_menus_collapse() {
        let wide = bar_layout(1920.0, 420.0, 260.0, SWITCH);
        assert!(!wide.collapsed);
        let (left, right) = wide.title.unwrap();
        assert!(
            ((left + right) / 2.0 - 960.0).abs() < 1e-3,
            "centered on the bar"
        );

        // Not enough room to center it: it moves into the free space, then
        // gets narrower than its text.
        let moved = bar_layout(1000.0, 420.0, 260.0, SWITCH);
        let (left, right) = moved.title.unwrap();
        assert!(left >= moved.menus.1 && (right - left - 260.0).abs() < 1e-3);
        let snug = bar_layout(960.0, 420.0, 260.0, SWITCH);
        assert!(!snug.collapsed);
        let (left, right) = snug.title.unwrap();
        assert!(right - left < 260.0);

        let narrow = bar_layout(640.0, 420.0, 260.0, SWITCH);
        assert!(narrow.collapsed);
        assert_eq!(
            narrow.menus,
            (ICON_WIDTH, ICON_WIDTH + COLLAPSED_MENUS_WIDTH)
        );
    }
}

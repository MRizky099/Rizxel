//! Render dock panel (Render mode): presets, then MagicaVoxel-style tabs
//! for light, sampling, display, the active layer's material, camera and
//! output, and a footer with the **Render** button at its bottom-right.
//!
//! Every setting edits the active document's [`RenderSettings`] through
//! undoable edits; a slider or color drag is one undo step.

use egui::{Color32, RichText, Ui};
use egui_phosphor::regular as icon;
use rizxel_app::{AppState, Document, LayerProps, Projection};
use rizxel_math::render::{
    BackgroundMode, Denoise, OutputKind, PixelFilter, SkyMode, ToneMap, MAX_BOUNCES,
    MAX_OUTPUT_SIDE, MAX_SAMPLES, MAX_TURNTABLE_FRAMES,
};
use rizxel_math::{Material, MaterialPreset, RenderPreset, RenderSettings, VoxelShape};

use super::UiState;
use crate::theme;

/// Render panel tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RenderTab {
    #[default]
    Light,
    Sample,
    Display,
    Material,
    Camera,
    Output,
}

impl RenderTab {
    const ROWS: [[RenderTab; 3]; 2] = [
        [RenderTab::Light, RenderTab::Sample, RenderTab::Display],
        [RenderTab::Material, RenderTab::Camera, RenderTab::Output],
    ];

    fn label(self) -> &'static str {
        match self {
            RenderTab::Light => "Light",
            RenderTab::Sample => "Sample",
            RenderTab::Display => "Display",
            RenderTab::Material => "Material",
            RenderTab::Camera => "Camera",
            RenderTab::Output => "Output",
        }
    }
}

/// Progress of a running output job, shown in the footer.
#[derive(Debug, Clone, PartialEq)]
pub struct JobStatus {
    /// `0..=1`.
    pub fraction: f32,
    /// e.g. "Frame 3 / 36 · 12 s left".
    pub text: String,
}

/// Render panel state (UI only).
#[derive(Debug, Default)]
pub struct RenderUi {
    pub tab: RenderTab,
    /// Name typed for *Save preset*.
    preset_name: String,
    /// The Render button was pressed this frame.
    pub start: bool,
    /// Cancel was pressed this frame.
    pub cancel: bool,
    /// *Export shape* was pressed this frame.
    pub export_shape: bool,
    /// Running job (`None` when idle).
    pub job: Option<JobStatus>,
    /// Message of the last finished job.
    pub last_result: Option<String>,
}

/// Height of the footer with the Render button.
const FOOTER_HEIGHT: f32 = 44.0;

pub fn show(ui: &mut Ui, state: &mut AppState, ui_state: &mut UiState) {
    let rect = ui.max_rect();
    let (body_rect, footer_rect) = rect.split_top_bottom_at_y(rect.bottom() - FOOTER_HEIGHT);
    ui.scope_builder(egui::UiBuilder::new().max_rect(body_rect), |ui| {
        egui::ScrollArea::vertical()
            .id_salt("render_panel_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Frame::NONE.inner_margin(8).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 4.0;
                    presets(ui, state, &mut ui_state.render);
                    ui.add_space(4.0);
                    tabs(ui, &mut ui_state.render.tab);
                    ui.add_space(4.0);
                    body(ui, state, &mut ui_state.render);
                });
            });
    });
    ui.painter().hline(
        footer_rect.x_range(),
        footer_rect.top(),
        (1.0, theme::BORDER),
    );
    ui.scope_builder(
        egui::UiBuilder::new().max_rect(footer_rect.shrink(8.0)),
        |ui| {
            footer(ui, state, &mut ui_state.render);
        },
    );
}

// ------------------------------------------------------------ presets

fn presets(ui: &mut Ui, state: &mut AppState, render: &mut RenderUi) {
    ui.horizontal(|ui| {
        ui.label(label("Preset"));
        egui::ComboBox::from_id_salt("render_preset")
            .selected_text("Apply…")
            .width(150.0)
            .show_ui(ui, |ui| {
                for preset in RenderPreset::ALL {
                    if ui.selectable_label(false, preset.label()).clicked() {
                        if let Some(doc) = state.doc_mut() {
                            doc.apply_render_preset(preset);
                        }
                    }
                }
                let names: Vec<String> = state
                    .render_presets
                    .presets
                    .iter()
                    .map(|p| p.name.clone())
                    .collect();
                if !names.is_empty() {
                    ui.separator();
                }
                for name in names {
                    ui.horizontal(|ui| {
                        if ui.selectable_label(false, &name).clicked() {
                            state.apply_user_render_preset(&name);
                        }
                        if ui
                            .small_button(icon::TRASH)
                            .on_hover_text("Delete this preset")
                            .clicked()
                        {
                            state.delete_render_preset(&name);
                        }
                    });
                }
            });
    });
    ui.horizontal(|ui| {
        let edit = egui::TextEdit::singleline(&mut render.preset_name)
            .hint_text("Preset name")
            .desired_width(150.0);
        ui.add(edit);
        let can_save = !render.preset_name.trim().is_empty();
        if ui
            .add_enabled(can_save, egui::Button::new("Save preset"))
            .on_hover_text("Keep these settings (without the output) as a preset")
            .clicked()
            && state.save_render_preset(&render.preset_name)
        {
            render.preset_name.clear();
        }
    });
}

fn tabs(ui: &mut Ui, tab: &mut RenderTab) {
    for (i, row) in RenderTab::ROWS.iter().enumerate() {
        let items: Vec<(RenderTab, &str)> = row.iter().map(|t| (*t, t.label())).collect();
        ui.horizontal(|ui| {
            ui.push_id(i, |ui| {
                super::options::segmented_fill(ui, "render_tabs", tab, &items)
            });
        });
    }
}

// ------------------------------------------------------------ body

fn body(ui: &mut Ui, state: &mut AppState, render: &mut RenderUi) {
    if render.tab == RenderTab::Camera {
        camera_projection(ui, state);
    }
    let Some(doc) = state.doc_mut() else {
        return;
    };
    if render.tab == RenderTab::Material {
        material_tab(ui, doc);
        return;
    }
    let mut s = doc.data.render.clone();
    match render.tab {
        RenderTab::Light => light_tab(ui, &mut s),
        RenderTab::Sample => sample_tab(ui, &mut s),
        RenderTab::Display => display_tab(ui, &mut s, render),
        RenderTab::Camera => camera_tab(ui, &mut s),
        RenderTab::Output => output_tab(ui, &mut s),
        RenderTab::Material => {}
    }
    let changed = s != doc.data.render;
    let merge = continues_drag(ui, "render_settings", changed);
    if changed {
        doc.set_render(s, merge);
    }
}

/// Whether an edit made this frame continues the edit of the previous
/// frame (a slider, drag value or color drag still held), so it folds into
/// the same undo step.
fn continues_drag(ui: &Ui, key: &str, changed: bool) -> bool {
    let id = egui::Id::new(("render_panel_drag", key));
    let down = ui.input(|i| i.pointer.any_down());
    let was = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    if changed {
        ui.data_mut(|d| d.insert_temp(id, down));
        was && down
    } else {
        if !down {
            ui.data_mut(|d| d.insert_temp(id, false));
        }
        false
    }
}

fn label(text: &str) -> RichText {
    RichText::new(text).size(12.0).color(theme::TEXT)
}

fn note(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(11.0).color(theme::TEXT_MUTED));
}

/// Collapsible section with a two-column grid.
fn section(ui: &mut Ui, title: &str, open: bool, add: impl FnOnce(&mut Ui)) {
    egui::CollapsingHeader::new(RichText::new(title).size(12.0).strong())
        .default_open(open)
        .show(ui, |ui| {
            egui::Grid::new(("render_section", title))
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, add);
        });
}

fn slider(
    ui: &mut Ui,
    name: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    suffix: &str,
) {
    ui.label(label(name));
    ui.add(
        egui::Slider::new(value, range)
            .suffix(suffix)
            .max_decimals(2),
    );
    ui.end_row();
}

fn drag_u32(ui: &mut Ui, name: &str, value: &mut u32, range: std::ops::RangeInclusive<u32>) {
    ui.label(label(name));
    ui.add(egui::DragValue::new(value).range(range).speed(1.0));
    ui.end_row();
}

fn check(ui: &mut Ui, name: &str, value: &mut bool) {
    ui.label(label(name));
    ui.checkbox(value, "");
    ui.end_row();
}

/// Color row. The value is written back only when the user changes it:
/// egui's RGB -> HSV -> RGB round trip alters the floats slightly.
fn color(ui: &mut Ui, name: &str, value: &mut [f32; 3]) {
    ui.label(label(name));
    let mut edit = *value;
    if ui.color_edit_button_rgb(&mut edit).changed() {
        *value = edit.map(|v| v.clamp(0.0, 1.0));
    }
    ui.end_row();
}

fn combo<T: PartialEq + Copy>(
    ui: &mut Ui,
    name: &str,
    value: &mut T,
    items: &[T],
    text: impl Fn(T) -> &'static str,
) {
    ui.label(label(name));
    egui::ComboBox::from_id_salt(("render_combo", name))
        .selected_text(text(*value))
        .show_ui(ui, |ui| {
            for item in items {
                ui.selectable_value(value, *item, text(*item));
            }
        });
    ui.end_row();
}

fn light_tab(ui: &mut Ui, s: &mut RenderSettings) {
    let l = &mut s.light;
    section(ui, "Sun", true, |ui| {
        check(ui, "Enabled", &mut l.sun);
        slider(ui, "Azimuth", &mut l.sun_azimuth, 0.0..=360.0, "°");
        slider(ui, "Elevation", &mut l.sun_elevation, -10.0..=90.0, "°");
        slider(ui, "Size", &mut l.sun_size, 0.0..=45.0, "°");
        slider(ui, "Intensity", &mut l.sun_intensity, 0.0..=20.0, "");
        color(ui, "Color", &mut l.sun_color);
        check(ui, "Studio lights", &mut l.studio_lights);
    });
    section(ui, "Sky", true, |ui| {
        combo(ui, "Sky", &mut l.sky, &SkyMode::ALL, SkyMode::label);
        match l.sky {
            SkyMode::Uniform => color(ui, "Color", &mut l.sky_color),
            SkyMode::Gradient => {
                color(ui, "Horizon", &mut l.sky_color);
                color(ui, "Zenith", &mut l.sky_zenith);
            }
            SkyMode::Image => {
                ui.label(label("Image"));
                ui.horizontal(|ui| {
                    let name = std::path::Path::new(&l.sky_image)
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "None".to_owned());
                    ui.label(RichText::new(name).size(11.0).color(theme::TEXT_DIM))
                        .on_hover_text(&l.sky_image);
                    if ui.small_button("Browse…").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Radiance HDR", &["hdr"])
                            .pick_file()
                        {
                            l.sky_image = path.to_string_lossy().into_owned();
                        }
                    }
                });
                ui.end_row();
                slider(ui, "Rotation", &mut l.sky_rotation, 0.0..=360.0, "°");
            }
        }
        slider(ui, "Intensity", &mut l.sky_intensity, 0.0..=10.0, "");
    });
    section(ui, "Fog", false, |ui| {
        check(ui, "Enabled", &mut l.fog);
        slider(ui, "Density", &mut l.fog_density, 0.0..=0.2, "");
        color(ui, "Color", &mut l.fog_color);
    });
}

fn sample_tab(ui: &mut Ui, s: &mut RenderSettings) {
    let p = &mut s.sample;
    section(ui, "Samples", true, |ui| {
        drag_u32(ui, "Samples", &mut p.samples, 1..=MAX_SAMPLES);
        combo(ui, "Denoise", &mut p.denoise, &Denoise::ALL, Denoise::label);
        combo(
            ui,
            "Filter",
            &mut p.filter,
            &PixelFilter::ALL,
            PixelFilter::label,
        );
        check(ui, "Pixelated", &mut p.pixelated);
    });
    section(ui, "Bounces", true, |ui| {
        drag_u32(ui, "Diffuse", &mut p.diffuse_bounces, 0..=MAX_BOUNCES);
        drag_u32(ui, "Specular", &mut p.specular_bounces, 0..=MAX_BOUNCES);
        drag_u32(
            ui,
            "Transmission",
            &mut p.transmission_bounces,
            0..=MAX_BOUNCES,
        );
        check(ui, "Russian roulette", &mut p.russian_roulette);
        slider(ui, "Clamp", &mut p.clamp, 0.0..=100.0, "");
    });
    note(
        ui,
        "Clamp 0 keeps every sample; lower values remove fireflies.",
    );
}

fn display_tab(ui: &mut Ui, s: &mut RenderSettings, render: &mut RenderUi) {
    let d = &mut s.display;
    section(ui, "Shape", true, |ui| {
        combo(
            ui,
            "Shape",
            &mut d.shape,
            &VoxelShape::ALL,
            VoxelShape::label,
        );
        slider(ui, "Cell", &mut d.cell, 0.5..=1.0, "");
        ui.label(label("Export"));
        let exportable = d.shape != VoxelShape::Cube;
        if ui
            .add_enabled(exportable, egui::Button::new("Export shape…"))
            .on_hover_text("Export the model as a mesh with this shape (OBJ, STL, glTF)")
            .on_disabled_hover_text("Pick a shape other than Cube to export it")
            .clicked()
        {
            render.export_shape = true;
        }
        ui.end_row();
    });
    note(
        ui,
        "Shapes are a view setting: the voxels and Model mode stay cubes.",
    );
    section(ui, "Ground", true, |ui| {
        check(ui, "Ground", &mut d.ground);
        color(ui, "Color", &mut d.ground_color);
        check(ui, "Shadow catcher", &mut d.shadow_catcher);
    });
    section(ui, "Background", true, |ui| {
        combo(
            ui,
            "Background",
            &mut d.background,
            &BackgroundMode::ALL,
            BackgroundMode::label,
        );
        match d.background {
            BackgroundMode::Color => color(ui, "Color", &mut d.background_color),
            BackgroundMode::Gradient => {
                color(ui, "Bottom", &mut d.background_color);
                color(ui, "Top", &mut d.background_top);
            }
            BackgroundMode::Sky | BackgroundMode::Transparent => {}
        }
    });
    section(ui, "Edges", true, |ui| {
        slider(ui, "Bevel", &mut d.bevel, 0.0..=0.5, "");
        check(ui, "Seams", &mut d.seams);
        check(ui, "Outline", &mut d.edges);
        color(ui, "Outline color", &mut d.edge_color);
        slider(ui, "Outline width", &mut d.edge_width, 0.5..=8.0, " px");
    });
}

/// Pers / Ortho / Iso buttons (view state, not saved).
fn camera_projection(ui: &mut Ui, state: &mut AppState) {
    ui.horizontal(|ui| {
        ui.label(label("Projection"));
        projection_buttons(ui, state);
    });
}

/// Shared by the Camera tab and the render bar.
pub fn projection_buttons(ui: &mut Ui, state: &mut AppState) {
    #[derive(Clone, Copy, PartialEq)]
    enum Kind {
        Pers,
        Ortho,
        Iso,
    }
    let iso = state.camera.is_ortho()
        && (state.camera.pitch - rizxel_app::ISO_PITCH).abs() < 0.01
        && (state.camera.yaw.rem_euclid(90.0) - 45.0).abs() < 0.01;
    let mut kind = match (state.camera.projection, iso) {
        (Projection::Perspective, _) => Kind::Pers,
        (Projection::Orthographic, true) => Kind::Iso,
        (Projection::Orthographic, false) => Kind::Ortho,
    };
    let before = kind;
    super::options::segmented(
        ui,
        "render_projection",
        &mut kind,
        &[
            (Kind::Pers, "Pers"),
            (Kind::Ortho, "Ortho"),
            (Kind::Iso, "Iso"),
        ],
    );
    if kind != before {
        let cam = &mut state.camera;
        match kind {
            Kind::Pers => cam.projection = Projection::Perspective,
            Kind::Ortho => cam.projection = Projection::Orthographic,
            Kind::Iso => {
                cam.snap(rizxel_app::ViewTarget::Corner(
                    rizxel_app::ViewCorner::FrontTopRight,
                ));
                cam.projection = Projection::Orthographic;
            }
        }
        cam.auto_ortho = false;
    }
}

fn camera_tab(ui: &mut Ui, s: &mut RenderSettings) {
    let c = &mut s.camera;
    section(ui, "Lens", true, |ui| {
        slider(ui, "Field of view", &mut c.fov, 5.0..=120.0, "°");
        slider(ui, "Aperture", &mut c.aperture, 0.0..=4.0, "");
        slider(ui, "Focus", &mut c.focus_distance, 0.1..=1000.0, "");
        drag_u32(ui, "Blades", &mut c.blades, 0..=12);
    });
    note(
        ui,
        "Click the model to focus there (with an aperture above 0).",
    );
    section(ui, "Film", true, |ui| {
        slider(ui, "Exposure", &mut c.exposure, -5.0..=5.0, " EV");
        combo(
            ui,
            "Tone map",
            &mut c.tone_map,
            &ToneMap::ALL,
            ToneMap::label,
        );
        slider(ui, "Gamma", &mut c.gamma, 1.0..=3.0, "");
        slider(ui, "Vignette", &mut c.vignette, 0.0..=1.0, "");
    });
    section(ui, "Bloom", false, |ui| {
        check(ui, "Bloom", &mut c.bloom);
        slider(ui, "Threshold", &mut c.bloom_threshold, 0.0..=10.0, "");
        slider(ui, "Strength", &mut c.bloom_strength, 0.0..=1.0, "");
    });
}

/// Output size presets.
const SIZES: [(&str, u32, u32); 7] = [
    ("720p", 1280, 720),
    ("1080p", 1920, 1080),
    ("1440p", 2560, 1440),
    ("4K", 3840, 2160),
    ("512²", 512, 512),
    ("1024²", 1024, 1024),
    ("2048²", 2048, 2048),
];

fn output_tab(ui: &mut Ui, s: &mut RenderSettings) {
    let o = &mut s.output;
    ui.horizontal(|ui| {
        let items: Vec<(OutputKind, &str)> =
            OutputKind::ALL.iter().map(|k| (*k, k.label())).collect();
        super::options::segmented_fill(ui, "output_kind", &mut o.kind, &items);
    });
    section(ui, "Size", true, |ui| {
        ui.label(label("Size"));
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(&mut o.width).range(1..=MAX_OUTPUT_SIDE));
            ui.label("×");
            ui.add(egui::DragValue::new(&mut o.height).range(1..=MAX_OUTPUT_SIDE));
            egui::ComboBox::from_id_salt("output_size")
                .selected_text("")
                .width(24.0)
                .show_ui(ui, |ui| {
                    for (name, w, h) in SIZES {
                        if ui.selectable_label(false, name).clicked() {
                            o.width = w;
                            o.height = h;
                        }
                    }
                });
        });
        ui.end_row();
        check(ui, "Transparent", &mut o.transparent);
    });
    section(ui, "File", true, |ui| {
        ui.label(label("Folder"));
        ui.horizontal(|ui| {
            let shown = if o.folder.is_empty() {
                "Next to the project".to_owned()
            } else {
                o.folder.clone()
            };
            ui.label(RichText::new(shown).size(11.0).color(theme::TEXT_DIM));
            if ui.small_button("…").clicked() {
                if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                    o.folder = dir.to_string_lossy().into_owned();
                }
            }
        });
        ui.end_row();
        ui.label(label("Name"));
        ui.add(egui::TextEdit::singleline(&mut o.name).desired_width(150.0));
        ui.end_row();
    });
    note(
        ui,
        "{name} is the project name, {frame} the turntable frame.",
    );
    match o.kind {
        OutputKind::Image => {}
        OutputKind::Photo => section(ui, "Photo-realistic", true, |ui| {
            slider(ui, "Time limit", &mut o.time_limit, 0.0..=3600.0, " s");
            check(ui, "16-bit PNG", &mut o.png16);
        }),
        OutputKind::Turntable => section(ui, "Turntable", true, |ui| {
            drag_u32(
                ui,
                "Frames",
                &mut o.turntable_frames,
                1..=MAX_TURNTABLE_FRAMES,
            );
            slider(ui, "Angle", &mut o.turntable_angle, -720.0..=720.0, "°");
            check(ui, "Path traced", &mut o.turntable_path_traced);
            drag_u32(ui, "Samples", &mut o.turntable_samples, 1..=MAX_SAMPLES);
            check(ui, "PNG sequence", &mut o.turntable_png);
            check(ui, "GIF", &mut o.turntable_gif);
            check(ui, "Sprite sheet", &mut o.turntable_sheet);
            drag_u32(ui, "GIF fps", &mut o.gif_fps, 1..=50);
        }),
    }
}

// ------------------------------------------------------------ material

/// Swatch color of a material: its tint, white for the default.
pub fn material_swatch(m: Option<&Material>) -> Color32 {
    let tint = m.map_or([1.0; 4], |m| m.tint);
    let [r, g, b] = [tint[0], tint[1], tint[2]].map(|v| (v.clamp(0.0, 1.0) * 255.0) as u8);
    Color32::from_rgb(r, g, b)
}

/// The active layer's material: pick, create, make unique, presets and the
/// full parameter set. Edits change every layer that shares the material.
fn material_tab(ui: &mut Ui, doc: &mut Document) {
    let index = doc.active_layer;
    let Some(layer) = doc.data.layers.get(index) else {
        return;
    };
    if layer.kind().is_group() {
        note(
            ui,
            "Groups have no material: pick a layer in the Layers panel.",
        );
        return;
    }
    let layer_name = layer.name.clone();
    let props = layer.props.clone();
    ui.label(
        RichText::new(format!("Layer \"{layer_name}\""))
            .size(12.0)
            .color(theme::TEXT_STRONG),
    );
    note(ui, "Alt+click the model to pick a layer.");

    let name =
        |doc: &Document, m: Option<u32>| match m.and_then(|m| doc.data.materials.get(m as usize)) {
            Some(mat) => mat.name.clone(),
            None => "Default".to_owned(),
        };
    let mut picked = props.material;
    let mut unique = false;
    ui.horizontal(|ui| {
        ui.label(label("Material"));
        egui::ComboBox::from_id_salt(("render_material", index))
            .selected_text(name(doc, picked))
            .width(130.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut picked, None, "Default");
                for (i, m) in doc.data.materials.iter().enumerate() {
                    ui.selectable_value(&mut picked, Some(i as u32), &m.name);
                }
            });
        if ui
            .small_button(icon::PLUS)
            .on_hover_text("New material for this layer")
            .clicked()
        {
            picked = doc.add_material();
        }
    });
    if picked != props.material {
        doc.labeled("Layer Material", |d| {
            d.set_layer_props(
                index,
                LayerProps {
                    material: picked,
                    ..props.clone()
                },
                false,
            )
        });
    }
    let Some(m) = picked else {
        note(
            ui,
            "The default material can't be edited: add one with + to change this layer.",
        );
        return;
    };
    let users = doc.material_users(m);
    if users > 1 {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("Shared by {users} layers"))
                    .size(11.0)
                    .color(theme::BOUND),
            );
            unique = ui
                .small_button("Make unique")
                .on_hover_text("Give this layer its own copy of the material")
                .clicked();
        });
    }
    if unique {
        doc.make_material_unique(index);
        return;
    }
    let Some(mut mat) = doc.data.materials.get(m as usize).cloned() else {
        return;
    };
    ui.horizontal_wrapped(|ui| {
        for preset in MaterialPreset::ALL {
            if ui.small_button(preset.label()).clicked() {
                mat.apply_preset(preset);
            }
        }
    });
    section(ui, "Surface", true, |ui| {
        ui.label(label("Name"));
        ui.add(egui::TextEdit::singleline(&mut mat.name).desired_width(130.0));
        ui.end_row();
        ui.label(label("Tint"));
        let [r, g, b, a] = mat.tint;
        let mut tint = egui::Rgba::from_rgba_unmultiplied(r, g, b, a);
        if egui::color_picker::color_edit_button_rgba(
            ui,
            &mut tint,
            egui::color_picker::Alpha::OnlyBlend,
        )
        .changed()
        {
            mat.tint = tint.to_rgba_unmultiplied().map(|v| v.clamp(0.0, 1.0));
        }
        ui.end_row();
        slider(ui, "Roughness", &mut mat.roughness, 0.0..=1.0, "");
        slider(ui, "Metallic", &mut mat.metallic, 0.0..=1.0, "");
        slider(ui, "Specular", &mut mat.specular, 0.0..=1.0, "");
        slider(ui, "Clear coat", &mut mat.clearcoat, 0.0..=1.0, "");
    });
    section(ui, "Glass", false, |ui| {
        slider(ui, "Transmission", &mut mat.transmission, 0.0..=1.0, "");
        slider(ui, "IOR", &mut mat.ior, 1.0..=3.0, "");
    });
    section(ui, "Emission", false, |ui| {
        color(ui, "Color", &mut mat.emission);
        slider(
            ui,
            "Strength",
            &mut mat.emission_strength,
            0.0..=Material::MAX_EMISSION_STRENGTH,
            "",
        );
    });
    let changed = doc.data.materials.get(m as usize) != Some(&mat);
    let merge = continues_drag(ui, "material", changed);
    if changed {
        doc.set_material(m as usize, mat, merge);
    }
}

// ------------------------------------------------------------ footer

/// Progress of a running job, or the last result, and the Render button
/// at the bottom-right (Cancel while a job runs).
fn footer(ui: &mut Ui, state: &AppState, render: &mut RenderUi) {
    let kind = state
        .doc()
        .map(|d| d.data.render.output.kind)
        .unwrap_or_default();
    let blocked = kind != OutputKind::Image && !state.render_view.path_tracing;
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if render.job.is_some() {
            if ui
                .add(
                    egui::Button::new(RichText::new("Cancel").size(13.0))
                        .min_size([72.0, 28.0].into()),
                )
                .clicked()
            {
                render.cancel = true;
            }
        } else {
            let text = RichText::new(format!("{}  Render", icon::CAMERA))
                .size(13.0)
                .color(Color32::WHITE);
            let button = egui::Button::new(text)
                .fill(theme::ACCENT)
                .min_size([96.0, 28.0].into());
            let tip = format!("Render {} ({})", kind.label(), output_size(state));
            if ui
                .add_enabled(!blocked, button)
                .on_hover_text(tip)
                .on_disabled_hover_text(
                    "This GPU backend has no compute shaders: only Image output works",
                )
                .clicked()
            {
                render.start = true;
            }
        }
        ui.add_space(6.0);
        match &render.job {
            Some(job) => {
                ui.add(
                    egui::ProgressBar::new(job.fraction)
                        .text(RichText::new(&job.text).size(11.0))
                        .desired_width(ui.available_width()),
                );
            }
            None => {
                let text = render.last_result.as_deref().unwrap_or("");
                ui.add(
                    egui::Label::new(RichText::new(text).size(11.0).color(theme::TEXT_DIM))
                        .truncate(),
                );
            }
        }
    });
}

fn output_size(state: &AppState) -> String {
    state.doc().map_or(String::new(), |d| {
        let o = &d.data.render.output;
        format!("{}×{}", o.width, o.height)
    })
}

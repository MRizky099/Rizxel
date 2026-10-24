//! Import and export dialogs: mesh export options (OBJ, STL, PLY, glTF),
//! Export Slices, the Voxelizer (OBJ/STL/PLY meshes) and heightmap import.

use std::path::PathBuf;

use egui::{Id, RichText};
use rizxel_app::{
    AppState, ExportOptions, FileFormat, GltfColors, HeightmapOptions, ObjLayout, PendingMesh,
    Pivot, SliceAxis, VoxelizerPreview, VoxelizerSettings, MAX_SLICE_SCALE,
};

use crate::theme;

/// Open import/export dialogs.
#[derive(Default)]
pub struct Dialogs {
    pub voxelizer: Option<VoxelizerDialog>,
    pub heightmap: Option<HeightmapDialog>,
    /// Last Voxelizer settings, reused for the next mesh.
    pub voxelizer_settings: VoxelizerSettings,
    /// Mesh format whose export options are open.
    pub export: Option<FileFormat>,
    /// Export settings, kept between exports; `scale` stays `None` here and
    /// the per-format scales live in `scales`.
    pub export_options: ExportOptions,
    scales: Vec<(FileFormat, f32)>,
    /// Export Slices dialog.
    pub slices: bool,
    pub slice_axis: SliceAxis,
    pub slice_scale: u32,
}

impl Dialogs {
    /// True while a dialog blocks the editor (shortcuts are off).
    pub fn is_open(&self) -> bool {
        self.voxelizer.is_some() || self.heightmap.is_some() || self.export.is_some() || self.slices
    }

    /// Units per voxel for `format` (the format's default until changed).
    pub fn scale(&self, format: FileFormat) -> f32 {
        self.scales
            .iter()
            .find(|(f, _)| *f == format)
            .map_or(format.default_scale(), |(_, s)| *s)
    }

    fn set_scale(&mut self, format: FileFormat, scale: f32) {
        match self.scales.iter_mut().find(|(f, _)| *f == format) {
            Some(entry) => entry.1 = scale,
            None => self.scales.push((format, scale)),
        }
    }

    /// The settings an export of `format` uses.
    pub fn options_for(&self, format: FileFormat) -> ExportOptions {
        ExportOptions {
            scale: Some(self.scale(format)),
            ..self.export_options
        }
    }
}

/// A mesh waiting for its voxelization settings.
pub struct VoxelizerDialog {
    pub mesh: PendingMesh,
    pub settings: VoxelizerSettings,
    /// Preview for `previewed` settings.
    preview: Option<Result<VoxelizerPreview, String>>,
    previewed: Option<VoxelizerSettings>,
}

impl VoxelizerDialog {
    pub fn new(mesh: PendingMesh, settings: VoxelizerSettings) -> Self {
        Self {
            mesh,
            settings,
            preview: None,
            previewed: None,
        }
    }
}

/// A heightmap waiting for its settings.
pub struct HeightmapDialog {
    pub path: PathBuf,
    pub color: Option<PathBuf>,
    pub options: HeightmapOptions,
}

fn title(ui: &mut egui::Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .font(theme::semibold(14.0))
            .color(theme::TEXT_STRONG),
    );
    ui.add_space(4.0);
}

fn file_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn report<E: std::fmt::Display>(state: &mut AppState, result: Result<String, E>, what: &str) {
    state.status_message = Some(match result {
        Ok(msg) => msg,
        Err(e) => {
            log::error!("{what}: {e}");
            format!("Failed to {what}: {e}")
        }
    });
}

/// Draws the open dialogs.
pub fn show(ctx: &egui::Context, state: &mut AppState, dialogs: &mut Dialogs) {
    export_options(ctx, state, dialogs);
    slices(ctx, state, dialogs);
    voxelizer(ctx, state, dialogs);
    heightmap(ctx, state, dialogs);
}

fn hint(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(11.0).color(theme::TEXT_DIM));
}

/// Options for mesh exports, then the save dialog.
fn export_options(ctx: &egui::Context, state: &mut AppState, dialogs: &mut Dialogs) {
    let Some(format) = dialogs.export else {
        return;
    };
    let mut scale = dialogs.scale(format);
    let mut action = None;
    egui::Modal::new(Id::new("export_options")).show(ctx, |ui| {
        ui.set_width(400.0);
        title(ui, &format!("Export {}", format.label()));
        hint(ui, format.export_note());
        ui.add_space(8.0);
        let o = &mut dialogs.export_options;
        egui::Grid::new("export_grid")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                ui.label("Scale");
                ui.horizontal(|ui| {
                    ui.add(
                        egui::DragValue::new(&mut scale)
                            .range(0.001..=1000.0)
                            .speed(0.01)
                            .max_decimals(4),
                    );
                    let unit = match format {
                        FileFormat::Glb | FileFormat::Gltf => "m per voxel",
                        FileFormat::Stl => "mm per voxel",
                        _ => "units per voxel",
                    };
                    ui.label(RichText::new(unit).color(theme::TEXT_DIM));
                    if scale != format.default_scale() && ui.small_button("Default").clicked() {
                        scale = format.default_scale();
                    }
                });
                ui.end_row();

                ui.label("Pivot");
                egui::ComboBox::from_id_salt("export_pivot")
                    .selected_text(o.pivot.label())
                    .show_ui(ui, |ui| {
                        for p in Pivot::ALL {
                            ui.selectable_value(&mut o.pivot, p, p.label());
                        }
                    });
                ui.end_row();

                if format == FileFormat::Obj {
                    ui.label("Layout");
                    ui.vertical(|ui| {
                        for l in ObjLayout::ALL {
                            ui.radio_value(&mut o.obj_layout, l, l.label());
                            hint(ui, l.note());
                        }
                    });
                    ui.end_row();
                }
                if matches!(format, FileFormat::Glb | FileFormat::Gltf) {
                    ui.label("Colors");
                    ui.vertical(|ui| {
                        for c in GltfColors::ALL {
                            ui.radio_value(&mut o.gltf_colors, c, c.label());
                        }
                    });
                    ui.end_row();
                }
                if format.allows_per_layer() {
                    ui.label("Files");
                    ui.checkbox(&mut o.per_layer, "One file per visible layer");
                    ui.end_row();
                }
            });
        ui.add_space(4.0);
        hint(
            ui,
            "Hidden faces between voxels are always removed; same-color faces are merged.",
        );
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Export...").clicked() {
                action = Some(true);
            }
            if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                action = Some(false);
            }
        });
    });
    if scale.is_finite() && scale > 0.0 {
        dialogs.set_scale(format, scale);
    }
    match action {
        Some(true) => {
            dialogs.export = None;
            crate::actions::export_with(state, format, &dialogs.options_for(format));
        }
        Some(false) => dialogs.export = None,
        None => {}
    }
}

/// Export Slices: axis and scale, then a folder.
fn slices(ctx: &egui::Context, state: &mut AppState, dialogs: &mut Dialogs) {
    if !dialogs.slices {
        return;
    }
    dialogs.slice_scale = dialogs.slice_scale.clamp(1, MAX_SLICE_SCALE);
    let mut action = None;
    egui::Modal::new(Id::new("export_slices")).show(ctx, |ui| {
        ui.set_width(360.0);
        title(ui, "Export Slices");
        hint(
            ui,
            "One PNG per voxel slice of the visible layers, into a folder.",
        );
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label("Axis");
            for a in SliceAxis::ALL {
                ui.radio_value(&mut dialogs.slice_axis, a, a.label());
            }
        });
        ui.horizontal(|ui| {
            ui.label("Scale");
            ui.add(
                egui::Slider::new(&mut dialogs.slice_scale, 1..=MAX_SLICE_SCALE)
                    .suffix(" px per voxel"),
            );
        });
        if let Some(doc) = state.doc() {
            let b = doc.data.bounds;
            let (w, h) = dialogs.slice_axis.image_size(b);
            let n = match dialogs.slice_axis {
                SliceAxis::X => b.x(),
                SliceAxis::Y => b.y(),
                SliceAxis::Z => b.z(),
            };
            hint(
                ui,
                &format!(
                    "{n} images of {}×{} px",
                    w * dialogs.slice_scale,
                    h * dialogs.slice_scale
                ),
            );
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Choose Folder...").clicked() {
                action = Some(true);
            }
            if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                action = Some(false);
            }
        });
    });
    match action {
        Some(true) => {
            dialogs.slices = false;
            if let Some(dir) = rfd::FileDialog::new()
                .set_title("Export Slices To")
                .pick_folder()
            {
                let result = state.export_slices(&dir, dialogs.slice_axis, dialogs.slice_scale);
                report(state, result, "export slices");
            }
        }
        Some(false) => dialogs.slices = false,
        None => {}
    }
}

fn voxelizer(ctx: &egui::Context, state: &mut AppState, dialogs: &mut Dialogs) {
    let Some(d) = dialogs.voxelizer.as_mut() else {
        return;
    };
    // Live preview: voxelize again whenever the settings change.
    if d.previewed != Some(d.settings) {
        d.preview = Some(d.mesh.preview(&d.settings).map_err(|e| e.to_string()));
        d.previewed = Some(d.settings);
    }
    let mut action = None;
    egui::Modal::new(Id::new("voxelizer_dialog")).show(ctx, |ui| {
        ui.set_width(380.0);
        title(ui, "Voxelizer");
        ui.label(
            RichText::new(format!(
                "{} · {} triangles",
                file_name(&d.mesh.path),
                d.mesh.triangles.len()
            ))
            .color(theme::TEXT_DIM),
        );
        ui.add_space(8.0);
        let s = &mut d.settings;
        ui.checkbox(
            &mut s.voxelize.detect_grid,
            "Use the mesh's own voxel grid when it has one",
        );
        let exact = matches!(&d.preview, Some(Ok(p)) if p.exact);
        ui.add_enabled_ui(!exact, |ui| {
            ui.horizontal(|ui| {
                ui.label("Size (longest side)");
                ui.add(egui::Slider::new(&mut s.voxelize.resolution, 1..=256).suffix(" voxels"));
            });
        });
        ui.add_space(4.0);
        ui.label("Rotation");
        egui::Grid::new("voxelizer_turns").show(ui, |ui| {
            for (axis, label, color) in [
                (0, "X", theme::AXIS_X),
                (1, "Y", theme::AXIS_Y),
                (2, "Z", theme::AXIS_Z),
            ] {
                ui.label(RichText::new(label).color(color));
                ui.label(format!("{}°", s.turns[axis] as u32 * 90));
                if ui.button("Turn 90°").clicked() {
                    s.turns[axis] = (s.turns[axis] + 1) % 4;
                }
                ui.end_row();
            }
        });
        ui.add_space(8.0);
        match &d.preview {
            Some(Ok(p)) => {
                ui.label(
                    RichText::new(format!(
                        "Result: {}×{}×{}, {} voxels{}",
                        p.size.x,
                        p.size.y,
                        p.size.z,
                        p.voxels,
                        if p.exact { " (exact grid)" } else { "" }
                    ))
                    .color(theme::TEXT_STRONG),
                );
            }
            Some(Err(e)) => {
                ui.label(RichText::new(e).color(theme::DANGER));
            }
            None => {}
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let ok = matches!(d.preview, Some(Ok(_)));
            if ui.add_enabled(ok, egui::Button::new("Import")).clicked() {
                action = Some(true);
            }
            if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                action = Some(false);
            }
        });
    });
    match action {
        Some(true) => {
            if let Some(d) = dialogs.voxelizer.take() {
                dialogs.voxelizer_settings = d.settings;
                let result = state.import_mesh(&d.mesh, &d.settings);
                report(state, result, "import");
            }
        }
        Some(false) => dialogs.voxelizer = None,
        None => {}
    }
}

fn heightmap(ctx: &egui::Context, state: &mut AppState, dialogs: &mut Dialogs) {
    let Some(d) = dialogs.heightmap.as_mut() else {
        return;
    };
    let mut action = None;
    egui::Modal::new(Id::new("heightmap_dialog")).show(ctx, |ui| {
        ui.set_width(380.0);
        title(ui, "Import Heightmap");
        ui.label(
            RichText::new(format!(
                "{}: brighter pixels make taller columns; images over 256 pixels are scaled down",
                file_name(&d.path)
            ))
            .color(theme::TEXT_DIM),
        );
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label("Max height");
            ui.add(egui::Slider::new(&mut d.options.max_height, 1..=256).suffix(" voxels"));
        });
        ui.horizontal(|ui| {
            ui.label("Colors");
            let label = d
                .color
                .as_deref()
                .map_or("Default color".to_owned(), file_name);
            ui.label(RichText::new(label).color(theme::TEXT_STRONG));
            if ui.button("Choose...").clicked() {
                if let Some(p) = rfd::FileDialog::new()
                    .set_title("Color Image")
                    .add_filter("Image", &rizxel_app::IMAGE_EXTENSIONS)
                    .pick_file()
                {
                    d.color = Some(p);
                }
            }
            if d.color.is_some() && ui.button("Clear").clicked() {
                d.color = None;
            }
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Import").clicked() {
                action = Some(true);
            }
            if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                action = Some(false);
            }
        });
    });
    match action {
        Some(true) => {
            if let Some(d) = dialogs.heightmap.take() {
                let result = state.import_heightmap(&d.path, d.color.as_deref(), d.options);
                report(state, result, "import the heightmap");
            }
        }
        Some(false) => dialogs.heightmap = None,
        None => {}
    }
}

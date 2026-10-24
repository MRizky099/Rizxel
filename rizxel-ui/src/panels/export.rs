//! Import and export dialogs: the Voxelizer (OBJ/STL/PLY meshes) and
//! heightmap import.

use std::path::PathBuf;

use egui::{Id, RichText};
use rizxel_app::{AppState, HeightmapOptions, PendingMesh, VoxelizerPreview, VoxelizerSettings};

use crate::theme;

/// Open import/export dialogs.
#[derive(Default)]
pub struct Dialogs {
    pub voxelizer: Option<VoxelizerDialog>,
    pub heightmap: Option<HeightmapDialog>,
    /// Last Voxelizer settings, reused for the next mesh.
    pub voxelizer_settings: VoxelizerSettings,
}

impl Dialogs {
    /// True while a dialog blocks the editor (shortcuts are off).
    pub fn is_open(&self) -> bool {
        self.voxelizer.is_some() || self.heightmap.is_some()
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
    voxelizer(ctx, state, dialogs);
    heightmap(ctx, state, dialogs);
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

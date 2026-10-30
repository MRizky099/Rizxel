//! Start-screen thumbnails: a small Render-mode raster render of a project,
//! written next to `recent.json` whenever it is saved or opened.

use std::path::Path;

use eframe::egui_wgpu::wgpu;
use rizxel_app::{AppState, Document, OrbitCamera, RgbaImage};

use crate::render::offscreen::OffscreenRaster;
use crate::viewport;

/// Thumbnail size in pixels (4:3).
pub const SIZE: [u32; 2] = [192, 144];

/// Renders `doc` from the default camera angle, framed on its voxels (or
/// its bounds when empty).
pub fn render(
    raster: &mut OffscreenRaster,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    doc: &Document,
) -> RgbaImage {
    let bounds = doc.data.bounds;
    let (lo, hi) = doc
        .occupied_bounds()
        .unwrap_or((glam::IVec3::ZERO, bounds.0.as_ivec3()));
    let (bmin, _) = rizxel_app::world_bounds(bounds);
    let mut camera = OrbitCamera::default();
    camera.frame_box(
        rizxel_app::orbit_base(bounds),
        bmin + lo.as_vec3(),
        bmin + hi.as_vec3(),
        SIZE[0] as f32 / SIZE[1] as f32,
    );
    let mut frame = viewport::output_frame(doc, &camera, SIZE, true);
    frame.look.transparent = false;
    raster.render(device, queue, &frame, SIZE)
}

/// Renders and writes the thumbnails `state` asked for (saved or opened
/// projects), then clears the list. Without a GPU the list is dropped.
pub fn update(
    state: &mut AppState,
    render_state: Option<&eframe::egui_wgpu::RenderState>,
    raster: &mut Option<OffscreenRaster>,
) {
    if state.thumbnail_requests.is_empty() {
        return;
    }
    let requests = std::mem::take(&mut state.thumbnail_requests);
    let (Some(rs), Some(paths)) = (render_state, state.paths()) else {
        return;
    };
    let raster = raster.get_or_insert_with(|| OffscreenRaster::new(&rs.device));
    for (id, path) in requests {
        let Some(doc) = state.documents.iter().find(|d| d.id() == id) else {
            continue;
        };
        let image = render(raster, &rs.device, &rs.queue, doc);
        let file = paths.thumbnail_file(&path);
        if let Err(e) = save(&file, &image) {
            log::warn!("thumbnail {}: {e}", file.display());
        }
    }
}

fn save(file: &Path, image: &RgbaImage) -> std::io::Result<()> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    rizxel_app::render_job::save_png(file, image)
}

/// Loads a thumbnail PNG as egui image data.
pub fn load(file: &Path) -> Option<egui::ColorImage> {
    let bytes = std::fs::read(file).ok()?;
    let img = image::load_from_memory(&bytes).ok()?.to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    Some(egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumbnail_renders_and_round_trips() {
        let Some(gpu) = crate::render::offscreen::headless_device() else {
            eprintln!("no GPU adapter: skipped");
            return;
        };
        let mut doc = Document::new("t");
        doc.begin_edit();
        doc.edit_voxels(
            0,
            &[rizxel_app::VoxelChange {
                pos: glam::IVec3::new(3, 0, 3),
                old: 0,
                new: 9,
            }],
        );
        doc.commit_edit();
        let mut raster = OffscreenRaster::new(&gpu.device);
        let image = render(&mut raster, &gpu.device, &gpu.queue, &doc);
        assert_eq!([image.width, image.height], SIZE);
        let dir = std::env::temp_dir().join(format!("rizxel_thumb_{}", std::process::id()));
        let file = dir.join("t.png");
        save(&file, &image).unwrap();
        let back = load(&file).unwrap();
        assert_eq!(back.size, [SIZE[0] as usize, SIZE[1] as usize]);
        std::fs::remove_dir_all(&dir).ok();
    }
}

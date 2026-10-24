//! Render outputs started from the Render panel's footer (Phase 11b).
//!
//! *Image* renders the raster scene offscreen and saves it at once. The
//! renderers use the window's wgpu device, so outputs share the GPU with
//! the viewport.

use std::time::Instant;

use eframe::egui_wgpu::{self, wgpu};
use rizxel_app::{render_job, AppState, Document, OrbitCamera, ViewMode};
use rizxel_math::render::OutputKind;

use crate::panels::render::RenderUi;
use crate::render::offscreen::OffscreenRaster;
use crate::viewport;

/// Output renderers, created on first use.
#[derive(Default)]
pub struct Outputs {
    raster: Option<OffscreenRaster>,
}

impl Outputs {
    /// Starts the output the footer asked for. Call once per frame.
    pub fn update(
        &mut self,
        render_state: Option<&egui_wgpu::RenderState>,
        state: &mut AppState,
        ui: &mut RenderUi,
    ) {
        let start = std::mem::take(&mut ui.start);
        std::mem::take(&mut ui.cancel);
        if !start {
            return;
        }
        let Some(rs) = render_state else {
            ui.last_result = Some("No GPU device: nothing was rendered".to_owned());
            return;
        };
        let Some(doc) = state.doc() else { return };
        let ao = state.view.ao(ViewMode::Render);
        let result = match doc.data.render.output.kind {
            OutputKind::Image => self.image(&rs.device, &rs.queue, doc, &state.camera, ao),
            kind => Err(format!("{} output is not available yet", kind.label())),
        };
        let message = match result {
            Ok(m) | Err(m) => m,
        };
        state.status_message = Some(message.clone());
        ui.last_result = Some(message);
    }

    /// *Image*: raster render at the output size, saved as PNG.
    fn image(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        doc: &Document,
        camera: &OrbitCamera,
        ao: bool,
    ) -> Result<String, String> {
        let started = Instant::now();
        let o = &doc.data.render.output;
        let size = [o.width, o.height];
        let frame = viewport::output_frame(doc, camera, size, ao);
        let raster = self
            .raster
            .get_or_insert_with(|| OffscreenRaster::new(device));
        let image = raster.render(device, queue, &frame, size);
        let path = render_job::output_path(o, &doc.name, doc.path.as_deref(), None, "png");
        render_job::save_png(&path, &image)
            .map_err(|e| format!("Could not save {}: {e}", path.display()))?;
        Ok(format!(
            "Saved {} ({}×{}, {:.1} s)",
            path.display(),
            size[0],
            size[1],
            started.elapsed().as_secs_f32()
        ))
    }
}

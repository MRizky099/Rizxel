//! Render outputs started from the Render panel's footer (Phase 11b).
//!
//! *Image* renders the raster scene offscreen and saves it at once.
//! *Photo-realistic* and *Turntable* run as jobs: every UI frame does a
//! slice of GPU work (a few samples of a tile, or one turntable frame), so
//! the window stays responsive, shows progress and can cancel. GIF and
//! sprite sheet encoding run on a worker thread at the end. The renderers
//! use the window's wgpu device; the viewport preview pauses meanwhile.

use std::path::PathBuf;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use eframe::egui_wgpu::{self, wgpu};
use rizxel_app::{render_job, AppState, Document, OrbitCamera, RgbaImage, ViewMode};
use rizxel_math::render::{OutputKind, OutputSettings};

use crate::panels::render::{JobStatus, RenderUi};
use crate::render::offscreen::OffscreenRaster;
use crate::render::pathtrace::{document_params, PathTracer, PhotoJob, PtInputs, SkyCache};
use crate::viewport;

/// GPU time spent on a job per UI frame.
const STEP_BUDGET: Duration = Duration::from_millis(40);

/// Output renderers (created on first use) and the running job.
#[derive(Default)]
pub struct Outputs {
    raster: Option<OffscreenRaster>,
    pt: Option<PathTracer>,
    sky: SkyCache,
    job: Option<Job>,
    /// The preview's pause state before the job paused it.
    was_paused: bool,
}

enum Job {
    Photo {
        job: Box<PhotoJob>,
        path: PathBuf,
        png16: bool,
    },
    Turntable(Box<Turntable>),
}

/// Where files go and what they are called.
#[derive(Clone)]
struct Naming {
    output: OutputSettings,
    project: String,
    project_path: Option<PathBuf>,
}

impl Naming {
    fn of(doc: &Document) -> Self {
        Self {
            output: doc.data.render.output.clone(),
            project: doc.name.clone(),
            project_path: doc.path.clone(),
        }
    }

    fn path(&self, frame: Option<(u32, u32)>, extension: &str) -> PathBuf {
        render_job::output_path(
            &self.output,
            &self.project,
            self.project_path.as_deref(),
            frame,
            extension,
        )
    }
}

struct Turntable {
    doc_id: u64,
    cameras: Vec<OrbitCamera>,
    yaws: Vec<f32>,
    next: usize,
    /// Path traced (else raster) frames.
    traced: Option<PtInputs>,
    current: Option<PhotoJob>,
    frames: Vec<RgbaImage>,
    naming: Naming,
    size: [u32; 2],
    ao: bool,
    started: Instant,
    written: usize,
    /// GIF and sprite sheet being written.
    finishing: Option<JoinHandle<Result<String, String>>>,
}

fn secs(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 60 {
        format!("{}:{:02}", s / 60, s % 60)
    } else {
        format!("{s} s")
    }
}

impl Outputs {
    /// Starts, steps, finishes or cancels the output. Call once per frame.
    pub fn update(
        &mut self,
        ctx: &egui::Context,
        render_state: Option<&egui_wgpu::RenderState>,
        state: &mut AppState,
        ui: &mut RenderUi,
    ) {
        let start = std::mem::take(&mut ui.start);
        let cancel = std::mem::take(&mut ui.cancel);
        let Some(rs) = render_state else {
            if start {
                ui.last_result = Some("No GPU device: nothing was rendered".to_owned());
            }
            return;
        };
        if cancel && self.job.is_some() {
            self.job = None;
            self.finish(state, ui, "Render cancelled".to_owned());
            return;
        }
        if start && self.job.is_none() {
            let result = self.start(&rs.device, &rs.queue, state);
            match result {
                Ok(Some(message)) => self.report(state, ui, message),
                Ok(None) => {
                    self.was_paused = state.render_view.paused;
                    state.render_view.paused = true;
                }
                Err(message) => self.report(state, ui, message),
            }
        }
        if self.job.is_some() {
            match self.step(&rs.device, &rs.queue, state) {
                Ok(Some(message)) => {
                    self.job = None;
                    self.finish(state, ui, message);
                }
                Ok(None) => {
                    ui.job = self.status();
                    ctx.request_repaint();
                }
                Err(message) => {
                    self.job = None;
                    self.finish(state, ui, message);
                }
            }
        } else {
            ui.job = None;
        }
    }

    pub fn is_running(&self) -> bool {
        self.job.is_some()
    }

    fn report(&self, state: &mut AppState, ui: &mut RenderUi, message: String) {
        log::info!("{message}");
        state.status_message = Some(message.clone());
        ui.last_result = Some(message);
    }

    fn finish(&mut self, state: &mut AppState, ui: &mut RenderUi, message: String) {
        state.render_view.paused = self.was_paused;
        ui.job = None;
        self.report(state, ui, message);
    }

    /// Starts the output kind the settings ask for. *Image* finishes here
    /// (`Some(message)`); the others start a job (`None`).
    fn start(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        state: &AppState,
    ) -> Result<Option<String>, String> {
        let doc = state.doc().ok_or("No document")?;
        let o = doc.data.render.output.clone();
        let size = [o.width, o.height];
        let ao = state.view.ao(ViewMode::Render);
        let needs_pt = o.kind == OutputKind::Photo
            || (o.kind == OutputKind::Turntable && o.turntable_path_traced);
        if needs_pt && !state.render_view.path_tracing {
            return Err("The path tracer needs Vulkan or Metal with compute shaders: only Image output works here".into());
        }
        match o.kind {
            OutputKind::Image => self.image(device, queue, doc, &state.camera, ao).map(Some),
            OutputKind::Photo => {
                let inputs = self.inputs(device, queue, doc);
                let params = document_params(
                    doc,
                    &state.camera,
                    size,
                    o.transparent,
                    inputs.sky.as_ref().map(|(_, i)| i.as_ref()),
                );
                let limit = (o.time_limit > 0.0).then(|| Duration::from_secs_f32(o.time_limit));
                let job = Box::new(PhotoJob::new(params, doc.data.render.sample.samples, limit));
                self.job = Some(Job::Photo {
                    job,
                    path: Naming::of(doc).path(None, "png"),
                    png16: o.png16,
                });
                Ok(None)
            }
            OutputKind::Turntable => {
                let yaws = render_job::turntable_yaws(
                    state.camera.yaw,
                    o.turntable_frames,
                    o.turntable_angle,
                );
                let cameras = yaws
                    .iter()
                    .map(|&yaw| OrbitCamera {
                        yaw,
                        auto_ortho: false,
                        ..state.camera
                    })
                    .collect();
                let traced = o
                    .turntable_path_traced
                    .then(|| self.inputs(device, queue, doc));
                self.job = Some(Job::Turntable(Box::new(Turntable {
                    doc_id: doc.id(),
                    cameras,
                    yaws,
                    next: 0,
                    traced,
                    current: None,
                    frames: Vec::new(),
                    naming: Naming::of(doc),
                    size,
                    ao,
                    started: Instant::now(),
                    written: 0,
                    finishing: None,
                })));
                Ok(None)
            }
        }
    }

    /// The document's scene uploaded to the output path tracer.
    fn inputs(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, doc: &Document) -> PtInputs {
        let sky = self.sky.get(&doc.data.render);
        let inputs = PtInputs::from_document(doc, sky);
        let pt = self.pt.get_or_insert_with(|| PathTracer::new(device));
        inputs.upload(pt, device, queue);
        inputs
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
        let path = Naming::of(doc).path(None, "png");
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

    /// One slice of the running job; `Some(message)` when it is done.
    fn step(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        state: &AppState,
    ) -> Result<Option<String>, String> {
        let Some(job) = &mut self.job else {
            return Ok(None);
        };
        match job {
            Job::Photo { job, path, png16 } => {
                let pt = self.pt.get_or_insert_with(|| PathTracer::new(device));
                if !job.step(pt, device, queue, STEP_BUDGET) {
                    return Ok(None);
                }
                let img = job.image();
                let saved = if *png16 {
                    render_job::save_png16(path, img.width, img.height, &img.pixels)
                } else {
                    render_job::save_png(path, &img.to_rgba8())
                };
                saved.map_err(|e| format!("Could not save {}: {e}", path.display()))?;
                Ok(Some(format!(
                    "Saved {} ({}×{}, {} spp, {})",
                    path.display(),
                    img.width,
                    img.height,
                    job.samples(),
                    secs(job.elapsed())
                )))
            }
            Job::Turntable(t) => {
                if let Some(handle) = &t.finishing {
                    if !handle.is_finished() {
                        return Ok(None);
                    }
                    let handle = t.finishing.take().ok_or("lost the GIF writer")?;
                    let extra = handle.join().map_err(|_| "GIF writer crashed")??;
                    let first = t.naming.path(Some((0, t.cameras.len() as u32)), "png");
                    let mut message = format!(
                        "Turntable: {} frames in {}",
                        t.cameras.len(),
                        secs(t.started.elapsed())
                    );
                    if t.written > 0 {
                        message += &format!(", PNGs from {}", first.display());
                    }
                    message += &extra;
                    return Ok(Some(message));
                }
                let Some(doc) = state.documents.iter().find(|d| d.id() == t.doc_id) else {
                    return Err("The document was closed: turntable cancelled".into());
                };
                if t.next < t.cameras.len() {
                    let camera = t.cameras[t.next];
                    let frame = match &t.traced {
                        Some(inputs) => {
                            let pt = self.pt.get_or_insert_with(|| PathTracer::new(device));
                            let job = t.current.get_or_insert_with(|| {
                                inputs.upload(pt, device, queue);
                                let params = document_params(
                                    doc,
                                    &camera,
                                    t.size,
                                    t.naming.output.transparent,
                                    inputs.sky.as_ref().map(|(_, i)| i.as_ref()),
                                );
                                PhotoJob::new(params, t.naming.output.turntable_samples, None)
                            });
                            if !job.step(pt, device, queue, STEP_BUDGET) {
                                return Ok(None);
                            }
                            let img = job.image().to_rgba8();
                            t.current = None;
                            img
                        }
                        None => {
                            let frame = viewport::output_frame(doc, &camera, t.size, t.ao);
                            let raster = self
                                .raster
                                .get_or_insert_with(|| OffscreenRaster::new(device));
                            raster.render(device, queue, &frame, t.size)
                        }
                    };
                    if t.naming.output.turntable_png {
                        let n = t.cameras.len() as u32;
                        let path = t.naming.path(Some((t.next as u32, n)), "png");
                        render_job::save_png(&path, &frame)
                            .map_err(|e| format!("Could not save {}: {e}", path.display()))?;
                        t.written += 1;
                    }
                    t.frames.push(frame);
                    t.next += 1;
                    return Ok(None);
                }
                // All frames done: GIF and sprite sheet on a worker thread.
                let frames = std::mem::take(&mut t.frames);
                let naming = t.naming.clone();
                let yaws = t.yaws.clone();
                t.finishing = Some(std::thread::spawn(move || {
                    finish_turntable(&frames, &naming, &yaws)
                }));
                Ok(None)
            }
        }
    }

    fn status(&self) -> Option<JobStatus> {
        match self.job.as_ref()? {
            Job::Photo { job, .. } => {
                let eta = job
                    .eta()
                    .map_or(String::new(), |d| format!(" · {} left", secs(d)));
                Some(JobStatus {
                    fraction: job.progress(),
                    text: format!("{:.0} %{eta}", job.progress() * 100.0),
                })
            }
            Job::Turntable(t) => {
                let n = t.cameras.len();
                if t.finishing.is_some() {
                    return Some(JobStatus {
                        fraction: 1.0,
                        text: "Writing GIF and sprite sheet…".to_owned(),
                    });
                }
                let within = t.current.as_ref().map_or(0.0, PhotoJob::progress);
                let fraction = ((t.next as f32 + within) / n.max(1) as f32).clamp(0.0, 1.0);
                let eta = if fraction > 0.01 {
                    format!(
                        " · {} left",
                        secs(t.started.elapsed().mul_f32((1.0 - fraction) / fraction))
                    )
                } else {
                    String::new()
                };
                Some(JobStatus {
                    fraction,
                    text: format!("Frame {} / {n}{eta}", (t.next + 1).min(n)),
                })
            }
        }
    }
}

/// Writes the turntable's GIF and sprite sheet (+ JSON) when asked for.
/// Returns the text added to the result message.
fn finish_turntable(frames: &[RgbaImage], naming: &Naming, yaws: &[f32]) -> Result<String, String> {
    let o = &naming.output;
    let mut message = String::new();
    if o.turntable_gif {
        let path = naming.path(None, "gif");
        render_job::save_gif(&path, frames, o.gif_fps)
            .map_err(|e| format!("Could not save {}: {e}", path.display()))?;
        message += &format!(", GIF {}", path.display());
    }
    if o.turntable_sheet {
        let (sheet, layout) = render_job::compose_sheet(frames)?;
        let stem = render_job::output_stem(&o.name, &naming.project, None);
        let mut sheet_naming = naming.clone();
        sheet_naming.output.name = format!("{stem}_sheet");
        let png = sheet_naming.path(None, "png");
        render_job::save_png(&png, &sheet)
            .map_err(|e| format!("Could not save {}: {e}", png.display()))?;
        let image_name = png
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let json = render_job::sheet_json(&layout, &image_name, o.gif_fps, yaws);
        let json_path = sheet_naming.path(None, "json");
        render_job::write_file(&json_path, json.as_bytes())
            .map_err(|e| format!("Could not save {}: {e}", json_path.display()))?;
        message += &format!(", sprite sheet {}", png.display());
    }
    Ok(message)
}

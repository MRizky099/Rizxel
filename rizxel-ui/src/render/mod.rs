//! Native wgpu viewport renderer (Phase 3).
//!
//! The scene is drawn into an offscreen 4x MSAA color + depth target sized to
//! the viewport, resolved, then blitted into egui's render pass through an
//! `egui_wgpu` paint callback. Rendering offscreen keeps depth testing and
//! MSAA independent of eframe's (depth-less, single-sample) window surface.
//!
//! Scene contents:
//! - Procedural "infinity" ground grid (fullscreen shader, `#aeaeae` lines,
//!   red X / blue Z axes, distance fade). From below the ground it only
//!   clears the background, and its lines are drawn over the voxels instead
//!   (see [`eye_below_ground`]).
//! - Thick screen-space lines (yellow model bounds).
//! - Filled translucent triangles (hovered ground cell).
//! - Opaque voxel mesh (Phase 4): greedy-meshed quads with per-vertex palette
//!   colors, flat-shaded in Model mode and Lambert-lit in Render mode. The
//!   vertex/index buffers are re-uploaded only when the mesh key changes.
//! - Reference images (Phase 10b): textured, alpha-blended quads, one
//!   texture per image uploaded once. *Behind* images are depth-tested
//!   against the model after the opaque voxels; *On top* images are drawn
//!   after the translucent layers without a depth test.
//!
//! GPU resources live in egui-wgpu's `CallbackResources`; each frame the UI
//! submits a [`SceneFrame`] describing what to draw.

use std::collections::HashMap;
use std::num::NonZeroU64;
use std::sync::Arc;

pub mod offscreen;
pub mod pathtrace;

use bytemuck::{Pod, Zeroable};
use eframe::egui_wgpu::{self, wgpu};
use egui::Color32;
use glam::{Mat4, Vec2, Vec3};
use rizxel_app::Mesh;

/// Samples per pixel of the offscreen target.
const MSAA_SAMPLES: u32 = 4;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// Initial capacity (in elements) of the dynamic vertex buffers.
const INITIAL_CAPACITY: u64 = 256;

/// Camera + style uniforms. Layout must match `Camera` in `scene.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct CameraUniform {
    view_proj: [[f32; 4]; 4],
    inv_view_proj: [[f32; 4]; 4],
    eye: [f32; 4],
    viewport: [f32; 4],
    grid_origin: [f32; 4],
    background: [f32; 4],
    grid_color: [f32; 4],
    axis_x_color: [f32; 4],
    axis_z_color: [f32; 4],
    /// xyz = unit view direction, w = 1 for orthographic.
    forward: [f32; 4],
    /// xyz = center of the grid fade (the eye, or the orbit target in
    /// orthographic mode).
    focus: [f32; 4],
    /// View toggles as 0 / 1: x = floor grid, y = voxel grid, z = ambient
    /// occlusion, w = transparent background.
    options: [f32; 4],
    /// Render mode key light: xyz = unit direction toward it, w = strength.
    light: [f32; 4],
    /// rgb = key light color.
    light_color: [f32; 4],
    /// rgb = ground color, w = 1 when the ground is drawn (Render mode).
    ground: [f32; 4],
    /// rgb = top color of the background gradient, w = 1 for a gradient.
    background_top: [f32; 4],
}

/// One thick line segment (instanced; see `vs_line`).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct LineInstance {
    pub a: [f32; 3],
    pub b: [f32; 3],
    pub color: [f32; 4],
    pub width_px: f32,
}

impl LineInstance {
    pub fn new(a: Vec3, b: Vec3, color: Color32, width_px: f32) -> Self {
        Self {
            a: a.to_array(),
            b: b.to_array(),
            color: rgba(color),
            width_px,
        }
    }
}

/// Vertex of a filled triangle (see `vs_fill`).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct FillVertex {
    pub pos: [f32; 3],
    pub color: [f32; 4],
}

impl FillVertex {
    pub fn new(pos: Vec3, color: Color32) -> Self {
        Self {
            pos: pos.to_array(),
            color: rgba(color),
        }
    }
}

/// Vertex of the opaque voxel mesh (see `vs_voxel`).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct VoxelVertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub color: [f32; 4],
    /// Index into the material table (see [`MaterialTable`]).
    pub material: u32,
    /// Ambient occlusion: 0 (most occluded) to 1 (open).
    pub ao: f32,
}

/// Vertex of a reference image quad (see `vs_reference`).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct RefVertex {
    pub pos: [f32; 3],
    pub uv: [f32; 2],
    pub opacity: f32,
}

/// Decoded RGBA8 pixels of a reference image.
#[derive(Debug)]
pub struct RefPixels {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// One reference image to draw this frame.
#[derive(Clone, Debug)]
pub struct ReferenceDraw {
    /// Identity of the image: its texture is uploaded once per key.
    pub key: usize,
    pub pixels: Arc<RefPixels>,
    /// World corners of the image's top-left, top-right, bottom-right and
    /// bottom-left pixels.
    pub corners: [Vec3; 4],
    pub opacity: f32,
    /// Drawn over the model without a depth test.
    pub on_top: bool,
}

impl ReferenceDraw {
    /// Two triangles, wound clockwise as seen from the image's front.
    pub fn vertices(&self) -> [RefVertex; 6] {
        let [tl, tr, br, bl] = self.corners;
        let v = |p: Vec3, u: f32, v: f32| RefVertex {
            pos: p.to_array(),
            uv: [u, v],
            opacity: self.opacity,
        };
        [
            v(tl, 0.0, 0.0),
            v(tr, 1.0, 0.0),
            v(br, 1.0, 1.0),
            v(tl, 0.0, 0.0),
            v(br, 1.0, 1.0),
            v(bl, 0.0, 1.0),
        ]
    }
}

/// A reference image's texture on the GPU.
struct RefTexture {
    bind_group: wgpu::BindGroup,
}

/// Number of entries in the material table: slot 0 is the default
/// material, slot `n` the document's material `n - 1`.
pub const MATERIAL_SLOTS: usize = rizxel_app::MAX_MATERIAL_SLOT + 1;

/// One material table entry. Layout must match `Material` in `scene.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct GpuMaterial {
    /// RGBA multiplier of the palette color.
    pub tint: [f32; 4],
    /// rgb = emission, a unused.
    pub emission: [f32; 4],
    /// x = metallic, y = roughness.
    pub params: [f32; 4],
}

impl GpuMaterial {
    pub fn new(m: &rizxel_math::Material) -> Self {
        Self {
            tint: m.tint,
            emission: [m.emission[0], m.emission[1], m.emission[2], 0.0],
            params: [m.metallic, m.roughness, 0.0, 0.0],
        }
    }
}

/// The renderer's material table for a document's materials.
pub type MaterialTable = [GpuMaterial; MATERIAL_SLOTS];

/// Material table: the default material, then `materials` (extra ones
/// beyond the last slot are dropped, matching [`rizxel_app::Document::material_slot`]).
pub fn material_table(materials: &[rizxel_math::Material]) -> Box<MaterialTable> {
    let default = GpuMaterial::new(&rizxel_math::Material::default());
    let mut table = Box::new([default; MATERIAL_SLOTS]);
    for (slot, m) in table[1..].iter_mut().zip(materials) {
        *slot = GpuMaterial::new(m);
    }
    table
}

/// Voxel mesh ready for upload, identified by `key` so the renderer uploads
/// it only once (the UI keys it by document id and revision).
#[derive(Debug, Default)]
pub struct GpuMesh {
    pub key: (u64, u64),
    pub vertices: Vec<VoxelVertex>,
    pub indices: Vec<u32>,
}

impl GpuMesh {
    /// Converts a model-space mesh, translating it by `offset` into world space.
    pub fn from_mesh(key: (u64, u64), mesh: &Mesh, offset: Vec3) -> Self {
        let vertices = mesh
            .positions
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let n = mesh.normals.get(i).copied().unwrap_or(Vec3::Y);
                let c = mesh.colors.get(i).copied().unwrap_or_default();
                VoxelVertex {
                    pos: (*p + offset).to_array(),
                    normal: n.to_array(),
                    color: [c.r, c.g, c.b, c.a].map(|v| v as f32 / 255.0),
                    material: mesh.materials.get(i).copied().unwrap_or(0) as u32,
                    ao: mesh
                        .ao
                        .get(i)
                        .map_or(1.0, |&a| f32::from(a) / f32::from(rizxel_engine::AO_OPEN)),
                }
            })
            .collect();
        Self {
            key,
            vertices,
            indices: mesh.indices.clone(),
        }
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }
}

/// Converts an egui color to straight (non-premultiplied) gamma-space RGBA.
fn rgba(c: Color32) -> [f32; 4] {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    [r, g, b, a].map(|v| v as f32 / 255.0)
}

/// Path-traced preview for this frame (Render mode).
#[derive(Clone)]
pub struct PtFrame {
    pub inputs: pathtrace::PtInputs,
    pub params: pathtrace::PtParams,
    /// Samples already accumulated (0 restarts).
    pub first: u32,
    /// Samples to add this frame (0: show the image as it is).
    pub count: u32,
    /// Filter and tone-map the samples into the shown image this frame.
    pub resolve: bool,
    /// Upscale without filtering (pixelated renders).
    pub nearest: bool,
}

impl std::fmt::Debug for PtFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PtFrame")
            .field("first", &self.first)
            .field("count", &self.count)
            .finish_non_exhaustive()
    }
}

/// Render mode's look from the document's render settings: key light,
/// ground and background (colors in sRGB, like the rest of the scene).
#[derive(Clone, Debug, PartialEq)]
pub struct RenderLook {
    /// Unit direction toward the key light.
    pub light_dir: Vec3,
    /// 1 = the default brightness.
    pub light_strength: f32,
    pub light_color: [f32; 3],
    /// Ground color, `None` without a ground.
    pub ground: Option<[f32; 3]>,
    /// Top color of a gradient background (`SceneFrame::background` is
    /// the bottom).
    pub background_top: Option<[f32; 3]>,
    /// Background pixels get alpha 0 (PNG output).
    pub transparent: bool,
}

impl Default for RenderLook {
    /// The fixed Phase 9 key light; no ground.
    fn default() -> Self {
        Self {
            light_dir: Vec3::new(0.4, 0.85, 0.35).normalize(),
            light_strength: 1.0,
            light_color: [1.0; 3],
            ground: None,
            background_top: None,
            transparent: false,
        }
    }
}

impl RenderLook {
    /// Look of `s`; returns it with the background's bottom color.
    pub fn from_settings(s: &rizxel_math::RenderSettings) -> (Self, Color32) {
        use rizxel_math::render::{sun_direction, BackgroundMode};
        let l = &s.light;
        let d = &s.display;
        let srgb = |c: [f32; 3]| {
            let [r, g, b] = c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
            Color32::from_rgb(r, g, b)
        };
        // No sun: light from straight above, dimmed, so the model still reads.
        let (dir, strength) = if l.sun {
            (
                sun_direction(l.sun_azimuth, l.sun_elevation.max(1.0)),
                (l.sun_intensity / 3.0).min(3.0),
            )
        } else {
            (Vec3::Y, 0.3)
        };
        let (bottom, top) = match d.background {
            BackgroundMode::Color => (d.background_color, None),
            BackgroundMode::Gradient => (d.background_color, Some(d.background_top)),
            BackgroundMode::Sky => (l.sky_color, Some(l.sky_zenith)),
            BackgroundMode::Transparent => (d.background_color, None),
        };
        let look = Self {
            light_dir: dir,
            light_strength: strength,
            light_color: l.sun_color,
            ground: d.ground.then_some(d.ground_color),
            background_top: top,
            transparent: d.background == BackgroundMode::Transparent || s.output.transparent,
        };
        (look, srgb(bottom))
    }
}

/// Everything the renderer needs for one frame. Built by the viewport UI.
#[derive(Clone, Debug)]
pub struct SceneFrame {
    pub view_proj: Mat4,
    pub eye: Vec3,
    /// Orbit target (the grid fades around it in orthographic mode).
    pub target: Vec3,
    /// Unit view direction.
    pub forward: Vec3,
    pub ortho: bool,
    pub distance: f32,
    /// XZ offset of the grid so lines align with voxel boundaries.
    pub grid_origin: Vec2,
    pub render_mode: bool,
    /// Draw the floor grid lines (the background is drawn either way).
    pub floor_grid: bool,
    /// Draw voxel edge lines on the model.
    pub voxel_grid: bool,
    /// Shade with the meshes' ambient occlusion.
    pub ao: bool,
    pub background: Color32,
    pub grid_color: Color32,
    pub axis_x: Color32,
    pub axis_z: Color32,
    pub lines: Vec<LineInstance>,
    pub fills: Vec<FillVertex>,
    /// Opaque voxel mesh in world space (`None` = nothing to draw).
    pub voxels: Option<Arc<GpuMesh>>,
    /// Translucent layer meshes with their alpha, farthest first.
    pub translucent: Vec<(Arc<GpuMesh>, f32)>,
    /// The document's materials (Render mode).
    pub materials: Vec<rizxel_math::Material>,
    /// Visible reference images, in draw order.
    pub references: Vec<ReferenceDraw>,
    /// Render mode light, ground and background.
    pub look: RenderLook,
    /// Maps this frame's NDC y to the full image's (scale, offset): a tile
    /// of a large output keeps the background gradient of the whole image.
    pub screen_y: [f32; 2],
    /// Path-traced preview instead of the raster scene.
    pub pt: Option<PtFrame>,
}

impl SceneFrame {
    fn uniform(&self, size_px: [u32; 2]) -> CameraUniform {
        let v4 = |c: Color32| rgba(c);
        let flag = |on: bool| if on { 1.0 } else { 0.0 };
        CameraUniform {
            view_proj: self.view_proj.to_cols_array_2d(),
            inv_view_proj: self.view_proj.inverse().to_cols_array_2d(),
            eye: self.eye.extend(self.distance).to_array(),
            viewport: [
                size_px[0] as f32,
                size_px[1] as f32,
                if self.render_mode { 1.0 } else { 0.0 },
                grid_fade_distance(self.distance),
            ],
            grid_origin: [
                self.grid_origin.x,
                self.grid_origin.y,
                self.screen_y[0],
                self.screen_y[1],
            ],
            background: v4(self.background),
            grid_color: v4(self.grid_color),
            axis_x_color: v4(self.axis_x),
            axis_z_color: v4(self.axis_z),
            forward: self.forward.extend(flag(self.ortho)).to_array(),
            focus: if self.ortho { self.target } else { self.eye }
                .extend(0.0)
                .to_array(),
            options: [
                flag(self.floor_grid),
                flag(self.voxel_grid),
                flag(self.ao),
                flag(self.look.transparent),
            ],
            light: self
                .look
                .light_dir
                .extend(self.look.light_strength)
                .to_array(),
            light_color: [
                self.look.light_color[0],
                self.look.light_color[1],
                self.look.light_color[2],
                0.0,
            ],
            ground: match self.look.ground {
                Some([r, g, b]) => [r, g, b, 1.0],
                None => [0.0; 4],
            },
            background_top: match self.look.background_top {
                Some([r, g, b]) => [r, g, b, 1.0],
                None => [0.0; 4],
            },
        }
    }
}

/// Whether the camera is under the ground plane (`y = 0`). The ground then
/// lies between the eye and the model, so the grid must not write its depth
/// (it would hide every voxel); `fs_grid_under` draws its lines on top.
/// Must match `eye_below_ground` in `scene.wgsl`.
pub fn eye_below_ground(eye: Vec3) -> bool {
    eye.y < 0.0
}

/// Grid fade radius: grows with zoom so the floor always reaches the horizon
/// smoothly, but never so far that minor lines turn into noise.
pub fn grid_fade_distance(distance: f32) -> f32 {
    (distance * 4.0).clamp(96.0, 3000.0)
}

/// Growable GPU buffer (vertex or index data).
struct DynBuffer {
    buffer: wgpu::Buffer,
    capacity: u64,
    stride: u64,
    usage: wgpu::BufferUsages,
    label: &'static str,
}

impl DynBuffer {
    fn new(device: &wgpu::Device, label: &'static str, stride: u64) -> Self {
        Self::with_usage(device, label, stride, wgpu::BufferUsages::VERTEX)
    }

    fn with_usage(
        device: &wgpu::Device,
        label: &'static str,
        stride: u64,
        usage: wgpu::BufferUsages,
    ) -> Self {
        let usage = usage | wgpu::BufferUsages::COPY_DST;
        Self {
            buffer: Self::alloc(device, label, stride * INITIAL_CAPACITY, usage),
            capacity: INITIAL_CAPACITY,
            stride,
            usage,
            label,
        }
    }

    fn alloc(
        device: &wgpu::Device,
        label: &str,
        size: u64,
        usage: wgpu::BufferUsages,
    ) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size,
            usage,
            mapped_at_creation: false,
        })
    }

    fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, data: &[u8]) {
        let count = data.len() as u64 / self.stride;
        if count > self.capacity {
            self.capacity = count.next_power_of_two();
            self.buffer = Self::alloc(device, self.label, self.capacity * self.stride, self.usage);
        }
        if !data.is_empty() {
            queue.write_buffer(&self.buffer, 0, data);
        }
    }
}

/// Size-dependent offscreen targets.
struct Targets {
    size: [u32; 2],
    msaa_view: wgpu::TextureView,
    depth_view: wgpu::TextureView,
    resolve_view: wgpu::TextureView,
    /// The resolved image (copied out by offscreen renders).
    resolve_texture: wgpu::Texture,
    blit_bind_group: wgpu::BindGroup,
}

/// Vertex and index buffers of one uploaded voxel mesh.
struct MeshSlot {
    vertices: DynBuffer,
    indices: DynBuffer,
    /// Key of the mesh in the buffers (`None` = empty).
    key: Option<(u64, u64)>,
    index_count: u32,
}

impl MeshSlot {
    fn new(device: &wgpu::Device) -> Self {
        Self {
            vertices: DynBuffer::new(
                device,
                "rizxel_voxel_vertices",
                std::mem::size_of::<VoxelVertex>() as u64,
            ),
            indices: DynBuffer::with_usage(
                device,
                "rizxel_voxel_indices",
                std::mem::size_of::<u32>() as u64,
                wgpu::BufferUsages::INDEX,
            ),
            key: None,
            index_count: 0,
        }
    }

    /// Uploads `mesh` when its key differs from the one in the buffers.
    fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, mesh: Option<&GpuMesh>) {
        let Some(mesh) = mesh.filter(|m| !m.indices.is_empty()) else {
            self.key = None;
            self.index_count = 0;
            return;
        };
        if self.key == Some(mesh.key) {
            return;
        }
        self.vertices
            .upload(device, queue, bytemuck::cast_slice(&mesh.vertices));
        self.indices
            .upload(device, queue, bytemuck::cast_slice(&mesh.indices));
        self.key = Some(mesh.key);
        self.index_count = mesh.indices.len() as u32;
    }

    /// Draws the mesh as instance `instance` (the translucent pass reads its
    /// alpha from the instance buffer).
    fn draw(&self, pass: &mut wgpu::RenderPass<'_>, instance: u32) {
        if self.index_count == 0 {
            return;
        }
        pass.set_vertex_buffer(0, self.vertices.buffer.slice(..));
        pass.set_index_buffer(self.indices.buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..self.index_count, 0, instance..instance + 1);
    }
}

/// Persistent GPU state, stored in egui-wgpu's `CallbackResources`.
pub struct ViewportRenderer {
    uniform_buffer: wgpu::Buffer,
    material_buffer: wgpu::Buffer,
    /// Material table on the GPU (re-uploaded only when it changes).
    materials: Option<Box<MaterialTable>>,
    scene_bind_group: wgpu::BindGroup,
    grid_pipeline: wgpu::RenderPipeline,
    /// Grid lines over the voxels when the eye is below the ground.
    grid_under_pipeline: wgpu::RenderPipeline,
    line_pipeline: wgpu::RenderPipeline,
    fill_pipeline: wgpu::RenderPipeline,
    voxel_pipeline: wgpu::RenderPipeline,
    /// Translucent layers: alpha blended, depth tested, no depth write.
    translucent_pipeline: wgpu::RenderPipeline,
    blit_pipeline: wgpu::RenderPipeline,
    blit_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// Reference images depth-tested against the model / drawn over it.
    reference_behind_pipeline: wgpu::RenderPipeline,
    reference_on_top_pipeline: wgpu::RenderPipeline,
    reference_sampler: wgpu::Sampler,
    /// Textures by [`ReferenceDraw::key`].
    reference_textures: HashMap<usize, RefTexture>,
    reference_vertices: DynBuffer,
    /// This frame's references: key and on-top flag, in draw order.
    reference_order: Vec<(usize, bool)>,
    lines: DynBuffer,
    fills: DynBuffer,
    opaque: MeshSlot,
    /// Translucent layer meshes by key.
    translucent: HashMap<(u64, u64), MeshSlot>,
    /// Keys of this frame's translucent meshes, farthest first.
    translucent_order: Vec<(u64, u64)>,
    /// One alpha per translucent draw (instance-rate vertex buffer).
    alphas: DynBuffer,
    targets: Option<Targets>,
    line_count: u32,
    fill_count: u32,
    /// Path tracer of the Render-mode preview (created on first use).
    pt: Option<pathtrace::PathTracer>,
    /// Blit of the path-traced image: tracer generation, nearest, group.
    pt_blit: Option<(u64, bool, wgpu::BindGroup)>,
    /// This frame shows the path-traced image.
    pt_shown: bool,
    linear_sampler: wgpu::Sampler,
}

impl ViewportRenderer {
    /// Creates the renderer and registers it in egui-wgpu's callback resources.
    pub fn install(render_state: &egui_wgpu::RenderState) {
        let renderer = Self::new(&render_state.device, render_state.target_format);
        render_state
            .renderer
            .write()
            .callback_resources
            .insert(renderer);
    }

    pub(crate) fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let scene_shader = device.create_shader_module(wgpu::include_wgsl!("scene.wgsl"));
        let blit_shader = device.create_shader_module(wgpu::include_wgsl!("blit.wgsl"));

        let uniform_size = std::mem::size_of::<CameraUniform>() as u64;
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rizxel_camera"),
            size: uniform_size,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let material_size = std::mem::size_of::<MaterialTable>() as u64;
        let material_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rizxel_materials"),
            size: material_size,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let uniform_entry = |binding, size| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: NonZeroU64::new(size),
            },
            count: None,
        };
        let scene_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rizxel_scene_layout"),
            entries: &[
                uniform_entry(0, uniform_size),
                uniform_entry(1, material_size),
            ],
        });
        let scene_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rizxel_scene"),
            layout: &scene_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: material_buffer.as_entire_binding(),
                },
            ],
        });
        let scene_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("rizxel_scene_pipeline_layout"),
                bind_group_layouts: &[Some(&scene_layout)],
                immediate_size: 0,
            });

        let msaa = wgpu::MultisampleState {
            count: MSAA_SAMPLES,
            mask: !0,
            alpha_to_coverage_enabled: false,
        };
        let depth = |write: bool, compare: wgpu::CompareFunction| {
            Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(write),
                depth_compare: Some(compare),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            })
        };
        let color_target = |blend: Option<wgpu::BlendState>| {
            [Some(wgpu::ColorTargetState {
                format: target_format,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })]
        };
        let opaque = color_target(None);
        let alpha = color_target(Some(wgpu::BlendState::ALPHA_BLENDING));

        let grid_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rizxel_grid"),
            layout: Some(&scene_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &scene_shader,
                entry_point: Some("vs_grid"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: depth(true, wgpu::CompareFunction::Always),
            multisample: msaa,
            fragment: Some(wgpu::FragmentState {
                module: &scene_shader,
                entry_point: Some("fs_grid"),
                compilation_options: Default::default(),
                targets: &opaque,
            }),
            multiview_mask: None,
            cache: None,
        });
        let premultiplied = color_target(Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING));
        let grid_under_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rizxel_grid_under"),
            layout: Some(&scene_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &scene_shader,
                entry_point: Some("vs_grid"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: depth(false, wgpu::CompareFunction::LessEqual),
            multisample: msaa,
            fragment: Some(wgpu::FragmentState {
                module: &scene_shader,
                entry_point: Some("fs_grid_under"),
                compilation_options: Default::default(),
                targets: &premultiplied,
            }),
            multiview_mask: None,
            cache: None,
        });

        let line_attrs = wgpu::vertex_attr_array![
            0 => Float32x3, 1 => Float32x3, 2 => Float32x4, 3 => Float32
        ];
        let line_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rizxel_lines"),
            layout: Some(&scene_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &scene_shader,
                entry_point: Some("vs_line"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<LineInstance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &line_attrs,
                })],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: depth(false, wgpu::CompareFunction::LessEqual),
            multisample: msaa,
            fragment: Some(wgpu::FragmentState {
                module: &scene_shader,
                entry_point: Some("fs_color"),
                compilation_options: Default::default(),
                targets: &alpha,
            }),
            multiview_mask: None,
            cache: None,
        });

        let fill_attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4];
        let fill_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rizxel_fills"),
            layout: Some(&scene_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &scene_shader,
                entry_point: Some("vs_fill"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<FillVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &fill_attrs,
                })],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: depth(false, wgpu::CompareFunction::LessEqual),
            multisample: msaa,
            fragment: Some(wgpu::FragmentState {
                module: &scene_shader,
                entry_point: Some("fs_color"),
                compilation_options: Default::default(),
                targets: &alpha,
            }),
            multiview_mask: None,
            cache: None,
        });

        let voxel_attrs = wgpu::vertex_attr_array![
            0 => Float32x3, 1 => Float32x3, 2 => Float32x4, 3 => Uint32, 4 => Float32
        ];
        let voxel_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<VoxelVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &voxel_attrs,
        };
        // The mesher emits CCW quads seen from outside: cull the backs.
        let voxel_primitive = wgpu::PrimitiveState {
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: Some(wgpu::Face::Back),
            ..Default::default()
        };
        let voxel_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rizxel_voxels"),
            layout: Some(&scene_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &scene_shader,
                entry_point: Some("vs_voxel"),
                compilation_options: Default::default(),
                buffers: &[Some(voxel_layout.clone())],
            },
            primitive: voxel_primitive,
            depth_stencil: depth(true, wgpu::CompareFunction::Less),
            multisample: msaa,
            fragment: Some(wgpu::FragmentState {
                module: &scene_shader,
                entry_point: Some("fs_voxel"),
                compilation_options: Default::default(),
                targets: &opaque,
            }),
            multiview_mask: None,
            cache: None,
        });
        let alpha_attrs = wgpu::vertex_attr_array![7 => Float32];
        let translucent_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rizxel_voxels_translucent"),
            layout: Some(&scene_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &scene_shader,
                entry_point: Some("vs_voxel_translucent"),
                compilation_options: Default::default(),
                buffers: &[
                    Some(voxel_layout),
                    Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<f32>() as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &alpha_attrs,
                    }),
                ],
            },
            primitive: voxel_primitive,
            depth_stencil: depth(false, wgpu::CompareFunction::Less),
            multisample: msaa,
            fragment: Some(wgpu::FragmentState {
                module: &scene_shader,
                entry_point: Some("fs_voxel_translucent"),
                compilation_options: Default::default(),
                targets: &alpha,
            }),
            multiview_mask: None,
            cache: None,
        });

        let blit_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rizxel_blit_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let blit_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("rizxel_blit_pipeline_layout"),
            bind_group_layouts: &[Some(&blit_layout)],
            immediate_size: 0,
        });
        // egui's main pass is single-sampled without depth.
        let blit_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rizxel_blit"),
            layout: Some(&blit_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &blit_shader,
                entry_point: Some("vs_blit"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &blit_shader,
                entry_point: Some("fs_blit"),
                compilation_options: Default::default(),
                targets: &opaque,
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("rizxel_blit_sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        // Reference images: the texture layout is the blit's (texture +
        // filtering sampler) as group 1 next to the scene uniforms.
        let reference_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("rizxel_reference_pipeline_layout"),
                bind_group_layouts: &[Some(&scene_layout), Some(&blit_layout)],
                immediate_size: 0,
            });
        let reference_attrs =
            wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32];
        let reference_pipeline = |label, compare| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&reference_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &scene_shader,
                    entry_point: Some("vs_reference"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<RefVertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &reference_attrs,
                    })],
                },
                // Both sides show; `front_facing` dims the back.
                primitive: wgpu::PrimitiveState {
                    front_face: wgpu::FrontFace::Cw,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: depth(false, compare),
                multisample: msaa,
                fragment: Some(wgpu::FragmentState {
                    module: &scene_shader,
                    entry_point: Some("fs_reference"),
                    compilation_options: Default::default(),
                    targets: &alpha,
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let reference_behind_pipeline =
            reference_pipeline("rizxel_reference_behind", wgpu::CompareFunction::LessEqual);
        let reference_on_top_pipeline =
            reference_pipeline("rizxel_reference_on_top", wgpu::CompareFunction::Always);
        let reference_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("rizxel_reference_sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        Self {
            uniform_buffer,
            material_buffer,
            materials: None,
            scene_bind_group,
            grid_pipeline,
            grid_under_pipeline,
            line_pipeline,
            fill_pipeline,
            voxel_pipeline,
            translucent_pipeline,
            blit_pipeline,
            blit_layout,
            sampler,
            reference_behind_pipeline,
            reference_on_top_pipeline,
            reference_sampler,
            reference_textures: HashMap::new(),
            reference_vertices: DynBuffer::new(
                device,
                "rizxel_reference_vertices",
                std::mem::size_of::<RefVertex>() as u64,
            ),
            reference_order: Vec::new(),
            lines: DynBuffer::new(
                device,
                "rizxel_lines",
                std::mem::size_of::<LineInstance>() as u64,
            ),
            fills: DynBuffer::new(
                device,
                "rizxel_fills",
                std::mem::size_of::<FillVertex>() as u64,
            ),
            opaque: MeshSlot::new(device),
            translucent: HashMap::new(),
            translucent_order: Vec::new(),
            alphas: DynBuffer::new(device, "rizxel_alphas", std::mem::size_of::<f32>() as u64),
            targets: None,
            line_count: 0,
            fill_count: 0,
            pt: None,
            pt_blit: None,
            pt_shown: false,
            linear_sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("rizxel_blit_linear"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
        }
    }

    /// Uploads the frame's voxel meshes whose keys are not on the GPU yet and
    /// frees the slots of meshes no longer drawn. Translucent slots are kept
    /// by key, so re-sorting them never re-uploads.
    fn upload_voxels(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, frame: &SceneFrame) {
        self.opaque.upload(device, queue, frame.voxels.as_deref());
        self.translucent
            .retain(|key, _| frame.translucent.iter().any(|(m, _)| m.key == *key));
        self.translucent_order.clear();
        let mut alphas = Vec::new();
        for (mesh, alpha) in &frame.translucent {
            if mesh.indices.is_empty() {
                continue;
            }
            self.translucent
                .entry(mesh.key)
                .or_insert_with(|| MeshSlot::new(device))
                .upload(device, queue, Some(mesh));
            self.translucent_order.push(mesh.key);
            alphas.push(*alpha);
        }
        self.alphas
            .upload(device, queue, bytemuck::cast_slice(&alphas));
    }

    /// Uploads the textures of new reference images, frees unused ones and
    /// fills the quad vertex buffer.
    fn upload_references(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &SceneFrame,
    ) {
        self.reference_textures
            .retain(|key, _| frame.references.iter().any(|r| r.key == *key));
        self.reference_order.clear();
        let mut vertices = Vec::with_capacity(frame.references.len() * 6);
        for r in &frame.references {
            let p = &r.pixels;
            if p.width == 0 || p.height == 0 || p.pixels.len() != (p.width * p.height * 4) as usize
            {
                continue;
            }
            if !self.reference_textures.contains_key(&r.key) {
                let texture = self.reference_texture(device, queue, p);
                self.reference_textures.insert(r.key, texture);
            }
            self.reference_order.push((r.key, r.on_top));
            vertices.extend(r.vertices());
        }
        self.reference_vertices
            .upload(device, queue, bytemuck::cast_slice(&vertices));
    }

    fn reference_texture(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        p: &RefPixels,
    ) -> RefTexture {
        let size = wgpu::Extent3d {
            width: p.width,
            height: p.height,
            depth_or_array_layers: 1,
        };
        // Gamma-space colors like the rest of the scene (non-sRGB target).
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("rizxel_reference"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &p.pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(p.width * 4),
                rows_per_image: Some(p.height),
            },
            size,
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rizxel_reference"),
            layout: &self.blit_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.reference_sampler),
                },
            ],
        });
        RefTexture { bind_group }
    }

    /// Draws this frame's references with `on_top` set (or not).
    fn draw_references(&self, pass: &mut wgpu::RenderPass<'_>, on_top: bool) {
        let mut pipeline_set = false;
        for (i, (key, top)) in self.reference_order.iter().enumerate() {
            let Some(texture) = self.reference_textures.get(key).filter(|_| *top == on_top) else {
                continue;
            };
            if !pipeline_set {
                pass.set_pipeline(if on_top {
                    &self.reference_on_top_pipeline
                } else {
                    &self.reference_behind_pipeline
                });
                pass.set_vertex_buffer(0, self.reference_vertices.buffer.slice(..));
                pipeline_set = true;
            }
            pass.set_bind_group(1, &texture.bind_group, &[]);
            let first = i as u32 * 6;
            pass.draw(first..first + 6, 0..1);
        }
    }

    /// (Re)creates the offscreen targets when the viewport size changes.
    fn ensure_targets(
        &mut self,
        device: &wgpu::Device,
        size: [u32; 2],
        format: wgpu::TextureFormat,
    ) {
        if self.targets.as_ref().is_some_and(|t| t.size == size) {
            return;
        }
        let extent = wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        };
        let raw_texture = |label, samples, format, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: extent,
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let texture = |label, samples, format, usage| {
            raw_texture(label, samples, format, usage)
                .create_view(&wgpu::TextureViewDescriptor::default())
        };
        let msaa_view = texture(
            "rizxel_msaa",
            MSAA_SAMPLES,
            format,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        );
        let depth_view = texture(
            "rizxel_depth",
            MSAA_SAMPLES,
            DEPTH_FORMAT,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        );
        let resolve_texture = raw_texture(
            "rizxel_resolve",
            1,
            format,
            wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
        );
        let resolve_view = resolve_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let blit_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rizxel_blit"),
            layout: &self.blit_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&resolve_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        self.targets = Some(Targets {
            size,
            msaa_view,
            depth_view,
            resolve_view,
            resolve_texture,
            blit_bind_group,
        });
    }

    /// Adds the frame's samples to the path-traced preview and prepares
    /// its blit.
    fn render_path_traced(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        f: &PtFrame,
    ) {
        let pt = self
            .pt
            .get_or_insert_with(|| pathtrace::PathTracer::new(device));
        f.inputs.upload(pt, device, queue);
        pt.resize(device, f.params.size());
        if f.count > 0 || f.resolve {
            pt.run(
                device,
                queue,
                encoder,
                &f.params,
                f.first..f.first + f.count,
                f.resolve,
            );
        }
        let generation = pt.generation();
        let stale = !matches!(
            &self.pt_blit,
            Some((g, n, _)) if *g == generation && *n == f.nearest
        );
        if stale {
            if let Some(view) = pt.image_view() {
                let sampler = if f.nearest {
                    &self.sampler
                } else {
                    &self.linear_sampler
                };
                let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("rizxel_pt_blit"),
                    layout: &self.blit_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(sampler),
                        },
                    ],
                });
                self.pt_blit = Some((generation, f.nearest, group));
            }
        }
        self.pt_shown = true;
    }

    /// Uploads per-frame data and records the offscreen scene pass.
    pub(crate) fn render_scene(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        frame: &SceneFrame,
        size: [u32; 2],
        format: wgpu::TextureFormat,
    ) {
        if let Some(pt) = &frame.pt {
            self.render_path_traced(device, queue, encoder, pt);
            return;
        }
        self.pt_shown = false;
        self.ensure_targets(device, size, format);
        queue.write_buffer(
            &self.uniform_buffer,
            0,
            bytemuck::bytes_of(&frame.uniform(size)),
        );
        self.lines
            .upload(device, queue, bytemuck::cast_slice(&frame.lines));
        self.fills
            .upload(device, queue, bytemuck::cast_slice(&frame.fills));
        self.line_count = frame.lines.len() as u32;
        self.fill_count = frame.fills.len() as u32;
        self.upload_voxels(device, queue, frame);
        self.upload_references(device, queue, frame);
        let table = material_table(&frame.materials);
        if self.materials.as_ref() != Some(&table) {
            queue.write_buffer(&self.material_buffer, 0, bytemuck::cast_slice(&table[..]));
            self.materials = Some(table);
        }

        let Some(targets) = &self.targets else {
            return;
        };
        let bg = rgba(frame.background).map(f64::from);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("rizxel_scene"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &targets.msaa_view,
                depth_slice: None,
                resolve_target: Some(&targets.resolve_view),
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: bg[0],
                        g: bg[1],
                        b: bg[2],
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Discard,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &targets.depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_bind_group(0, &self.scene_bind_group, &[]);

        pass.set_pipeline(&self.grid_pipeline);
        pass.draw(0..3, 0..1);

        // Voxels after the grid so the depth test orders them against the
        // ground; overlays after the voxels so voxels hide what is behind them.
        if self.opaque.index_count > 0 {
            pass.set_pipeline(&self.voxel_pipeline);
            self.opaque.draw(&mut pass, 0);
        }
        // Tracing references: behind the model, before the translucent
        // layers so those still blend over them.
        self.draw_references(&mut pass, false);
        // Translucent layers after every opaque one, farthest first; each
        // draw is one instance whose alpha comes from the instance buffer.
        if !self.translucent_order.is_empty() {
            pass.set_pipeline(&self.translucent_pipeline);
            pass.set_vertex_buffer(1, self.alphas.buffer.slice(..));
            for (i, key) in self.translucent_order.iter().enumerate() {
                if let Some(slot) = self.translucent.get(key) {
                    slot.draw(&mut pass, i as u32);
                }
            }
        }
        self.draw_references(&mut pass, true);
        if frame.floor_grid && eye_below_ground(frame.eye) {
            pass.set_pipeline(&self.grid_under_pipeline);
            pass.draw(0..3, 0..1);
        }

        if self.fill_count > 0 {
            pass.set_pipeline(&self.fill_pipeline);
            pass.set_vertex_buffer(0, self.fills.buffer.slice(..));
            pass.draw(0..self.fill_count, 0..1);
        }
        if self.line_count > 0 {
            pass.set_pipeline(&self.line_pipeline);
            pass.set_vertex_buffer(0, self.lines.buffer.slice(..));
            pass.draw(0..6, 0..self.line_count);
        }
    }
}

/// Per-frame paint callback handed to egui.
pub struct ViewportCallback {
    pub frame: SceneFrame,
    pub target_format: wgpu::TextureFormat,
    /// Viewport size in physical pixels.
    pub size_px: [u32; 2],
}

impl egui_wgpu::CallbackTrait for ViewportCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let Some(renderer) = resources.get_mut::<ViewportRenderer>() {
            renderer.render_scene(
                device,
                queue,
                encoder,
                &self.frame,
                self.size_px,
                self.target_format,
            );
        }
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        let Some(renderer) = resources.get::<ViewportRenderer>() else {
            return;
        };
        if renderer.pt_shown {
            if let Some((_, _, group)) = &renderer.pt_blit {
                pass.set_pipeline(&renderer.blit_pipeline);
                pass.set_bind_group(0, group, &[]);
                pass.draw(0..3, 0..1);
            }
            return;
        }
        let Some(targets) = &renderer.targets else {
            return;
        };
        pass.set_pipeline(&renderer.blit_pipeline);
        pass.set_bind_group(0, &targets.blit_bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_struct_sizes_match_wgsl() {
        // WGSL struct: 2 mat4 + 14 vec4 = 128 + 224 bytes.
        assert_eq!(std::mem::size_of::<CameraUniform>(), 352);
        // vec3 + vec3 + vec4 + f32 (tightly packed vertex attributes).
        assert_eq!(std::mem::size_of::<LineInstance>(), 44);
        assert_eq!(std::mem::size_of::<FillVertex>(), 28);
        // vec3 + vec3 + vec4 + u32 + f32.
        assert_eq!(std::mem::size_of::<VoxelVertex>(), 48);
        // vec3 + vec2 + f32.
        assert_eq!(std::mem::size_of::<RefVertex>(), 24);
        // 3 vec4 per material, 256 slots.
        assert_eq!(std::mem::size_of::<MaterialTable>(), 48 * 256);
    }

    #[test]
    fn gpu_mesh_is_offset_and_normalized() {
        let mesh = Mesh::unit_cube(rizxel_math::Rgba8::rgb(255, 0, 0));
        let gpu = GpuMesh::from_mesh((1, 2), &mesh, Vec3::new(-4.0, 0.0, -4.0));
        assert_eq!(gpu.key, (1, 2));
        assert_eq!(gpu.vertices.len(), 8);
        assert_eq!(gpu.triangle_count(), 12);
        assert_eq!(gpu.vertices[0].pos, [-4.0, 0.0, -4.0]);
        assert_eq!(gpu.vertices[0].color, [1.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn voxel_faces_are_front_facing_on_screen() {
        // A CCW-from-outside voxel face must be CCW in NDC for the camera in
        // front of it, otherwise back-face culling would hide the model.
        use rizxel_app::{OrbitCamera, ViewFace, ViewTarget};
        let mut volume = rizxel_math::VoxelVolume::new(rizxel_math::BoundSize::new(4, 4, 4));
        volume.set(glam::IVec3::ZERO, 1);
        let mesh = rizxel_engine::mesh_volume(&volume, &rizxel_math::Palette::default());
        for (face, normal) in [
            (ViewFace::Front, Vec3::Z),
            (ViewFace::Right, Vec3::X),
            (ViewFace::Top, Vec3::Y),
        ] {
            let mut cam = OrbitCamera::default();
            cam.snap(ViewTarget::Face(face));
            let vp = cam.view_proj(Vec3::splat(0.5), 1.0);
            let tri = mesh
                .indices
                .chunks_exact(3)
                .find(|t| mesh.normals[t[0] as usize] == normal)
                .unwrap();
            let [a, b, c] = [0, 1, 2].map(|i| vp.project_point3(mesh.positions[tri[i] as usize]));
            let area = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
            assert!(area > 0.0, "{face:?} face is clockwise on screen");
        }
    }

    #[test]
    fn shaders_parse_and_validate() {
        for (name, src) in [
            ("scene", include_str!("scene.wgsl")),
            ("blit", include_str!("blit.wgsl")),
            ("pathtrace", pathtrace::TRACE_SOURCE),
            ("denoise", pathtrace::RESOLVE_SOURCE),
        ] {
            let module = naga::front::wgsl::parse_str(src)
                .unwrap_or_else(|e| panic!("{name}.wgsl: {}", e.emit_to_string(src)));
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::empty(),
            )
            .validate(&module)
            .unwrap_or_else(|e| panic!("{name}.wgsl invalid: {e:?}"));
        }
    }

    #[test]
    fn ground_hides_nothing_from_below() {
        // From below (view cube Bottom face, or orbiting under the model) the
        // ground must not write depth, or it hides every voxel above y = 0.
        use rizxel_app::{OrbitCamera, ViewFace, ViewTarget};

        let target = Vec3::new(0.0, 8.0, 0.0);
        let mut cam = OrbitCamera {
            distance: 60.0,
            ..OrbitCamera::default()
        };
        cam.snap(ViewTarget::Face(ViewFace::Bottom));
        assert!(eye_below_ground(cam.eye(target)));
        cam.pitch = -20.0;
        assert!(eye_below_ground(cam.eye(target)));
        // Looking slightly up at the model from above the ground.
        cam.pitch = -5.0;
        assert!(!eye_below_ground(cam.eye(target)));
        cam.pitch = 30.0;
        assert!(!eye_below_ground(cam.eye(target)));
        assert!(
            !eye_below_ground(Vec3::ZERO),
            "an eye on the ground is above it"
        );
    }

    #[test]
    fn ground_hides_nothing_from_below_in_ortho() {
        // The orthographic eye sits far back on the line of sight, so any
        // upward view is "below the ground" and a level face view is not.
        use rizxel_app::{OrbitCamera, ViewFace, ViewTarget};
        let target = Vec3::new(0.0, 8.0, 0.0);
        let mut cam = OrbitCamera::default();
        cam.toggle_projection();
        cam.snap(ViewTarget::Face(ViewFace::Bottom));
        assert!(eye_below_ground(cam.eye(target)));
        cam.pitch = -5.0;
        assert!(eye_below_ground(cam.eye(target)));
        cam.snap(ViewTarget::Face(ViewFace::Front));
        assert!(!eye_below_ground(cam.eye(target)));
        cam.snap(ViewTarget::Face(ViewFace::Top));
        assert!(!eye_below_ground(cam.eye(target)));
    }

    #[test]
    fn view_toggles_reach_the_uniform() {
        let mut frame = SceneFrame {
            view_proj: Mat4::IDENTITY,
            eye: Vec3::new(1.0, 2.0, 3.0),
            target: Vec3::new(4.0, 5.0, 6.0),
            forward: Vec3::NEG_Z,
            ortho: false,
            distance: 10.0,
            grid_origin: Vec2::ZERO,
            render_mode: false,
            floor_grid: true,
            voxel_grid: false,
            ao: false,
            background: Color32::BLACK,
            grid_color: Color32::WHITE,
            axis_x: Color32::RED,
            axis_z: Color32::BLUE,
            lines: Vec::new(),
            fills: Vec::new(),
            voxels: None,
            translucent: Vec::new(),
            materials: Vec::new(),
            references: Vec::new(),
            look: RenderLook::default(),
            screen_y: [1.0, 0.0],
            pt: None,
        };
        let u = frame.uniform([100, 100]);
        assert_eq!(u.options[..2], [1.0, 0.0]);
        assert_eq!(u.forward, [0.0, 0.0, -1.0, 0.0]);
        assert_eq!(
            u.focus[..3],
            [1.0, 2.0, 3.0],
            "perspective fades around the eye"
        );
        frame.ortho = true;
        frame.floor_grid = false;
        frame.voxel_grid = true;
        let u = frame.uniform([100, 100]);
        assert_eq!(u.options[..2], [0.0, 1.0]);
        assert_eq!(u.forward[3], 1.0);
        assert_eq!(
            u.focus[..3],
            [4.0, 5.0, 6.0],
            "ortho fades around the target"
        );
    }

    #[test]
    fn material_table_puts_the_default_first() {
        let glow = rizxel_math::Material {
            emission: [0.2, 0.8, 1.0],
            ..rizxel_math::Material::new("Neon")
        };
        let table = material_table(std::slice::from_ref(&glow));
        assert_eq!(
            table[0],
            GpuMaterial::new(&rizxel_math::Material::default())
        );
        assert_eq!(table[1].emission, [0.2, 0.8, 1.0, 0.0]);
        assert_eq!(table[2], table[0], "unused slots hold the default");
        // Vertices carry the slot.
        let mut mesh = Mesh::unit_cube(rizxel_math::Rgba8::WHITE);
        mesh.materials = vec![1; 8];
        let gpu = GpuMesh::from_mesh((0, 0), &mesh, Vec3::ZERO);
        assert!(gpu.vertices.iter().all(|v| v.material == 1));
    }

    #[test]
    fn grid_fade_is_clamped() {
        assert_eq!(grid_fade_distance(1.0), 96.0);
        assert_eq!(grid_fade_distance(100.0), 400.0);
        assert_eq!(grid_fade_distance(1e6), 3000.0);
    }

    #[test]
    fn colors_are_straight_alpha() {
        let c = Color32::from_rgba_unmultiplied(255, 0, 0, 128);
        let [r, g, b, a] = rgba(c);
        assert!((r - 1.0).abs() < 0.02 && g == 0.0 && b == 0.0);
        assert!((a - 128.0 / 255.0).abs() < 1e-3);
    }
}

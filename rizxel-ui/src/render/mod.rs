//! Native wgpu viewport renderer (Phase 3).
//!
//! The scene is drawn into an offscreen 4x MSAA color + depth target sized to
//! the viewport, resolved, then blitted into egui's render pass through an
//! `egui_wgpu` paint callback. Rendering offscreen keeps depth testing and
//! MSAA independent of eframe's (depth-less, single-sample) window surface.
//!
//! Scene contents:
//! - Procedural "infinity" ground grid (fullscreen shader, `#aeaeae` lines,
//!   red X / blue Z axes, distance fade).
//! - Thick screen-space lines (yellow model bounds).
//! - Filled translucent triangles (hovered ground cell).
//! - Opaque voxel mesh (Phase 4): greedy-meshed quads with per-vertex palette
//!   colors, flat-shaded in Model mode and Lambert-lit in Render mode. The
//!   vertex/index buffers are re-uploaded only when the mesh key changes.
//!
//! GPU resources live in egui-wgpu's `CallbackResources`; each frame the UI
//! submits a [`SceneFrame`] describing what to draw.

use std::num::NonZeroU64;
use std::sync::Arc;

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

/// Everything the renderer needs for one frame. Built by the viewport UI.
#[derive(Clone, Debug)]
pub struct SceneFrame {
    pub view_proj: Mat4,
    pub eye: Vec3,
    pub distance: f32,
    /// XZ offset of the grid so lines align with voxel boundaries.
    pub grid_origin: Vec2,
    pub render_mode: bool,
    pub background: Color32,
    pub grid_color: Color32,
    pub axis_x: Color32,
    pub axis_z: Color32,
    pub lines: Vec<LineInstance>,
    pub fills: Vec<FillVertex>,
    /// Opaque voxel mesh in world space (`None` = nothing to draw).
    pub voxels: Option<Arc<GpuMesh>>,
}

impl SceneFrame {
    fn uniform(&self, size_px: [u32; 2]) -> CameraUniform {
        let v4 = |c: Color32| rgba(c);
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
            grid_origin: [self.grid_origin.x, self.grid_origin.y, 0.0, 0.0],
            background: v4(self.background),
            grid_color: v4(self.grid_color),
            axis_x_color: v4(self.axis_x),
            axis_z_color: v4(self.axis_z),
        }
    }
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
    blit_bind_group: wgpu::BindGroup,
}

/// Persistent GPU state, stored in egui-wgpu's `CallbackResources`.
pub struct ViewportRenderer {
    uniform_buffer: wgpu::Buffer,
    scene_bind_group: wgpu::BindGroup,
    grid_pipeline: wgpu::RenderPipeline,
    line_pipeline: wgpu::RenderPipeline,
    fill_pipeline: wgpu::RenderPipeline,
    voxel_pipeline: wgpu::RenderPipeline,
    blit_pipeline: wgpu::RenderPipeline,
    blit_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    lines: DynBuffer,
    fills: DynBuffer,
    voxel_vertices: DynBuffer,
    voxel_indices: DynBuffer,
    targets: Option<Targets>,
    line_count: u32,
    fill_count: u32,
    /// Key of the mesh currently in the voxel buffers.
    voxel_key: Option<(u64, u64)>,
    voxel_index_count: u32,
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

    fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let scene_shader = device.create_shader_module(wgpu::include_wgsl!("scene.wgsl"));
        let blit_shader = device.create_shader_module(wgpu::include_wgsl!("blit.wgsl"));

        let uniform_size = std::mem::size_of::<CameraUniform>() as u64;
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rizxel_camera"),
            size: uniform_size,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let scene_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rizxel_scene_layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: NonZeroU64::new(uniform_size),
                },
                count: None,
            }],
        });
        let scene_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rizxel_scene"),
            layout: &scene_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
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
            0 => Float32x3, 1 => Float32x3, 2 => Float32x4
        ];
        let voxel_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rizxel_voxels"),
            layout: Some(&scene_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &scene_shader,
                entry_point: Some("vs_voxel"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<VoxelVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &voxel_attrs,
                })],
            },
            // The mesher emits CCW quads seen from outside: cull the backs.
            primitive: wgpu::PrimitiveState {
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
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

        Self {
            uniform_buffer,
            scene_bind_group,
            grid_pipeline,
            line_pipeline,
            fill_pipeline,
            voxel_pipeline,
            blit_pipeline,
            blit_layout,
            sampler,
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
            voxel_vertices: DynBuffer::new(
                device,
                "rizxel_voxel_vertices",
                std::mem::size_of::<VoxelVertex>() as u64,
            ),
            voxel_indices: DynBuffer::with_usage(
                device,
                "rizxel_voxel_indices",
                std::mem::size_of::<u32>() as u64,
                wgpu::BufferUsages::INDEX,
            ),
            targets: None,
            line_count: 0,
            fill_count: 0,
            voxel_key: None,
            voxel_index_count: 0,
        }
    }

    /// Uploads the voxel mesh when its key differs from the one on the GPU.
    fn upload_voxels(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        mesh: Option<&GpuMesh>,
    ) {
        let Some(mesh) = mesh.filter(|m| !m.indices.is_empty()) else {
            self.voxel_key = None;
            self.voxel_index_count = 0;
            return;
        };
        if self.voxel_key == Some(mesh.key) {
            return;
        }
        self.voxel_vertices
            .upload(device, queue, bytemuck::cast_slice(&mesh.vertices));
        self.voxel_indices
            .upload(device, queue, bytemuck::cast_slice(&mesh.indices));
        self.voxel_key = Some(mesh.key);
        self.voxel_index_count = mesh.indices.len() as u32;
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
        let texture = |label, samples, format, usage| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: extent,
                    mip_level_count: 1,
                    sample_count: samples,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
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
        let resolve_view = texture(
            "rizxel_resolve",
            1,
            format,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        );
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
            blit_bind_group,
        });
    }

    /// Uploads per-frame data and records the offscreen scene pass.
    fn render_scene(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        frame: &SceneFrame,
        size: [u32; 2],
        format: wgpu::TextureFormat,
    ) {
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
        self.upload_voxels(device, queue, frame.voxels.as_deref());

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
        if self.voxel_index_count > 0 {
            pass.set_pipeline(&self.voxel_pipeline);
            pass.set_vertex_buffer(0, self.voxel_vertices.buffer.slice(..));
            pass.set_index_buffer(
                self.voxel_indices.buffer.slice(..),
                wgpu::IndexFormat::Uint32,
            );
            pass.draw_indexed(0..self.voxel_index_count, 0, 0..1);
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
        // WGSL struct: 2 mat4 + 7 vec4 = 128 + 112 bytes.
        assert_eq!(std::mem::size_of::<CameraUniform>(), 240);
        // vec3 + vec3 + vec4 + f32 (tightly packed vertex attributes).
        assert_eq!(std::mem::size_of::<LineInstance>(), 44);
        assert_eq!(std::mem::size_of::<FillVertex>(), 28);
        // vec3 + vec3 + vec4.
        assert_eq!(std::mem::size_of::<VoxelVertex>(), 40);
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

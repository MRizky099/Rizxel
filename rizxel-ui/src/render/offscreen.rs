//! Offscreen rendering for the *Image* output (Phase 11b): the Render-mode
//! raster scene drawn at any size with 4x MSAA and read back as RGBA8.
//!
//! Large images are drawn in tiles of at most [`TILE`] pixels: each tile
//! crops the projection to its part of the frame, so a 8192² render never
//! needs more GPU memory than one tile's MSAA targets.

use eframe::egui_wgpu::wgpu;
use glam::Mat4;
use rizxel_app::RgbaImage;

use super::{SceneFrame, ViewportRenderer};

/// Pixel format of rendered images (straight RGBA after the readback).
pub const OUTPUT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// Largest tile side.
pub const TILE: u32 = 2048;

/// A device and queue without a window, for tests, the `render` example
/// and the benchmark. `None` when the machine has no usable adapter (CI).
pub fn headless_device() -> Option<(wgpu::Device, wgpu::Queue, wgpu::AdapterInfo)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        ..Default::default()
    }))
    .ok()?;
    let info = adapter.get_info();
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("rizxel_headless"),
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .ok()?;
    Some((device, queue, info))
}

/// Copies `size` pixels of an RGBA8 `texture` at `origin` into a tightly
/// packed buffer (blocks until the GPU is done).
pub fn read_rgba8(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    origin: [u32; 2],
    size: [u32; 2],
) -> Vec<u8> {
    let [w, h] = size;
    let row = w * 4;
    let padded =
        row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("rizxel_readback"),
        size: u64::from(padded) * u64::from(h),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d {
                x: origin[0],
                y: origin[1],
                z: 0,
            },
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(h),
            },
        },
        wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);
    let bytes = map_read(device, &buffer);
    let mut out = Vec::with_capacity((row * h) as usize);
    for y in 0..h as usize {
        let start = y * padded as usize;
        out.extend_from_slice(&bytes[start..start + row as usize]);
    }
    out
}

/// Maps a `MAP_READ` buffer and returns its bytes (blocking).
pub fn map_read(device: &wgpu::Device, buffer: &wgpu::Buffer) -> Vec<u8> {
    let (tx, rx) = std::sync::mpsc::channel();
    buffer.map_async(wgpu::MapMode::Read, .., move |r| {
        let _ = tx.send(r);
    });
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    let mapped = matches!(rx.recv(), Ok(Ok(())));
    let bytes = if mapped {
        buffer
            .get_mapped_range(..)
            .map(|v| v.to_vec())
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    if mapped {
        buffer.unmap();
    }
    let len = buffer.size() as usize;
    if bytes.len() == len {
        bytes
    } else {
        vec![0; len]
    }
}

/// `view_proj` cropped to the pixels `x0..x1`, `y0..y1` (top-left origin)
/// of a `size` image: that region fills the whole clip space.
pub fn crop_view_proj(view_proj: Mat4, size: [u32; 2], tile: [u32; 4]) -> Mat4 {
    let [w, h] = size.map(|v| v.max(1) as f32);
    let [x0, y0, x1, y1] = tile.map(|v| v as f32);
    // NDC of the tile edges (y up).
    let (nx0, nx1) = (x0 / w * 2.0 - 1.0, x1 / w * 2.0 - 1.0);
    let (ny0, ny1) = (1.0 - y1 / h * 2.0, 1.0 - y0 / h * 2.0);
    let sx = 2.0 / (nx1 - nx0);
    let sy = 2.0 / (ny1 - ny0);
    let crop = Mat4::from_cols_array(&[
        sx,
        0.0,
        0.0,
        0.0,
        0.0,
        sy,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
        0.0,
        -(nx0 + nx1) * 0.5 * sx,
        -(ny0 + ny1) * 0.5 * sy,
        0.0,
        1.0,
    ]);
    crop * view_proj
}

/// [`SceneFrame::screen_y`] of tile `tile` of a `size` image whose own
/// mapping is `full`.
pub fn tile_screen_y(full: [f32; 2], size: [u32; 2], tile: [u32; 4]) -> [f32; 2] {
    let h = size[1].max(1) as f32;
    let (ny0, ny1) = (
        1.0 - tile[3] as f32 / h * 2.0,
        1.0 - tile[1] as f32 / h * 2.0,
    );
    let (scale, offset) = ((ny1 - ny0) * 0.5, (ny0 + ny1) * 0.5);
    [full[0] * scale, full[0] * offset + full[1]]
}

/// Tiles covering a `size` image, row by row: `[x0, y0, x1, y1]`.
pub fn tiles(size: [u32; 2], tile: u32) -> Vec<[u32; 4]> {
    let tile = tile.max(1);
    let mut out = Vec::new();
    for y in (0..size[1]).step_by(tile as usize) {
        for x in (0..size[0]).step_by(tile as usize) {
            out.push([x, y, (x + tile).min(size[0]), (y + tile).min(size[1])]);
        }
    }
    out
}

/// The raster renderer drawing into offscreen RGBA8 targets.
pub struct OffscreenRaster {
    renderer: ViewportRenderer,
}

impl OffscreenRaster {
    pub fn new(device: &wgpu::Device) -> Self {
        Self {
            renderer: ViewportRenderer::new(device, OUTPUT_FORMAT),
        }
    }

    /// Renders `frame` at `size` pixels. With `frame.look.transparent` the
    /// background has alpha 0 and the image is straight (unpremultiplied).
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &SceneFrame,
        size: [u32; 2],
    ) -> RgbaImage {
        let size = size.map(|v| v.max(1));
        let mut pixels = vec![0u8; (size[0] * size[1] * 4) as usize];
        for t in tiles(size, TILE) {
            let tile_size = [t[2] - t[0], t[3] - t[1]];
            let mut tile_frame = frame.clone();
            tile_frame.view_proj = crop_view_proj(frame.view_proj, size, t);
            tile_frame.screen_y = tile_screen_y(frame.screen_y, size, t);
            let mut encoder = device.create_command_encoder(&Default::default());
            self.renderer.render_scene(
                device,
                queue,
                &mut encoder,
                &tile_frame,
                tile_size,
                OUTPUT_FORMAT,
            );
            queue.submit([encoder.finish()]);
            let Some(targets) = &self.renderer.targets else {
                continue;
            };
            let tile_pixels =
                read_rgba8(device, queue, &targets.resolve_texture, [0, 0], tile_size);
            let row = (tile_size[0] * 4) as usize;
            for (y, src) in tile_pixels.chunks_exact(row).enumerate() {
                let start = (((t[1] as usize + y) * size[0] as usize) + t[0] as usize) * 4;
                pixels[start..start + row].copy_from_slice(src);
            }
        }
        if frame.look.transparent {
            rizxel_app::render_job::unpremultiply(&mut pixels);
        } else {
            for px in pixels.chunks_exact_mut(4) {
                px[3] = 255;
            }
        }
        RgbaImage {
            width: size[0],
            height: size[1],
            pixels,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;

    /// Document with a 4³ block of color 9 in the middle of 8³ bounds.
    fn block_doc() -> rizxel_app::Document {
        let mut doc = rizxel_app::Document::new("block");
        doc.set_bounds(rizxel_math::BoundSize::new(8, 8, 8), false);
        let changes: Vec<_> = (2..6)
            .flat_map(|x| (0..4).flat_map(move |y| (2..6).map(move |z| glam::IVec3::new(x, y, z))))
            .map(|pos| rizxel_app::VoxelChange {
                pos,
                old: 0,
                new: 9,
            })
            .collect();
        doc.begin_edit();
        doc.edit_voxels(0, &changes);
        doc.commit_edit();
        doc
    }

    fn camera() -> rizxel_app::OrbitCamera {
        rizxel_app::OrbitCamera {
            distance: 20.0,
            ..Default::default()
        }
    }

    #[test]
    fn image_has_the_output_size_and_a_transparent_background() {
        let Some((device, queue, info)) = headless_device() else {
            eprintln!("no GPU adapter: skipped");
            return;
        };
        eprintln!("adapter: {} ({:?})", info.name, info.backend);
        let mut doc = block_doc();
        let mut s = doc.data.render.clone();
        s.output.transparent = true;
        doc.set_render(s, false);
        let frame = crate::viewport::output_frame(&doc, &camera(), [64, 48], true);
        assert!(frame.look.transparent);
        let mut raster = OffscreenRaster::new(&device);
        let img = raster.render(&device, &queue, &frame, [64, 48]);
        assert_eq!((img.width, img.height), (64, 48));
        assert_eq!(img.pixels.len(), 64 * 48 * 4);
        assert_eq!(img.pixel(0, 0)[3], 0, "background is transparent");
        assert_eq!(img.pixel(32, 24)[3], 255, "the model is opaque");

        // Opaque output: the background is the gradient's color.
        let mut s = doc.data.render.clone();
        s.output.transparent = false;
        doc.set_render(s, false);
        let frame = crate::viewport::output_frame(&doc, &camera(), [64, 48], true);
        let img = raster.render(&device, &queue, &frame, [64, 48]);
        assert_eq!(img.pixel(0, 0)[3], 255);
        // Tiled renders stitch to the same picture.
        let tiled = {
            let mut r = OffscreenRaster::new(&device);
            let mut out = img.clone();
            out.pixels.fill(0);
            for t in tiles([64, 48], 20) {
                let mut f = frame.clone();
                f.view_proj = crop_view_proj(frame.view_proj, [64, 48], t);
                f.screen_y = tile_screen_y(frame.screen_y, [64, 48], t);
                let part = r.render(&device, &queue, &f, [t[2] - t[0], t[3] - t[1]]);
                for y in t[1]..t[3] {
                    for x in t[0]..t[2] {
                        let i = ((y * 64 + x) * 4) as usize;
                        let p = part.pixel(x - t[0], y - t[1]);
                        out.pixels[i..i + 4].copy_from_slice(&p);
                    }
                }
            }
            out
        };
        let differing = img
            .pixels
            .chunks_exact(4)
            .zip(tiled.pixels.chunks_exact(4))
            .filter(|(a, b)| a.iter().zip(b.iter()).any(|(x, y)| x.abs_diff(*y) > 8))
            .count();
        assert!(differing < 64 * 48 / 50, "{differing} pixels differ");
    }

    #[test]
    fn output_framing_matches_the_viewport() {
        let doc = block_doc();
        let cam = camera();
        let fov = doc.data.render.camera.fov.to_radians();
        let target = rizxel_app::orbit_base(doc.data.bounds) + cam.pan;
        // A 300×150 viewport anywhere on screen frames like a 1200×600 image.
        let rect = egui::Rect::from_min_size(egui::pos2(40.0, 70.0), egui::vec2(300.0, 150.0));
        let view = crate::viewport::Projection::with_fov(&cam, target, rect, fov);
        let out = crate::viewport::output_projection(&doc, &cam, [1200, 600]);
        assert!(view.view_proj().abs_diff_eq(out.view_proj(), 1e-5));
        assert_eq!(view.eye(), out.eye());
    }

    #[test]
    fn tiles_cover_the_image_once() {
        let t = tiles([5000, 3000], 2048);
        assert_eq!(t.len(), 3 * 2);
        let area: u64 = t
            .iter()
            .map(|r| u64::from(r[2] - r[0]) * u64::from(r[3] - r[1]))
            .sum();
        assert_eq!(area, 5000 * 3000);
        assert_eq!(tiles([10, 10], 2048), vec![[0, 0, 10, 10]]);
    }

    #[test]
    fn cropped_projection_maps_the_tile_to_clip_space() {
        let vp = camera().view_proj(Vec3::ZERO, 2.0);
        let size = [400, 200];
        // A point drawn at pixel (300, 50) of the full image...
        let p = vp.inverse().project_point3(Vec3::new(0.5, 0.5, 0.5));
        // ...is at the center of the tile [200, 0, 400, 100].
        let c = crop_view_proj(vp, size, [200, 0, 400, 100]).project_point3(p);
        assert!(c.x.abs() < 1e-4 && c.y.abs() < 1e-4, "{c:?}");
        // The whole image as one tile changes nothing.
        let same = crop_view_proj(vp, size, [0, 0, 400, 200]);
        assert!(same.abs_diff_eq(vp, 1e-5));
    }
}

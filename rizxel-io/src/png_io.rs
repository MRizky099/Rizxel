//! Minimal RGBA PNG encode/decode (palette textures, Goxel blocks).

use std::io::Cursor;

use crate::RzxlError;

/// Largest accepted PNG side when decoding (palette textures are 256×1,
/// Goxel blocks 64×64; textures may be larger).
const MAX_SIDE: u32 = 8192;

/// Decoded image: width, height and tightly packed RGBA8 pixels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RgbaImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl RgbaImage {
    /// RGBA of pixel `(x, y)`, clamped to the image.
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let x = x.min(self.width.saturating_sub(1));
        let y = y.min(self.height.saturating_sub(1));
        let i = ((y * self.width + x) * 4) as usize;
        match self.pixels.get(i..i + 4) {
            Some(p) => [p[0], p[1], p[2], p[3]],
            None => [0; 4],
        }
    }
}

fn png_error(e: impl std::fmt::Display) -> RzxlError {
    RzxlError::Invalid(format!("PNG: {e}"))
}

/// Encodes RGBA8 pixels as a PNG.
pub fn encode_rgba(width: u32, height: u32, pixels: &[u8]) -> Result<Vec<u8>, RzxlError> {
    if pixels.len() != (width * height * 4) as usize {
        return Err(png_error("pixel buffer does not match the image size"));
    }
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(png_error)?;
        writer.write_image_data(pixels).map_err(png_error)?;
    }
    Ok(out)
}

/// Decodes any 8/16-bit PNG (gray, gray+alpha, RGB, RGBA, indexed) to RGBA8.
pub fn decode_rgba(bytes: &[u8]) -> Result<RgbaImage, RzxlError> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(png_error)?;
    let (w, h) = (reader.info().width, reader.info().height);
    if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE {
        return Err(png_error(format!("unsupported image size {w}×{h}")));
    }
    let size = reader
        .output_buffer_size()
        .ok_or_else(|| png_error("image too large"))?;
    let mut buf = vec![0; size];
    let info = reader.next_frame(&mut buf).map_err(png_error)?;
    let channels = match info.color_type {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Indexed => return Err(png_error("palette was not expanded")),
    };
    let mut pixels = Vec::with_capacity((w * h * 4) as usize);
    for row in buf.chunks(info.line_size).take(h as usize) {
        for px in row.chunks_exact(channels).take(w as usize) {
            pixels.extend_from_slice(&match *px {
                [g] => [g, g, g, 255],
                [g, a] => [g, g, g, a],
                [r, g, b] => [r, g, b, 255],
                [r, g, b, a] => [r, g, b, a],
                _ => [0, 0, 0, 0],
            });
        }
    }
    if pixels.len() != (w * h * 4) as usize {
        return Err(png_error("truncated image data"));
    }
    Ok(RgbaImage {
        width: w,
        height: h,
        pixels,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgba_roundtrip() {
        let pixels: Vec<u8> = (0..64u8).collect();
        let png = encode_rgba(4, 4, &pixels).unwrap();
        let img = decode_rgba(&png).unwrap();
        assert_eq!((img.width, img.height), (4, 4));
        assert_eq!(img.pixels, pixels);
        assert_eq!(img.pixel(1, 0), [4, 5, 6, 7]);
        assert!(encode_rgba(4, 4, &pixels[..10]).is_err());
        assert!(decode_rgba(b"not a png").is_err());
    }
}

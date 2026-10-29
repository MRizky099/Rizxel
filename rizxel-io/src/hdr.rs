//! Radiance `.hdr` (RGBE) reader for the path tracer's sky image
//! (equirectangular environment maps). Supports the usual `-Y h +X w`
//! orientation, flat and run-length encoded scanlines.

use crate::RzxlError;

/// Largest accepted side (an 8K × 4K map is plenty for a sky).
pub const MAX_HDR_SIDE: u32 = 8192;

/// Linear RGB pixels, row by row from the top.
#[derive(Debug, Clone, PartialEq)]
pub struct HdrImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<[f32; 3]>,
}

fn bad(msg: impl Into<String>) -> RzxlError {
    RzxlError::Invalid(format!("HDR: {}", msg.into()))
}

fn rgbe(p: [u8; 4]) -> [f32; 3] {
    if p[3] == 0 {
        return [0.0; 3];
    }
    let f = 2f32.powi(i32::from(p[3]) - 136);
    [p[0], p[1], p[2]].map(|c| (f32::from(c) + 0.5) * f)
}

/// Decodes a Radiance HDR file.
pub fn read_hdr(bytes: &[u8]) -> Result<HdrImage, RzxlError> {
    // Header lines up to an empty line, then the resolution line.
    let mut pos = 0;
    let line = |pos: &mut usize| -> Result<String, RzxlError> {
        let start = *pos;
        let end = bytes[start..]
            .iter()
            .position(|&b| b == b'\n')
            .ok_or_else(|| bad("truncated header"))?;
        *pos = start + end + 1;
        Ok(String::from_utf8_lossy(&bytes[start..start + end])
            .trim_end_matches('\r')
            .to_owned())
    };
    let magic = line(&mut pos)?;
    if !magic.starts_with("#?") {
        return Err(bad("not a Radiance file"));
    }
    loop {
        let l = line(&mut pos)?;
        if l.is_empty() {
            break;
        }
        if let Some(format) = l.strip_prefix("FORMAT=") {
            if format.trim() != "32-bit_rle_rgbe" {
                return Err(bad(format!("unsupported format {format}")));
            }
        }
    }
    let res = line(&mut pos)?;
    let parts: Vec<&str> = res.split_whitespace().collect();
    let (height, width) = match parts.as_slice() {
        ["-Y", h, "+X", w] => (h.parse::<u32>(), w.parse::<u32>()),
        _ => return Err(bad(format!("unsupported orientation {res:?}"))),
    };
    let (height, width) = (
        height.map_err(|_| bad("bad height"))?,
        width.map_err(|_| bad("bad width"))?,
    );
    if width == 0 || height == 0 || width > MAX_HDR_SIDE || height > MAX_HDR_SIDE {
        return Err(bad(format!("unsupported size {width}×{height}")));
    }
    let data = &bytes[pos..];
    let mut pixels = Vec::with_capacity((width * height) as usize);
    let mut at = 0usize;
    let mut scan = vec![[0u8; 4]; width as usize];
    let take = |at: &mut usize, n: usize| -> Result<&[u8], RzxlError> {
        let s = data
            .get(*at..*at + n)
            .ok_or_else(|| bad("truncated pixels"))?;
        *at += n;
        Ok(s)
    };
    for _ in 0..height {
        let head = data
            .get(at..at + 4)
            .ok_or_else(|| bad("truncated pixels"))?;
        let rle = (8..32768).contains(&width)
            && head[0] == 2
            && head[1] == 2
            && (usize::from(head[2]) << 8 | usize::from(head[3])) == width as usize;
        if rle {
            at += 4;
            for channel in 0..4 {
                let mut x = 0usize;
                while x < width as usize {
                    let count = take(&mut at, 1)?[0] as usize;
                    if count > 128 {
                        let run = count - 128;
                        let value = take(&mut at, 1)?[0];
                        if x + run > width as usize {
                            return Err(bad("run past the scanline"));
                        }
                        for px in &mut scan[x..x + run] {
                            px[channel] = value;
                        }
                        x += run;
                    } else {
                        if count == 0 || x + count > width as usize {
                            return Err(bad("bad literal run"));
                        }
                        let values = take(&mut at, count)?;
                        for (px, v) in scan[x..x + count].iter_mut().zip(values) {
                            px[channel] = *v;
                        }
                        x += count;
                    }
                }
            }
        } else {
            for px in scan.iter_mut() {
                let p = take(&mut at, 4)?;
                *px = [p[0], p[1], p[2], p[3]];
            }
        }
        pixels.extend(scan.iter().map(|&p| rgbe(p)));
    }
    Ok(HdrImage {
        width,
        height,
        pixels,
    })
}

impl HdrImage {
    /// Irradiance on an upward-facing surface lit by this equirectangular
    /// sky's upper half (rows above the horizon).
    pub fn up_irradiance(&self) -> [f32; 3] {
        let (w, h) = (self.width as usize, self.height as usize);
        let mut sum = [0f64; 3];
        for y in 0..h / 2 {
            let theta = (y as f64 + 0.5) / h as f64 * std::f64::consts::PI;
            // Solid angle of one texel times cos(theta).
            let weight = theta.cos()
                * theta.sin()
                * (std::f64::consts::PI / h as f64)
                * (2.0 * std::f64::consts::PI / w as f64);
            for px in &self.pixels[y * w..(y + 1) * w] {
                for c in 0..3 {
                    sum[c] += f64::from(px[c]) * weight;
                }
            }
        }
        sum.map(|v| v as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(w: u32, h: u32) -> Vec<u8> {
        format!("#?RADIANCE\nFORMAT=32-bit_rle_rgbe\n\n-Y {h} +X {w}\n").into_bytes()
    }

    #[test]
    fn flat_and_rle_scanlines_decode() {
        // Flat: 2×1, pixel (1.0, 0.5, 0.25) = mantissas 128, 64, 32 at e=129.
        let mut flat = header(2, 1);
        flat.extend_from_slice(&[128, 64, 32, 129, 0, 0, 0, 0]);
        let img = read_hdr(&flat).unwrap();
        assert_eq!((img.width, img.height), (2, 1));
        assert!((img.pixels[0][0] - 1.0).abs() < 0.01);
        assert!((img.pixels[0][1] - 0.5).abs() < 0.01);
        assert_eq!(img.pixels[1], [0.0; 3]);

        // RLE: 8×1 scanline, every channel one run of 8.
        let mut rle = header(8, 1);
        rle.extend_from_slice(&[2, 2, 0, 8]);
        for value in [128u8, 128, 128, 129] {
            rle.extend_from_slice(&[128 + 8, value]);
        }
        let img = read_hdr(&rle).unwrap();
        assert_eq!(img.pixels.len(), 8);
        assert!(img.pixels.iter().all(|p| (p[2] - 1.0).abs() < 0.01));

        // A uniform sky of radiance 1 gives an irradiance of about pi.
        let sky = HdrImage {
            width: 64,
            height: 32,
            pixels: vec![[1.0; 3]; 64 * 32],
        };
        assert!((sky.up_irradiance()[0] - std::f32::consts::PI).abs() < 0.05);
    }

    #[test]
    fn broken_files_are_rejected() {
        assert!(read_hdr(b"P6\n").is_err());
        let mut truncated = header(4, 4);
        truncated.extend_from_slice(&[1, 2, 3]);
        assert!(read_hdr(&truncated).is_err());
        let mut sideways = b"#?RADIANCE\n\n+X 4 -Y 4\n".to_vec();
        sideways.extend_from_slice(&[0; 64]);
        assert!(read_hdr(&sideways).is_err());
        let mut huge = header(MAX_HDR_SIDE + 1, 1);
        huge.extend_from_slice(&[0; 16]);
        assert!(read_hdr(&huge).is_err());
    }
}

//! Animated GIF writer for turntable renders (Phase 11b). Each frame gets
//! its own 256-color palette (NeuQuant); fully transparent pixels stay
//! transparent.

use crate::png_io::RgbaImage;
use crate::RzxlError;

/// GIF frames are at most this many pixels wide or high.
pub const MAX_GIF_SIDE: u32 = u16::MAX as u32;

fn gif_error(e: impl std::fmt::Display) -> RzxlError {
    RzxlError::Invalid(format!("GIF: {e}"))
}

/// Encodes `frames` (all the same size) as a looping GIF at `fps` frames
/// per second.
pub fn encode_gif(frames: &[RgbaImage], fps: u32) -> Result<Vec<u8>, RzxlError> {
    let Some(first) = frames.first() else {
        return Err(gif_error("no frames"));
    };
    let (w, h) = (first.width, first.height);
    if w == 0 || h == 0 || w > MAX_GIF_SIDE || h > MAX_GIF_SIDE {
        return Err(gif_error(format!("unsupported size {w}×{h}")));
    }
    // Delays are in hundredths of a second.
    let delay = (100 / fps.clamp(1, 100)).max(2) as u16;
    let mut out = Vec::new();
    {
        let mut encoder =
            gif::Encoder::new(&mut out, w as u16, h as u16, &[]).map_err(gif_error)?;
        encoder
            .set_repeat(gif::Repeat::Infinite)
            .map_err(gif_error)?;
        for (i, f) in frames.iter().enumerate() {
            if (f.width, f.height) != (w, h) || f.pixels.len() != (w * h * 4) as usize {
                return Err(gif_error(format!("frame {i} has another size")));
            }
            let mut pixels = f.pixels.clone();
            let mut frame = gif::Frame::from_rgba_speed(w as u16, h as u16, &mut pixels, 10);
            frame.delay = delay;
            frame.dispose = gif::DisposalMethod::Background;
            encoder.write_frame(&frame).map_err(gif_error)?;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(value: u8) -> RgbaImage {
        RgbaImage {
            width: 4,
            height: 3,
            pixels: (0..12).flat_map(|i| [value, i * 20, 0, 255]).collect(),
        }
    }

    #[test]
    fn frames_and_delay_roundtrip() {
        let frames: Vec<RgbaImage> = (0..5).map(|i| frame(i * 50)).collect();
        let bytes = encode_gif(&frames, 10).unwrap();
        assert_eq!(&bytes[..6], b"GIF89a");
        let mut options = gif::DecodeOptions::new();
        options.set_color_output(gif::ColorOutput::RGBA);
        let mut decoder = options.read_info(bytes.as_slice()).unwrap();
        let mut count = 0;
        while let Some(f) = decoder.read_next_frame().unwrap() {
            assert_eq!((f.width, f.height), (4, 3));
            assert_eq!(f.delay, 10);
            count += 1;
        }
        assert_eq!(count, 5);
        assert!(encode_gif(&[], 10).is_err());
        assert!(encode_gif(
            &[
                frame(0),
                RgbaImage {
                    width: 1,
                    height: 1,
                    pixels: vec![0; 4]
                }
            ],
            10
        )
        .is_err());
    }
}

//! PNG and JPEG files for reference images: import (with downscaling) and
//! the checks a `.rzxl` loader runs on embedded images.

use std::io::Cursor;

use image::{DynamicImage, ImageFormat, ImageReader, Limits};
use rizxel_math::ImageData;

use crate::png_io::RgbaImage;
use crate::RzxlError;

/// Largest reference image side kept in a project; imports are scaled down
/// to it.
pub const MAX_REFERENCE_SIDE: u32 = 4096;
/// Largest image side accepted for import before scaling down.
const MAX_IMPORT_SIDE: u32 = 16384;

fn image_error(e: impl std::fmt::Display) -> RzxlError {
    RzxlError::Invalid(format!("image: {e}"))
}

/// Opens PNG or JPEG bytes, refusing other formats and images with a side
/// over `max_side` before any pixel is decoded.
fn reader(bytes: &[u8], max_side: u32) -> Result<ImageReader<Cursor<&[u8]>>, RzxlError> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(image_error)?;
    match reader.format() {
        Some(ImageFormat::Png | ImageFormat::Jpeg) => {}
        _ => return Err(image_error("only PNG and JPEG images are supported")),
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(max_side);
    limits.max_image_height = Some(max_side);
    // RGBA8 of the largest image plus decoder working memory.
    limits.max_alloc = Some(u64::from(max_side) * u64::from(max_side) * 4 * 2);
    reader.limits(limits);
    Ok(reader)
}

fn decode(bytes: &[u8], max_side: u32) -> Result<DynamicImage, RzxlError> {
    reader(bytes, max_side)?.decode().map_err(image_error)
}

/// Decodes PNG or JPEG bytes to RGBA8, scaled down (keeping the aspect
/// ratio) so neither side exceeds `max_side`. Heightmap import uses it.
pub fn decode_fitted(bytes: &[u8], max_side: u32) -> Result<RgbaImage, RzxlError> {
    let mut image = decode(bytes, MAX_IMPORT_SIDE)?;
    let (w, h) = (image.width(), image.height());
    let max_side = max_side.max(1);
    if w.max(h) > max_side {
        let k = max_side as f64 / w.max(h) as f64;
        let nw = ((w as f64 * k).round() as u32).clamp(1, max_side);
        let nh = ((h as f64 * k).round() as u32).clamp(1, max_side);
        image = image.resize_exact(nw, nh, image::imageops::FilterType::Triangle);
    }
    let image = image.into_rgba8();
    Ok(RgbaImage {
        width: image.width(),
        height: image.height(),
        pixels: image.into_raw(),
    })
}

/// Decodes an embedded reference image to RGBA8 (the renderer's texture).
pub fn decode_reference(data: &ImageData) -> Result<RgbaImage, RzxlError> {
    let image = decode(data.bytes(), MAX_REFERENCE_SIDE)?.into_rgba8();
    Ok(RgbaImage {
        width: image.width(),
        height: image.height(),
        pixels: image.into_raw(),
    })
}

/// Checks that an embedded image decodes completely and has the size the
/// project states.
pub fn check_reference(data: &ImageData) -> Result<(), RzxlError> {
    let image = decode(data.bytes(), MAX_REFERENCE_SIDE)?;
    if (image.width(), image.height()) != (data.width, data.height) {
        return Err(image_error(format!(
            "the image is {}×{}, the project says {}×{}",
            image.width(),
            image.height(),
            data.width,
            data.height
        )));
    }
    Ok(())
}

/// An image file ready to become a reference.
#[derive(Debug, Clone)]
pub struct ImportedImage {
    pub image: ImageData,
    /// Original size when the image was scaled down to
    /// [`MAX_REFERENCE_SIDE`].
    pub downscaled_from: Option<(u32, u32)>,
}

/// Reads a PNG or JPEG file's bytes for a reference image. Images with a
/// side over [`MAX_REFERENCE_SIDE`] are scaled down (aspect kept) and
/// re-encoded in their own format; others keep their bytes as they are.
pub fn import_image(bytes: Vec<u8>) -> Result<ImportedImage, RzxlError> {
    let format = reader(&bytes, MAX_IMPORT_SIDE)?.format();
    let image = decode(&bytes, MAX_IMPORT_SIDE)?;
    let (w, h) = (image.width(), image.height());
    if w.max(h) <= MAX_REFERENCE_SIDE {
        return Ok(ImportedImage {
            image: ImageData::new(bytes, w, h),
            downscaled_from: None,
        });
    }
    let scale = MAX_REFERENCE_SIDE as f32 / w.max(h) as f32;
    let (nw, nh) = (
        ((w as f32 * scale).round() as u32).clamp(1, MAX_REFERENCE_SIDE),
        ((h as f32 * scale).round() as u32).clamp(1, MAX_REFERENCE_SIDE),
    );
    let small = image.resize_exact(nw, nh, image::imageops::FilterType::Triangle);
    let mut out = Cursor::new(Vec::new());
    match format {
        Some(ImageFormat::Jpeg) => {
            let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90);
            small.into_rgb8().write_with_encoder(encoder)
        }
        _ => small.into_rgba8().write_to(&mut out, ImageFormat::Png),
    }
    .map_err(image_error)?;
    Ok(ImportedImage {
        image: ImageData::new(out.into_inner(), nw, nh),
        downscaled_from: Some((w, h)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::png_io::encode_rgba;

    /// `w`×`h` PNG with a red top-left pixel.
    pub(crate) fn png(w: u32, h: u32) -> Vec<u8> {
        let mut pixels = vec![255u8; (w * h * 4) as usize];
        pixels[1] = 0;
        pixels[2] = 0;
        encode_rgba(w, h, &pixels).unwrap()
    }

    fn jpeg(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbImage::from_pixel(w, h, image::Rgb([10, 200, 30]));
        let mut out = Cursor::new(Vec::new());
        img.write_to(&mut out, ImageFormat::Jpeg).unwrap();
        out.into_inner()
    }

    #[test]
    fn png_and_jpeg_import_unchanged() {
        let bytes = png(3, 2);
        let i = import_image(bytes.clone()).unwrap();
        assert_eq!((i.image.width, i.image.height), (3, 2));
        assert_eq!(i.image.bytes(), bytes.as_slice());
        assert!(i.downscaled_from.is_none());
        let rgba = decode_reference(&i.image).unwrap();
        assert_eq!(rgba.pixel(0, 0), [255, 0, 0, 255]);
        let j = import_image(jpeg(8, 4)).unwrap();
        assert_eq!((j.image.width, j.image.height), (8, 4));
        check_reference(&j.image).unwrap();
    }

    #[test]
    fn large_images_are_scaled_down() {
        let i = import_image(png(MAX_REFERENCE_SIDE * 2, 10)).unwrap();
        assert_eq!(i.downscaled_from, Some((MAX_REFERENCE_SIDE * 2, 10)));
        assert_eq!((i.image.width, i.image.height), (MAX_REFERENCE_SIDE, 5));
        check_reference(&i.image).unwrap();
        let j = import_image(jpeg(10, MAX_REFERENCE_SIDE + 4)).unwrap();
        assert_eq!(j.image.height, MAX_REFERENCE_SIDE);
        check_reference(&j.image).unwrap();
    }

    #[test]
    fn hostile_images_fail_cleanly() {
        // Not an image, an unsupported format, a truncated PNG.
        assert!(import_image(b"hello world".to_vec()).is_err());
        assert!(import_image(b"GIF89a\x01\x00\x01\x00".to_vec()).is_err());
        let full = png(16, 16);
        let cut = full[..full.len() / 2].to_vec();
        assert!(import_image(cut.clone()).is_err());
        assert!(check_reference(&ImageData::new(cut, 16, 16)).is_err());
        // A stated size that does not match, a side over the file limit.
        assert!(check_reference(&ImageData::new(full, 16, 17)).is_err());
        let wide = png(MAX_REFERENCE_SIDE + 1, 1);
        assert!(check_reference(&ImageData::new(wide, MAX_REFERENCE_SIDE + 1, 1)).is_err());
    }
}

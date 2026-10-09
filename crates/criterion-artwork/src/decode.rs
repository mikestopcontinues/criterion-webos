use crate::{ArtworkError, DecodedArtwork};
use image::{ColorType, DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits};
use std::io::Cursor;

pub const MAX_ENCODED_BYTES: usize = 4 * 1024 * 1024;
const MAX_SIDE: u32 = 2048;
const MAX_DECODE_PIXELS: u64 = 4 * 1024 * 1024;
const MAX_DECODE_BYTES: u64 = 16 * 1024 * 1024;

pub fn decode_artwork(content_type: &str, encoded: &[u8]) -> Result<DecodedArtwork, ArtworkError> {
    if encoded.len() > MAX_ENCODED_BYTES {
        return Err(ArtworkError::EncodedTooLarge);
    }
    let format = admitted_format(content_type, encoded)?;
    if format == ImageFormat::WebP {
        crate::webp::preflight(encoded)?;
    } else if format == ImageFormat::Png && !encoded.ends_with(b"\0\0\0\0IEND\xae\x42\x60\x82") {
        // Reading the first frame does not require the maintained PNG decoder
        // to consume IEND. A complete public asset must contain its terminator.
        return Err(ArtworkError::InvalidImage);
    } else if format == ImageFormat::Jpeg && !encoded.ends_with(&[0xff, 0xd9]) {
        return Err(ArtworkError::InvalidImage);
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    let mut reader = ImageReader::with_format(Cursor::new(encoded), format);
    reader.limits(limits);
    let decoder = reader.into_decoder().map_err(decode_error)?;
    let (width, height) = decoder.dimensions();
    check_dimensions(width, height)?;
    if !matches!(
        decoder.color_type(),
        ColorType::L8 | ColorType::La8 | ColorType::Rgb8 | ColorType::Rgba8
    ) {
        return Err(ArtworkError::UnsupportedFormat);
    }
    if decoder.total_bytes() > MAX_DECODE_BYTES {
        return Err(ArtworkError::DecodeLimit);
    }
    let mut image = DynamicImage::from_decoder(decoder)
        .map_err(decode_error)?
        .into_rgba8();
    if u64::from(width) * u64::from(height) > 1024 * 1024 {
        let alpha = image.pixels().next().ok_or(ArtworkError::InvalidImage)?.0[3];
        let varying_alpha = image.pixels().any(|pixel| pixel.0[3] != alpha);
        if varying_alpha {
            for pixel in image.pixels_mut() {
                let alpha = u16::from(pixel.0[3]);
                for channel in &mut pixel.0[..3] {
                    *channel = ((u16::from(*channel) * alpha + 127) / 255) as u8;
                }
            }
        }
        image = DynamicImage::ImageRgba8(image)
            .thumbnail(1024, 1024)
            .into_rgba8();
        if varying_alpha {
            for pixel in image.pixels_mut() {
                let alpha = u16::from(pixel.0[3]);
                for channel in &mut pixel.0[..3] {
                    *channel = (u16::from(*channel) * 255 + alpha / 2)
                        .checked_div(alpha)
                        .unwrap_or(0)
                        .min(255) as u8;
                }
            }
        }
    }
    Ok(DecodedArtwork {
        dimensions: [image.width() as usize, image.height() as usize],
        rgba: image.into_raw(),
    })
}

fn admitted_format(content_type: &str, encoded: &[u8]) -> Result<ImageFormat, ArtworkError> {
    if content_type.len() > 128 {
        return Err(ArtworkError::UnsupportedFormat);
    }
    let mime = content_type.split(';').next().unwrap_or("").trim();
    let format = if mime.eq_ignore_ascii_case("image/png") {
        ImageFormat::Png
    } else if mime.eq_ignore_ascii_case("image/jpeg") {
        ImageFormat::Jpeg
    } else if mime.eq_ignore_ascii_case("image/webp") {
        ImageFormat::WebP
    } else {
        return Err(ArtworkError::UnsupportedFormat);
    };
    if image::guess_format(encoded).map_err(|_| ArtworkError::InvalidImage)? != format {
        return Err(ArtworkError::UnsupportedFormat);
    }
    Ok(format)
}

pub(crate) fn check_dimensions(width: u32, height: u32) -> Result<(), ArtworkError> {
    if width == 0
        || height == 0
        || width > MAX_SIDE
        || height > MAX_SIDE
        || u64::from(width) * u64::from(height) > MAX_DECODE_PIXELS
    {
        return Err(ArtworkError::DecodeLimit);
    }
    Ok(())
}

fn decode_error(error: image::ImageError) -> ArtworkError {
    match error {
        image::ImageError::Limits(_) => ArtworkError::DecodeLimit,
        _ => ArtworkError::InvalidImage,
    }
}

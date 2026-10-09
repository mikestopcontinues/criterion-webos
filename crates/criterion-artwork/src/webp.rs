//! The maintained decoder checks an extended image's frame dimensions only
//! after decoding it. Admit every nested frame header before that allocation.
use crate::{ArtworkError, decode::check_dimensions};

pub(crate) fn preflight(encoded: &[u8]) -> Result<(), ArtworkError> {
    if encoded.len() < 20
        || &encoded[..4] != b"RIFF"
        || &encoded[8..12] != b"WEBP"
        || u64::from(u32::from_le_bytes(encoded[4..8].try_into().unwrap())) + 8
            != encoded.len() as u64
    {
        return Err(ArtworkError::InvalidImage);
    }
    let mut position = 12_usize;
    let mut chunks = 0_u8;
    let mut canvas = None;
    let mut frame = None;
    while position < encoded.len() {
        chunks += 1;
        if chunks > 64 {
            return Err(ArtworkError::DecodeLimit);
        }
        let header = encoded
            .get(position..position + 8)
            .ok_or(ArtworkError::InvalidImage)?;
        let size = u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize;
        let start = position + 8;
        let end = start.checked_add(size).ok_or(ArtworkError::InvalidImage)?;
        let data = encoded.get(start..end).ok_or(ArtworkError::InvalidImage)?;
        match &header[..4] {
            b"VP8X" => {
                if position != 12 || data.len() != 10 || canvas.is_some() {
                    return Err(ArtworkError::InvalidImage);
                }
                if data[0] & 2 != 0 {
                    return Err(ArtworkError::UnsupportedFormat);
                }
                if data[0] & 0xc1 != 0 || data[1..4] != [0, 0, 0] {
                    return Err(ArtworkError::InvalidImage);
                }
                let dimensions = (little_u24(&data[4..7]) + 1, little_u24(&data[7..10]) + 1);
                check_dimensions(dimensions.0, dimensions.1)?;
                canvas = Some(dimensions);
            }
            b"VP8 " => {
                if data.len() < 10 || data[0] & 1 != 0 || data[3..6] != [0x9d, 1, 0x2a] {
                    return Err(ArtworkError::InvalidImage);
                }
                let dimensions = (
                    u32::from(u16::from_le_bytes(data[6..8].try_into().unwrap()) & 0x3fff),
                    u32::from(u16::from_le_bytes(data[8..10].try_into().unwrap()) & 0x3fff),
                );
                admit_frame(&mut frame, dimensions)?;
            }
            b"VP8L" => {
                if data.len() < 5 || data[0] != 0x2f {
                    return Err(ArtworkError::InvalidImage);
                }
                let dimensions = u32::from_le_bytes(data[1..5].try_into().unwrap());
                if dimensions >> 29 != 0 {
                    return Err(ArtworkError::InvalidImage);
                }
                admit_frame(
                    &mut frame,
                    ((dimensions & 0x3fff) + 1, ((dimensions >> 14) & 0x3fff) + 1),
                )?;
            }
            b"ANIM" | b"ANMF" => return Err(ArtworkError::UnsupportedFormat),
            // Metadata and unknown chunks are bounded and skipped. They are
            // never decoded or used for EXIF orientation/color conversion.
            _ => {}
        }
        position = end
            .checked_add(size % 2)
            .ok_or(ArtworkError::InvalidImage)?;
        if !size.is_multiple_of(2) && encoded.get(end) != Some(&0) {
            return Err(ArtworkError::InvalidImage);
        }
    }
    let frame = frame.ok_or(ArtworkError::InvalidImage)?;
    if canvas.is_some_and(|canvas| canvas != frame) {
        return Err(ArtworkError::InvalidImage);
    }
    Ok(())
}

fn admit_frame(frame: &mut Option<(u32, u32)>, dimensions: (u32, u32)) -> Result<(), ArtworkError> {
    check_dimensions(dimensions.0, dimensions.1)?;
    if frame.replace(dimensions).is_some() {
        return Err(ArtworkError::InvalidImage);
    }
    Ok(())
}

fn little_u24(bytes: &[u8]) -> u32 {
    u32::from(bytes[0]) | (u32::from(bytes[1]) << 8) | (u32::from(bytes[2]) << 16)
}

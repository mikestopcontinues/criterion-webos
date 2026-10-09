use criterion_artwork::{ArtworkError, ImageRole, decode_artwork};

#[test]
fn valid_png_pixels_reach_the_ui_as_rgba_without_color_or_alpha_loss() {
    let artwork = decode_artwork(
        ImageRole::Card,
        "image/png",
        include_bytes!("fixtures/two-pixels.png"),
    )
    .unwrap();
    assert_eq!(artwork.dimensions(), [2, 1]);
    assert_eq!(artwork.rgba(), &[255, 0, 0, 255, 0, 255, 0, 128]);
}

#[test]
fn oversized_header_is_rejected_before_missing_pixel_data_can_trigger_a_decode() {
    assert_eq!(
        decode_artwork(
            ImageRole::Card,
            "image/png",
            include_bytes!("fixtures/width-bomb.png")
        )
        .unwrap_err(),
        ArtworkError::DecodeLimit
    );
}

#[test]
fn encoded_size_is_capped_even_when_the_declared_image_is_small() {
    let mut encoded = include_bytes!("fixtures/two-pixels.png").to_vec();
    encoded.resize(4 * 1024 * 1024 + 1, 0);
    assert_eq!(
        decode_artwork(ImageRole::Card, "image/png", &encoded).unwrap_err(),
        ArtworkError::EncodedTooLarge
    );
}

#[test]
fn mime_type_must_be_allowed_and_match_the_actual_image_signature() {
    let png = include_bytes!("fixtures/two-pixels.png");
    for mime in [
        "text/html",
        "image/svg+xml",
        "image/gif",
        "image/jpeg",
        "image/webp",
        "",
    ] {
        assert_eq!(
            decode_artwork(ImageRole::Card, mime, png).unwrap_err(),
            ArtworkError::UnsupportedFormat
        );
    }
    assert_eq!(
        decode_artwork(ImageRole::Card, " Image/PNG ; harmless=parameter", png)
            .unwrap()
            .dimensions(),
        [2, 1]
    );
}

#[test]
fn valid_large_image_is_downsampled_before_admission_without_changing_its_aspect() {
    use image::ImageEncoder;
    let pixels = vec![67_u8; 2048 * 1024 * 4];
    let mut encoded = Vec::new();
    image::codecs::png::PngEncoder::new(&mut encoded)
        .write_image(&pixels, 2048, 1024, image::ExtendedColorType::Rgba8)
        .unwrap();
    let artwork = decode_artwork(ImageRole::Card, "image/png", &encoded).unwrap();
    assert_eq!(artwork.dimensions(), [1024, 512]);
    assert_eq!(artwork.rgba().len(), 1024 * 512 * 4);
    assert_eq!(&artwork.rgba()[..4], &[67, 67, 67, 67]);
}

#[test]
fn transparent_logo_edges_keep_visible_color_when_downsampled() {
    use image::ImageEncoder;
    let mut pixels = Vec::with_capacity(2048 * 1024 * 4);
    for _ in 0..2048 * 1024 / 2 {
        pixels.extend_from_slice(&[255, 255, 255, 255, 0, 0, 0, 0]);
    }
    let mut encoded = Vec::new();
    image::codecs::png::PngEncoder::new(&mut encoded)
        .write_image(&pixels, 2048, 1024, image::ExtendedColorType::Rgba8)
        .unwrap();
    let artwork = decode_artwork(ImageRole::Card, "image/png", &encoded).unwrap();
    assert_eq!(artwork.dimensions(), [1024, 512]);
    assert_eq!(&artwork.rgba()[..4], &[255, 255, 255, 128]);
}

#[test]
fn webp_small_canvas_cannot_hide_an_oversized_nested_frame() {
    let mut chunks = Vec::new();
    riff_chunk(&mut chunks, b"VP8X", &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    riff_chunk(&mut chunks, b"VP8 ", &[0, 0, 0, 0x9d, 1, 0x2a, 1, 8, 1, 0]);
    let mut encoded = b"RIFF".to_vec();
    encoded.extend_from_slice(&(chunks.len() as u32 + 4).to_le_bytes());
    encoded.extend_from_slice(b"WEBP");
    encoded.extend_from_slice(&chunks);
    assert_eq!(
        decode_artwork(ImageRole::Card, "image/webp", &encoded).unwrap_err(),
        ArtworkError::DecodeLimit
    );
}

#[test]
fn a_truncated_png_is_not_admitted_even_if_its_pixel_stream_is_complete() {
    let png = include_bytes!("fixtures/two-pixels.png");
    for length in [0, 8, 33, png.len() - 12, png.len() - 1] {
        assert_eq!(
            decode_artwork(ImageRole::Card, "image/png", &png[..length]).unwrap_err(),
            ArtworkError::InvalidImage
        );
    }
}

#[test]
fn real_jpeg_and_lossless_webp_pixels_decode_with_only_the_admitted_formats() {
    use image::ImageEncoder;
    let pixels = [32, 64, 96, 32, 64, 96];
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 100)
        .write_image(&pixels, 2, 1, image::ExtendedColorType::Rgb8)
        .unwrap();
    let decoded = decode_artwork(ImageRole::Card, "image/jpeg", &jpeg).unwrap();
    assert_eq!(decoded.dimensions(), [2, 1]);
    for pixel in decoded.rgba().as_chunks::<4>().0 {
        assert!(
            pixel[0].abs_diff(32) <= 3 && pixel[1].abs_diff(64) <= 3 && pixel[2].abs_diff(96) <= 3
        );
        assert_eq!(pixel[3], 255);
    }
    let mut webp = Vec::new();
    image::codecs::webp::WebPEncoder::new_lossless(&mut webp)
        .write_image(
            &[255, 0, 0, 255, 0, 255, 0, 128],
            2,
            1,
            image::ExtendedColorType::Rgba8,
        )
        .unwrap();
    let decoded = decode_artwork(ImageRole::Card, "image/webp", &webp).unwrap();
    assert_eq!(decoded.dimensions(), [2, 1]);
    assert_eq!(decoded.rgba(), &[255, 0, 0, 255, 0, 255, 0, 128]);
    for truncated in [&jpeg[..jpeg.len() - 1], &webp[..webp.len() - 1]] {
        let mime = if truncated.starts_with(&[0xff, 0xd8]) {
            "image/jpeg"
        } else {
            "image/webp"
        };
        assert_eq!(
            decode_artwork(ImageRole::Card, mime, truncated).unwrap_err(),
            ArtworkError::InvalidImage
        );
    }
    let marker = jpeg
        .windows(2)
        .position(|part| part == [0xff, 0xc0])
        .unwrap();
    jpeg[marker + 7..marker + 9].copy_from_slice(&2049_u16.to_be_bytes());
    assert_eq!(
        decode_artwork(ImageRole::Card, "image/jpeg", &jpeg).unwrap_err(),
        ArtworkError::DecodeLimit
    );
}

#[test]
fn incomplete_webp_chunks_animation_and_canvas_disagreement_are_rejected() {
    let cases = [
        (
            b"RIFF\x04\x00\x00\x00WEBP".to_vec(),
            ArtworkError::InvalidImage,
        ),
        (
            b"RIFF\x0c\x00\x00\x00WEBPVP8L\xff\xff\xff\xff".to_vec(),
            ArtworkError::InvalidImage,
        ),
    ];
    for (bytes, error) in cases {
        assert_eq!(
            decode_artwork(ImageRole::Card, "image/webp", &bytes).unwrap_err(),
            error
        );
    }
    let mut chunks = Vec::new();
    riff_chunk(&mut chunks, b"VP8X", &[2, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let mut animated = b"RIFF".to_vec();
    animated.extend_from_slice(&(chunks.len() as u32 + 4).to_le_bytes());
    animated.extend_from_slice(b"WEBP");
    animated.extend_from_slice(&chunks);
    assert_eq!(
        decode_artwork(ImageRole::Card, "image/webp", &animated).unwrap_err(),
        ArtworkError::UnsupportedFormat
    );
    chunks.clear();
    riff_chunk(&mut chunks, b"VP8X", &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    riff_chunk(&mut chunks, b"VP8L", &[0x2f, 1, 0, 0, 0]);
    let mut mismatch = b"RIFF".to_vec();
    mismatch.extend_from_slice(&(chunks.len() as u32 + 4).to_le_bytes());
    mismatch.extend_from_slice(b"WEBP");
    mismatch.extend_from_slice(&chunks);
    assert_eq!(
        decode_artwork(ImageRole::Card, "image/webp", &mismatch).unwrap_err(),
        ArtworkError::InvalidImage
    );
    mismatch[39..43].copy_from_slice(&2048_u32.to_le_bytes());
    assert_eq!(
        decode_artwork(ImageRole::Card, "image/webp", &mismatch).unwrap_err(),
        ArtworkError::DecodeLimit
    );
}

#[test]
fn fully_transparent_filtered_pixels_do_not_divide_by_zero_or_retain_hidden_color() {
    use image::ImageEncoder;
    let mut pixels = vec![0; 2048 * 1024 * 4];
    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel.copy_from_slice(&[255, 0, 0, 0]);
    }
    pixels[..4].copy_from_slice(&[0, 255, 0, 1]);
    let mut encoded = Vec::new();
    image::codecs::png::PngEncoder::new(&mut encoded)
        .write_image(&pixels, 2048, 1024, image::ExtendedColorType::Rgba8)
        .unwrap();
    let decoded = decode_artwork(ImageRole::Card, "image/png", &encoded).unwrap();
    assert!(decoded.rgba().iter().all(|byte| *byte == 0));
}

#[test]
fn high_bit_depth_images_are_rejected_before_expanding_into_larger_working_buffers() {
    use image::ImageEncoder;
    let mut encoded = Vec::new();
    image::codecs::png::PngEncoder::new(&mut encoded)
        .write_image(&[255; 8], 1, 1, image::ExtendedColorType::Rgba16)
        .unwrap();
    assert_eq!(
        decode_artwork(ImageRole::Card, "image/png", &encoded).unwrap_err(),
        ArtworkError::UnsupportedFormat
    );
}

#[test]
fn malformed_lossless_huffman_tree_is_rejected_without_panicking() {
    // A bounded 1x1 image can still carry an oversubscribed entropy tree.
    // Three one-bit codes plus a deep tail use twice the available codespace.
    let mut lengths = vec![1, 1, 1];
    lengths.extend(2..=14);
    lengths.extend([15, 15]);
    assert_eq!(
        decode_artwork(
            ImageRole::Card,
            "image/webp",
            &lossless_with_lengths(&lengths)
        )
        .unwrap_err(),
        ArtworkError::InvalidImage
    );
}

#[test]
fn valid_maximum_depth_lossless_tree_still_decodes_while_incomplete_trees_fail() {
    let mut lengths: Vec<_> = (1..=14).collect();
    lengths.extend([15, 15]);
    let decoded = decode_artwork(
        ImageRole::Card,
        "image/webp",
        &lossless_with_lengths(&lengths),
    )
    .unwrap();
    assert_eq!(decoded.dimensions(), [1, 1]);
    assert_eq!(decoded.rgba(), &[0, 0, 0, 255]);
    for lengths in [&[1, 1, 1][..], &[2, 2]] {
        assert_eq!(
            decode_artwork(
                ImageRole::Card,
                "image/webp",
                &lossless_with_lengths(lengths)
            )
            .unwrap_err(),
            ArtworkError::InvalidImage
        );
    }
}

fn lossless_with_lengths(lengths: &[u16]) -> Vec<u8> {
    let mut bits = Vec::new();
    let mut write = |value: u16, count: usize| {
        bits.extend((0..count).map(|shift| ((value >> shift) & 1) as u8));
    };
    write(0, 1); // no transforms
    write(0, 1); // no color cache
    write(0, 1); // no meta Huffman image
    write(0, 1); // normal first Huffman tree
    write(15, 4); // all nineteen code-length alphabet entries
    for symbol in [
        17, 18, 0, 1, 2, 3, 4, 5, 16, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
    ] {
        write(if symbol < 16 { 4 } else { 0 }, 3);
    }
    write(1, 1); // remaining unused alphabet entries have zero length
    write(2, 3); // six bits for the symbol count
    write(lengths.len() as u16 - 2, 6);
    for &length in lengths {
        write(length.reverse_bits() >> 12, 4);
    }
    // Red, blue, alpha and distance trees each contain one fixed symbol.
    for symbol in [0, 0, 255, 0] {
        write(1, 1);
        write(0, 1);
        write(u16::from(symbol > 1), 1);
        write(symbol, if symbol > 1 { 8 } else { 1 });
    }
    write(0, 1); // first green symbol, whose complete-tree code is zero
    let mut data = vec![0x2f, 0, 0, 0, 0]; // 1x1 VP8L header
    for part in bits.chunks(8) {
        data.push(
            part.iter()
                .enumerate()
                .fold(0, |value, (shift, bit)| value | bit << shift),
        );
    }
    data.extend([0; 8]); // bounded bit-reader lookahead
    let mut chunks = Vec::new();
    riff_chunk(&mut chunks, b"VP8L", &data);
    let mut encoded = b"RIFF".to_vec();
    encoded.extend_from_slice(&(chunks.len() as u32 + 4).to_le_bytes());
    encoded.extend_from_slice(b"WEBP");
    encoded.extend_from_slice(&chunks);
    encoded
}

fn riff_chunk(target: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    target.extend_from_slice(kind);
    target.extend_from_slice(&(data.len() as u32).to_le_bytes());
    target.extend_from_slice(data);
    if !data.len().is_multiple_of(2) {
        target.push(0);
    }
}

#[test]
fn full_hd_backdrop_preserves_native_detail_instead_of_thumbnail_upscaling() {
    use image::ImageEncoder;
    let mut pixels = Vec::with_capacity(1920 * 1080 * 4);
    for _ in 0..1920 * 1080 / 2 {
        pixels.extend_from_slice(&[255, 255, 255, 255, 0, 0, 0, 255]);
    }
    let mut encoded = Vec::new();
    image::codecs::png::PngEncoder::new(&mut encoded)
        .write_image(&pixels, 1920, 1080, image::ExtendedColorType::Rgba8)
        .unwrap();
    let backdrop = decode_artwork(ImageRole::Backdrop, "image/png", &encoded).unwrap();
    assert_eq!(backdrop.dimensions(), [1920, 1080]);
    assert_eq!(backdrop.rgba().len(), 1920 * 1080 * 4);
    assert_eq!(&backdrop.rgba()[..8], &[255, 255, 255, 255, 0, 0, 0, 255]);
}

#[test]
fn card_role_fits_thin_and_tall_images_inside_its_dimension_budget() {
    use image::ImageEncoder;
    for (width, height, dimensions) in [(2048, 128, [1024, 64]), (128, 2048, [64, 1024])] {
        let pixels = vec![67u8; width as usize * height as usize * 4];
        let mut encoded = Vec::new();
        image::codecs::png::PngEncoder::new(&mut encoded)
            .write_image(&pixels, width, height, image::ExtendedColorType::Rgba8)
            .unwrap();
        let card = decode_artwork(ImageRole::Card, "image/png", &encoded).unwrap();
        assert_eq!(card.dimensions(), dimensions);
        assert!(card.rgba().len() <= 4 * 1024 * 1024);
    }
}

#[test]
fn larger_backdrops_fit_the_native_canvas_without_exceeding_output_allocation() {
    use image::ImageEncoder;
    for (width, height, dimensions) in [
        (2048, 1152, [1920, 1080]),
        (1024, 2048, [540, 1080]),
        (2048, 128, [1920, 120]),
    ] {
        let pixels = vec![67u8; width as usize * height as usize * 4];
        let mut encoded = Vec::new();
        image::codecs::png::PngEncoder::new(&mut encoded)
            .write_image(&pixels, width, height, image::ExtendedColorType::Rgba8)
            .unwrap();
        let backdrop = decode_artwork(ImageRole::Backdrop, "image/png", &encoded).unwrap();
        assert_eq!(backdrop.dimensions(), dimensions);
        assert!(backdrop.rgba().len() <= 1920 * 1080 * 4);
        assert_eq!(&backdrop.rgba()[..4], &[67, 67, 67, 67]);
    }
}

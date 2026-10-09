use criterion_ui::AppUi;
#[test]
fn decoded_artwork_admission_rejects_oversized_images_and_bounds_cache() {
    let mut ui = AppUi::new();
    assert!(
        ui.admit_image(
            "too-big",
            egui::ColorImage::new([2049, 1], vec![egui::Color32::WHITE; 2049])
        )
        .is_err()
    );
    for index in 0..40 {
        ui.admit_image(
            &format!("image-{index}"),
            egui::ColorImage::new([480, 270], vec![egui::Color32::WHITE; 480 * 270]),
        )
        .unwrap();
    }
    assert_eq!(ui.image_cache_len(), 32);
    assert!(ui.image_cache_bytes() <= 24 * 1024 * 1024);
}
#[test]
fn evicted_artwork_is_not_retained_or_uploaded_before_a_frame() {
    let mut ui = AppUi::new();
    for index in 0..100 {
        ui.admit_image(
            &format!("image-{index}"),
            egui::ColorImage::new([1024, 1024], vec![egui::Color32::WHITE; 1024 * 1024]),
        )
        .unwrap();
    }
    let mut frame = ui.render(
        egui::RawInput::default(),
        &criterion_ui::ViewData::default(),
    );
    let upload_bytes: usize = frame
        .output
        .textures_delta
        .set
        .values()
        .flat_map(|deltas| deltas.iter())
        .map(|delta| delta.image.size()[0] * delta.image.size()[1] * 4)
        .sum();
    frame.output.textures_delta.clear();
    assert!(
        upload_bytes <= 32 * 1024 * 1024,
        "queued {upload_bytes} bytes"
    );
}

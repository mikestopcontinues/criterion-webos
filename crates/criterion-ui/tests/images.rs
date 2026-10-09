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

#[test]
fn artwork_presence_includes_pending_and_uploaded_images_and_excludes_evictions() {
    let mut ui = AppUi::new();
    assert!(!ui.has_image("fixture"));
    ui.admit_image(
        "fixture",
        egui::ColorImage::new([1, 1], vec![egui::Color32::WHITE]),
    )
    .unwrap();
    assert!(ui.has_image("fixture"));
    let mut frame = ui.render(
        egui::RawInput::default(),
        &criterion_ui::ViewData::default(),
    );
    frame.output.textures_delta.clear();
    assert!(ui.has_image("fixture"));
    for index in 0..32 {
        ui.admit_image(
            &format!("next-{index}"),
            egui::ColorImage::new([1, 1], vec![egui::Color32::WHITE]),
        )
        .unwrap();
    }
    assert!(!ui.has_image("fixture"));
    assert!(ui.has_image("next-31"));
}

#[test]
fn discard_artwork_removes_pending_pixels_and_releases_uploaded_texture() {
    let mut ui = AppUi::new();
    ui.admit_image(
        "pending",
        egui::ColorImage::new([2, 2], vec![egui::Color32::WHITE; 4]),
    )
    .unwrap();
    assert!(ui.discard_image("pending"));
    assert!(!ui.discard_image("pending"));
    assert_eq!(ui.image_cache_len(), 0);
    assert_eq!(ui.image_cache_bytes(), 0);
    ui.admit_image(
        "uploaded",
        egui::ColorImage::new([1, 1], vec![egui::Color32::WHITE]),
    )
    .unwrap();
    let mut frame = ui.render(
        egui::RawInput::default(),
        &criterion_ui::ViewData::default(),
    );
    let id = *frame
        .output
        .textures_delta
        .set
        .iter()
        .find(|(_, deltas)| deltas.iter().any(|delta| delta.image.size() == [1, 1]))
        .expect("uploaded artwork texture")
        .0;
    assert!(
        !frame
            .output
            .textures_delta
            .set
            .values()
            .flatten()
            .any(|delta| delta.image.size() == [2, 2])
    );
    frame.output.textures_delta.clear();
    assert!(ui.discard_image("uploaded"));
    assert!(!ui.has_image("uploaded"));
    assert_eq!(ui.image_cache_bytes(), 0);
    let mut frame = ui.render(
        egui::RawInput::default(),
        &criterion_ui::ViewData::default(),
    );
    let freed = frame.output.textures_delta.free.contains(&id);
    frame.output.textures_delta.clear();
    assert!(freed);
}

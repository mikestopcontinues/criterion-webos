use criterion_ui::check_gles_capabilities;
#[test]
fn renderer_requires_uint_indices_on_actual_gles2() {
    assert!(check_gles_capabilities("OpenGL ES 2.0 LG GLES", true).is_ok());
    assert!(check_gles_capabilities("OpenGL ES 2.0 LG GLES", false).is_err());
    assert!(check_gles_capabilities("OpenGL ES 3.2 Mesa 25.2", false).is_ok());
    for value in [
        "",
        "4.6 (Core Profile) Mesa",
        "OpenGL ES 1.1",
        "OpenGL ES bogus",
    ] {
        assert!(check_gles_capabilities(value, true).is_err(), "{value}");
    }
}

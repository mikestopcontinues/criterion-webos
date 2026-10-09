// SPDX-License-Identifier: GPL-3.0-or-later
use criterion_platform::{Size, Surface};
#[test]
fn wide_canvas_is_centered_inside_a_taller_drawable() {
    let surface = Surface {
        window: Size {
            width: 640,
            height: 400,
        },
        drawable: Size {
            width: 1280,
            height: 800,
        },
    };
    let canvas = Size {
        width: 1920,
        height: 1080,
    };
    let fit = surface.fit(canvas).unwrap();
    assert_eq!((fit.x, fit.y, fit.width, fit.height), (0, 40, 1280, 720));
    assert!((fit.scale - 0.666_666_666_666_666_6).abs() < 0.000_000_1);
    assert_eq!(
        surface.pointer_to_logical(canvas, [320.0, 200.0]),
        Some([960.0, 540.0])
    );
    assert_eq!(surface.pointer_to_logical(canvas, [320.0, 10.0]), None);
}

#[test]
fn a_canvas_that_collapses_below_one_pixel_has_no_usable_viewport() {
    let surface = Surface {
        window: Size {
            width: 1,
            height: 1000,
        },
        drawable: Size {
            width: 1,
            height: 1000,
        },
    };
    assert_eq!(
        surface.fit(Size {
            width: 1920,
            height: 1
        }),
        None
    );
    assert_eq!(
        surface.pointer_to_logical(
            Size {
                width: 1920,
                height: 1
            },
            [0.0, 500.0]
        ),
        None
    );
}

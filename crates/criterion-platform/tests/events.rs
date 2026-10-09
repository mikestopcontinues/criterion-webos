// SPDX-License-Identifier: GPL-3.0-or-later
use criterion_platform::{Event, EventLayout, KeyEvent, decode_event};

#[test]
fn webos_up_repeat_uses_the_tv_keyboard_payload() {
    // Independent SDL fork witness: event type=KEYDOWN, window=7, state=0x101,
    // scancode=82 (Up), keycode=0x40000052. Slot12 deliberately differs.
    let raw = [
        0x00, 0x03, 0x00, 0x00, 0x11, 0x22, 0x33, 0x44, 0x07, 0x00, 0x00, 0x00, 0xaa, 0xbb, 0xcc,
        0xdd, 0x01, 0x01, 0x00, 0x00, 0x52, 0x00, 0x00, 0x00, 0x52, 0x00, 0x00, 0x40, 0x00, 0x00,
        0x00, 0x00,
    ];
    assert_eq!(
        decode_event(&raw, EventLayout::WebOs).unwrap(),
        Event::Key(KeyEvent {
            pressed: true,
            repeat: true,
            scancode: 82,
            keycode: 1_073_741_906,
        })
    );
}

#[test]
fn desktop_escape_release_uses_stock_sdl_without_a_tv_shift() {
    // Stock SDL2 KEYUP: state=0/repeat=0 at12/13, scancode=41, keycode=27.
    let raw = [
        0x01, 0x03, 0x00, 0x00, 0x11, 0x22, 0x33, 0x44, 0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0xaa,
        0xbb, 0x29, 0x00, 0x00, 0x00, 0x1b, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];
    assert_eq!(
        decode_event(&raw, EventLayout::Desktop).unwrap(),
        Event::Key(KeyEvent {
            pressed: false,
            repeat: false,
            scancode: 41,
            keycode: 27,
        })
    );
}

#[test]
fn tv_committed_utf8_text_starts_after_the_extra_payload_word() {
    let raw = [
        0x03, 0x03, 0x00, 0x00, 0, 0, 0, 0, 0, 0, 0, 0, b'b', b'a', b'd', 0, b'c', b'a', b'f',
        0xc3, 0xa9, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0,
    ];
    assert_eq!(
        decode_event(&raw, EventLayout::WebOs).unwrap(),
        Event::Text("café".into())
    );
}

#[test]
fn lifecycle_notifications_keep_the_platform_order_and_quit_distinct() {
    let raw = [
        [0x03, 0x01, 0, 0],
        [0x04, 0x01, 0, 0],
        [0x05, 0x01, 0, 0],
        [0x06, 0x01, 0, 0],
        [0, 0x01, 0, 0],
    ];
    let observed: Vec<_> = raw
        .iter()
        .map(|packet| decode_event(packet, EventLayout::WebOs).unwrap())
        .collect();
    assert_eq!(
        observed,
        vec![
            Event::Lifecycle(criterion_platform::Lifecycle::WillBackground),
            Event::Lifecycle(criterion_platform::Lifecycle::Background),
            Event::Lifecycle(criterion_platform::Lifecycle::WillForeground),
            Event::Lifecycle(criterion_platform::Lifecycle::Foreground),
            Event::Quit,
        ]
    );
}

#[test]
fn drawing_waits_for_did_foreground_and_quit_is_terminal() {
    use criterion_platform::{Activity, Lifecycle};
    let mut state = Activity::Foreground;
    state.observe(&Event::Lifecycle(Lifecycle::WillBackground));
    assert_eq!(state, Activity::Background);
    state.observe(&Event::Lifecycle(Lifecycle::WillForeground));
    assert_eq!(state, Activity::Background);
    state.observe(&Event::Lifecycle(Lifecycle::Foreground));
    assert_eq!(state, Activity::Foreground);
    state.observe(&Event::Quit);
    state.observe(&Event::Lifecycle(Lifecycle::Foreground));
    assert_eq!(state, Activity::Closed);
}

#[test]
fn platform_termination_closes_the_window_lifetime() {
    let event = decode_event(&[1, 1, 0, 0], EventLayout::WebOs).unwrap();
    assert_eq!(event, Event::Quit);
    let mut activity = criterion_platform::Activity::Foreground;
    activity.observe(&event);
    assert_eq!(activity, criterion_platform::Activity::Closed);
}

#[test]
fn pointer_and_flipped_wheel_preserve_signed_motion() {
    let pointer = [
        0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xf6, 0xff, 0xff, 0xff, 0xec,
        0xff, 0xff, 0xff,
    ];
    let wheel = [
        3, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0xfe, 0xff, 0xff, 0xff, 1, 0,
        0, 0,
    ];
    assert_eq!(
        decode_event(&pointer, EventLayout::Desktop).unwrap(),
        Event::PointerMoved { x: -10, y: -20 }
    );
    assert_eq!(
        decode_event(&wheel, EventLayout::WebOs).unwrap(),
        Event::Scroll {
            horizontal: -1,
            vertical: 2
        }
    );
}

#[test]
fn pointer_select_and_focus_loss_arrive_as_separate_events() {
    let button = [
        1, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0x80, 2, 0, 0, 0x68, 1, 0, 0,
    ];
    let focus_lost = [0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 13];
    assert_eq!(
        decode_event(&button, EventLayout::WebOs).unwrap(),
        Event::PointerButton {
            button: 1,
            pressed: true,
            x: 640,
            y: 360
        }
    );
    assert_eq!(
        decode_event(&focus_lost, EventLayout::Desktop).unwrap(),
        Event::KeyboardFocus(false)
    );
}

#[test]
fn ime_composition_keeps_its_selection_separate_from_committed_text() {
    let raw = [
        2, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xaa, 0xbb, 0xcc, 0xdd, b't', b'e', b's', b't', 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0,
        2, 0, 0, 0,
    ];
    assert_eq!(
        decode_event(&raw, EventLayout::WebOs).unwrap(),
        Event::Composition {
            text: "test".into(),
            start: 1,
            length: 2
        }
    );
}

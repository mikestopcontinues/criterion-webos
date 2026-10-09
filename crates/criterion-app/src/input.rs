// SPDX-License-Identifier: GPL-3.0-or-later
//! Pure SDL-to-UI input. Runtime/controller work, native IME requests and GL remain external.
//!
//! Scancode mappings and repeat-marked fresh-press handling follow the pinned PlxNative
//! source at acca94a449a5f7db2c662d6501bd88199ce66d4e, app/events.rs and app/run.rs.
//! The 110 ms directional gate adopts screens/registry.rs's PANEL_REPEAT_MS convention;
//! it is our navigation policy, not a measurement of the official app's cadence.
//! No PlxNative player, synthetic-input transport, key aliases or branding is copied.
//! SDL composition selection offsets have no established byte/character unit here;
//! egui receives transient preedit text without an invented character range.
use criterion_platform::{Activity, Event, Lifecycle, Size, Surface};
use criterion_ui::Action;
use std::time::Duration;

/// Maximum platform events accepted between calls to `take_frame`.
pub const MAX_EVENTS_PER_FRAME: usize = 128;
/// Shared UTF-8 byte budget for committed text and composition in one frame.
pub const MAX_TEXT_BYTES_PER_FRAME: usize = 4096;

/// A complete input batch. Apply cancellation before actions; always deliver `raw` to
/// egui, including CPU-only background frames, so releases reach its persistent state.
/// Overflow discards all queued commands/text; only bounded cancellation effects remain.
/// At most 128 actions and 136 egui events (including 8 cancellation effects) are returned.
pub struct InputFrame {
    pub actions: Vec<Action>,
    pub raw: egui::RawInput,
    pub activity: Activity,
    pub cancel_interactions: bool,
    pub overflowed: bool,
}

/// Stateful input admission with fixed held-key/cancellation storage and bounded queues.
/// The caller injects time since application startup; backward times are clamped.
/// UI-owned field focus controls text acceptance, independently of native IME activation.
pub struct InputAdapter {
    actions: Vec<Action>,
    held: [Option<Duration>; 8],
    activity: Activity,
    keyboard_focused: bool,
    cancel_interactions: bool,
    surface: Surface,
    events: Vec<egui::Event>,
    clock: Duration,
    pointer_down: bool,
    pointer_visible: bool,
    text_input: bool,
    composing: bool,
    modifier_mask: u8,
    editing_held: [bool; 2],
    pending_cancel: CancelEffects,
    published: PublishedInput,
    processed: usize,
    overflowed: bool,
    text_bytes: usize,
}
impl InputAdapter {
    /// Start foregrounded using the window/drawable allocation read by the platform.
    pub fn new(surface: Surface) -> Self {
        Self {
            actions: Vec::new(),
            held: [None; 8],
            activity: Activity::Foreground,
            keyboard_focused: true,
            cancel_interactions: false,
            surface,
            events: Vec::new(),
            clock: Duration::ZERO,
            pointer_down: false,
            pointer_visible: false,
            text_input: false,
            composing: false,
            modifier_mask: 0,
            editing_held: [false; 2],
            pending_cancel: CancelEffects::default(),
            published: PublishedInput::default(),
            processed: 0,
            overflowed: false,
            text_bytes: 0,
        }
    }
    /// Admit one platform event with its current surface and injected clock. Lifecycle
    /// and keyboard-focus state still settle after overflow; ordinary input stays blocked.
    /// Only the primary pointer button activates this TV interface; other buttons are ignored.
    pub fn push(&mut self, event: Event, surface: Surface, now: Duration) {
        self.surface = surface;
        self.clock = self.clock.max(now);
        let now = self.clock;
        self.activity.observe(&event);
        if self.processed == MAX_EVENTS_PER_FRAME && !self.overflowed {
            self.overflow();
        }
        self.processed = self
            .processed
            .saturating_add(1)
            .min(MAX_EVENTS_PER_FRAME + 1);
        match &event {
            Event::Lifecycle(Lifecycle::WillBackground | Lifecycle::Background) | Event::Quit => {
                self.cancel();
                self.pending_cancel.focus = true;
                return;
            }
            Event::Lifecycle(_) => return,
            Event::KeyboardFocus(focused) => {
                self.keyboard_focused = *focused;
                if !focused {
                    self.cancel();
                    self.pending_cancel.focus = true;
                } else if !self.overflowed {
                    self.events.push(egui::Event::WindowFocused(true));
                }
                return;
            }
            _ => {}
        }
        if self.activity != Activity::Foreground || !self.keyboard_focused || self.overflowed {
            return;
        }
        if let Event::Key(key) = event
            && let Some((slot, action)) = navigation(key.scancode)
        {
            if !key.pressed {
                self.held[slot] = None;
                return;
            }
            if key.repeat
                && let Some(previous) = self.held[slot]
                && (matches!(action, Action::Select | Action::Back)
                    || now.saturating_sub(previous) < Duration::from_millis(110))
            {
                return;
            }
            self.held[slot] = Some(now);
            self.actions.push(action);
        } else {
            match event {
                Event::PointerMoved { x, y } => {
                    if let Some([x, y]) =
                        surface.pointer_to_logical(logical_size(), [x as f32, y as f32])
                    {
                        self.pointer_visible = true;
                        self.events
                            .push(egui::Event::PointerMoved(egui::pos2(x, y)));
                    } else {
                        self.leave_pointer();
                    }
                }
                Event::PointerButton {
                    button: 1,
                    pressed,
                    x,
                    y,
                } => {
                    if !pressed && !self.pointer_down {
                        return;
                    }
                    if let Some([x, y]) =
                        surface.pointer_to_logical(logical_size(), [x as f32, y as f32])
                    {
                        self.pointer_down = pressed;
                        self.pointer_visible = true;
                        self.events.push(egui::Event::PointerButton {
                            pos: egui::pos2(x, y),
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: self.modifiers(),
                        });
                    } else {
                        self.leave_pointer();
                    }
                }
                Event::PointerLeft => self.leave_pointer(),
                Event::Key(key) if (224..=231).contains(&key.scancode) => {
                    let before = self.modifiers();
                    let bit = 1u8 << (key.scancode - 224);
                    if key.pressed {
                        self.modifier_mask |= bit;
                    } else {
                        self.modifier_mask &= !bit;
                    }
                    let after = self.modifiers();
                    if before != after {
                        self.events.push(egui::Event::ModifiersChanged(after));
                    }
                }
                Event::Key(key) if self.text_input => {
                    if let Some((slot, editing)) = editing_key(key.scancode) {
                        if key.pressed && self.composing {
                            return;
                        }
                        if !key.pressed && !self.editing_held[slot] {
                            return;
                        }
                        self.editing_held[slot] = key.pressed;
                        self.events.push(egui::Event::Key {
                            key: editing,
                            physical_key: Some(editing),
                            pressed: key.pressed,
                            repeat: key.repeat,
                            modifiers: self.modifiers(),
                        });
                    }
                }
                Event::Scroll {
                    horizontal,
                    vertical,
                } => self.events.push(egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Line,
                    delta: egui::vec2(horizontal as f32, vertical as f32),
                    phase: egui::TouchPhase::Move,
                    modifiers: self.modifiers(),
                }),
                Event::Composition { text, .. } if self.text_input => {
                    if !self.admit_text(&text) {
                        return;
                    }
                    self.composing = !text.is_empty();
                    self.events.push(egui::Event::Ime(egui::ImeEvent::Preedit {
                        text,
                        active_range_chars: None,
                    }));
                }
                Event::Text(text) if self.text_input && !text.is_empty() => {
                    if !self.admit_text(&text) {
                        return;
                    }
                    let event = if self.composing {
                        egui::Event::Ime(egui::ImeEvent::Commit(text))
                    } else {
                        egui::Event::Text(text)
                    };
                    self.composing = false;
                    self.events.push(event);
                }
                _ => {}
            }
        }
    }
    /// Set the UI's active-field intent before polling input. This performs no native call.
    /// Disabling drops queued edits and ends composition without committing candidates.
    pub fn set_text_input(&mut self, enabled: bool) {
        self.text_input = enabled;
        if !enabled {
            self.text_bytes = 0;
            self.events.retain(|event| {
                !matches!(
                    event,
                    egui::Event::Text(_)
                        | egui::Event::Ime(_)
                        | egui::Event::Key {
                            key: egui::Key::Backspace | egui::Key::Delete,
                            ..
                        }
                )
            });
            for (slot, pending) in self.pending_cancel.editing.iter_mut().enumerate() {
                *pending |= self.editing_held[slot] || self.published.editing[slot];
            }
            self.editing_held = [false; 2];
            self.pending_cancel.composing |= self.composing || self.published.composing;
            self.composing = false;
        }
    }
    /// Drain one complete batch and reopen its event/byte budget. Cancellation effects
    /// precede any fresh admitted input. Screen coordinates remain the UI logical canvas.
    pub fn take_frame(&mut self, now: Duration) -> InputFrame {
        self.clock = self.clock.max(now);
        self.processed = 0;
        self.text_bytes = 0;
        let overflowed = std::mem::take(&mut self.overflowed);
        let mut events = Vec::new();
        std::mem::take(&mut self.pending_cancel).append_to(&mut events);
        events.append(&mut self.events);
        for event in &events {
            self.published.observe(event);
        }
        let mut raw = egui::RawInput {
            focused: self.activity == Activity::Foreground && self.keyboard_focused && !overflowed,
            events,
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(criterion_ui::LOGICAL_SIZE[0], criterion_ui::LOGICAL_SIZE[1]),
            )),
            time: Some(self.clock.as_secs_f64()),
            ..Default::default()
        };
        if let Some(viewport) = raw.viewports.get_mut(&egui::ViewportId::ROOT) {
            viewport.native_pixels_per_point =
                self.surface.fit(logical_size()).map(|fit| fit.scale as f32);
            viewport.focused = Some(raw.focused);
        }
        InputFrame {
            actions: std::mem::take(&mut self.actions),
            raw,
            activity: self.activity,
            cancel_interactions: std::mem::take(&mut self.cancel_interactions),
            overflowed,
        }
    }
    fn overflow(&mut self) {
        self.cancel();
        self.overflowed = true;
        self.pending_cancel.focus = true;
    }
    fn admit_text(&mut self, text: &str) -> bool {
        if text.len() > MAX_TEXT_BYTES_PER_FRAME.saturating_sub(self.text_bytes) {
            self.overflow();
            return false;
        }
        self.text_bytes += text.len();
        true
    }
    fn cancel(&mut self) {
        self.text_bytes = 0;
        self.actions.clear();
        self.events.clear();
        self.held = [None; 8];
        self.cancel_interactions = true;
        for (slot, pending) in self.pending_cancel.editing.iter_mut().enumerate() {
            *pending |= self.editing_held[slot] || self.published.editing[slot];
        }
        self.pending_cancel.modifiers |= self.modifier_mask != 0 || self.published.modifiers;
        self.pending_cancel.composing |= self.composing || self.published.composing;
        self.pending_cancel.pointer_down |= self.pointer_down || self.published.pointer_down;
        self.pending_cancel.pointer_gone |= self.pointer_visible || self.published.pointer_visible;
        self.editing_held = [false; 2];
        self.modifier_mask = 0;
        self.composing = false;
        self.text_input = false;
        self.pointer_down = false;
        self.pointer_visible = false;
    }
    fn modifiers(&self) -> egui::Modifiers {
        let ctrl = self.modifier_mask & 0x11 != 0;
        egui::Modifiers {
            ctrl,
            command: ctrl,
            shift: self.modifier_mask & 0x22 != 0,
            alt: self.modifier_mask & 0x44 != 0,
            mac_cmd: false,
        }
    }
    fn leave_pointer(&mut self) {
        if self.pointer_down {
            self.cancel();
        } else if self.pointer_visible {
            self.events.push(egui::Event::PointerGone);
            self.pointer_visible = false;
        }
    }
}

// State already returned to the consumer remains distinct from newly ingested state.
// A queued release/empty preedit can be discarded, while the consumer still holds its down.
#[derive(Default)]
struct PublishedInput {
    pointer_down: bool,
    pointer_visible: bool,
    composing: bool,
    editing: [bool; 2],
    modifiers: bool,
}
impl PublishedInput {
    fn observe(&mut self, event: &egui::Event) {
        match event {
            egui::Event::PointerMoved(_) => self.pointer_visible = true,
            egui::Event::PointerButton {
                button: egui::PointerButton::Primary,
                pressed,
                ..
            } => {
                self.pointer_down = *pressed;
                self.pointer_visible = true;
            }
            egui::Event::PointerGone => self.pointer_visible = false,
            egui::Event::Key { key, pressed, .. } => match key {
                egui::Key::Backspace => self.editing[0] = *pressed,
                egui::Key::Delete => self.editing[1] = *pressed,
                _ => {}
            },
            egui::Event::ModifiersChanged(modifiers) => {
                self.modifiers = *modifiers != egui::Modifiers::NONE
            }
            egui::Event::Ime(egui::ImeEvent::Preedit { text, .. }) => {
                self.composing = !text.is_empty()
            }
            egui::Event::Ime(egui::ImeEvent::Commit(_)) => self.composing = false,
            _ => {}
        }
    }
}

// Cancellation is latched separately from queued input: repeated will/did notifications
// cannot erase releases before the caller takes the frame.
#[derive(Default)]
struct CancelEffects {
    pointer_down: bool,
    pointer_gone: bool,
    composing: bool,
    editing: [bool; 2],
    modifiers: bool,
    focus: bool,
}
impl CancelEffects {
    fn append_to(self, events: &mut Vec<egui::Event>) {
        for (held, key) in self
            .editing
            .into_iter()
            .zip([egui::Key::Backspace, egui::Key::Delete])
        {
            if held {
                events.push(egui::Event::Key {
                    key,
                    physical_key: Some(key),
                    pressed: false,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                });
            }
        }
        if self.modifiers {
            events.push(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
        }
        if self.composing {
            events.push(egui::Event::Ime(egui::ImeEvent::Preedit {
                text: String::new(),
                active_range_chars: None,
            }));
        }
        if self.pointer_down {
            // Releasing at the previous valid target could activate it. Move outside
            // the logical canvas first, farther than egui's click-distance threshold
            // even for a press at its origin. PointerGone alone does not release buttons.
            let outside = egui::pos2(
                -criterion_ui::LOGICAL_SIZE[0],
                -criterion_ui::LOGICAL_SIZE[1],
            );
            events.push(egui::Event::PointerMoved(outside));
            events.push(egui::Event::PointerButton {
                pos: outside,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            });
        }
        if self.pointer_gone {
            events.push(egui::Event::PointerGone);
        }
        if self.focus {
            events.push(egui::Event::WindowFocused(false));
        }
    }
}

fn editing_key(scancode: u32) -> Option<(usize, egui::Key)> {
    match scancode {
        42 => Some((0, egui::Key::Backspace)),
        76 => Some((1, egui::Key::Delete)),
        _ => None,
    }
}

fn logical_size() -> Size {
    Size {
        width: criterion_ui::LOGICAL_SIZE[0] as u32,
        height: criterion_ui::LOGICAL_SIZE[1] as u32,
    }
}

fn navigation(scancode: u32) -> Option<(usize, Action)> {
    match scancode {
        79 => Some((0, Action::Right)),
        80 => Some((1, Action::Left)),
        81 => Some((2, Action::Down)),
        82 => Some((3, Action::Up)),
        40 => Some((4, Action::Select)),
        88 => Some((5, Action::Select)),
        41 => Some((6, Action::Back)),
        482 => Some((7, Action::Back)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::InputAdapter;
    use criterion_platform::{Event, KeyEvent, Size, Surface};
    use criterion_ui::Action;
    use std::time::Duration;
    fn surface() -> Surface {
        Surface {
            window: Size {
                width: 1920,
                height: 1080,
            },
            drawable: Size {
                width: 1920,
                height: 1080,
            },
        }
    }
    fn key(scancode: u32, keycode: i32, pressed: bool, repeat: bool) -> Event {
        Event::Key(KeyEvent {
            scancode,
            keycode,
            pressed,
            repeat,
        })
    }

    #[test]
    fn lg_back_with_no_keycode_produces_one_navigation_action() {
        let mut input = InputAdapter::new(surface());
        input.push(key(482, 0, true, false), surface(), Duration::ZERO);
        let frame = input.take_frame(Duration::ZERO);
        assert_eq!(frame.actions, vec![Action::Back]);
        assert!(
            frame.raw.events.is_empty(),
            "navigation is not also delivered to egui"
        );
    }
    #[test]
    fn physical_navigation_maps_without_legacy_or_editing_aliases() {
        let mut input = InputAdapter::new(surface());
        for (scancode, keycode) in [
            (79, 1_073_741_903),
            (80, 1_073_741_904),
            (81, 1_073_741_905),
            (82, 1_073_741_906),
            (40, 13),
            (88, 1_073_741_912),
            (41, 27),
            (482, 0),
        ] {
            input.push(
                key(scancode, keycode, true, false),
                surface(),
                Duration::ZERO,
            );
            input.push(
                key(scancode, keycode, false, false),
                surface(),
                Duration::ZERO,
            );
        }
        for (scancode, keycode) in [
            (77, 1_073_741_901),
            (20, 113),
            (42, 8),
            (461, 0),
            (270, 1_073_742_094),
        ] {
            input.push(
                key(scancode, keycode, true, false),
                surface(),
                Duration::ZERO,
            );
        }
        let frame = input.take_frame(Duration::ZERO);
        assert_eq!(
            frame.actions,
            vec![
                Action::Right,
                Action::Left,
                Action::Down,
                Action::Up,
                Action::Select,
                Action::Select,
                Action::Back,
                Action::Back
            ]
        );
        assert!(frame.raw.events.is_empty());
    }

    #[test]
    fn hardware_repeats_are_paced_and_activation_repeats_are_suppressed() {
        let mut input = InputAdapter::new(surface());
        input.push(
            key(82, 1_073_741_906, true, false),
            surface(),
            Duration::ZERO,
        );
        input.push(
            key(82, 1_073_741_906, true, true),
            surface(),
            Duration::from_millis(109),
        );
        input.push(
            key(82, 1_073_741_906, true, true),
            surface(),
            Duration::from_millis(110),
        );
        input.push(
            key(81, 1_073_741_905, false, false),
            surface(),
            Duration::from_millis(111),
        );
        input.push(
            key(82, 1_073_741_906, true, true),
            surface(),
            Duration::from_millis(219),
        );
        input.push(
            key(82, 1_073_741_906, true, true),
            surface(),
            Duration::from_millis(220),
        );
        input.push(
            key(40, 13, true, false),
            surface(),
            Duration::from_millis(300),
        );
        input.push(
            key(40, 13, true, true),
            surface(),
            Duration::from_millis(800),
        );
        // A repeat-marked first press is fresh when the IME swallowed its preceding key-up.
        input.push(
            key(482, 0, true, true),
            surface(),
            Duration::from_millis(820),
        );
        input.push(
            key(450, 0, true, false),
            surface(),
            Duration::from_millis(830),
        );
        input.push(
            key(482, 0, true, true),
            surface(),
            Duration::from_millis(1000),
        );
        assert_eq!(
            input.take_frame(Duration::from_millis(1000)).actions,
            vec![
                Action::Up,
                Action::Up,
                Action::Up,
                Action::Select,
                Action::Back
            ]
        );
    }

    #[test]
    fn background_cancels_queued_actions_and_quit_is_terminal() {
        use criterion_platform::{Activity, Lifecycle};
        let mut input = InputAdapter::new(surface());
        input.push(
            key(82, 1_073_741_906, true, false),
            surface(),
            Duration::ZERO,
        );
        input.push(
            Event::Lifecycle(Lifecycle::WillBackground),
            surface(),
            Duration::ZERO,
        );
        input.push(
            key(40, 13, true, false),
            surface(),
            Duration::from_millis(10),
        );
        input.push(
            Event::Lifecycle(Lifecycle::WillForeground),
            surface(),
            Duration::from_millis(20),
        );
        input.push(
            key(482, 0, true, false),
            surface(),
            Duration::from_millis(30),
        );
        let hidden = input.take_frame(Duration::from_millis(40));
        assert_eq!(hidden.activity, Activity::Background);
        assert!(hidden.actions.is_empty());
        assert!(hidden.cancel_interactions);
        assert!(!hidden.raw.focused);
        input.push(
            Event::Lifecycle(Lifecycle::Foreground),
            surface(),
            Duration::from_millis(50),
        );
        input.push(
            key(40, 13, true, true),
            surface(),
            Duration::from_millis(60),
        );
        let restored = input.take_frame(Duration::from_millis(70));
        assert_eq!(restored.actions, vec![Action::Select]);
        assert!(restored.raw.focused);
        assert!(!restored.cancel_interactions);
        input.push(Event::Quit, surface(), Duration::from_millis(80));
        input.push(
            Event::Lifecycle(Lifecycle::Foreground),
            surface(),
            Duration::from_millis(90),
        );
        input.push(
            key(482, 0, true, false),
            surface(),
            Duration::from_millis(100),
        );
        let closed = input.take_frame(Duration::from_millis(110));
        assert_eq!(closed.activity, Activity::Closed);
        assert!(closed.actions.is_empty());
        assert!(closed.cancel_interactions);
        assert!(!closed.raw.focused);
    }

    #[test]
    fn pointer_coordinates_follow_the_centered_high_dpi_canvas() {
        let tall = Surface {
            window: Size {
                width: 640,
                height: 400,
            },
            drawable: Size {
                width: 1280,
                height: 800,
            },
        };
        let mut input = InputAdapter::new(tall);
        input.push(
            Event::PointerMoved { x: 320, y: 200 },
            tall,
            Duration::from_secs(1),
        );
        input.push(
            Event::PointerButton {
                button: 1,
                pressed: true,
                x: 320,
                y: 200,
            },
            tall,
            Duration::from_secs(1),
        );
        input.push(
            Event::PointerButton {
                button: 1,
                pressed: false,
                x: 320,
                y: 200,
            },
            tall,
            Duration::from_secs(1),
        );
        let frame = input.take_frame(Duration::from_millis(1250));
        assert_eq!(
            frame.raw.events,
            vec![
                egui::Event::PointerMoved(egui::pos2(960.0, 540.0)),
                egui::Event::PointerButton {
                    pos: egui::pos2(960.0, 540.0),
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE
                },
                egui::Event::PointerButton {
                    pos: egui::pos2(960.0, 540.0),
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE
                },
            ]
        );
        assert_eq!(
            frame.raw.screen_rect,
            Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1920.0, 1080.0)
            ))
        );
        assert_eq!(frame.raw.time, Some(1.25));
        assert!(
            (frame.raw.viewport().native_pixels_per_point.unwrap() - 0.666_666_7).abs() < 0.000_001
        );
    }

    #[test]
    fn release_outside_the_canvas_cancels_without_clicking_or_sticking() {
        let tall = Surface {
            window: Size {
                width: 640,
                height: 400,
            },
            drawable: Size {
                width: 1280,
                height: 800,
            },
        };
        let mut input = InputAdapter::new(tall);
        let context = egui::Context::default();
        input.push(
            Event::PointerButton {
                button: 1,
                pressed: true,
                x: 320,
                y: 200,
            },
            tall,
            Duration::ZERO,
        );
        let mut output = context.run_ui(input.take_frame(Duration::ZERO).raw, |_| {});
        output.textures_delta.clear();
        assert!(context.input(|i| i.pointer.primary_down()));
        input.push(
            Event::PointerButton {
                button: 1,
                pressed: false,
                x: 320,
                y: 10,
            },
            tall,
            Duration::from_millis(10),
        );
        let canceled = input.take_frame(Duration::from_millis(20));
        assert!(canceled.cancel_interactions);
        assert_eq!(canceled.raw.events.last(), Some(&egui::Event::PointerGone));
        let mut output = context.run_ui(canceled.raw, |_| {});
        output.textures_delta.clear();
        assert!(!context.input(|i| i.pointer.primary_down()));
        assert!(!context.input(|i| i.pointer.primary_clicked()));
    }

    #[test]
    fn committed_text_requires_an_active_field_and_composition_is_transient() {
        let mut input = InputAdapter::new(surface());
        input.push(Event::Text("hidden".into()), surface(), Duration::ZERO);
        assert!(input.take_frame(Duration::ZERO).raw.events.is_empty());
        input.set_text_input(true);
        input.push(
            Event::Composition {
                text: "te".into(),
                start: 0,
                length: 2,
            },
            surface(),
            Duration::ZERO,
        );
        input.push(Event::Text("test".into()), surface(), Duration::ZERO);
        input.push(Event::Text("!".into()), surface(), Duration::ZERO);
        assert_eq!(
            input.take_frame(Duration::ZERO).raw.events,
            vec![
                egui::Event::Ime(egui::ImeEvent::Preedit {
                    text: "te".into(),
                    active_range_chars: None
                }),
                egui::Event::Ime(egui::ImeEvent::Commit("test".into())),
                egui::Event::Text("!".into()),
            ]
        );
        input.push(Event::Text("stale".into()), surface(), Duration::ZERO);
        input.set_text_input(false);
        input.push(Event::Text("ignored".into()), surface(), Duration::ZERO);
        assert!(input.take_frame(Duration::ZERO).raw.events.is_empty());
    }

    #[test]
    fn editing_keys_and_wheel_carry_linux_modifiers_without_navigation_aliases() {
        let mut input = InputAdapter::new(surface());
        input.set_text_input(true);
        input.push(
            key(225, 1_073_742_049, true, false),
            surface(),
            Duration::ZERO,
        );
        input.push(
            key(224, 1_073_742_048, true, false),
            surface(),
            Duration::ZERO,
        );
        input.push(key(42, 8, true, false), surface(), Duration::ZERO);
        input.push(key(42, 8, false, false), surface(), Duration::ZERO);
        input.push(
            key(82, 1_073_741_906, true, false),
            surface(),
            Duration::ZERO,
        );
        input.push(
            Event::Scroll {
                horizontal: 2,
                vertical: -3,
            },
            surface(),
            Duration::ZERO,
        );
        input.push(
            key(225, 1_073_742_049, false, false),
            surface(),
            Duration::ZERO,
        );
        input.push(
            key(224, 1_073_742_048, false, false),
            surface(),
            Duration::ZERO,
        );
        let ctrl_shift = egui::Modifiers {
            ctrl: true,
            shift: true,
            command: true,
            ..Default::default()
        };
        let frame = input.take_frame(Duration::ZERO);
        assert_eq!(frame.actions, vec![Action::Up]);
        assert_eq!(
            frame.raw.events,
            vec![
                egui::Event::ModifiersChanged(egui::Modifiers::SHIFT),
                egui::Event::ModifiersChanged(ctrl_shift),
                egui::Event::Key {
                    key: egui::Key::Backspace,
                    physical_key: Some(egui::Key::Backspace),
                    pressed: true,
                    repeat: false,
                    modifiers: ctrl_shift
                },
                egui::Event::Key {
                    key: egui::Key::Backspace,
                    physical_key: Some(egui::Key::Backspace),
                    pressed: false,
                    repeat: false,
                    modifiers: ctrl_shift
                },
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Line,
                    delta: egui::vec2(2.0, -3.0),
                    phase: egui::TouchPhase::Move,
                    modifiers: ctrl_shift
                },
                egui::Event::ModifiersChanged(egui::Modifiers {
                    ctrl: true,
                    command: true,
                    ..Default::default()
                }),
                egui::Event::ModifiersChanged(egui::Modifiers::NONE),
            ]
        );
    }

    #[test]
    fn paired_background_notifications_preserve_the_pointer_cancellation() {
        use criterion_platform::Lifecycle;
        let mut input = InputAdapter::new(surface());
        let context = egui::Context::default();
        input.push(
            Event::PointerButton {
                button: 1,
                pressed: true,
                x: 300,
                y: 300,
            },
            surface(),
            Duration::ZERO,
        );
        let mut output = context.run_ui(input.take_frame(Duration::ZERO).raw, |_| {});
        output.textures_delta.clear();
        assert!(context.input(|i| i.pointer.primary_down()));
        input.push(
            Event::Lifecycle(Lifecycle::WillBackground),
            surface(),
            Duration::ZERO,
        );
        input.push(
            Event::Lifecycle(Lifecycle::Background),
            surface(),
            Duration::ZERO,
        );
        let hidden = input.take_frame(Duration::ZERO);
        assert!(hidden.cancel_interactions);
        let mut output = context.run_ui(hidden.raw, |_| {});
        output.textures_delta.clear();
        assert!(!context.input(|i| i.pointer.primary_down()));
        assert!(!context.input(|i| i.pointer.primary_clicked()));
    }

    #[test]
    fn event_overflow_discards_the_entire_command_batch_and_releases_input() {
        let mut input = InputAdapter::new(surface());
        let context = egui::Context::default();
        input.push(
            Event::PointerButton {
                button: 1,
                pressed: true,
                x: 300,
                y: 300,
            },
            surface(),
            Duration::ZERO,
        );
        let mut output = context.run_ui(input.take_frame(Duration::ZERO).raw, |_| {});
        output.textures_delta.clear();
        for _ in 0..129 {
            input.push(key(40, 13, true, false), surface(), Duration::ZERO);
        }
        input.push(key(482, 0, true, false), surface(), Duration::ZERO);
        let overflow = input.take_frame(Duration::ZERO);
        assert!(overflow.overflowed);
        assert!(overflow.cancel_interactions);
        assert!(
            overflow.actions.is_empty(),
            "no partial commands survive overflow"
        );
        assert!(!overflow.raw.focused);
        assert!(overflow.raw.events.len() <= 136);
        let mut output = context.run_ui(overflow.raw, |_| {});
        output.textures_delta.clear();
        assert!(!context.input(|i| i.pointer.primary_down()));
        assert!(!context.input(|i| i.pointer.primary_clicked()));
        input.push(key(482, 0, true, false), surface(), Duration::ZERO);
        let recovered = input.take_frame(Duration::ZERO);
        assert!(!recovered.overflowed);
        assert_eq!(recovered.actions, vec![Action::Back]);
    }

    #[test]
    fn excessive_text_cancels_the_batch_without_retaining_the_payload() {
        let mut input = InputAdapter::new(surface());
        input.set_text_input(true);
        input.push(
            key(82, 1_073_741_906, true, false),
            surface(),
            Duration::ZERO,
        );
        input.push(
            Event::Text(format!("{}x", "é".repeat(2048))),
            surface(),
            Duration::ZERO,
        );
        let rejected = input.take_frame(Duration::ZERO);
        assert!(rejected.overflowed);
        assert!(rejected.actions.is_empty());
        assert!(!rejected.raw.events.iter().any(|event| matches!(
            event,
            egui::Event::Text(_) | egui::Event::Ime(egui::ImeEvent::Commit(_))
        )));
        input.set_text_input(true);
        input.push(Event::Text("a".repeat(2048)), surface(), Duration::ZERO);
        input.push(Event::Text("b".repeat(2048)), surface(), Duration::ZERO);
        input.push(
            Event::Composition {
                text: "c".into(),
                start: 0,
                length: 0,
            },
            surface(),
            Duration::ZERO,
        );
        let rejected = input.take_frame(Duration::ZERO);
        assert!(
            rejected.overflowed,
            "composition and commits share the frame byte budget"
        );
        assert!(!rejected.raw.events.iter().any(|event| matches!(
            event,
            egui::Event::Text(_) | egui::Event::Ime(egui::ImeEvent::Commit(_))
        )));
    }

    #[test]
    fn composition_does_not_hide_a_previously_held_editing_key_release() {
        let mut input = InputAdapter::new(surface());
        let context = egui::Context::default();
        input.set_text_input(true);
        input.push(key(42, 8, true, false), surface(), Duration::ZERO);
        let mut output = context.run_ui(input.take_frame(Duration::ZERO).raw, |_| {});
        output.textures_delta.clear();
        assert!(context.input(|state| state.key_down(egui::Key::Backspace)));
        input.push(
            Event::Composition {
                text: "candidate".into(),
                start: 0,
                length: 0,
            },
            surface(),
            Duration::ZERO,
        );
        input.push(key(42, 8, false, false), surface(), Duration::ZERO);
        let mut output = context.run_ui(input.take_frame(Duration::ZERO).raw, |_| {});
        output.textures_delta.clear();
        assert!(!context.input(|state| state.key_down(egui::Key::Backspace)));
    }

    #[test]
    fn cancellation_of_a_near_origin_press_cannot_become_a_click() {
        let mut input = InputAdapter::new(surface());
        let context = egui::Context::default();
        input.push(
            Event::PointerButton {
                button: 1,
                pressed: true,
                x: 0,
                y: 0,
            },
            surface(),
            Duration::ZERO,
        );
        let mut output = context.run_ui(input.take_frame(Duration::ZERO).raw, |_| {});
        output.textures_delta.clear();
        assert!(context.input(|state| state.pointer.primary_down()));
        input.push(Event::PointerLeft, surface(), Duration::from_millis(10));
        let mut output = context.run_ui(input.take_frame(Duration::from_millis(10)).raw, |_| {});
        output.textures_delta.clear();
        assert!(!context.input(|state| state.pointer.primary_down()));
        assert!(!context.input(|state| state.pointer.primary_clicked()));
    }

    #[test]
    fn queued_pointer_release_survives_background_cancellation() {
        let mut input = InputAdapter::new(surface());
        let context = egui::Context::default();
        input.push(
            Event::PointerButton {
                button: 1,
                pressed: true,
                x: 300,
                y: 300,
            },
            surface(),
            Duration::ZERO,
        );
        let mut output = context.run_ui(input.take_frame(Duration::ZERO).raw, |_| {});
        output.textures_delta.clear();
        assert!(context.input(|state| state.pointer.primary_down()));
        input.push(
            Event::PointerButton {
                button: 1,
                pressed: false,
                x: 300,
                y: 300,
            },
            surface(),
            Duration::from_millis(10),
        );
        input.push(
            Event::Lifecycle(criterion_platform::Lifecycle::WillBackground),
            surface(),
            Duration::from_millis(10),
        );
        let mut output = context.run_ui(input.take_frame(Duration::from_millis(10)).raw, |_| {});
        output.textures_delta.clear();
        assert!(!context.input(|state| state.pointer.primary_down()));
        assert!(!context.input(|state| state.pointer.primary_clicked()));
    }

    #[test]
    fn queued_editing_release_survives_field_deactivation() {
        let mut input = InputAdapter::new(surface());
        let context = egui::Context::default();
        input.set_text_input(true);
        input.push(key(42, 8, true, false), surface(), Duration::ZERO);
        let mut output = context.run_ui(input.take_frame(Duration::ZERO).raw, |_| {});
        output.textures_delta.clear();
        assert!(context.input(|state| state.key_down(egui::Key::Backspace)));
        input.push(key(42, 8, false, false), surface(), Duration::ZERO);
        input.set_text_input(false);
        let mut output = context.run_ui(input.take_frame(Duration::ZERO).raw, |_| {});
        output.textures_delta.clear();
        assert!(!context.input(|state| state.key_down(egui::Key::Backspace)));
    }

    #[test]
    fn queued_empty_preedit_survives_field_deactivation() {
        let mut input = InputAdapter::new(surface());
        input.set_text_input(true);
        input.push(
            Event::Composition {
                text: "candidate".into(),
                start: 0,
                length: 0,
            },
            surface(),
            Duration::ZERO,
        );
        assert_eq!(
            input.take_frame(Duration::ZERO).raw.events,
            vec![egui::Event::Ime(egui::ImeEvent::Preedit {
                text: "candidate".into(),
                active_range_chars: None
            }),]
        );
        input.push(
            Event::Composition {
                text: String::new(),
                start: 0,
                length: 0,
            },
            surface(),
            Duration::ZERO,
        );
        input.set_text_input(false);
        assert_eq!(
            input.take_frame(Duration::ZERO).raw.events,
            vec![egui::Event::Ime(egui::ImeEvent::Preedit {
                text: String::new(),
                active_range_chars: None
            }),]
        );
    }
}

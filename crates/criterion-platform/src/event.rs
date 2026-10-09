// SPDX-License-Identifier: GPL-3.0-or-later
// Portions adapted from PlxNative; Copyright © 2026 Gleb Linnik.
// Modified for Criterion Unofficial. See ../NOTICE.md for exact provenance.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventLayout {
    Desktop,
    WebOs,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Lifecycle {
    WillBackground,
    Background,
    WillForeground,
    Foreground,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KeyEvent {
    pub pressed: bool,
    pub repeat: bool,
    pub scancode: u32,
    pub keycode: i32,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Event {
    Composition {
        text: String,
        start: i32,
        length: i32,
    },
    PointerButton {
        button: u8,
        pressed: bool,
        x: i32,
        y: i32,
    },
    WindowChanged,
    KeyboardFocus(bool),
    PointerLeft,
    PointerMoved {
        x: i32,
        y: i32,
    },
    Scroll {
        horizontal: i32,
        vertical: i32,
    },
    Text(String),
    Key(KeyEvent),
    Lifecycle(Lifecycle),
    Quit,
    Unhandled(u32),
}

fn word(raw: &[u8], offset: usize) -> Result<u32, &'static str> {
    let bytes: [u8; 4] = raw
        .get(offset..offset + 4)
        .ok_or("truncated SDL event")?
        .try_into()
        .map_err(|_| "truncated SDL event")?;
    Ok(u32::from_le_bytes(bytes))
}

pub fn decode_event(raw: &[u8], layout: EventLayout) -> Result<Event, &'static str> {
    let kind = word(raw, 0)?;
    match kind {
        0x100 | 0x101 => return Ok(Event::Quit),
        0x103 => return Ok(Event::Lifecycle(Lifecycle::WillBackground)),
        0x104 => return Ok(Event::Lifecycle(Lifecycle::Background)),
        0x105 => return Ok(Event::Lifecycle(Lifecycle::WillForeground)),
        0x106 => return Ok(Event::Lifecycle(Lifecycle::Foreground)),
        _ => {}
    }
    if kind == 0x300 || kind == 0x301 {
        if layout == EventLayout::WebOs {
            let state = word(raw, 16)?;
            return Ok(Event::Key(KeyEvent {
                pressed: state & 0xff == 1,
                repeat: state & 0x100 != 0,
                scancode: word(raw, 20)?,
                keycode: word(raw, 24)? as i32,
            }));
        }
        return Ok(Event::Key(KeyEvent {
            pressed: *raw.get(12).ok_or("truncated SDL event")? == 1,
            repeat: *raw.get(13).ok_or("truncated SDL event")? != 0,
            scancode: word(raw, 16)?,
            keycode: word(raw, 20)? as i32,
        }));
    }
    if kind == 0x200 {
        return Ok(match *raw.get(12).ok_or("truncated SDL window event")? {
            5 | 6 | 18 => Event::WindowChanged,
            11 => Event::PointerLeft,
            12 => Event::KeyboardFocus(true),
            13 => Event::KeyboardFocus(false),
            14 => Event::Quit,
            _ => Event::Unhandled(kind),
        });
    }
    if kind == 0x401 || kind == 0x402 {
        return Ok(Event::PointerButton {
            button: *raw.get(16).ok_or("truncated SDL button event")?,
            pressed: *raw.get(17).ok_or("truncated SDL button event")? == 1,
            x: word(raw, 20)? as i32,
            y: word(raw, 24)? as i32,
        });
    }
    if kind == 0x400 {
        return Ok(Event::PointerMoved {
            x: word(raw, 20)? as i32,
            y: word(raw, 24)? as i32,
        });
    }
    if kind == 0x403 {
        let horizontal = word(raw, 16)? as i32;
        let vertical = word(raw, 20)? as i32;
        let flipped = word(raw, 24)? == 1;
        return Ok(Event::Scroll {
            horizontal: if flipped {
                horizontal.saturating_neg()
            } else {
                horizontal
            },
            vertical: if flipped {
                vertical.saturating_neg()
            } else {
                vertical
            },
        });
    }
    if kind == 0x302 || kind == 0x303 {
        let offset = if layout == EventLayout::WebOs { 16 } else { 12 };
        let field = raw
            .get(offset..offset + 32)
            .ok_or("truncated SDL text event")?;
        let end = field
            .iter()
            .position(|byte| *byte == 0)
            .ok_or("unterminated SDL text event")?;
        let text = std::str::from_utf8(&field[..end]).map_err(|_| "invalid SDL text UTF-8")?;
        if kind == 0x302 {
            return Ok(Event::Composition {
                text: text.into(),
                start: word(raw, offset + 32)? as i32,
                length: word(raw, offset + 36)? as i32,
            });
        }
        return Ok(Event::Text(text.into()));
    }
    Ok(Event::Unhandled(kind))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Activity {
    Foreground,
    Background,
    Closed,
}
impl Activity {
    pub fn observe(&mut self, event: &Event) {
        if *self == Self::Closed {
            return;
        }
        match event {
            Event::Quit => *self = Self::Closed,
            Event::Lifecycle(Lifecycle::WillBackground | Lifecycle::Background) => {
                *self = Self::Background
            }
            Event::Lifecycle(Lifecycle::Foreground) => *self = Self::Foreground,
            _ => {}
        }
    }
}

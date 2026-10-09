# Upstream attribution

The keyboard-layout, inline-text-layout and foreground-admission implementations in `src/event.rs` are adapted from PlxNative 0.8.0, commit `acca94a449a5f7db2c662d6501bd88199ce66d4e`:

- [`rust-modules/src/app/events.rs`](https://github.com/GLinnik21/plx-native/blob/acca94a449a5f7db2c662d6501bd88199ce66d4e/rust-modules/src/app/events.rs), raw keyboard decoding.
- [`rust-modules/src/textinput.rs`](https://github.com/GLinnik21/plx-native/blob/acca94a449a5f7db2c662d6501bd88199ce66d4e/rust-modules/src/textinput.rs), desktop versus LG inline text offsets.
- [`rust-modules/src/app/window_activity.rs`](https://github.com/GLinnik21/plx-native/blob/acca94a449a5f7db2c662d6501bd88199ce66d4e/rust-modules/src/app/window_activity.rs), background and did-foreground admission.

Copyright © 2026 Gleb Linnik. Licensed under GNU GPL version 3 or, at your option, any later version. The modifications expose checked, pure event decoding and explicit activity state; they omit PlxNative's synthetic input, remote FIFO, screen system, player and text queue. No upstream names, logos, splash artwork or product branding are included. [Upstream licensing](https://github.com/GLinnik21/plx-native/blob/acca94a449a5f7db2c662d6501bd88199ce66d4e/LICENSING.md) and [trademark reservation](https://github.com/GLinnik21/plx-native/blob/acca94a449a5f7db2c662d6501bd88199ce66d4e/TRADEMARKS.md) identify those separate terms.

This crate is distributed without warranty; see the project's [GPL license](../../LICENSE). The window's narrow SDL2 declarations and other behavior are original integration code based on SDL's public C API. SDL is linked as a platform library rather than copied or bundled by this crate; release packaging owns the applicable library notices.

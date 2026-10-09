# Platform test evidence

`events.rs` contains handcrafted, literal little-endian SDL event packets. They are independent inputs and expected values from the C ABI and pinned integration source; none is a TV capture or a decoder round trip.

The authoritative provenance is:

- [PlxNative keyboard decoder](https://github.com/GLinnik21/plx-native/blob/acca94a449a5f7db2c662d6501bd88199ce66d4e/rust-modules/src/app/events.rs): LG state/scancode/keycode at 16/20/24; stock state/repeat at 12/13 and scancode/keycode at 16/20. Its documented measured scancodes include Up 82 and LG Back 482; the stock SDL scancode/keycode declarations define Escape 41/27 and Up 82/1073741906.
- [PlxNative inline text decoder](https://github.com/GLinnik21/plx-native/blob/acca94a449a5f7db2c662d6501bd88199ce66d4e/rust-modules/src/textinput.rs): text is an inline 32-byte field, at 16 on LG and 12 on desktop.
- [SDL2 event header](https://github.com/libsdl-org/SDL/blob/release-2.26.5/include/SDL_events.h): event tags, window subtypes, pointer coordinates, button bytes, wheel direction and lifecycle order. `SDL_APP_TERMINATING` closes the application lifetime.
- [Community webOS SDL header](https://github.com/webosbrew/SDL-webOS/blob/webOS-2.30.x/include/SDL_events.h): the `SDL_WEBOS_BROKEN_ABI` inputSource field independently corroborates the shifted keyboard and inline composition/text layouts. This header is corroboration, not proof of the stock C4 library version or complete ABI.

The fixture selection deliberately distinguishes shifted fields, released keys, repeated keys, signed motion, flipped scrolling, UTF-8 commits, composition selection and will/did foreground. `surface.rs` uses worked geometry examples with literal viewport and pointer results.

`startup_failure.rs` links host SDL and runs each case in an isolated subprocess with a deliberately nonexistent video driver. It exercises failed-constructor recovery and rejection/preservation of another SDL event owner. These cases cannot create a window, graphics context or GPU session. Older SDL's partial event initialization motivates explicit failure cleanup; [SDL 2.0.10's initialization implementation](https://github.com/libsdl-org/SDL/blob/release-2.0.10/src/SDL.c) is the source rationale. The maintained host version may already roll back this dependency, so its recovery case is not a regression witness for that older fork.

A successful fixture/CPU run does not establish actual stock-TV event delivery, IME focus behavior, rendered pixels, compositor presentation, physical remote input, audio or licensed playback. The designated host/native executor owns those acceptance checks.

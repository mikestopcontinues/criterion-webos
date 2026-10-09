# Native Platform

`crates/criterion-platform` owns the SDL2 window, GLES context, input, text input and foreground lifecycle. Rendering and licensed playback have separate lifetimes. The platform performs no content fetching, decoding or account storage.

## Target and ABI

The crate's `sdl` feature links SDL2 for the pinned Linux host environment. Its `webos` feature includes `sdl` and `webos-runtime` and explicitly selects the ARM32 little-endian LG fork. `webos-runtime` admits the native auxiliary-vector implementation without SDL, as required by the packaged pipe broker. Architecture alone cannot select an SDL library ABI. The default build contains pure event and geometry behavior and needs no SDL or GPU.

Stock SDL2 and the LG fork differ in keyboard and inline text layout. The adapter preserves raw scancode/keycode values, including private Back/media scancodes whose keycode can be zero. The event buffer is aligned and bounded for one `SDL_PollEvent` call; it is not an array of SDL event unions. [Crate attribution](../crates/criterion-platform/NOTICE.md) owns exact pinned upstream reuse and licensing, and [test provenance](../crates/criterion-platform/tests/README.md) identifies the independent ABI fixtures.

Both host and TV request GLES2 and RGBA8. The backend measures window/drawable and framebuffer properties, rejecting inadequate RGBA channels; panel resolution does not establish allocation size. SDL-reported version/profile attributes can be cached requests, so the renderer must inspect the driver's actual GL_VERSION and GLES support. The host window permits resize and high DPI while the TV window requests fullscreen. Linux host graphics evidence remains separate from C4 behavior.

## Ownership and lifetime

Open the window on the application's main thread, before other SDL users. No external code may initialize SDL video/events concurrently or change the current context during the window's lifetime. The webOS entry point must set `SDL_WEBOS_ACCESS_POLICY_KEYS_BACK=true` before starting threads; the library validates it instead of mutating the process environment. The native entry point warms the platform's auxiliary-vector cache before SDL or worker startup.

A window owns the video/event dependency, GL context and SDL window on its initialization thread. `Window` cannot move or share across threads. Construction rejects existing video/event owners. Failure releases acquired references; disposal stops owned text input, destroys the GL context/window and releases its video reference. It never calls global `SDL_Quit`.

The renderer uses `gl_proc_address`, `surface`, `context_info`, `poll_event`, `text_input` and `present`. GL pointers may be null and must remain on the owning thread within the context lifetime. The renderer must dispose its GL resources before the window. Geometry converts window pointer positions through drawable scale and centered logical viewport; zero-size or collapsed viewports cannot admit drawing/input.

Background notifications block swaps. Will-foreground does not reopen them; did-foreground admits drawing again. Quit/termination closes admission permanently. The application owns resource restoration and focus after reactivation. Text input requires keyboard focus; a void SDL request does not prove the TV keyboard appeared. Inline IME composition is distinct from committed UTF-8 text. Ignored drop strings and desktop extended-edit strings are freed through SDL; the unverified extended-edit ABI is not admitted on the LG branch.

## Evidence boundary

Literal fixtures establish decoder and geometry behavior. Isolated CPU startup tests establish error recovery and preservation of existing SDL event ownership. Host rendering, actual stock-TV event delivery, IME activation/reopening, lifecycle restoration and physical remote behavior require their designated executors. A successful SDL swap has no compositor completion acknowledgment.

## Auxiliary vector

The current ARM32 little-endian `webos-runtime` target supplies an original GPL Rust `getauxval(unsigned long)` implementation. It reads only fixed `/proc/self/auxv` through Linux kernel syscalls and retains one immutable bounded result, including a failed read. It allocates no heap storage and delegates no symbol. The safe `auxv::warm_auxiliary_vector()` interface warms this same cache before SDL or worker startup in the application, native caller and broker. The broker retains its unsafe-code prohibition and adds no SDL dependency. The SDK compatibility archive is excluded from linking; `libdl` remains required by Rust thread creation. [Development tooling](development.md) owns the complete cross-link and ELF/device gate.

The [GNU/Linux function contract](https://man7.org/linux/man-pages/man3/getauxval.3.html) returns unsigned values, including a present zero. Missing or unavailable entries return zero with thread-local `ENOENT`; successful lookup preserves the incoming errno. The [proc format](https://man7.org/linux/man-pages/man5/proc_pid_auxv.5.html) uses native unsigned-long pairs ending in two zeros. This adapter admits only the current target's little-endian 32-bit encoding. Its conservative policy rejects duplicate types, nonzero `AT_NULL` values, missing/truncated terminators, trailing bytes and oversized vectors; duplicate refusal is an adapter policy, not a general ELF prohibition.

`auxv.rs` owns entry and read-attempt limits, including partial reads and bounded interrupted-read retries. The fixed proc descriptor uses close-on-exec/nonblocking flags, is released on every parser path and is never retried after Linux close. Original [ARM EABI syscalls](https://man7.org/linux/man-pages/man2/syscall.2.html) preserve the frame register and stack alignment. Cache initialization consists of the bounded reader and immutable publication, with no libc file calls, application callbacks, logging or allocation. `__errno_location` runs outside initialization and only raw TLS pointers cross cache waiting. This is thread-safe initialization; signal-handler recursion is not an admitted use.

Pure fixtures exercise the same parser/cache interface without opening proc or replacing host libc. They cover zero/missing/unsigned results, partial/interrupted reads, malformed/oversized data and concurrent immutable publication. ARM object compilation and disassembly establish source/ABI acceptance; root must verify the complete application's linked symbol ownership, archive exclusion and actual native behavior separately.

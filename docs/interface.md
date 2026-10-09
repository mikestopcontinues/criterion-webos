# Native interface

`criterion-ui` owns a pure remote navigation model, bounded egui scene painting and a main-thread GLES painter. The application supplies provider-admitted metadata and decoded public artwork, executes typed commands, and rejects stale asynchronous publication before updating the UI. Platform, account, provider and playback operations remain outside this crate.

## Application seam

`AppUi::handle(Action, &ViewData)` transforms one remote action and returns `Command` values. Card activation resolves the current coordinates to a validated provider `MediaId` before yielding. Navigation and detail history retain focus and scroll; filter draft changes are separate from committed selections. The application maps filter indices to the admitted provider options. `ViewData` borrows the current hero, rails, grid, optional detail and filter menu; it does not own account data or URLs.

`AppUi::render(RawInput, &ViewData)` returns an egui `FullOutput`, visible card rectangles and public artwork keys. The scene uses a logical 1920×1080 canvas. [The reference application](reference-app.md) owns observed behavior and evidence; [view.rs](../crates/criterion-ui/src/view.rs) owns layout values. CPU geometry checks cover the observed four-column landscape All Films grid. The current core includes Home, New, the grid, filter draft/Apply/Reset, detail actions, supplement tabs and an information modal. Search keyboard/group interaction and pointer/text consumers are an unfinished layer; current painting does not establish complete official-app parity.

Media identity and artwork identity are distinct. A card has a typed media key and an optional artwork key; the hero independently accepts background and title-logo keys. `admit_image` accepts bounded decoded pixels after the runtime validates origin, decode limits and publication generation. Admission queues no upload. A frame uploads only surviving cache entries, so bursts cannot retain evicted images in egui's pending texture delta. The caller consumes each frame's texture delta before producing the next frame. Cache and admission bounds are enforced in [images.rs](../crates/criterion-ui/src/images.rs).

Visible cards alone are painted; offscreen rails are skipped. Labels and buttons bound their display text before shaping, and synopsis pages bound both characters and rows. The full admitted synopsis remains available through bounded information pages. Metadata labels elide to their visible line. The default egui fonts do not establish complete CJK or other international glyph coverage; typography and image crop parity require rendered reference comparison.

## GLES lifetime

`GlowRenderer::new` reads the current context's actual `GL_VERSION`. GLES 2 additionally requires `GL_OES_element_index_uint`, because the pinned maintained painter uses unsigned 32-bit mesh indices. ES 100 shaders are selected explicitly. [The vendored painter notice](../crates/criterion-ui/vendor/egui_glow/NOTICE.md) describes narrow cleanup fixes and upstream provenance; the crate brings no window system or bundled SDL.

The caller keeps the context current on the calling thread and disposes the painter before dropping the platform window. `paint(drawable_size, context, &mut output)` validates the surface and painter before consuming shapes or texture deltas, preserving a frame on admission errors. Rendering uniformly fits the logical canvas to the drawable size with centered black letterboxing, matching the platform's inverse pointer transform. Painter destruction is idempotent and releases shaders/programs, buffers, textures and any allocated VAO; failed initialization releases already-created objects.

Pure navigation and host egui frame tests establish CPU behavior and geometry only. Actual GLES initialization, extension availability, resource lifetime, color, artwork, resizing and performance require the application's serialized native checks. The TV and physical remote remain separate verification boundaries described in [development.md](development.md).

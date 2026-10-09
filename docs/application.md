# Application presentation

`criterion-app` separates loaded display data from asynchronous publication, native input and graphics. [The presentation module](../crates/criterion-app/src/presentation.rs) consumes provider-admitted discovery, catalog, Search and detail records. The controller owns requests, cancellation, navigation history and stale-result rejection; a projection does not fetch data or infer subscriber access.

The projection prepares display strings and artwork identities when data is loaded. `with_view` lends a `ViewData` for one callback, constructing temporary arrays of borrowed cards, rails and filter labels. It does not join metadata or clone display strings on each frame. Search retains each projected card once and changes the selected index set when the result group changes. Detail metadata is joined once, and playlist rows preserve the supplied order. Filter labels remain owned after the provider options leave scope; the controller must separately retain their typed values for applying selections.

## Identity and artwork

Catalog, Search and detail cards use the admitted `MediaId` directly. Discovery cards and navigation items retain their exact `ContentTarget`, including nonmedia Discover and reserved routes. The projection never constructs a slug from a title, adds an invented media identifier or manufactures a hero from a film row. [The native interface](interface.md) owns activation and painting behavior.

Artwork bindings pair a bounded opaque key with either an exact media identifier and image label or a validated editorial image. Keys are SHA-256 values over a versioned, type-tagged source identity. Labels and editorial URLs remain distinguishable even when a media target repeats; identical sources share one binding. The renderer receives only the key. The artwork owner validates and loads the binding's source, then owns decoded-pixel admission and stale publication.

Desktop editorial selection uses the supplied width candidates for the logical canvas: the smallest adequate width, or the largest available desktop candidate. It does not substitute a mobile source, a different image family or fabricated artwork. A logo-only slide keeps its text title empty and retains the supplied logo. Discovery hero activation opens its typed target; a display label does not establish a playable asset or entitlement.

## Represented data and current gaps

Discovery preserves the interleaved order of media rails and navigation rows. The current UI accepts one hero and ordered rails. The projection selects only the first slide of the first slideshow and reports additional slides, banners, row CTAs, gallery presentation settings, account-fed rows and unsupported new-window targets through structured gap counts. Unrepresented artwork is not loaded. Missing hero target, CTA or desktop artwork remains explicit; it does not produce a replacement hero. Account-fed rows do not establish account synchronization.

Search group counts come from the provider's admitted counts. All includes every returned kind; Films, Collections and Supplements select their exact kinds while preserving order. The displayed total is the selected group's provider count rather than the number of returned cards. An empty selected group yields the empty state.

Detail uses the supplied media identity and playlist items without inventing routes. Collection, category and series metadata use the collection interaction surface; a supplement retains its supplemental surface. Live uses the existing play surface while retaining its typed public schedule separately, with a count recording the unrepresented schedule rows. A missing schedule media identifier stays absent. UTC timestamps do not establish the currently airing program, local timezone, progress or licensed playback. Absent runtime remains explicit for the renderer to omit.

## Memory and verification

`estimated_bytes` supports the controller's navigation-cache eviction policy. It sums retained container capacities, owned display-string capacities and the exposed lengths of opaque provider strings. Private provider allocation capacity, allocator overhead and temporary callback arrays are outside this estimate, so it is not a measurement or ceiling for process RSS.

Fixture-backed CPU tests exercise the projection's borrowed view, exact identities, Search groups, playlist order, artwork source keys, partial Discovery mapping and retained-memory estimate. They do not establish native rendering, reference-app visual parity, physical remote behavior or licensed playback. [The reference observations](reference-app.md) and [development procedures](development.md) own those separate acceptance checks.

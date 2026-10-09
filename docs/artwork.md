# Public artwork

`criterion-artwork` owns admitted public image requests and bounded CPU decoding. The application owns request selection, generations and cancellation; [the native interface](interface.md) owns cache admission and main-thread texture uploads. Catalog metadata and asset-family provenance belong to [the provider contract](provider-contract.md). A missing or rejected image remains missing; this module supplies no invented replacement artwork.

## Application interface

Construct an `ArtworkSource` from provider-admitted `MediaId` and `ImageLabel` values or an `EditorialImage`. There is no raw-URL loading interface. [source.rs](../crates/criterion-artwork/src/source.rs) rechecks the complete fixed HTTPS origin, path family and query before a request. It accepts only the provider's public JW media labels and editorial thumbnail paths. Credentials, arbitrary paths, alternate ports, fragments and additional query parameters are rejected.

Use one `ArtworkLoader` per application runtime and poll `load` on Tokio. Excess requests return `Busy` immediately rather than accumulating a wait queue. Each accepted request carries its network admission through completion. Decoding moves to a blocking worker whose owned permit remains in the worker if the caller drops its future. The application can cancel network work and reject an obsolete generation, but an issued CPU decode must settle; cancellation does not interrupt the codec.

The loader publishes no UI state, focus or feedback. Before calling `AppUi::admit_image`, the application checks that the request still belongs to the active generation, then converts `DecodedArtwork::dimensions()` and `rgba()` with egui's unmultiplied-RGBA constructor. Request scheduling and cache keys stay in the application. The pure synchronous `decode_artwork` interface supports offline validation; its caller owns thread placement and concurrency.

## Admission and allocation

[loader.rs](../crates/criterion-artwork/src/loader.rs) owns request and worker admission, connection/body deadlines, HTTP header bounds and transport behavior. It uses verified static roots and the provider's explicit Rust TLS backend. Redirects, proxies, retries, referrers and automatic content decoding are disabled. Only identity content coding is admitted. The body cap is enforced against declared lengths and every received chunk, before decoding.

[decode.rs](../crates/criterion-artwork/src/decode.rs) owns encoded size, dimensions, pixel count, native output bytes and renderer admission limits. MIME and signature must agree on PNG, JPEG or WebP. Only supported eight-bit decoded colors are admitted; no higher-depth or float image expands into a larger working buffer. Pixel dimensions and native output bytes are checked before the full image allocation. Large images are reduced with the maintained image library's aspect-preserving thumbnail filter. Varying alpha is premultiplied before filtering and unpremultiplied afterward, with a transparent-pixel guard; uniform alpha avoids the extra quantization.

PNG and JPEG require complete file terminators. [webp.rs](../crates/criterion-artwork/src/webp.rs) bounds RIFF chunk framing and verifies both canvas and nested VP8/VP8L dimensions before decoding. It rejects animation, duplicate image frames, inconsistent dimensions, malformed lengths/padding and excess chunks. This preflight is necessary because the maintained WebP decoder can otherwise check a nested frame's dimensions only after allocating and decoding it.

The pinned [image `Limits` contract](https://docs.rs/image/0.25.10/image/struct.Limits.html) makes dimensions strict and allocation accounting best effort. PNG receives its allocation budget at construction; JPEG and WebP do not enforce a complete working-heap budget. Their internal codec allocations are not included in an application-wide memory guarantee. Input, frame dimensions, application-owned image buffers, worker concurrency and final RGBA are bounded separately. The configured allocation value is not a claim that the process or codec working heap stays below that value. [Cargo.toml](../crates/criterion-artwork/Cargo.toml) pins the maintained pure Rust codecs without optional native codecs or the image library's default feature set.

The [vendored WebP correction](../crates/criterion-artwork/vendor/image-webp/PROVENANCE.md) widens canonical Huffman validation while retaining the released decoder's code and table representation. Invalid maximum-depth trees fail before table construction. The source archive checksum and unchanged upstream licenses are preserved beside the patch; the root Cargo manifest must select that patch.

The filter operates in the library's channel space. ICC profiles, EXIF orientation and color-management metadata are not applied; this does not establish colorimetric or orientation parity with the official app. Provider URL/identifier payloads, encoded pixels and decoder details remain absent from Debug output and scalar errors.

## Verification scope

[Decode tests](../crates/criterion-artwork/tests/decode.rs) exercise actual PNG/JPEG/WebP bytes, malformed containers and headers, size/color limits, downsampling and transparent-logo edges. [Loader tests](../crates/criterion-artwork/src/loader/tests.rs) use fresh controlled local TLS certificates and real HTTP responses to exercise chain/hostname verification, redirects, MIME/coding, streaming caps, deadlines and admission/cancellation. Their certificate/origin substitution is private to test builds and retains HTTPS verification.

The [opt-in public smoke example](../crates/criterion-artwork/examples/public_artwork.rs) decodes one previously admitted public image and reports only dimensions and RGBA byte count. It does not archive provider pixels. Fixture tests and a live host decode do not prove native TLS, GPU artwork, rendering performance, device behavior, account access or subscriber playback. Those remain the application's distinct acceptance checks.

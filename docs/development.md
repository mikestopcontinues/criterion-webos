# Development Toolchain

Edit source and manage Git on the host. `./dev image` builds the pinned Rust/SDL/Mesa development image; `./dev cargo <args>` executes Cargo inside it. `./dev run <command> <args>` supplies the same environment for focused tools. Each checkout has a separate target volume; Cargo's download cache is shared and lock-protected. Keep private sessions outside the repository and its mounted tree. The runner passes no TV or account credentials.

The pinned nightly Cargo validates Rust source freshness with content checksums, preserving incremental compilation when source timestamps are unchanged. Build-script `rerun-if-changed` inputs retain Cargo's timestamp rules; relevant generated/native inputs require their own dependency checks.

[Dockerfile](../Dockerfile) owns the pinned Rust image, nightly compiler, development libraries and ARM64-native NDK artifact identity. [rust-toolchain.toml](../rust-toolchain.toml) owns the Rust channel. `./dev native-image` builds the relocated SDK image on an ARM64 host; `./dev native cargo <args>` uses that image. ARM compilation requires `--target arm-unknown-linux-gnueabi -Z build-std=std,panic_abort` to rebuild the standard library against the TV's baseline. [.cargo/config.toml](../.cargo/config.toml) owns target-only compiler and link flags; host checks use the development image.

The base-image references use [Docker's official ECR Public publisher](https://www.docker.com/press-release/docker-official-images-available-amazon-elastic-container-registry/) with OCI index digest pins. This avoids Docker Hub's anonymous pull quota; ECR Public has its own [one-pull-per-second anonymous quota](https://docs.aws.amazon.com/general/latest/gr/ecr-public.html). Registry reachability and blob admission remain required for a successful image build.

The pinned SDK's wrapper contains an invalid formatted sysroot argument. Native commands use its underlying `gcc.br_real` driver with an explicit sysroot, retaining the driver's Cortex-A9/soft-float defaults. The original Rust [auxiliary-vector owner](native-platform.md#auxiliary-vector) replaces the SDK compatibility archive; its redistribution grant was unavailable. `libdl` remains required by Rust thread initialization. An SDK C link verifies ARM32 EABI5, the stock SDL/GLES dependencies and baseline glibc symbols. A Rust cross-link, final ELF inspection and exact-device execution remain required before admitting any native package. The SDK library's host link metadata does not establish the LG runtime event ABI.

The application uses the stock TV SDL/graphics stack; host SDL/Mesa checks do not prove LG event delivery, DRM, physical remote feel or native GPU performance. [The product contract](product.md) owns the separate acceptance criteria. Development networking is enabled for dependency preparation and live public-provider experiments; locked verification must explicitly separate offline checks from those live checks.

## Application checks

[Native packaging](native-package.md) describes sealed development artifacts; [the native caller](native-caller.md) remains a separate disposable device-admission executable. The complete native development executable uses `criterion-app`'s `sdl` host feature or `webos` ARM feature. Run the workspace behavior, formatting and strict Clippy commands in the README before source admission. CI also cross-links the complete application and the packaged lifetime broker. A successful SDK build is not a device execution result.

Run the SDL/GLES E2E separately with one display owner:

```sh
./dev run xvfb-run -a -s '-screen 0 1920x1080x24' \
  env SDL_VIDEO_X11_FORCE_EGL=1 cargo test -p criterion-app \
  --bin criterion-unofficial --features sdl --locked \
  native_window_input_and_search_frame_present_end_to_end \
  -- --ignored --nocapture --test-threads=1
```

The runner's init process lets Xvfb receive its readiness signal and reaps children. The host test explicitly uses SDL's EGL path; the window still requires synchronized presentation. Its ignored `.local/e2e/native-search.ppm` capture contains synthetic Search input and no subscriber state. Inspect the settled framebuffer separately from the state assertions. Serialize GPU checks across agents; no host display fixture establishes LG compositor behavior.

The separately ignored `native_public_catalog_search_artwork_and_detail_roundtrip_end_to_end` uses the production anonymous catalog/artwork transports. Run it with the same display command and that exact test name. It drives SDL All Films→detail→Back and native text entry→Search→Films→detail→Back. It checks immediate admitted group counts and restored query/group/card focus, waits for current owning artwork, paints/reads/presents GLES and saves ignored public-detail and public-search captures. It invokes no account/linking/player action. This opt-in live check requires current network/provider availability; Cargo's `--offline` only controls dependency resolution. CI uses the synthetic Search E2E, keeping provider availability outside the reproducible gate.

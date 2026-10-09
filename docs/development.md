# Development Toolchain

Edit source and manage Git on the host. `./dev image` builds the pinned Rust/SDL/Mesa development image; `./dev cargo <args>` executes Cargo inside it. `./dev run <command> <args>` supplies the same environment for focused tools. Each checkout has a separate target volume; Cargo's download cache is shared and lock-protected. Local commands do not receive TV or account credentials.

[Dockerfile](../Dockerfile) owns the pinned Rust image, nightly compiler, development libraries and ARM64-native NDK artifact identity. [rust-toolchain.toml](../rust-toolchain.toml) owns the Rust channel. The native stage contains the verified ARM32 webOS sysroot; relocation, cross-link flags, rebuilt standard library and ELF admission must be implemented and checked before a native package is usable.

The application uses the stock TV SDL/graphics stack; host SDL/Mesa checks do not prove LG event delivery, DRM, physical remote feel or native GPU performance. [The product contract](product.md) owns the separate acceptance criteria. Development networking is enabled for dependency preparation and live public-provider experiments; locked verification must explicitly separate offline checks from those live checks.

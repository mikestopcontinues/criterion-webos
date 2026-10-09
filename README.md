# Criterion Unofficial

Criterion Unofficial is a Rust application being developed for LG webOS TVs. The product target is the full Criterion Channel TV experience: discovery, collections, search, film details and supplements, My List, Continue Watching, licensed playback, captions, seeking and account synchronization.

The development implementation includes public discovery, catalog/filter/Search/detail adapters, a native remote interface, bounded artwork and a read-only My List flow. Local behavior, host SDL/GLES rendering, [live host device linking/logout](docs/session.md) and [the initial production My List read](docs/account.md) are verified. Complete account synchronization, secure persistence, licensed C4 playback and a verified release remain unfinished. The project is independent of Criterion, Janus Films, JWX and LG; it requires a current Criterion subscription and does not grant access to content.

Read [the product contract](docs/product.md) for acceptance and [TASKS.md](TASKS.md) for current work and the resumable checkpoint. [AGENTS.md](AGENTS.md) owns engineering, security, coordination and evidence rules. Release packages will be published with verified functionality and explicit limitations.

## Development

Use the pinned Docker runner:

```sh
./dev image
./dev cargo test --workspace --all-features --locked
./dev cargo test -p criterion-platform --features sdl --locked
./dev cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
./dev cargo fmt --all -- --check
```

[The development guide](docs/development.md) describes the rendered application E2E, ARM cross compilation and separate native/device verification boundaries. [Native development packaging](docs/native-package.md), [application ownership](docs/application.md) and [the packaged service probe](docs/probe-player.md) describe runtime and playback-admission seams.

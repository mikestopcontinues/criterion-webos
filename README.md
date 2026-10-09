# Criterion Unofficial

Criterion Unofficial is a Rust application being developed for LG webOS TVs. The product target is the full Criterion Channel TV experience: discovery, collections, search, film details and supplements, My List, Continue Watching, licensed playback, captions, seeking and account synchronization.

Implementation and actual subscription playback are not yet verified. The project is independent of Criterion, Janus Films, JWX and LG. It requires the captain's current Criterion account for subscriber validation; it does not grant access to content.

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

[The development guide](docs/development.md) describes ARM cross compilation and the separate native/device verification boundaries.

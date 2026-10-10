# Criterion Unofficial

Criterion Unofficial is a Rust application being developed for LG webOS TVs. The product target is the full Criterion Channel TV experience: discovery, collections, search, film details and supplements, My List, Continue Watching, licensed playback, captions, seeking and account synchronization.

The development implementation includes public discovery with manual hero traversal and retained Back selection, catalog/filter/Search/detail adapters, native middleware details with Featured cards and local first-tab sorting, distinct root/card runtime labels, ephemeral Detail membership, Series saved-progress selection and typed Play actions with unavailable feedback, a native remote interface, bounded artwork, grouped My List and supplied Continue Watching composition. Synthetic CPU and host SDL/GLES checks verify navigation and private-state retirement; a separate live public journey verifies catalog, Search, Detail and Back with current metadata and focused artwork.

Explicit [My List Add/Remove](docs/list-writes.md) connects native Detail to a durable issued-write fence with synthetic CPU coverage. Its deployment initializer, actual provider writes and C4 usability remain unverified.

[Live host device linking/logout](docs/session.md), initial My List and one filtered group, current-root Detail membership with exact Back/logout, supplied Home Continue Watching, one composed anonymous native Film/Information/Back journey, and one host native Film playback-configuration read have separate [provider admission](docs/account.md). Complete account synchronization, secure persistence, licensed C4 playback and a verified release remain unfinished. The project is independent of Criterion, Janus Films, JWX and LG; it requires a current Criterion subscription and does not grant access to content.

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

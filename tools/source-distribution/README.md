# Corresponding-source candidate tooling

This workflow prepares a deterministic development source candidate for the native application and both disposable probes. [The licensing owner](../../docs/licenses.md#corresponding-source-and-release-completion) owns the release requirements; a successful tool run establishes input structure, hashes and archive identity. The release owner must match the final executable/IPK receipts and assess every source exclusion before publishing.

The source candidate contains the complete committed public Git tree, including project logs, local/modified vendors, embedded resources, interface definitions, build/install scripts and license/notice texts. Git commit/tree/blob hashes establish the complete revision. Dirty or changed HEAD, omitted files, symlinks, submodules, private/output path components and APK/IPK files refuse the entire export. Public synthetic test literals remain source; content heuristics are not a security boundary. Do not commit credentials, account/device material or private receipts anywhere in the public tree.

The archive also includes every registry `.crate` in the project lock and in the separately pinned Rust runtime lock. This deliberately over-includes platform and test dependencies; it is not an incorporated-code graph. Only Cargo's generated version-4 lock subset is accepted, with exact package framing, unquoted name/version/source/checksum keys with double-quoted scalar values, and the generated dependencies string array. Unsupported syntax/source registries, unresolved inputs and conflicting checksums refuse admission. The full official `rust-src` gzip component preserves compiler-builtins, stdarch, portable-SIMD, LLVM unwinder and their source/license/resources. Its raw channel manifest, runtime lock and complete `COPYRIGHT-library.html` are included. Neither crate scripts nor Rust installers run here.

## Execution and fixed inputs

Host Git owns capture. The existing `criterion-player-probe-tools:20261009` Docker image owns strict TypeScript compilation, Node 16 checks, source inspection and archive construction. Use the repository's pinned installed `tools/player-probe/node_modules`; no extra dependency or image is needed. Host Git is resolved from `/opt/homebrew/bin:/usr/bin:/bin` with global/system config, hooks, fsmonitor, replacement objects and optional locks disabled. Run the host preparation with the host's actual Node runtime after compiling the TypeScript.

Prepare only these ordinary, single-link public inputs under `.local/source-distribution/input/`:

| Input | Role |
| --- | --- |
| `source-request.json` | Exact clean source commit, runtime input hashes and individual owner-declared exclusions |
| `rust-channel.toml` | Pinned installed toolchain's `lib/rustlib/multirust-channel-manifest.toml` |
| `rust-src-nightly.tar.gz` | Complete dated official source component selected by that channel manifest |
| `COPYRIGHT-library.html` | Pinned toolchain's `share/doc/rust/COPYRIGHT-library.html`, preserving full runtime notices/licenses |
| `registry/<name>-<version>.crate` | Every exact source archive printed by `requirements.ts`, each checked against its lock checksum |

The request is strict JSON with exactly `schemaVersion: 1`, `sourceCommit`, `runtime` and `exclusions`. Runtime has exactly `channelManifestSha256`, `sourceArchiveSha256` and `copyrightSha256` (lowercase SHA256). Source commit is the full lowercase Git SHA1. Each exclusion has exactly `component`, `basis` and a public ASCII `reason` of 20–512 characters. All fixed components must appear once:

- `stock-sdl2`, `stock-egl`, `stock-gles2`, `stock-lunaservice2`, `stock-glib2`, `stock-libc`, `stock-libm`, `stock-libpthread`, `stock-librt`, `stock-libdl`, `stock-libgcc-s`: `basis: "system-library"`.
- `glibc-startup-nonshared`, `gcc-crtstuff`: `basis: "linked-file-permission"`.

These are explicit release-owner declarations, not legal eligibility inferred by the tool. They cover this conservative three-artifact matrix; consult the final maps, [GNU file grants](../../NOTICES.md#gnu-startup-and-nonshared-code), actual dynamic interfaces and [SDK provenance limits](../../docs/licenses.md#sdk-source-provenance-and-its-limits). A blanket `SDK` exclusion is rejected. The manifest preserves those limits and the pinned native-toolchain/Buildroot/glibc/GCC source IDs. It does not assert reproduction of the imported SDK's unavailable generated configuration or unknown builder deltas.

The requested native matrix is fixed: `criterion-app`, `criterion-broker-probe` and `criterion-native-caller-probe`, each with its `webos` feature, ARM `arm-unknown-linux-gnueabi`, release, `-Z build-std=std,panic_abort`, and `-Z build-std-features=compiler-builtins-mem`. The complete project's committed build configuration remains in the source. Final artifact feature/source receipts must match independently.

Compile and check from the actual worktree, using its canonical installed dependencies:

```sh
docker run --rm --init --network none \
  --mount type=bind,src="$PWD",dst=/workspace \
  --workdir /workspace criterion-player-probe-tools:20261009 \
  sh -c 'node tools/player-probe/node_modules/typescript/bin/tsc -p tools/source-distribution/tsconfig.json --outDir /tmp/source-distribution-checked && /opt/node16/bin/node /tmp/source-distribution-checked/tools/source-distribution/tests/run.js && node tools/player-probe/node_modules/typescript/bin/tsc -p tools/source-distribution/tsconfig.json'
```

Capture clean host Git after writing the ignored public request and compiling the tool:

```sh
node .local/source-distribution/compiled/tools/source-distribution/prepare.js
```

This writes an exclusive sealed `prepared/<sourceCommit>/project.json` from committed Git blobs. It does not export the checkout or its ignored files. List exact registry inputs before dependency preparation:

```sh
docker run --rm --init --network none \
  --mount type=bind,src="$PWD",dst=/workspace \
  --workdir /workspace criterion-player-probe-tools:20261009 \
  /opt/node16/bin/node .local/source-distribution/compiled/tools/source-distribution/requirements.js
```

Copy matching `.crate` archives from the canonical Cargo cache or download the printed public `https://static.crates.io/` URLs with checksum verification. Acquire the dated runtime component from `https://static.rust-lang.org/dist/<date>/rust-src-nightly.tar.gz`; its archive checksum must match both the pinned channel and the request. Input acquisition is separate from offline admission. Do not copy a cache/index directory, arbitrary SDK/compiler binaries or private build receipts into the source inputs.

Construct the offline candidate:

```sh
docker run --rm --init --network none \
  --mount type=bind,src="$PWD",dst=/workspace \
  --workdir /workspace criterion-player-probe-tools:20261009 \
  /opt/node16/bin/node .local/source-distribution/compiled/tools/source-distribution/build.js
```

## Candidate admission and release handoff

Output is exclusive `output/<sourceCommit>/criterion-source-<sourceCommit>.tar.gz` plus `seal.json`. The source contains `SOURCE-MANIFEST.json`: exact file hashes, sizes/modes, revision, requested build matrix, runtime identities, SDK pins and the individual declarations. The external seal binds archive bytes and manifest bytes. It contains no host paths, timestamps, private request payloads or execution receipts. A missing seal or any failed command is an unadmitted output. Existing prepared/candidate files are never replaced; preserve them or remove only your own obsolete ignored outputs before an intentional repeat.

The writer uses fixed USTAR/gzip, owner/group 0, epoch timestamp, sorted paths, directories 0755 and source files 0644/0755. Independent GNU tar listing and full content comparison check every logical member; byte comparison with canonical reconstruction rejects extended/hidden metadata and alternative archive representations. The tool never extracts supplied archives to disk. Traversal, link/special entries and duplicates in the runtime source archive are refused. Raw checksum-verified registry archives remain intact data.

Bounds are explicit: 4,096 project files, 128 MiB project content, 32 MiB each source input/file, 16,384 runtime members and 128 MiB runtime inflation, 1,024 package stanzas per lock, 8,192 final files, 256 MiB aggregate source content and 320 MiB final archive/inflation. The channel and each Cargo lock are at most 2 MiB; runtime copyright is at most 8 MiB; prepared metadata is at most 192 MiB. Each native Git/tar child has a 60-second bound. These bounds include the practical current full-lock union; build failure does not establish release readiness.

The recipient has the preferred form for modification and raw locked source archives, plus the pinned source component and project build/install scripts. Standard Cargo dependency preparation may unpack the `.crate` archives; this tool does not bundle a private Cargo cache or promise an offline reconstruction of the imported SDK. The release owner must check the actual candidate against final app/caller/broker source/feature/link receipts, resolve any required generated inputs, preserve applicable rights and put an exact matching source link beside each GPL IPK at no further charge. This workflow does not publish, discharge global corresponding-source obligations or establish C4/runtime/provider/playback acceptance.

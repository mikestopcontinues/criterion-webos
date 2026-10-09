# Native development package

`tools/package-native` packages the root-verified Rust executable as `com.mikestopcontinues.criterion.unofficial`, titled **Criterion Unofficial**, version `0.1.0`. The descriptor marks a development build. It declares a native app, fixed `criterion-unofficial` main, lifecycle interface V2 and no relaunch handling or unproved service permissions. [LG's application descriptor reference](https://webostv.developer.lge.com/develop/references/appinfo-json) owns the standard fields; the native lifecycle convention follows the source-verified Plx manifest and [native application guidance](https://www.webosose.org/docs/tutorials/native-apps/developing-built-in-native-apps/). Actual C4 lifecycle behavior remains device admission.

## Input and source admission

Root stages exactly `.local/native-package/input/criterion-unofficial` and `build-receipt.json`. The packager takes no command-line arguments, never compiles or executes that binary, and performs no network or device operation. [Development tooling](development.md) owns the SDK, target, link flags and compiler. Root verifies a clean source commit, freezes the complete release compile-input hash map before and after the SDK build, and inspects dynamic libraries and required symbol versions.

The JSON receipt has exactly these fields:

```json
{
  "schemaVersion": 1,
  "appId": "com.mikestopcontinues.criterion.unofficial",
  "version": "0.1.0",
  "target": "arm-unknown-linux-gnueabi",
  "profile": "release",
  "sourceCommit": "40 lowercase hexadecimal characters",
  "cargoLockSha256": "64 lowercase hexadecimal characters",
  "executableSha256": "64 lowercase hexadecimal characters",
  "sourceSha256": { "repository/relative/input": "64 lowercase hexadecimal characters" }
}
```

`sourceCommit` is the clean compile-source commit attested by root. `sourceSha256` admits bounded repository-relative crate Rust/C/header/shader/TOML paths and fixed root configuration inputs. The minimum required map includes root `Cargo.toml`, `Dockerfile`, `rust-toolchain.toml`, `.cargo/config.toml`, and the app's manifest and entry point. Root supplies the conservative complete compile-input map, including vendored source and build scripts. The packager verifies every admitted source hash, the locked dependencies and executable bytes against the current checkout before staging and after packaging. This checks the attestation's known inputs; it does not independently establish compiler provenance or reproducibility.

The executable limit is 32MiB. Admission requires ELF32 little-endian ARM EABI5 with explicit soft-float ABI, an executable load segment, exactly one `/lib/ld-linux.so.3` interpreter and exactly one read/write, nonexecutable GNU stack. Program-header counts, spans and segment file bounds are checked. Files are bounded regular inputs without followed symlinks. The receipt limit is 256KiB, with at most 1,024 source paths. Metadata admission cannot prove SDK library compatibility or successful TV execution.

## Archive construction

The tool reuses [the player probe's pinned Docker image and dependency lock](probe-player.md#local-build-and-verification). It adds host-only [Debian binutils `2.40-2`](https://packages.debian.org/bookworm/binutils) for GNU ar and uses the base image's GNU tar. Compiler declarations and CPU tests use Node 16; Node 24 runs the pinned official CLI `3.2.6` with an explicit TV profile and isolated, build-local home directory. The CLI dependency-audit limitation remains owned by [the probe tooling topic](probe-player.md#local-build-and-verification). No tooling dependencies enter the application.

The app payload contains only the fixed executable, descriptor, original geometric icon, GPL license, exact [full notice compendium](../NOTICES.md) and retained platform, egui_glow and image-webp attribution/license texts. The compendium is a bounded regular input under the existing 1MiB per-asset limit; its exact bytes and `0644` mode enter the fixed payload inventory and seal. The CLI adds one fixed package descriptor. It packages no service, WAM SDK, JavaScript, source, account data or credentials.

The pinned CLI creates writable generated metadata and directories. Both native and probe packagers preserve its original unsigned artifact, admit only its known generated modes, and compare every regular member with build-owned buffers. They reconstruct a fresh tree from those buffers without extracting archive paths to disk. Fixed GNU tar and ar commands produce an unsigned USTAR archive with root numeric ownership, fixed timestamps, directories `0755`, nonexecutables `0644` and the binary `0755`. Final admission requires those exact modes.

The archive limit is 34MiB, including bounded decompression; control expansion is limited to 16KiB. Exactly three ordered ar members are allowed: `debian-binary`, `control.tar.gz`, `data.tar.gz`. Control has the exact ten fields emitted by the pinned CLI, fixed ID/version/ARM architecture and bounded positive installed size. The CLI reports filesystem bytes including directories in that field. Installer hooks, extra or duplicate members, changed bytes, links, special files, unexpected directories, traversal, corrupted gzip and content hidden after tar end markers are rejected. Normal PAX metadata is inspected by GNU tar before reconstruction. Subprocesses use fixed executables/arguments, `shell:false`, bounded output/deadlines and an explicit environment that excludes inherited tar options; member reads go only to stdout.

The package carries the project's license, copied-code notices and the current full notice compendium. [The licensing inventory](licenses.md) owns its scope and unresolved release obligations; the exact compiled artifact, complete runtime grants and corresponding source distribution remain root's publication gate.

## Canonical local commands

Run from the actual checkout after root supplies its verified input and matching source revision:

```sh
docker build -t criterion-player-probe-tools:20261009 tools/player-probe
docker run --rm --mount type=bind,src="$PWD",dst=/workspace \
  --workdir /workspace/tools/player-probe criterion-player-probe-tools:20261009 \
  npm ci --ignore-scripts --no-audit --no-fund
docker run --rm --network none --mount type=bind,src="$PWD",dst=/workspace \
  --workdir /workspace criterion-player-probe-tools:20261009 \
  sh -c 'node tools/player-probe/node_modules/typescript/bin/tsc -p tools/package-native/tsconfig.json && /opt/node16/bin/node .local/native-package/compiled/tools/package-native/tests/run.js && node .local/native-package/compiled/tools/package-native/build.js'
```

The shared probe regression/package gate uses its existing canonical `npm run compile && npm test && npm run package` Docker command. Tests exercise public admission behavior and actual CPU GNU tar/ar subprocesses using explicitly identified metadata fixtures. They never run a fixture executable. Root separately checks the exact SDK-produced app artifact and the resulting package.

Ignored output lives under `.local/native-package`: original CLI archive/log, staging, normalization tree, `ipks/com.mikestopcontinues.criterion.unofficial_0.1.0_arm.ipk`, raw check receipts and `package-seal.json`. The seal records the build attestation, source/tooling hashes, actual tool versions, original and normalized IPK hashes, exact payload byte hashes and modes. It identifies a specific development artifact; installation, native rendering, physical input, account behavior and licensed playback remain separate root-owned gates.

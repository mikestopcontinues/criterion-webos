# Native application caller admission

`tools/native-caller-probe` is a disposable Rust executable, separate from production UI, account and playback owners. Its fixed package ID is `com.mikestopcontinues.criterion.probe.native`; LS2 registers exactly that app ID with the service name `<app ID>.caller`. Root must verify the exact installed package, named registration, actual bus sender and service rights on the C4. Host SSH Luna calls and successful SDK linking cannot establish application admission.

The [packaged lifetime probe](probe-player.md) owns its fixed service, protocol and caller allowlist. The native caller has no command-line parameters, external paths, proxy identity, anonymous registration or alternate sender policy. A registration rejection ends the probe. PlxNative's pinned [Keymanager adapter](https://github.com/GLinnik21/plx-native/blob/acca94a449a5f7db2c662d6501bd88199ce66d4e/rust-modules/platform/src/keymanager.rs) uses anonymous registration after a named-form rejection on its development set; that source does not establish this C4's rights.

## Lifetime and admission

The native entry owns the stock SDL foreground window and a private GLib context on its main thread. It warms the SDK auxiliary cache before SDL, requests the existing platform Back policy and checks actual GL version for GLES2. Rendering is a fixed background with no provider/account material. [Native platform](native-platform.md) owns the SDL seam.

Requests are fixed `attach`, `ping`, `close`, then one read-only Keymanager call. Each method is issued at most once. Four stable callback records bound storage; at most three bus calls can remain outstanding after a ping timeout. Responses are admitted only with the exact service name, matching nonzero response token, bounded 4KiB JSON object and exact protocol schema. Hub errors, unknown fields, duplicate fields, arrays, extra JSON and out-of-range counters/PIDs/subscriber counts fail admission. Raw fields and library errors are never printed or retained. The maintained [LS2 API](https://www.webosose.org/docs/reference/luna-service2-library/luna-service2-library-api-reference/) supplies the public call/registration contract; LG TV policy remains a device gate.

Liveness requires a running initial attach snapshot and a higher ping counter from the same PID. The initial counter remains the ping baseline while subscription heartbeats arrive. Retirement requires the issued close's exact reply: same PID, counter at least the highest acknowledged attach/ping value, closed state, no subscribers, acknowledged stop, confirmed process/pipe cleanup and zero exit. Every reply must be admitted before its original deadline, including after a tick; exact-deadline replies are refused. A terminal subscription does not complete or reopen the one-shot close owner.

Requests have three-second deadlines and dispatch has a twenty-second absolute bound. Each tick drains at most 64 SDL events and 32 nonblocking GLib iterations. Back, Quit and background stop new work; an issued close keeps pumping to its original deadline. Rejection, malformed replies and cancellation acceptance cannot claim retirement. On disposal, publication stops before cancellation and unregister. Callback records and context are freed only after successful unregister; failure quarantines the fixed records/context until process exit and reports unconfirmed bus cleanup. Native initialization, registration and teardown are synchronous library calls; root must apply an outer device-execution watchdog and establish process retirement separately if a library call stalls or native cleanup fails.

## Keymanager boundary

The only Keymanager request is `exportKey` for fixed synthetic name `criterion.native-caller.absent.v1`. It never creates, changes or removes a key. The documented absent-key reply establishes only read-only call admission; an existing public key is validated without retaining its value and fails the expected absent-name result. Neither proves AES-GCM rights, app-owned encryption, TEE backing, durable credentials or a secure store. LG exposes no separate capability endpoint in its [Keymanager3 reference](https://webostv.developer.lge.com/develop/references/keymanager3); emulator support is absent.

A later, separately authorized device test can collision-check a disposable app-owned key, generate it, encrypt/decrypt synthetic bytes and reject tampering. It must abort every acquired unfinished operation and remove only the exact key whose creation was confirmed. Ambiguous creation or cleanup cannot justify deleting a pre-existing key or persisting account credentials. Production session persistence remains undecided until actual rights and authenticated cleanup are established.

## CPU and SDK checks

Root owns workspace/lock admission and packaging. After admission, use the canonical [development toolchain](development.md):

```sh
./dev cargo test -p criterion-native-caller-probe --locked
./dev cargo clippy -p criterion-native-caller-probe --all-targets --all-features --locked -- -D warnings
./dev native cargo build -p criterion-native-caller-probe --features webos --release --locked \
  --target arm-unknown-linux-gnueabi -Z build-std=std,panic_abort
```

`abi.c` is compile/link-only against real SDK headers and libraries. It asserts ARM32 `LSError` layout, 32-bit tokens, one-byte LS2 C `bool`, four-byte GLib `gboolean` and every used function signature/symbol. Rust repeats the layout assertions. The fixture and native executable must not be run as host verification. CPU fixtures cover protocol and close ownership; SDK compilation establishes source/link acceptance. Actual SDL/LS2 callbacks, foreground behavior, caller identity and Keymanager policy require root's serialized device admission. Generated binaries, ABI fixtures and receipts remain ignored under `.local/native-caller`.

## Disposable package

`tools/package-native/caller-build.ts` packages only the fixed native caller. It reuses the [native package's pinned Docker/CLI/input/ELF and strict archive construction](native-package.md), with no additional dependency manifest. The descriptor has main `criterion-native-caller-probe`, type `native`, version `0.1.0`, lifecycle V2, no relaunch handling and no unproved service permissions. The payload is the root-staged executable, fixed descriptor, original icon, GPL text and exact caller/platform notices; the CLI adds only the fixed package descriptor. It bundles no SDK libraries, JavaScript service, account data or credentials.

Root stages `.local/native-caller-package/input/criterion-native-caller-probe` and `build-receipt.json` after clean source/lock admission, the actual SDK build and ELF/library inspection. `CallerReceipt` has the same nine bounded fields as the production [build attestation](native-package.md#input-and-source-admission), with literal caller identity. Production receipt admission stays fixed to the production app and rejects caller receipts. Caller source admission includes fixed root configuration, crate compile sources and `tools/native-caller-probe` Rust/C/header/TOML/shader inputs. Minimum required caller inputs are its manifest, main/lib, native mod/bus/ffi and `abi.c`; root supplies the conservative complete compile-input map.

The packager verifies source/lock/executable hashes before staging and after packaging, and never executes the ELF. The attestation identifies root's known compiler inputs; it does not independently prove compiler provenance or reproducibility. The shared normalizer admits only the fixed build-local output path, preserves the unsigned CLI original, reconstructs from sealed buffers, and audits exact files/control/bytes with directories and main `0755`, other files `0644`. Executable, receipt, archive/decompression, source counts, subprocess output and deadlines retain the existing strict limits.

Run from the matching checkout with the already prepared pinned tooling and root-staged input; all execution remains in Docker:

```sh
docker run --rm --network none --mount type=bind,src="$PWD",dst=/workspace \
  --workdir /workspace criterion-player-probe-tools:20261009 \
  sh -c 'node tools/player-probe/node_modules/typescript/bin/tsc -p tools/package-native/tsconfig.json && /opt/node16/bin/node .local/native-package/compiled/tools/package-native/tests/run.js && node .local/native-package/compiled/tools/package-native/caller-build.js'
```

The guarded module import lets CPU tests exercise fixed caller identity, receipt/source integrity, metadata and actual GNU tar/ar normalization using explicit nonexecuted ELF metadata fixtures. Successful fixtures cannot establish native caller rights. Ignored output under `.local/native-caller-package` contains original CLI archive/log, staging/normalization tree, raw receipts, `package-seal.json` and `ipks/com.mikestopcontinues.criterion.probe.native_0.1.0_arm.ipk`. The seal records root's build attestation, current tooling/source hashes, exact tool versions, original/final archive hashes and every payload hash/mode. Root must collision-check the disposable app ID and use the exact admitted package through the serialized device queue. Complete compiled-library notices/corresponding source remain the separate publication inventory.

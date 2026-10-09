# Native application caller admission

`tools/native-caller-probe` is a disposable Rust executable, separate from production UI, account and playback owners. Its proposed package ID is `com.mikestopcontinues.criterion.probe.native`; LS2 registers exactly that app ID with the service name `<app ID>.caller`. Root must verify the exact installed package, named registration, actual bus sender and service rights on the C4. Host SSH Luna calls and successful SDK linking cannot establish application admission.

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

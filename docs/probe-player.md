# Packaged player lifetime probe

`tools/player-probe` builds two disposable WAM apps and one packaged JavaScript service. The probe displays actual Rust pipe acknowledgments and cleanup state. The PLAYER app also offers one explicit Widevine access measurement, which may initialize browser/DRM internals. Provider, account, media, license and playback integration remain absent; actual EME access from the packaged origin and C4 support remain unadmitted.

## Source and admission

The paired WAM app IDs are `com.mikestopcontinues.criterion.probe.ui` and `com.mikestopcontinues.criterion.probe.player`; the player owns `com.mikestopcontinues.criterion.probe.player.bridge`. The separate disposable native caller has the exact admitted ID `com.mikestopcontinues.criterion.probe.native`. Root must collision-check the live catalog before installing these disposable apps. App identity, service jail ELF execution, service activity lifetime and foreground/return behavior require root's serialized exact-C4 admission. The production application identity is excluded.

These are disposable development packages. [Binary licensing](licenses.md#disposable-ui-and-player-probes) owns their current public-distribution notice gap and release boundary; MAIN and the native caller are separate packages.

The [LG service reference](https://webostv.developer.lge.com/develop/references/webos-service-reference) owns `message.sender`, request tokens, subscriptions and cancellation. Public methods reject every sender except the three exact app IDs; payload identity fields never authenticate a caller. Native `LSRegisterApplicationService(<id>.caller, <id>)` registration does not prove the sender published to this service. No named-service alias or prefix is admitted; root must prove the exact native sender on the C4. Local caller fixtures establish no native SDL/LS2 rights or licensed playback. [Subscription lifetime](https://webostv.developer.lge.com/develop/guides/js-service-faq) and the service's one-second broker heartbeat cover app background/launch overlap. [Node runtime versions](https://webostv.developer.lge.com/develop/guides/js-service-basics) identify TV25's Node 16.20.2. Source is checked with Node 16 declarations and executed under that exact runtime in Docker. Node 24 in the tools image runs compilers and packaging only.

The service uses the TV's documented `webos-service` system module. Its constructor/types and actual bus semantics remain device admission. The unchanged stock LG SDK and license are owned by [vendor attribution](../tools/player-probe/vendor/NOTICE.md). Both app launch events are registered before activation through the current [webOSSystem lifecycle contract](https://webostv.developer.lge.com/develop/guides/app-lifecycle-management). Back in the player launches the fixed UI; UI Back awaits confirmed stop/reap and uses platform Back. Both apps hold at most one subscription each while backgrounded; teardown cancels owned handles and retires callbacks. A confirmed terminal response cannot be reopened by a delayed one-shot reply.

## Pipe and lifetime contract

`bridge.ts` spawns only its packaged `bin/criterion-broker-probe`, with fixed arguments, `shell:false`, `detached:false` and explicit private stdin/stdout/stderr pipes. There is no executable, path, argument or URL parameter in LS2 requests. Methods are `attach`, `ping` and `close`, with exact version `0.1.0` and unchanged reply fields. Requests admit only the required fields. At most three subscriptions, one per exact caller, share one child. A synchronous per-caller slot also bounds outstanding ping/close requests to three before any await. Issued actions retain their slots until settlement across subscription cancellation and service disposal. The controller and WAM snapshot parser share the admitted caller cap in [protocol.ts](../tools/player-probe/src/protocol.ts). The last release stops the child; a replacement requires confirmed cleanup.

The Rust broker accepts canonical positive numbered `ping` lines and `stop`; its replies contain only process ID, counter and fixed protocol words. Input frames are bounded to 48 bytes, the sequence to 1,000,000, and queues to one entry. Production lease is 10 seconds and absolute lifetime 120 seconds. Parent-pipe EOF ends it. Output has a 250ms acknowledgment deadline so blocked readers cannot retain the broker indefinitely. Argument validation admits only the bounded lease/lifetime mode.

The bridge admits only matching PID and sequential acknowledgments, startup/ping deadlines of one second, stdout chunks at most 4KiB/total 32KiB/frames 48 bytes and stderr at most 4KiB without retaining its text. Close ends stdin, waits 1 second, sends SIGTERM and waits 500ms, then SIGKILL and waits 1 second. Only the actual child `close` event confirms retirement of process and pipes; [Node 16 child-process documentation](https://nodejs.org/download/release/v16.20.2/docs/api/child_process.html) distinguishes it from `exit` and signal acceptance. Missing close quarantines the child and forbids replacement. UI connection/actions have 3-second deadlines and suppress publication after disposal.

## Explicit PLAYER access measurement

[wam.ts](../tools/player-probe/src/wam.ts) keeps the MSE/EME API-presence indicators and exposes **Check Widevine access** only in PLAYER. Launch, activation and relaunch do not start it. The action consumes its once-only owner even if admission fails. [eme.ts](../tools/player-probe/src/eme.ts) owns at most two independent `requestMediaKeySystemAccess("com.widevine.alpha", [configuration])` queries; each submits exactly one fixed configuration:

| Label | Audio robustness | Video robustness |
| --- | --- | --- |
| `hw-video` | `SW_SECURE_CRYPTO` | `HW_SECURE_ALL` |
| `sw-crypto` | `SW_SECURE_CRYPTO` | `SW_SECURE_CRYPTO` |

Both configurations request `initDataTypes: ["cenc"]`, `sessionTypes: ["temporary"]`, and `distinctiveIdentifier`/`persistentState: "not-allowed"`. The audio capability is `audio/mp4; codecs="mp4a.40.2"`; video is `video/mp4; codecs="avc1.640028"`; each capability explicitly requests `encryptionScheme: "cenc"`. These source-selected codec and robustness inputs establish no observed Criterion stream format or production policy. A settled `NotSupportedError` permits the other declared measurement; there is no automatic policy selection or downgrade.

Admission requires PLAYER role, a secure top-level visible document, an attached running broker snapshot no older than one second, and an action-time ping acknowledgment with a higher counter from the same PID. Guards before submission and after awaited work retain that PID, counter and freshness requirement. The original 12-second action deadline starts before the ping and includes guards, ping, queries and configuration projection. Ping is bounded to three seconds and each query to six seconds, capped by the original deadline. A pending first query prevents the second; timeout, malformed selection or lost admission ends the sequence. Transfer, Back, close, hidden visibility, pagehide or a terminal bridge state stop further work and publication through the existing WAM/client lifetime.

For resolved access the owner observes `keySystem` and `getConfiguration()`, validates the fixed selected fields and emits two coarse rows through `textContent`, at most 1024 characters. Rows contain attempted/settled state, static outcome/error categories and validated configuration fields. Unknown values, extra selected fields, getter failures, raw errors and CDM strings are withheld. Absent or null selected encryption schemes remain distinct from confirmed `cenc`; resolved access alone cannot confirm it. Late settlement updates only the existing settled witness, without configuration inspection, another query or publication. JavaScript cannot preempt a blocking native call, and timeout or disposal cannot cancel an issued query or prove CDM quiescence.

The action creates no `MediaKeys` or session and calls no `generateRequest`, license, media, account or persistence API. Its actual execution still needs a separately bounded exclusive TV writer phase through Elgee's canonical deployment entry point, with exact package/origin/process ownership and companion cleanup. Browser/CDM initialization can have internal effects; the [HTML CSP](../tools/player-probe/packaging/index.html) restrictions `connect-src 'none'` and `media-src 'none'` do not establish absence of CDM network activity.

The unchanged [native caller](native-caller.md#keymanager-boundary) automatically attempts its fixed absent-name Keymanager read after acknowledged broker cleanup when liveness and foreground admission hold. A future key-free EME measurement must not exercise that native-caller sequence. Broker acknowledgment couples the measurement to the existing service lifetime; it does not authenticate the DOM EME result as a native-caller response.

## Local build and verification

Run all compilers and tests in Docker from the actual worktree. The broker's default host build remains dependency-free and permits standalone `rustc` checks. Its native `webos` feature selects the platform's auxiliary-vector runtime without SDL; [native platform](native-platform.md#auxiliary-vector) owns this shared runtime implementation.

```sh
docker build -t criterion-player-probe-tools:20261009 tools/player-probe
docker run --rm --mount type=bind,src="$PWD",dst=/workspace \
  --workdir /workspace/tools/player-probe criterion-player-probe-tools:20261009 npm ci --ignore-scripts
./dev run rustc --edition=2024 --deny warnings tools/player-probe/broker/src/main.rs \
  -o .local/player-probe/criterion-broker-probe
docker run --rm --network none --mount type=bind,src="$PWD",dst=/workspace \
  --workdir /workspace/tools/player-probe criterion-player-probe-tools:20261009 \
  sh -c 'npm run compile && npm test'
```

Tests use actual CPU subprocesses for pipe validation, EOF, lease/absolute expiry, blocked output, overlapping subscriptions, cancellation, replacement, escalation and inherited-pipe cleanup. Controlled external LS2 fixtures cover untrusted senders and callback ordering. [EME owner and WAM fixtures](../tools/player-probe/tests/eme.test.ts) exercise exact configuration arguments, controlled deadlines, stale/PID guards, late settlement, bounded projection and forbidden API traps, including the actual WAM composition with a simulated DOM. These checks do not execute a browser, EME/CDM, GPU or TV service.

After root workspace admission, build the target with the canonical SDK and rebuild `std`; [development tooling](development.md) owns the target, compiler, SDK and link configuration:

```sh
./dev native cargo build -p criterion-broker-probe --features webos --release --locked \
  --target arm-unknown-linux-gnueabi -Z build-std=std,panic_abort
./dev native run cp /target/arm-unknown-linux-gnueabi/release/criterion-broker-probe \
  .local/player-probe/criterion-broker-probe-arm
docker run --rm --network none --mount type=bind,src="$PWD",dst=/workspace \
  --workdir /workspace/tools/player-probe criterion-player-probe-tools:20261009 \
  sh -c 'npm run compile && npm run package'
```

The packager requires exactly this fixed staging path and no command-line arguments. It rejects a host executable, admits bounded ELF32 little-endian ARM EABI5 soft-float input and copies it with mode 0755. The actual SDK ELF receipt must separately verify interpreter and dynamic library/version requirements; ELF header validation alone cannot establish runtime compatibility.

`build.ts` compiles the apps and Node 16 service, packages one app per IPK with pinned official CLI 3.2.6 and explicit TV profile, then compares every archived payload file byte-for-byte with its sealed staging input. The pinned CLI emits writable metadata/directories; the packager preserves that raw artifact and reconstructs a normalized, strictly audited archive from sealed buffers using pinned GNU tar/ar. [Native packaging](native-package.md#archive-construction) owns the shared normalization and tool-version contract. Only generated package metadata is additional. Services declare the documented [public methods](https://webostv.developer.lge.com/develop/references/services-json), plus the `id` required by the pinned CLI's `loadServiceInfo/getPkgServiceNames` source. Tests, TypeScript sources, dependencies and development tooling are excluded. Symlinks, extra files, mismatched versions/IDs, changed payloads and missing executable permissions fail packaging.

Outputs remain ignored under `.local/player-probe`: CPU/ARM executables, raw check receipts, staging, two IPKs and `package-seal.json`. The seal records exact source/input, binary, staging and IPK SHA256 hashes. Normalized archive ownership, timestamps and modes are fixed; root must install the exact audited artifacts and record their hashes.

The host-only official CLI's transitive dependencies currently produce npm audit findings, including glob/format-parser denial of service and a proxy-address advisory. No CLI server is invoked; package inputs and arguments are build-owned, packaging is offline, and none of these dependencies enter either app or service. The audit receipt remains an explicit tooling limitation rather than a clean dependency-audit claim.

# Packaged player lifetime probe

`tools/player-probe` builds two disposable WAM apps and one packaged JavaScript service. The probe displays actual Rust pipe acknowledgments and cleanup state. It contains no provider, account, media, CDM, license or playback implementation. MSE/EME indicators report API presence only; they make no key-system or network request.

## Source and admission

The paired app IDs are `com.mikestopcontinues.criterion.probe.ui` and `com.mikestopcontinues.criterion.probe.player`; the player owns `com.mikestopcontinues.criterion.probe.player.bridge`. Root must collision-check the live catalog before installing either package. App identity, service jail ELF execution, service activity lifetime and foreground/return behavior require root's serialized exact-C4 admission. Two WAM callers establish a smaller admission surface; they cannot establish native SDL caller rights or licensed playback.

The [LG service reference](https://webostv.developer.lge.com/develop/references/webos-service-reference) owns `message.sender`, request tokens, subscriptions and cancellation. Public methods reject every sender except the two exact app IDs; payload identity fields never authenticate a caller. [Subscription lifetime](https://webostv.developer.lge.com/develop/guides/js-service-faq) and the service's one-second broker heartbeat cover app background/launch overlap. [Node runtime versions](https://webostv.developer.lge.com/develop/guides/js-service-basics) identify TV25's Node 16.20.2. Source is checked with Node 16 declarations and executed under that exact runtime in Docker. Node 24 in the tools image runs compilers and packaging only.

The service uses the TV's documented `webos-service` system module. Its constructor/types and actual bus semantics remain device admission. The unchanged stock LG SDK and license are owned by [vendor attribution](../tools/player-probe/vendor/NOTICE.md). Both app launch events are registered before activation through the current [webOSSystem lifecycle contract](https://webostv.developer.lge.com/develop/guides/app-lifecycle-management). Back in the player launches the fixed UI; UI Back awaits confirmed stop/reap and uses platform Back. Both apps hold at most one subscription each while backgrounded; teardown cancels owned handles and retires callbacks. A confirmed terminal response cannot be reopened by a delayed one-shot reply.

## Pipe and lifetime contract

`bridge.ts` spawns only its packaged `bin/criterion-broker-probe`, with fixed arguments, `shell:false`, `detached:false` and explicit private stdin/stdout/stderr pipes. There is no executable, path, argument or URL parameter in LS2 requests. Methods are `attach`, `ping` and `close`, with exact paired version. Requests admit only the required fields. At most two subscriptions, one per exact caller, share one child. A synchronous per-caller slot also bounds outstanding ping/close requests to two before any await. The last release stops it; a replacement requires confirmed cleanup.

The Rust broker accepts canonical positive numbered `ping` lines and `stop`; its replies contain only process ID, counter and fixed protocol words. Input frames are bounded to 48 bytes, the sequence to 1,000,000, and queues to one entry. Production lease is 10 seconds and absolute lifetime 120 seconds. Parent-pipe EOF ends it. Output has a 250ms acknowledgment deadline so blocked readers cannot retain the broker indefinitely. Argument validation admits only the bounded lease/lifetime mode.

The bridge admits only matching PID and sequential acknowledgments, startup/ping deadlines of one second, stdout chunks at most 4KiB/total 32KiB/frames 48 bytes and stderr at most 4KiB without retaining its text. Close ends stdin, waits 1 second, sends SIGTERM and waits 500ms, then SIGKILL and waits 1 second. Only the actual child `close` event confirms retirement of process and pipes; [Node 16 child-process documentation](https://nodejs.org/download/release/v16.20.2/docs/api/child_process.html) distinguishes it from `exit` and signal acceptance. Missing close quarantines the child and forbids replacement. UI connection/actions have 3-second deadlines and suppress publication after disposal.

## Local build and verification

Run all compilers and tests in Docker from the actual worktree. Root owns broker admission to the shared Cargo workspace and lock. Before that admission, the authorized ignored standalone harness may reference the canonical `broker/src/main.rs`, with no dependencies and the root release profile.

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

Tests use actual CPU subprocesses for pipe validation, EOF, lease/absolute expiry, blocked output, overlapping subscriptions, cancellation, replacement, escalation and inherited-pipe cleanup. Controlled external LS2 fixtures cover untrusted senders and callback ordering. These checks do not execute a browser, GPU or TV service.

After root workspace admission, build the target with the canonical SDK and rebuild `std`; [development tooling](development.md) owns the target, compiler, SDK and link configuration:

```sh
./dev native cargo build -p criterion-broker-probe --release --locked \
  --target arm-unknown-linux-gnueabi -Z build-std=std,panic_abort
./dev native run cp /target/arm-unknown-linux-gnueabi/release/criterion-broker-probe \
  .local/player-probe/criterion-broker-probe-arm
docker run --rm --network none --mount type=bind,src="$PWD",dst=/workspace \
  --workdir /workspace/tools/player-probe criterion-player-probe-tools:20261009 \
  sh -c 'npm run compile && npm run package'
```

The packager requires exactly this fixed staging path and no command-line arguments. It rejects a host executable, admits bounded ELF32 little-endian ARM EABI5 soft-float input and copies it with mode 0755. The actual SDK ELF receipt must separately verify interpreter and dynamic library/version requirements; ELF header validation alone cannot establish runtime compatibility.

`build.ts` compiles the apps and Node 16 service, packages one app per IPK with pinned official CLI 3.2.6 and explicit TV profile, then compares every archived payload file byte-for-byte with its sealed staging input. Only generated package metadata is additional. Services declare the documented [public methods](https://webostv.developer.lge.com/develop/references/services-json), plus the `id` required by the pinned CLI's `loadServiceInfo/getPkgServiceNames` source. Tests, TypeScript sources, dependencies and development tooling are excluded. Symlinks, extra files, mismatched versions/IDs, changed payloads and missing executable permissions fail packaging.

Outputs remain ignored under `.local/player-probe`: CPU/ARM executables, raw check receipts, staging, two IPKs and `package-seal.json`. The seal records exact source/input, binary, staging and IPK SHA256 hashes. IPK archive timestamps can change across builds; root must install the exact audited artifacts and record their hashes.

The host-only official CLI's transitive dependencies currently produce npm audit findings, including glob/format-parser denial of service and a proxy-address advisory. No CLI server is invoked; package inputs and arguments are build-owned, packaging is offline, and none of these dependencies enter either app or service. The audit receipt remains an explicit tooling limitation rather than a clean dependency-audit claim.

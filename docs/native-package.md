# Native development package

The inert [`packageNativeMain` API](../tools/package-native/index.ts) packages an already checked Rust executable into an audited, unsigned MAIN development IPK. Importing the module performs no work; the caller supplies the inputs and execution adapter explicitly. It does not compile or execute the application, contact a device or provide an operational CLI.

The [fixed descriptor](../tools/package-native/src/manifest.ts) identifies **Criterion Unofficial** as a native development app with the `criterion-unofficial` main, lifecycle interface V2, no relaunch handling and the DB8 permission required by [My List writes](list-writes.md#storage-and-deployment-boundary). [LG's application descriptor reference](https://webostv.developer.lge.com/develop/references/appinfo-json) owns the standard fields; the native lifecycle convention follows the source-verified Plx manifest and [native application guidance](https://www.webosose.org/docs/tutorials/native-apps/developing-built-in-native-apps/). Requested permissions do not establish C4 rights or lifecycle behavior.

## Target prerequisites

The maintained [read-only prerequisite reader](../tools/native-deployment/index.ts) has confirmed model `OLED77C4PUA.DUSQLJR`, firmware `33.31.69` and SDK `10.3.1`. Its target process reports Node `16.20.2` on Linux `arm`. These observations are bound to a stable boot/compositor identity and must be refreshed before device phases.

The ten probed libraries are present as hash-stable ELF32 little-endian ARM EABI5 files: `libSDL2-2.0.so.0`, `libgcc_s.so.1`, `librt.so.1`, `libpthread.so.0`, `libm.so.6`, `libdl.so.2`, `libc.so.6`, `ld-linux.so.3`, `libEGL.so.1` and `libGLESv2.so.2`. This covers the supplied MAIN requirements and fixed graphics probes, not native-caller dependencies. File presence and header metadata do not establish required symbol versions, successful loading or application execution.

A separate bounded read from the executing ELF32 little-endian ARM Node process reports `AT_HWCAP = 3649750`, with the NEON mask `4096` (bit 12) set. This admits the target's ARM32 NEON capability; `/proc/cpuinfo` architecture `8` and ASIMD alone would not establish it. Criterion instruction execution, required symbols and native runtime compatibility remain unadmitted alongside the [compiler and ELF gates](development.md). The exact-package install, native launch/render and complete cleanup checkpoint remains outstanding; no Criterion installation or licensed playback is admitted by these reads.

### Runtime observer

The inert [runtime observer](../tools/native-deployment/runtime.ts) accepts checked executable requirements and emits one bounded read of the fixed installed MAIN alias, its process and required mapped libraries. It binds the expected executable hash to one PID, start time and boot identity, then checks canonical file paths, mapped device/inode identity, ELF metadata and streamed hashes. Duplicate, deleted, drifting, invalid or unresolved observations refuse; the caller owns the original deadline and actual process/output closure.

[Controlled stock Node fixtures](../tools/native-deployment/tests/runtime.test.ts) cover these rules, including application-owned proc entries read by the privileged observer. Actual C4 observation remains unadmitted. Process and mapped-file facts do not establish whole-package identity, symbol resolution, uninterrupted scheduling, foreground state or rendering. Those require the separate canonical device checkpoint.

## Input and source admission

[`NativeMainInput`](../tools/package-native/src/contract.ts) supplies four values:

| Input | Required custody |
| --- | --- |
| `sourceRoot` | Absolute, canonical source directory containing the matching receipt inputs, lockfile, packaging sources, assets and locked official CLI. The caller keeps it immutable throughout packaging. |
| `executable` | Checked ARM executable bytes, copied before the first await. |
| `buildReceipt` | Matching schema-1 receipt bytes, also copied before the first await. |
| `outputDirectory` | Fresh `/workspace/.local/native-package/exports/<run>` directory, where `<run>` is a unique lowercase alphabetic name of 1–64 characters. The canonical `/workspace/.local/native-package` mount must already exist. |

The [receipt validator](../tools/package-native/src/receipt.ts) admits the fixed application, version, ARM target and release profile, then checks the executable, `Cargo.lock` and every supplied source hash before staging and after packaging. It admits bounded repository-relative paths and requires its minimum source map. `sourceCommit` is checked for its hexadecimal format; the API does not inspect Git or prove that this is the compile-source commit, that the checkout is clean or that the map contains every build input. Its identity scope is exactly **`build-receipt-content-only`**. SDK/compiler provenance, complete source capture, runtime inputs and reproducibility require separate producer evidence. [Development tooling](development.md) owns the native compiler and target contracts; [current work](../TASKS.md) owns producer and deployment readiness.

The [executable admission](../tools/package-native/src/admission.ts) checks bounded ELF32 little-endian ARM EABI5 metadata, explicit soft-float ABI, an executable load segment, the fixed dynamic interpreter and one read/write, nonexecutable GNU stack. Regular input reads reject followed symlinks and overbound files. These checks establish admitted bytes and metadata; they do not establish SDK library compatibility or successful TV execution.

## Execution and output ownership

[`PackagingExecution`](../tools/package-native/src/contract.ts) requires the fixed packaging image and Node 24.20.0 runtime checked by [the implementation](../tools/package-native/build.ts), an original absolute deadline, a monotonic clock and an injected settled executor. Each issued command carries that same deadline, a timeout bounded by its remaining budget, fixed arguments/environment and bounded output. The executor returns explicit process closure with status and streams; an unresolved outcome, nonzero exit, signal or timeout stops packaging. A bounded acknowledged command result remains available through `PackagingCommandError` when deadline refusal prevents log custody.

The API must run **inside a caller-bounded, joined packaging process or container**. The caller enforces the original deadline across its entire lifetime, including the unchanged normalizer's synchronous GNU children. Cooperative checks before and after awaited work cannot interrupt a blocked synchronous child. No command receives a renewed outer budget.

The sole ELF metadata command is GNU `readelf -d` against the byte-matching staged executable. Its bounded report yields 1–10 safe, unique `DT_NEEDED` SONAMEs in their observed order. The tool and executable hashes enter the seal. This is a requirements observation, not a platform library allowlist, symbol-version check or ABI admission; the executable is never loaded or run.

The fixed official CLI runs with an explicit TV profile and a fresh, isolated home directory. The producer supplies a [bounded private copy](../tools/package-native/src/cli-configuration.ts) of the locked CLI configuration and mounts that child writable beneath the otherwise read-only dependency tree. It checks the original configuration's identity and contents; profile changes affect only the private copy. Source and full dependencies remain read-only.

The tool reuses [the player probe's pinned image and dependency lock](probe-player.md#local-build-and-verification); its dependency-audit limitation is owned there. No tooling dependencies enter the app. Fresh staging, CLI output, logs and final outputs use exclusive creation. Existing output directories and artifacts are never overwritten. The unchanged GNU normalizer receives only a newly reserved empty sibling under `/workspace/.local/native-package/normalization/<run>`; failure leaves earned outputs as evidence.

The sole intended operational consumer is [Elgee's canonical deployment entry point](/Users/mike/Code/elgee-tv/TASKS.md#deployment-pipeline); its owner tracks readiness. Packaging does not acquire TV custody or create a parallel deployment command; [the shared TV policy](../AGENTS.md#shared-tv) owns that boundary.

## Archive construction

The fixed payload contains the executable, descriptor, original geometric icon, GPL license, exact [full notice compendium](../NOTICES.md) and retained platform, egui_glow and image-webp attribution/license texts. The CLI adds one package descriptor. The app contains no service, WAM SDK, JavaScript, account data or credentials. [The payload manifest](../tools/package-native/src/manifest.ts) owns its exact inventory; [the licensing inventory](licenses.md) owns the notices' scope and unresolved release obligations.

The packager preserves the original unsigned CLI artifact, admits its known generated modes and compares every regular member with build-owned buffers. The unchanged normalizer reconstructs a fresh tree from those buffers without extracting archive paths to disk. Fixed GNU tar/ar operations produce an unsigned USTAR archive with root numeric ownership, fixed timestamps, directories `0755`, nonexecutables `0644` and the binary `0755`.

The [archive audit](../tools/package-native/src/archive.ts) bounds archive/decompressed sizes, requires exactly three ordered ar members and the fixed control fields, and verifies every payload byte and mode. It rejects hooks, extra or duplicate members, changed bytes, links, special files, unexpected directories, traversal, corrupted gzip and content hidden after tar end markers. GNU reads inspect permitted PAX metadata before reconstruction using fixed executables, bounded streams and an environment excluding inherited tar options.

The returned [`NativeMainExport`](../tools/package-native/src/contract.ts) contains the admitted build receipt and its hash; normalized IPK and package-seal paths, hashes and sizes; the exact archive audit; and executable requirements containing the app ID, executable hash and observed SONAMEs. Both the report and seal declare `status: "development"` and `identityScope: "build-receipt-content-only"`. The seal records source/tooling hashes, actual tool versions, original CLI artifact identity, normalized payload identity and the dynamic metadata report. It is written last, after source/tool rechecks and an audit of the exact final IPK bytes.

## Development verification scope

[Canonical CI](../.github/workflows/ci.yml) compiles the tooling and runs the unchanged 61 admission/archive cases on the stock Node 16 runtime, then the 33-case export runner on the required Node 24 runtime. The export runner comprises 22 API cases and 11 reused ELF admission cases. Controlled clocks cover deadline refusal and acknowledged command evidence; injected settled executors cover closure, source drift, fresh-directory ownership, malformed metadata and failed packaging. Actual CPU GNU normalization and archive auditing operate on explicitly synthetic ELF metadata and injected readelf/CLI outputs. These fixtures do not establish an actual SDK build, real executable metadata, official producer execution or TV behavior.

The maintained producer has separately passed a complete SDK build, executable inspection and audited development export for an explicit read-only application source basis. Its matched CPU source gate also passed. Source, executable and package payload custody are verified, with joined command closure and build-container cleanup. This export does not admit the current My List manifest, deployment initializer or C4 storage behavior; a changed application needs its own matching package evidence.

Separate development reconciliation binds the actual SDK app, caller and broker artifacts to their matching packages and corresponding-source candidate. [The licensing reconciliation](licenses.md#development-reconciliation) owns the current notice, font and source matching scopes and remaining limits; exact retained checks belong in [the primary journal](../logs/2026-10-10.md). That evidence is distinct from API fixture coverage.

Public binary distribution, complete runtime attribution, source-exclusion eligibility and reproducibility remain unadmitted. Installation, native rendering, physical input, account behavior and licensed playback require separate device/provider evidence; development packaging establishes none of them.

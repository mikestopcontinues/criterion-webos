# Criterion Unofficial Agent Instructions

Read `README.md`, `TASKS.md`, today's primary log and the relevant owning topics before work. Address the captain naturally as Captain. Shared instructions and skills remain canonical in `~/.agents`; load the document skill when project knowledge changes.

## Product and evidence

Build the commissioned Rust application for LG webOS with verified official Criterion app behavior. Rust owns the application; platform integration follows an established licensed playback contract. Never substitute a mock catalog, synthetic stream, undocumented entitlement assumption or copied screenshot for real provider and device acceptance. Keep reference-app observations, local tests, native TV behavior, physical input and audible output distinct.

`TASKS.md` owns unfinished work and the next checkpoint. `docs/` owns current contracts, requirements and reasoning. `README.md` explains the product and development. `logs/YYYY-MM-DD.md` contains append-only events and evidence using America/Chicago dates. Each agent appends its own timestamped entries naming the actual branch and agent. The primary shared log lives in `/Users/mike/Code/criterion-webos/logs/` even when worktrees have private copies. Agree on file ownership before edits.

## Engineering

Use Rust for application and native code. Use strict TypeScript for web playback integration, tooling and browser tests where needed; no unchecked JavaScript alternate implementation. Choose small, clear modules with explicit state ownership. Prefer established dependencies and proven patterns; remove obsolete paths rather than adding compatibility layers or speculative abstractions. Every asynchronous owner cancels or rejects stale publication and disposes its resources.

Keep credentials and private account/device evidence outside Git. Redact tokens, cookies, signed media/license URLs and personally identifying data from logs. Provider data and external URLs require bounded validation. Maintain TLS verification, host/origin restrictions, bounded queues/caches/deadlines and secure local session storage. Do not bypass DRM or use a provider owner credential as a subscriber credential.

Use isolated `agent/` Git worktrees for substantive code changes. Run appropriate formatting, diagnostics, behavioral tests and relevant integrated checks before coherent local commits. Preserve unrelated staging and omit AI co-author trailers. Rebase onto current `master`, integrate fast-forward only, and remove completed owned worktrees after preserving needed evidence. The captain authorized public GitHub publication and the project landing page; publish truthful functionality and validation details.

Use reproducible, pinned Docker tooling for development, ARM cross compilation and host test environments. Host Git/source editing and the designated host TV executor remain separate. Unit and integration tests exercise public behavior; E2E tests exercise the complete rendered application. Record genuine failing evidence before fixes. Tests using fixtures must be identified as fixture-backed and cannot prove provider playback or TV behavior.

## Shared TV

This project shares the captain's rooted C4 with Elgee. All agents always have concurrent TV read access. Genuine read-only discovery, getters, subscriptions and reads need no exclusive lease; launches, waking, installation, file/settings changes and destructive effects require one exclusive writer assigned by the orchestrator. Classify operations by their effects, including diagnostics and inspectors. Read [Elgee's device contract](/Users/mike/Code/elgee-tv/docs/device.md#progressive-deployment-and-device-rotation) before TV access and its [current write schedule](/Users/mike/Code/elgee-tv/TASKS.md#orchestrated-device-work) before requesting custody. The captain's pause or reservation overrides writes without suspending standing read authorization; read access does not authorize waking the TV.

Future feature deployments must use the single canonical command being implemented in Elgee's `deployment/` directory. [Elgee's deployment owner](/Users/mike/Code/elgee-tv/TASKS.md#deployment-pipeline) owns its readiness; deployment waits for the usable, verified entry point. Private commissioning runners and standalone installers are internal pipeline operations, not alternate deployment paths.

Before a device mutation capture the exact source/package and fresh catalog, foreground, audio/links/volume/mute, all power timers, screensaver, OLED protection and Elgee input-policy originals. Preserve newer user choices, existing applications, pairing and native input ownership. Do not take over third-party apps, HDMI or casting without current direct authorization. Never release the queue with unresolved native operations or owned cleanup. Physical remote and audible output require separate user observation.

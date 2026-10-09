# Native Progress Wire Contract

The signed native reference establishes progress request encoding and client reply admission. It does not establish our subscriber's server acceptance or mutation settlement. [The provider contract](provider-contract.md#native-account-and-progress) owns position units, completion and reporting cadence; [the account boundary](account.md) owns regional routes, private header roles and issued-write uncertainty. Exact public-symbol traces and redacted receipt digests are indexed in [the primary source journal](../logs/2026-10-09.md).

## Request encoding

The deployed `ContinueWatchingRequest` serializer always emits `media_id`, `pos` and `dur`. It omits `commentary_track`, `series_id` and `series_title` when their values are null, and emits each nonnull string. These generated default checks run before nullable encoding. The deployed Json configuration has `encodeDefaults=false` and `explicitNulls=true`; the latter flag does not make these default-null fields appear.

The installed service factory selects its Kotlin serializer converter for this DTO. It encodes a JSON object into UTF-8 bytes with `Content-Type: application/json; charset=utf-8`. The static chain is `xo0`/`yo0.y` → `cx4` request-factory selection → `vg1.a` → `k70.k`; `bq0.a` supplies the Json instance. No serialization or HTTP transmission was executed for this proof.

## Reply admission

The native `lk3.j` method returns Kotlin `Unit` through its coroutine continuation. The installed built-in converter precedes the JSON converter, closes the response body and returns the Unit singleton without decoding a response object or validating its MIME. The complete HTTP and callback paths distinguish these cases:

| HTTP status | Installed native client behavior |
| --- | --- |
| 200–299, excluding 204 and 205 | Runs the Unit converter. An empty 200 can succeed without JSON or a response MIME requirement. |
| 204 or 205 | Closes the body and supplies null; the ordinary nonnullable coroutine callback rejects that null result. |
| Outside 200–299 | Completes as an HTTP failure. |

This distinction comes from `e5.m`/`iv.b` → `oz4.u` for converter selection, `n62.a` → `bx4.invoke` → `j00.f` for ordinary coroutine body adaptation, and `s24.c` for HTTP status handling. The Unit type alone would miss the 204/205 edge. Transport or callback failures can still prevent completion; actual server reply framing remains unobserved.

A Unit completion reports client HTTP completion, not an `Applied` mutation. A progress adapter needs its own bounded transport and the account owner's issued-write lifetime and uncertainty contract. It must not require the list-write `sync` object, automatically replay an unconfirmed report, or infer remote settlement from a later read.

## Native catalog duration

The traced Float32 catalog durations are seconds for these subtypes:

| Subtype | Source interpretation |
| --- | --- |
| Film, Supplement, Episode | `sy6.W` truncates Float to Int fields labeled `durationSeconds`. `xe2.o` forwards them to `v84.p`; `v84.l` divides by 3,600 and 60 for hour/minute display. Fractional and remaining seconds are discarded for that display. |
| Original | `x27.F` truncates Float to Long and passes it unchanged into `h21.k`, labeled `durationSeconds`. A separate progress-ratio denominator multiplies it by 1,000. Original details formatting was not established. |
| Live | No nongenerated duration consumer was established by the bounded trace; units and formatting remain unadmitted. |

The hour/minute formatter's public resources independently resolve to `%d min` and `%1$d h %2$d min`. These subtype-specific consumer traces do not make absent metadata zero, establish accepted server ranges, or replace the admitted player clock for progress reporting. [Native DTO admission](account.md#subscriber-method-admission) continues to preserve an optional Float32 value; any UI conversion must select a proved subtype and apply explicit bounds.

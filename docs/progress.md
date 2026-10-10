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

## Series resume selection

[The native selector](../crates/criterion-app/src/native_resume.rs) derives Series actions and the initial displayed season from one successfully admitted current Continue Watching response. The application retains its bounded immutable position snapshot under the root account epoch. Exact duplicate IDs use the last supplied row within that response; this preserves existing project behavior without inferring chronology or merging separate responses. Missing positions do not establish complete account coverage. Displayed fractions use the proved Film, Supplement and Episode slots; Original does not acquire an inferred fraction.

The first Seasons group owns episode selection. The primary action requires its first season's first Episode, while the initial displayed season is selected independently and can show a saved later season when the first season is empty. The traced chooser prioritizes incomplete episode index, then season index, then saved fraction, preserving the first exact tie. With no incomplete candidate, it advances after the last completed episode in supplied order or returns Watch Again at the end. The completion cutoff uses the Episode's truncated catalog duration, separately from the saved-position ratio. The primary progress map includes all supplied playlist groups; the displayed-season map includes only the first Seasons group. Supplied season numbers remain label operands rather than sort keys.

Actual application tests cover selection, completed-to-next and terminal behavior, empty seasons, failed/stale reads, same-token relinking and account/frame retirement. Navigation caches anonymous Detail defaults rather than private derived actions and fractions. Background, logout and disposal erase the snapshot and derived state while retaining anonymous metadata. The offline host SDL/GLES journey uses actual input to reach the last child in a 511-Episode synthetic fixture, checks the complete boundary resume caption and progress pixels, then checks cached Back after logout with default primary action, visible first-card focus and no saved fraction. English action captions are project wording, not verified reference localization. These selections do not execute autoplay, establish a player clock, convert saved seconds into a licensed start position, or report progress; those require the admitted player and write owners.

## Native catalog duration

The traced Float32 catalog durations are seconds for these subtypes:

| Subtype | Source interpretation |
| --- | --- |
| Film, Supplement, Episode | `sy6.W` truncates Float to Int fields labeled `durationSeconds`. `xe2.o` forwards them to `v84.p`; `v84.l` divides by 3,600 and 60 for hour/minute display. Fractional and remaining seconds are discarded for that display. |
| Original | `x27.F` truncates Float to Long and passes it unchanged into `h21.k`, labeled `durationSeconds`. A separate progress-ratio denominator multiplies it by 1,000. Root Detail formatting follows the separate Header and Information consumers below. |
| Live | No nongenerated duration consumer was established by the bounded trace; units and formatting remain unadmitted. |

The hour/minute formatter's public resources independently resolve to `%d min` and `%1$d h %2$d min`. These subtype-specific consumer traces do not make absent metadata zero, establish accepted server ranges, or replace the admitted player clock for progress reporting. [Native DTO admission](account.md#subscriber-method-admission) continues to preserve an optional Float32 value; any UI conversion must select a proved subtype and apply explicit bounds.

## Native card runtime display

Detail children, including Featured cards, My List and supplied Continue Watching use one optional final runtime label from [the presentation owner](../crates/criterion-app/src/presentation.rs). Film, Supplement and Episode narrow the admitted Float32 directly to saturated Int32, independently from the root consumers below. Present zero displays `0 min`; absent duration omits the label. An exact hour retains its zero minutes (`1 h 0 min`), and positive overflow saturates to `596523 h 14 min`. Original, Series, containers and Live omit a runtime on this card surface because their consumer is not established by the retained source proof.

Owned cards charge the label's retained capacity before admission, and the renderer borrows it without reformatting each frame. Public website cards keep their existing unsigned caption convention. Synthetic projection, actual Application input and CPU geometry tests cover subtype gates, omission, zero, fractions, saturation, dormant seasons, navigation and allocation boundaries. Source-proved arithmetic and local painting remain separate from official-app pixels, localized wording, live provider delivery and C4 rendering.

## Root Detail runtime display

[The native projection](../crates/criterion-app/src/presentation/native_detail.rs) consumes the admitted optional finite, nonnegative Float32 duration as seconds and truncates/saturates it to signed Int64. Header formatting divides the Int64 into hours/minutes before narrowing each part to signed Int32; Information narrows seconds to signed Int32 before its positive guard and division. Keeping these consumers separate preserves their distinct overflow behavior. Nonpositive admitted values display `0m`; absent values remain omitted.

Film, Original and Supplement expose runtime in both surfaces. Episode exposes it only in Information. Series exposes its supplied year without an aggregate runtime; Category, Collection, Franchise and Live expose neither runtime nor year in these lines. Header and Information each own one final metadata string, charged by retained capacity before projection admission and borrowed during painting. Public website Detail keeps its existing unsigned formatting. The card surface follows its independent rules above; localized-label parity remains unverified.

Synthetic projection and actual Application tests cover subtype/absence gates, ordinary and wrapping arithmetic, both painted surfaces, navigation restoration and allocation limits. They establish source and host behavior separately from official-app pixels, provider delivery and C4 rendering.

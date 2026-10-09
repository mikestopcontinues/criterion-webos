# Criterion Provider Contract

Public browsing works through Criterion Channel's current first-party web routes. These are observed, undocumented service contracts; they establish technical behavior, not a supported third-party API or provider permission. Subscriber authentication, device linking, account synchronization and licensed playback remain unverified for our webOS application. [Product acceptance](product.md) owns those admission requirements.

## Public catalog

Use HTTPS at `www.criterionchannel.com`. The following public GET routes returned JSON without cookies or authorization. [Fixture provenance](../tests/fixtures/provider/provenance.json) records exact requests, capture times and hashes; the [fixture index](../tests/fixtures/provider/README.md) describes the bounded extracts.

| Route | Response and parameters |
| --- | --- |
| `/api/all-films/results` | `{ items, total, paging }`; `page_limit` and optional `pagination_key`. `paging.next_pagination_key` is an opaque continuation token; absence ends pagination. No previous-page token was observed. |
| `/api/all-films/filters` | `{ sortOptions: [{ label, value }], filterGroups: [{ label, value, options: [{ label, value }] }] }`. |
| `/api/search?q=<encoded query>` | `{ playlist, type_counts, paging }`. The web client requests one result set and filters its content types locally; no search continuation request is established. |
| `/api/media/<media ID>` | A single detailed media object, including optional nested `playlists`. The web helper accepts `fresh=1` for a fresh request. |

All-films filter parameters are `genres`, `decades`, `countries` and `directors`, containing comma-separated option values. Treat option values as bounded provider identifiers and preserve their case/Unicode; observed values include `González-juan-pablo` and `Hung-sammo`. Encode values through the URL query builder instead of concatenating raw strings. Sorting uses `sort=title|director|year|primary_country_slug|duration` and `sortDir=asc|desc`; defaults are title ascending. The web client requests 60 items per page. A live request combining `genres=horror`, duration sorting and descending direction returned films in descending duration order. Search trims the query and waits 250 milliseconds after changes. These client rules come from the [all-films bundle](https://www.criterionchannel.com/_next/static/chunks/06e9ymw11xjmc.js) and [search bundle](https://www.criterionchannel.com/_next/static/chunks/04xx7_qje3b36.js).

Catalog IDs are exactly eight ASCII alphanumeric characters in the current [media helper](https://www.criterionchannel.com/_next/static/chunks/3-c73_d_zowt2.js). Observed minimal film-list items contain `mediaid`, `title`, `contentType`, `duration` in seconds and `release_date`. Captured collection/category items omit release dates and use duration zero; no missing-duration response was observed. Client rendering tolerates missing runtime metadata, which is a source-derived tolerance rather than a demonstrated provider omission. Search and nested lists can additionally supply `criterion_id`, `feedid`, title variants, `paywall`, `drm`, commentary labels, license window timestamps, `collection_count`, `logo`, `teaser` and `deeplink`. Bound required metadata, strings, arrays, nesting and payload size; optional detail fields are not list requirements.

Detail metadata includes `director`, `starring`, `country` and `language` string arrays; description variants; genre and content warnings; trailer/introduction IDs; commentary labels; and `playlists`. Generic playlists use `{ key, title, type: "GENERIC_PLAYLIST", playlistId, playlist: [...] }`. Observed keys include `playlist_primary`, `playlist_supplements`, `playlist_collections_appears_in` and `playlist_categories_appears_in`. The current client also recognizes season playlists with `type: "seasons"` and nested season `episodes`, but no season response has been admitted as a fixture. [Detail rendering](https://www.criterionchannel.com/_next/static/chunks/2tpknm2s8cqz7.js) owns those client expectations.

The description helper considers medium/long variants: details prefer long then medium, cards medium then long; staff descriptions can override separately. Rendering treats these as text with newline breaks. The inspected responses contain plain text; arbitrary provider text must remain escaped, bounded text rather than executable HTML.

Paywall values consumed by the client are `registered`, `subscriber` and `all`. A catalog `drm` flag or license window is metadata, not permission to play. Availability depends on the current entitled response and location; cached page and API metadata can disagree. Public detail responses can contain `sources`, `tracks` and legacy `vimeo_id`; the catalog adapter must ignore them. They do not establish the current subscriber playback contract.

The web image helper constructs `https://cdn.jwplayer.com/v2/media/<ID>/images/<label>.webp?width=<positive integer>`, defaulting to `default_16x9`; some rails use `regalia_16x9`. The verified landscape route redirects to `https://img.jwplayer.com/v1/media/<ID>/images/default_16x9.webp?width=480`. The first-party [homepage](https://www.criterionchannel.com/) also configures the Fresh from Theaters rail with `imageJWLabel: default_2x3` and `imageAspectRatio: 150`. Its portrait URL family is `https://img.jwplayer.com/v1/media/<ID>/images/default_2x3.webp?width=480`; a direct HEAD for its displayed item returned image/WebP without fallback. This establishes a portrait asset family, not universal availability or the official TV app's exact label selection. Generate bounded URLs from validated IDs and fixed labels/hosts; treat absent portrait artwork explicitly rather than substituting landscape content as portrait proof. Public artwork remains provider content, not project-owned distribution artwork.

## Web session and entitlement

The public client uses Auth0 and Cleeng. Its configured `__session` cookie and server session storage do not establish cookie attributes, native-app authentication or shared sessions between packaged and hosted origins. The inspected code exposes no current device-code acquisition, verification, polling or expiry contract. The observed `/activate` route redirects to `/app-update`; [the current notice](https://www.criterionchannel.com/app-update) requires updated official apps. The [FAQ](https://www.criterion.com/faq/channel) distinguishes linked Criterion accounts from paid Channel access.

| Source-derived web route | Consumed contract |
| --- | --- |
| `GET /auth/profile` | HTTP 204 means no user; otherwise successful JSON supplies a user. Fields consumed include `sub`, `collaborator`, `cleeng_id`, `firstname`, `lastname`, `email` and `marketing_emails`. Full schema remains unknown. |
| `/login?returnTo=<path>` | Hosted navigation; signup adds `screen_hint=signup`. Logout navigates to `/logout` after local session invalidation. |
| `GET /api/auth/refresh-token` | Nonempty `token` string; optional `forced === true`. Forced refresh adds `force=1` and `x-forced-refresh: 1`. |
| `POST /api/auth/clear-session` | No body; used after expired/revoked sessions. |
| `GET /api/subscription/check-entitlement` | Access requires `accessGranted === true`; subscriber additionally excludes `grantType === "collaborator"`. |
| `POST /api/subscription/sso-login` | No body; consumes `responseData.jwt` and `responseData.refreshToken` for Cleeng. |
| `PUT /api/profile/update` | JSON `{ email, first_name, last_name, marketing_emails }`; success causes profile reread. |
| `POST /api/auth/logout-all-devices` | No body; client checks success status. No per-device list/remove contract was found. |

The [profile SDK](https://www.criterionchannel.com/_next/static/chunks/041rmhcvia7aq.js), [auth/entitlement client](https://www.criterionchannel.com/_next/static/chunks/1z1ya1oc-yyrn.js), [navigation](https://www.criterionchannel.com/_next/static/chunks/3yjgq_qr-o81n.js), [Cleeng handoff](https://www.criterionchannel.com/_next/static/chunks/3x9g_1b8uocho.js), [profile editor](https://www.criterionchannel.com/_next/static/chunks/2wyhltnt9y3o6.js) and [device logout client](https://www.criterionchannel.com/_next/static/chunks/3k3sjiru8d99i.js) establish these expectations. None of these routes was invoked in this investigation.

The authenticated fetcher refreshes on HTTP 401 only when JSON contains `authExpired === true`; it retries with a bearer token, at most twice. A second retried expired response clears the local session. Refresh `error.code` values `missing_session`/`missing_refresh_token` clear the local session; `failed_to_refresh_token` with `error.cause_code=invalid_grant` means revoked. Other failures are transient. A user lacking both collaborator status and `cleeng_id` is held by an account-linking error. Real response nullability, token lifetime, refresh persistence and native linking parameters remain unknown. Do not infer the production middleware host from a compiled staging fallback.

## My List and progress

The current [list hooks](https://www.criterionchannel.com/_next/static/chunks/3yjgq_qr-o81n.js) and [watchlist actions](https://www.criterionchannel.com/_next/static/chunks/1z1ya1oc-yyrn.js) consume authenticated contracts; no authenticated list requests or changes were performed.

| Route | Expected shape |
| --- | --- |
| `GET /api/my-list-ids` | `{ watchlist: [media ID], positions: [{ media_id, pos, dur, commentary_track? }] }`. |
| `GET /api/watch-list` | `{ playlist, type_counts?, paging }`; `page_limit`, optional `content_type` and `pagination_key`. |
| `POST /api/watch-list` | JSON `{ content_type, media_id }`; check HTTP success. |
| `DELETE /api/watch-list/<ID>` | Check HTTP success. |
| `GET /api/continue-watching` | `{ playlist, positions?, paging }`; `page_limit`, optional `pagination_key`. |
| `DELETE /api/continue-watching/<ID>` | Remove saved continuation; reread lists. |
| `POST /api/continue-watching` | JSON `{ device_id, media_id, pos, dur, series_id?, series_title?, commentary_track? }`. |

Progress positions and duration are rounded seconds. `series_title` is sent only with a series ID. The [player modal](https://www.criterionchannel.com/_next/static/chunks/3ses1hbaqk5_a.js) schedules a 20-second heartbeat after time updates, flushes on close/background, uses a visibility/unload beacon, and writes `pos=dur` on completion. Tracking excludes live and compilation content. Resume uses a saved position only before completion; saved commentary is restored. The [completion helper](https://www.criterionchannel.com/_next/static/chunks/3-c73_d_zowt2.js) treats durations below 300 seconds as complete at 95 percent, otherwise 98 percent. Cross-device ordering, conflict handling, rounding acceptance and deletion semantics require subscriber verification.

## Licensed playback boundary

The [player loader](https://www.criterionchannel.com/_next/static/chunks/3-c73_d_zowt2.js) obtains `GET /api/jw/player` as `{ url, exp? }`. On a subsequent loader call within 120 seconds of epoch-second expiry, it invalidates/reloads the script; no scheduled refresh timer is established. The [playback client](https://www.criterionchannel.com/_next/static/chunks/1n_n48e9_udja.js) fetches authenticated `GET /api/playback/<ID>`, with `safari=true` for Safari and a separate `drm_policy=cast` request for casting. It consumes `playlist[0].sources`, optional tracks/image/title, and hands them to JW Player. After auth retry, the client classifies HTTP 401/403 as entitlement failures, 404 as unavailable, and 429 with `x-playback-route-limit` as its first-party rate limit; actual causes remain unverified. It retries once on HTTP 408 or server status >=500, or requests again after missing sources; network exceptions without numeric status are not covered by that retry.

No player-bootstrap, playback, stream or license request was performed. Actual signed URL format/lifetime, license policy, headers, codec/quality/audio tracks, CDM security level, permitted origins and target-TV entitlement remain unknown. JW's generic [DRM integration](https://docs.jwplayer.com/platform/docs/protection-studio-drm-jwx-web-player-integration) describes authenticated server signing and expiring source/license URLs; those examples do not establish Criterion's exact deployed policy. Publisher secrets never belong in the client, and DRM stays with the licensed platform implementation.

## Implementation boundary

The first Rust slice may consume the verified public catalog with TLS validation, bounded requests/cache, fixed origins, validated metadata and explicit network/error states. Authentication and player design need current official-app and target-device evidence before claiming parity. Preserve account tokens, cookies, personal data and signed source/license URLs outside Git and logs. Keep fixture-backed behavior separate from live provider, TV, physical-input and audible validation.

[Criterion's current terms](https://www.criterion.com/terms) reserve service/content rights and restrict reverse engineering and collecting/compiling service data. Public reachability does not establish a supported client license; provider authorization and distribution rights remain unresolved. This document records technical observations and published terms, without a legal conclusion.

# Public Provider Fixtures

These are bounded extracts from unauthenticated Criterion Channel JSON responses. [Provenance](provenance.json) owns exact URLs, response dates, download completion timestamps, raw-response SHA-256 hashes, extraction rules and admitted-file SHA-256 hashes. It also records the public homepage's portrait-image label/aspect metadata and one direct image HEAD; no image pixels are included. [The provider contract](../../../docs/provider-contract.md) explains the observed schema and its limits.

| File | Coverage |
| --- | --- |
| `all-films.json` | Minimal public catalog items and first continuation token. |
| `all-films-next.json` | Following the returned opaque token; distinct items and next token. |
| `all-films-filtered.json` | Genre filter plus descending duration sort. |
| `filters.json` | Current sort values and four filter groups; first three options per group plus two mixed-case/Unicode director identifiers. |
| `search.json` | Heterogeneous film/collection results and content-type counts. |
| `media-film.json` | Detailed film, string-array metadata and collection/category links. |
| `media-collection.json` | Generic primary playlist; at most three items per nested playlist. |
| `media-supplements.json` | Commentary label and supplement/collection/category lists; at most two items per nested playlist. |

Editorial descriptions, media sources/tracks and legacy Vimeo identifiers are removed, except the first 20 words of the film's `description_medium` for text-field parser coverage. Remaining values are public factual catalog metadata. No account, session, signed playback/license or private-device data is present. The extraction limits are test bounds, not provider behavior or complete catalog snapshots. Totals and availability are transient; tests must not treat captured cardinalities or windows as current service promises. Fixtures establish parser/behavior cases and never prove subscriber synchronization, licensed playback or TV operation.

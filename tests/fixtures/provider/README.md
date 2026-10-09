# Public Provider Fixtures

These are bounded extracts from unauthenticated Criterion Channel JSON responses and parsed discovery HTML. [Provenance](provenance.json) owns exact URLs, response dates, download completion timestamps, raw-response SHA-256 hashes, extraction rules and admitted-file SHA-256 hashes. It also records public portrait and editorial image metadata with direct HEAD receipts; no image pixels are included. [The provider contract](../../../docs/provider-contract.md) explains the observed schema and its limits.

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
| `discovery-home.json` / `.html` | All Home block/slide/navigation order and labels; first three media per static row. HTML retains the captured component envelope with projected blocks and two reencoded Flight pushes. |
| `discovery-new.json` / `.html` | All New block/slide/navigation order and labels; first three media per static row, including supplements. Same bounded transport projection. |
| `discovery-newly-added.json` | The actual See More destination's single supplied grid; first three items. |
| `media-popular-category.json` | Home Popular Movies' See More category and first three primary playlist items. |
| `auth-metadata.json` | Public Auth0 issuer/endpoints and advertised capabilities; no actual grant, client admission or credential response. |

Editorial descriptions/bodies, promotional video URLs, media sources/tracks and legacy Vimeo identifiers are removed, except the first 20 words of the film's `description_medium` for text-field parser coverage. Remaining values are public factual catalog metadata. No account, session, signed playback/license or private-device data is present. The extraction limits are test bounds, not provider behavior or complete catalog snapshots. Totals and availability are transient; tests must not treat captured cardinalities or windows as current service promises. Fixtures establish parser/behavior cases and never prove subscriber synchronization, licensed playback or TV operation.

The discovery HTML fixtures are JSON-reencoded bounded extracts, with no original rendered markup or upstream bundle code. They preserve the observed import/element references and split a projected record across two type-1 strings. Raw pages contain additional non-JSON Flight records and many more items; transport tests must treat omitted data as fixture limits. The fixtures establish a current web HTML parser seam, not the official TV app's transport or composition.

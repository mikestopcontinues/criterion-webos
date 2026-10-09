use criterion_provider::{
    BrowseRequest, Catalog, Error, Filter, FilterGroup, FilterValue, MediaId, MediaKind,
    PageCursor, PosterShape, Request, RequestTransport, Response, Sort, SortDirection, poster_url,
};

struct FixtureTransport;

struct BodyFixture(Vec<u8>);

impl RequestTransport for BodyFixture {
    async fn get(&self, _: Request) -> Result<Response, Error> {
        Ok(Response {
            status: 200,
            content_type: "application/json".into(),
            body: self.0.clone(),
        })
    }
}

impl RequestTransport for FixtureTransport {
    async fn get(&self, _: Request) -> Result<Response, Error> {
        Ok(Response {
            status: 200,
            content_type: "application/json".into(),
            body: include_bytes!("../../../tests/fixtures/provider/all-films.json").to_vec(),
        })
    }
}

#[tokio::test]
async fn browse_returns_the_public_catalog_title() {
    let page = Catalog::with_transport(FixtureTransport)
        .browse(&BrowseRequest::default())
        .await
        .unwrap();
    assert_eq!(page.items[0].title, "2 or 3 Things I Know About Her");
}

#[tokio::test]
async fn browse_rejects_a_response_with_an_invalid_media_identity() {
    let catalog = Catalog::with_transport(BodyFixture(br#"{"items":[{"contentType":"film","duration":5245,"mediaid":"../secret","release_date":"1967-01-01","title":"Secret"}],"paging":{"page_limit":2},"total":1}"#.to_vec()));
    assert_eq!(
        catalog.browse(&BrowseRequest::default()).await,
        Err(Error::InvalidResponse)
    );
}

struct CursorFixture;

impl RequestTransport for CursorFixture {
    async fn get(&self, request: Request) -> Result<Response, Error> {
        assert_eq!(
            request.url.origin().ascii_serialization(),
            "https://www.criterionchannel.com"
        );
        assert_eq!(request.url.path(), "/api/all-films/results");
        assert_eq!(
            request
                .url
                .query_pairs()
                .find(|(key, _)| key == "pagination_key")
                .map(|(_, value)| value.into_owned()),
            Some("opaque/+?=&#".into())
        );
        FixtureTransport.get(request).await
    }
}

#[tokio::test]
async fn browse_preserves_an_opaque_cursor_as_one_query_value() {
    let page = Catalog::with_transport(CursorFixture)
        .browse(&BrowseRequest {
            cursor: Some(PageCursor::new("opaque/+?=&#").unwrap()),
            ..BrowseRequest::default()
        })
        .await
        .unwrap();
    assert_eq!(page.items[0].title, "2 or 3 Things I Know About Her");
}

struct FilterFixture;

impl RequestTransport for FilterFixture {
    async fn get(&self, request: Request) -> Result<Response, Error> {
        let query: std::collections::BTreeMap<_, _> = request
            .url
            .query_pairs()
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect();
        assert_eq!(query.get("page_limit").map(String::as_str), Some("2"));
        assert_eq!(
            query.get("genres").map(String::as_str),
            Some("horror,thriller")
        );
        assert_eq!(query.get("sort").map(String::as_str), Some("duration"));
        assert_eq!(query.get("sortDir").map(String::as_str), Some("desc"));
        FixtureTransport.get(request).await
    }
}

#[tokio::test]
async fn browse_uses_the_verified_filter_and_sort_contract() {
    let request = BrowseRequest {
        page_limit: 2,
        sort: Sort::Duration,
        direction: SortDirection::Descending,
        filters: vec![
            Filter {
                group: FilterGroup::Genres,
                value: FilterValue::new("horror").unwrap(),
            },
            Filter {
                group: FilterGroup::Genres,
                value: FilterValue::new("thriller").unwrap(),
            },
        ],
        ..BrowseRequest::default()
    };
    assert!(
        Catalog::with_transport(FilterFixture)
            .browse(&request)
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn browse_preserves_display_metadata_and_the_next_page() {
    let page = Catalog::with_transport(FixtureTransport)
        .browse(&BrowseRequest::default())
        .await
        .unwrap();
    assert_eq!(page.total, 3041);
    assert_eq!(page.next_cursor, Some(PageCursor::new("2").unwrap()));
    assert_eq!(page.items[0].id.as_str(), "L5Z3RaiC");
    assert_eq!(page.items[0].kind, MediaKind::Film);
    assert_eq!(page.items[0].duration_seconds, 5245);
    assert_eq!(page.items[0].release_date.as_deref(), Some("1967-01-01"));
}

struct SearchFixture;

#[tokio::test]
async fn search_preserves_a_mixed_film_and_original_result_with_typed_counts() {
    // Synthetic bounded regression for the content type in the live Godard query.
    let body = br#"{"playlist":[{"mediaid":"ABCDEF01","title":"Film fixture","contentType":"film","duration":120},{"mediaid":"ABCDEF02","title":"Original fixture","contentType":"original","duration":71}],"type_counts":{"film":1,"original":1},"paging":{"page_limit":100}}"#;
    let results = Catalog::with_transport(BodyFixture(body.to_vec()))
        .search("Godard")
        .await
        .unwrap();
    assert_eq!(results.items.len(), 2);
    assert_eq!(results.items[0].kind, MediaKind::Film);
    assert_eq!(results.items[1].title, "Original fixture");
    assert_eq!(results.items[1].kind, MediaKind::Original);
    assert_eq!(results.items[1].duration_seconds, 71);
    assert_eq!(results.type_counts.len(), 2);
    assert!(results.type_counts.iter().all(|count| count.count == 1));
    assert!(
        results
            .type_counts
            .iter()
            .any(|count| count.kind == MediaKind::Original)
    );
}

#[tokio::test]
async fn search_admits_counts_for_all_known_catalog_kinds_and_rejects_unknown_kinds() {
    // Synthetic capacity check, not a claim that one observed query returns all kinds.
    let mut body = serde_json::json!({"playlist":[], "type_counts":{
        "film":1,"collection":1,"category":1,"supplement":1,"series":1,"original":1,"live":1
    }});
    let results = Catalog::with_transport(BodyFixture(serde_json::to_vec(&body).unwrap()))
        .search("Criterion")
        .await
        .unwrap();
    assert_eq!(results.type_counts.len(), 7);
    body["type_counts"]["original"] = serde_json::json!(1_000_001);
    assert_eq!(
        Catalog::with_transport(BodyFixture(serde_json::to_vec(&body).unwrap()))
            .search("Criterion")
            .await,
        Err(Error::InvalidResponse)
    );
    body["type_counts"] = serde_json::json!({"unexpected":1});
    assert_eq!(
        Catalog::with_transport(BodyFixture(serde_json::to_vec(&body).unwrap()))
            .search("Criterion")
            .await,
        Err(Error::InvalidResponse)
    );
}

#[tokio::test]
async fn original_detail_is_distinct_and_unknown_media_types_still_fail_closed() {
    let body = br#"{"mediaid":"ABCDEF02","title":"Original fixture","contentType":"original","duration":71}"#;
    let detail = Catalog::with_transport(BodyFixture(body.to_vec()))
        .detail(&MediaId::new("ABCDEF02").unwrap())
        .await
        .unwrap();
    assert_eq!(detail.media.kind, MediaKind::Original);
    assert_eq!(detail.media.duration_seconds, 71);
    assert!(detail.live_schedule.is_empty());
    let body = br#"{"playlist":[{"mediaid":"ABCDEF02","title":"Unknown fixture","contentType":"unreviewed","duration":71}],"type_counts":{"film":1}}"#;
    assert_eq!(
        Catalog::with_transport(BodyFixture(body.to_vec()))
            .search("Criterion")
            .await,
        Err(Error::InvalidResponse)
    );
}

impl RequestTransport for SearchFixture {
    async fn get(&self, request: Request) -> Result<Response, Error> {
        assert_eq!(request.url.path(), "/api/search");
        assert_eq!(
            request.url.query_pairs().collect::<Vec<_>>(),
            vec![("q".into(), "Hitcher &?=# 雪".into())]
        );
        Ok(Response { status: 200, content_type: "application/json".into(), body: br#"{"playlist":[{"mediaid":"qvwT6mJ4","title":"The Hitcher","contentType":"film","duration":5851,"release_date":"1986-01-01"},{"mediaid":"rqsQ0P3H","title":"Highway Horror","contentType":"collection","duration":0}],"type_counts":{"film":1,"collection":1},"paging":{"page_limit":100}}"#.to_vec() })
    }
}

#[tokio::test]
async fn search_returns_films_and_collections_with_an_encoded_query() {
    let results = Catalog::with_transport(SearchFixture)
        .search("Hitcher &?=# 雪")
        .await
        .unwrap();
    assert_eq!(results.items.len(), 2);
    assert_eq!(results.items[0].title, "The Hitcher");
    assert_eq!(results.items[1].kind, MediaKind::Collection);
    assert_eq!(results.items[1].duration_seconds, 0);
    assert_eq!(
        results
            .type_counts
            .iter()
            .find(|count| count.kind == MediaKind::Film)
            .unwrap()
            .count,
        1
    );
}

#[tokio::test]
async fn browse_options_return_typed_current_filters_and_sort_choices() {
    let options = Catalog::with_transport(BodyFixture(
        include_bytes!("../../../tests/fixtures/provider/filters.json").to_vec(),
    ))
    .options()
    .await
    .unwrap();
    assert_eq!(options.filter_groups.len(), 4);
    assert_eq!(options.filter_groups[0].group, FilterGroup::Genres);
    assert_eq!(
        options.filter_groups[0].options[0].label,
        "Action/Adventure"
    );
    assert_eq!(
        options.filter_groups[0].options[0].value.as_str(),
        "action-adventure"
    );
    assert_eq!(options.sort_options[3].sort, Sort::Country);
}

#[tokio::test]
async fn detail_preserves_supplements_commentary_and_category_metadata() {
    let detail = Catalog::with_transport(BodyFixture(
        include_bytes!("../../../tests/fixtures/provider/media-supplements.json").to_vec(),
    ))
    .detail(&MediaId::new("pH6QX6Zq").unwrap())
    .await
    .unwrap();
    assert_eq!(detail.media.title, "The Night of the Hunter");
    assert_eq!(detail.directors, vec!["Charles Laughton"]);
    assert_eq!(
        detail.commentary_tracks,
        vec!["Commentary: Terry Sanders, Robert Gitt, F. X. Feeney, and Preston Neal Jones"]
    );
    assert_eq!(detail.playlists[0].key, "playlist_supplements");
    assert_eq!(detail.playlists[0].items.len(), 2);
    assert_eq!(detail.playlists[0].items[0].kind, MediaKind::Supplement);
    assert_eq!(detail.playlists[2].items[0].kind, MediaKind::Category);
}

struct StatusFixture(u16);

impl RequestTransport for StatusFixture {
    async fn get(&self, request: Request) -> Result<Response, Error> {
        let mut response = FixtureTransport.get(request).await?;
        response.status = self.0;
        Ok(response)
    }
}

#[tokio::test]
async fn catalog_rejects_redirects_and_error_statuses_before_reading_catalog_data() {
    for status in [301, 302, 307, 401, 429, 503] {
        let result = Catalog::with_transport(StatusFixture(status))
            .browse(&BrowseRequest::default())
            .await;
        assert_eq!(result, Err(Error::HttpStatus(status)));
    }
}

#[tokio::test]
async fn catalog_accepts_the_exact_body_limit_and_rejects_one_more_byte() {
    let mut body = include_bytes!("../../../tests/fixtures/provider/all-films.json").to_vec();
    body.resize(2 * 1024 * 1024, b' ');
    assert!(
        Catalog::with_transport(BodyFixture(body.clone()))
            .browse(&BrowseRequest::default())
            .await
            .is_ok()
    );
    body.push(b' ');
    assert_eq!(
        Catalog::with_transport(BodyFixture(body))
            .browse(&BrowseRequest::default())
            .await,
        Err(Error::ResponseTooLarge)
    );
}

#[tokio::test]
async fn catalog_rejects_excessive_nesting_even_in_ignored_playback_fields() {
    let mut value: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../tests/fixtures/provider/all-films.json"
    ))
    .unwrap();
    let mut nested = serde_json::json!("SECRET_SOURCE_SENTINEL");
    for _ in 0..17 {
        nested = serde_json::json!([nested]);
    }
    value["sources"] = nested;
    assert_eq!(
        Catalog::with_transport(BodyFixture(serde_json::to_vec(&value).unwrap()))
            .browse(&BrowseRequest::default())
            .await,
        Err(Error::InvalidResponse)
    );
}

#[tokio::test]
async fn catalog_rejects_unbounded_display_strings_and_card_counts() {
    let base: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../tests/fixtures/provider/all-films.json"
    ))
    .unwrap();
    let mut long_title = base.clone();
    long_title["items"][0]["title"] = serde_json::json!("t".repeat(513));
    let mut too_many_cards = base.clone();
    too_many_cards["items"] = serde_json::json!(vec![base["items"][0].clone(); 101]);
    for body in [long_title, too_many_cards] {
        assert_eq!(
            Catalog::with_transport(BodyFixture(serde_json::to_vec(&body).unwrap()))
                .browse(&BrowseRequest::default())
                .await,
            Err(Error::InvalidResponse)
        );
    }
}

#[test]
fn filter_values_preserve_case_and_unicode_from_the_current_provider() {
    assert_eq!(
        FilterValue::new("González-juan-pablo").unwrap().as_str(),
        "González-juan-pablo"
    );
    assert_eq!(
        FilterValue::new("jia-zhang-Ke").unwrap().as_str(),
        "jia-zhang-Ke"
    );
}

struct NeverTransport;

impl RequestTransport for NeverTransport {
    async fn get(&self, _: Request) -> Result<Response, Error> {
        panic!("invalid request reached transport")
    }
}

#[tokio::test]
async fn catalog_rejects_invalid_requests_before_transport() {
    let catalog = Catalog::with_transport(NeverTransport);
    for limit in [0, 101] {
        assert_eq!(
            catalog
                .browse(&BrowseRequest {
                    page_limit: limit,
                    ..BrowseRequest::default()
                })
                .await,
            Err(Error::InvalidRequest)
        );
    }
    for query in ["", "   ", "bad\u{0}query"] {
        assert_eq!(catalog.search(query).await, Err(Error::InvalidRequest));
    }
    assert_eq!(
        catalog.search(&"s".repeat(257)).await,
        Err(Error::InvalidRequest)
    );
}

#[test]
fn identifiers_cursors_and_filters_reject_invalid_input_without_retaining_it() {
    for id in [
        "",
        "1234567",
        "123456789",
        "../token",
        "12%2f345",
        "a?secret",
        "雪123456",
    ] {
        assert_eq!(MediaId::new(id), Err(Error::InvalidRequest));
    }
    for cursor in ["", "secret\n"] {
        assert_eq!(PageCursor::new(cursor), Err(Error::InvalidRequest));
    }
    assert_eq!(
        PageCursor::new(&"s".repeat(513)),
        Err(Error::InvalidRequest)
    );
    for filter in ["", "horror,token", "horror&token", "a/b", "a_token"] {
        assert_eq!(FilterValue::new(filter), Err(Error::InvalidRequest));
    }
    assert!(
        !format!("{:?}", PageCursor::new("TOKEN_SENTINEL").unwrap()).contains("TOKEN_SENTINEL")
    );
}

struct MimeFixture(&'static str);

impl RequestTransport for MimeFixture {
    async fn get(&self, request: Request) -> Result<Response, Error> {
        let mut response = FixtureTransport.get(request).await?;
        response.content_type = self.0.into();
        Ok(response)
    }
}

#[tokio::test]
async fn catalog_accepts_json_content_type_parameters_and_rejects_other_types() {
    assert!(
        Catalog::with_transport(MimeFixture("application/json; charset=utf-8"))
            .browse(&BrowseRequest::default())
            .await
            .is_ok()
    );
    for mime in ["", "text/html", "text/plain", "application/jsonp"] {
        assert_eq!(
            Catalog::with_transport(MimeFixture(mime))
                .browse(&BrowseRequest::default())
                .await,
            Err(Error::InvalidResponse)
        );
    }
}

#[tokio::test]
async fn catalog_rejects_invalid_dates_durations_totals_and_detail_resource_counts() {
    let browse: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../tests/fixtures/provider/all-films.json"
    ))
    .unwrap();
    let mut bad_date = browse.clone();
    bad_date["items"][0]["release_date"] = serde_json::json!("2026-02-31");
    let mut bad_duration = browse.clone();
    bad_duration["items"][0]["duration"] = serde_json::json!(4294967295_u32);
    let mut bad_total = browse;
    bad_total["total"] = serde_json::json!(4294967295_u32);
    for value in [bad_date, bad_duration, bad_total] {
        assert_eq!(
            Catalog::with_transport(BodyFixture(serde_json::to_vec(&value).unwrap()))
                .browse(&BrowseRequest::default())
                .await,
            Err(Error::InvalidResponse)
        );
    }
    let detail: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../tests/fixtures/provider/media-film.json"
    ))
    .unwrap();
    let mut too_many_names = detail.clone();
    too_many_names["director"] = serde_json::json!(vec!["Director"; 129]);
    let mut too_many_playlists = detail.clone();
    too_many_playlists["playlists"] = serde_json::json!(vec![detail["playlists"][0].clone(); 33]);
    for value in [too_many_names, too_many_playlists] {
        assert_eq!(
            Catalog::with_transport(BodyFixture(serde_json::to_vec(&value).unwrap()))
                .detail(&MediaId::new("qvwT6mJ4").unwrap())
                .await,
            Err(Error::InvalidResponse)
        );
    }
}

#[test]
fn poster_urls_use_verified_fixed_origins_and_explicit_shapes() {
    assert_eq!(
        poster_url(&MediaId::new("ie7lqmcm").unwrap(), PosterShape::Portrait)
            .unwrap()
            .as_str(),
        "https://img.jwplayer.com/v1/media/ie7lqmcm/images/default_2x3.webp?width=480"
    );
    assert_eq!(
        poster_url(&MediaId::new("qvwT6mJ4").unwrap(), PosterShape::Landscape)
            .unwrap()
            .as_str(),
        "https://img.jwplayer.com/v1/media/qvwT6mJ4/images/default_16x9.webp?width=480"
    );
}

#[tokio::test]
async fn detail_uses_the_medium_description_when_long_text_is_blank() {
    let mut detail: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../tests/fixtures/provider/media-film.json"
    ))
    .unwrap();
    detail["description_long"] = serde_json::json!(" \n\t ");
    let result = Catalog::with_transport(BodyFixture(serde_json::to_vec(&detail).unwrap()))
        .detail(&MediaId::new("qvwT6mJ4").unwrap())
        .await
        .unwrap();
    assert_eq!(
        result.description.as_deref(),
        Some(
            "A young man is plunged into a grisly game of cat and mouse when he picks up a deranged hitchhiker"
        )
    );
}

#[tokio::test]
async fn diagnostics_and_errors_do_not_disclose_provider_payloads() {
    use std::error::Error as _;
    let mut detail: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../tests/fixtures/provider/media-film.json"
    ))
    .unwrap();
    detail["description_long"] = serde_json::json!("PRIVATE_DIAGNOSTIC_SENTINEL");
    detail["sources"] =
        serde_json::json!([{"file":"https://media.example/?token=PRIVATE_SOURCE_SENTINEL"}]);
    let result = Catalog::with_transport(BodyFixture(serde_json::to_vec(&detail).unwrap()))
        .detail(&MediaId::new("qvwT6mJ4").unwrap())
        .await
        .unwrap();
    assert_eq!(
        result.description.as_deref(),
        Some("PRIVATE_DIAGNOSTIC_SENTINEL")
    );
    let diagnostic = format!("{result:?}");
    for secret in [
        "PRIVATE_DIAGNOSTIC_SENTINEL",
        "PRIVATE_SOURCE_SENTINEL",
        "qvwT6mJ4",
    ] {
        assert!(!diagnostic.contains(secret));
    }
    detail["contentType"] = serde_json::json!("PRIVATE_ERROR_SENTINEL");
    let error = Catalog::with_transport(BodyFixture(serde_json::to_vec(&detail).unwrap()))
        .detail(&MediaId::new("qvwT6mJ4").unwrap())
        .await
        .unwrap_err();
    assert_eq!(error, Error::InvalidResponse);
    assert!(!format!("{error} {error:?}").contains("PRIVATE_ERROR_SENTINEL"));
    assert!(error.source().is_none());
}

#[tokio::test]
async fn empty_search_and_optional_detail_fields_are_valid() {
    let search = Catalog::with_transport(BodyFixture(
        br#"{"playlist":[],"type_counts":{},"paging":{"page_limit":100}}"#.to_vec(),
    ))
    .search("absent")
    .await
    .unwrap();
    assert!(search.items.is_empty());
    let detail = Catalog::with_transport(BodyFixture(
        br#"{"mediaid":"qvwT6mJ4","title":"The Hitcher","contentType":"film","duration":5851}"#
            .to_vec(),
    ))
    .detail(&MediaId::new("qvwT6mJ4").unwrap())
    .await
    .unwrap();
    assert!(detail.description.is_none());
    assert!(detail.playlists.is_empty());
    assert!(detail.media.release_date.is_none());
}

#[tokio::test]
async fn detail_rejects_a_different_media_identity_and_malformed_schema() {
    assert_eq!(
        Catalog::with_transport(BodyFixture(
            include_bytes!("../../../tests/fixtures/provider/media-film.json").to_vec()
        ))
        .detail(&MediaId::new("pH6QX6Zq").unwrap())
        .await,
        Err(Error::InvalidResponse)
    );
    for body in [br#"[]"#.as_slice(), br#"{"items":null,"total":0,"paging":{}}"#, br#"{"items":[{"mediaid":"qvwT6mJ4","title":"The Hitcher","contentType":"film","duration":-1}],"total":1,"paging":{}}"#] {
        assert_eq!(Catalog::with_transport(BodyFixture(body.to_vec())).browse(&BrowseRequest::default()).await, Err(Error::InvalidResponse));
    }
}

#[tokio::test]
async fn ignored_json_strings_and_nesting_have_exact_bounded_edges() {
    let base: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../tests/fixtures/provider/all-films.json"
    ))
    .unwrap();
    let mut value = base.clone();
    value["ignored"] = serde_json::json!("s".repeat(65_536));
    assert!(
        Catalog::with_transport(BodyFixture(serde_json::to_vec(&value).unwrap()))
            .browse(&BrowseRequest::default())
            .await
            .is_ok()
    );
    value["ignored"] = serde_json::json!("s".repeat(65_537));
    assert_eq!(
        Catalog::with_transport(BodyFixture(serde_json::to_vec(&value).unwrap()))
            .browse(&BrowseRequest::default())
            .await,
        Err(Error::InvalidResponse)
    );
    value["ignored"] = serde_json::json!(r#"quotes \" backslash \\ braces [[{{ }}}]] escaped"#);
    assert!(
        Catalog::with_transport(BodyFixture(serde_json::to_vec(&value).unwrap()))
            .browse(&BrowseRequest::default())
            .await
            .is_ok()
    );
    let mut nested = serde_json::json!("bounded");
    for _ in 0..15 {
        nested = serde_json::json!([nested]);
    }
    value["ignored"] = nested.clone();
    assert!(
        Catalog::with_transport(BodyFixture(serde_json::to_vec(&value).unwrap()))
            .browse(&BrowseRequest::default())
            .await
            .is_ok()
    );
    value["ignored"] = serde_json::json!([nested]);
    assert_eq!(
        Catalog::with_transport(BodyFixture(serde_json::to_vec(&value).unwrap()))
            .browse(&BrowseRequest::default())
            .await,
        Err(Error::InvalidResponse)
    );
    value = base;
    value["ignored"] = serde_json::json!(vec![0; 33_000]);
    assert_eq!(
        Catalog::with_transport(BodyFixture(serde_json::to_vec(&value).unwrap()))
            .browse(&BrowseRequest::default())
            .await,
        Err(Error::InvalidResponse)
    );
}

#[tokio::test]
async fn search_normalizes_edge_whitespace_before_requesting_results() {
    assert!(
        Catalog::with_transport(SearchFixture)
            .search("  Hitcher &?=# 雪  ")
            .await
            .is_ok()
    );
}

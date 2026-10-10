//! Synthetic contract fixtures derived from signed TV 11.0.18 Detail receipts.
//! These exercise the public client and establish no live provider admission.
//! Contract SHA256: af8258d51bbea3427702a4c01a3a84e5df1fe2da784627a64b84e51b30119bb1.
use crate::*;
use criterion_provider::MediaId;
use std::sync::atomic::{AtomicUsize, Ordering};

const INIT: &[u8] = br#"{"country":"CA","token":"synthetic-bootstrap","baseUrl":{"us":"https://mw.criterion.com/api/us","ca":"https://mw.criterion.com/api/ca"}}"#;
struct DetailFixture {
    body: Vec<u8>,
    status: u16,
    calls: AtomicUsize,
}
impl Transport for DetailFixture {
    async fn send(&self, _request: Request) -> Result<Response, Error> {
        if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            return Ok(Response {
                status: 200,
                body: SecretBody::new(INIT.to_vec()),
            });
        }
        Ok(Response {
            status: self.status,
            body: SecretBody::new(self.body.clone()),
        })
    }
}
async fn read_status(body: &[u8], status: u16) -> Result<NativeDetail, Error> {
    let client = AccountClient::with_transport(DetailFixture {
        body: body.to_vec(),
        status,
        calls: AtomicUsize::new(0),
    });
    client.bootstrap().await.unwrap();
    client.detail(&MediaId::new("Film0001").unwrap()).await
}
async fn read(body: &str) -> Result<NativeDetail, Error> {
    read_status(body.as_bytes(), 200).await
}

#[tokio::test]
async fn native_detail_public_read_preserves_direct_film_identity_and_kind() {
    let detail = read(r#"{"contentType":"film","mediaid":"Film0001","title":"Fixture film"}"#)
        .await
        .unwrap();
    assert_eq!(detail.media.id.as_str(), "Film0001");
    assert_eq!(detail.media.title, "Fixture film");
    assert_eq!(detail.media.kind, MediaKind::Film);
    assert!(detail.playlists.is_empty());
}

#[tokio::test]
async fn native_detail_preserves_raw_film_metadata_without_description_preference() {
    let detail = read(r#"{"contentType":"film","mediaid":"Film0001","title":"Fixture film","description":"Short","description_long":"First\nSecond","description_medium":"Medium","description_pull_quote":"Quote","description_staff":"Staff","director":["Director"],"genre":"Drama","playlist_supplements":"Supp0001","logo":true,"is_first_tab_sortable":false}"#).await.unwrap();
    assert_eq!(detail.metadata.description.as_deref(), Some("Short"));
    assert_eq!(
        detail.metadata.description_long.as_deref(),
        Some("First\nSecond")
    );
    assert_eq!(
        detail.metadata.description_medium.as_deref(),
        Some("Medium")
    );
    assert_eq!(
        detail.metadata.description_pull_quote.as_deref(),
        Some("Quote")
    );
    assert_eq!(detail.metadata.description_staff.as_deref(), Some("Staff"));
    assert_eq!(detail.metadata.director, Some(vec!["Director".into()]));
    assert_eq!(detail.metadata.genre.as_deref(), Some("Drama"));
    assert_eq!(
        detail.metadata.playlist_supplements.as_deref(),
        Some("Supp0001")
    );
    assert_eq!(detail.metadata.logo, Some(true));
    assert_eq!(detail.is_first_tab_sortable, Some(false));
    assert!(
        detail
            .metadata
            .description_long
            .as_ref()
            .unwrap()
            .contains('\n')
    );
    assert!(!format!("{detail:?} {:?}", detail.metadata).contains("First"));
}

#[tokio::test]
async fn native_detail_metadata_respects_each_native_kind_and_nullable_fields() {
    for (kind, expected) in [
        ("category", MediaKind::Category),
        ("collection", MediaKind::Collection),
        ("series", MediaKind::Series),
        ("original", MediaKind::Original),
        ("episode", MediaKind::Episode),
        ("franchise", MediaKind::Franchise),
        ("live", MediaKind::Live),
        ("film", MediaKind::Film),
        ("supplement", MediaKind::Supplement),
    ] {
        let body = format!(
            r#"{{"contentType":"{kind}","mediaid":"Film0001","title":"Native","description":"Raw","deeplink":null}}"#
        );
        let detail = read(&body).await.unwrap();
        assert_eq!(detail.media.kind, expected);
        assert_eq!(detail.metadata.description.as_deref(), Some("Raw"));
        assert_eq!(detail.metadata.deeplink, None);
        assert_eq!(detail.media.id.as_str(), "Film0001");
    }
    let series = read(r#"{"contentType":"series","mediaid":"Film0001","title":"Series","director":null,"starring":[],"trailer":null,"introduction_primary":null,"description_staff":""}"#).await.unwrap();
    assert_eq!(series.metadata.director, None);
    assert_eq!(series.metadata.starring, Some(Vec::new()));
    assert_eq!(series.metadata.description_staff.as_deref(), Some(""));
    let original = read(r#"{"contentType":"original","mediaid":"Film0001","title":"Original","franchise_id":"Franch01","introduction_primary":"","genre":null}"#).await.unwrap();
    assert_eq!(original.metadata.franchise_id.as_deref(), Some("Franch01"));
    assert_eq!(original.metadata.introduction_primary.as_deref(), Some(""));
    assert_eq!(original.metadata.genre, None);
    let collection = read(r#"{"contentType":"collection","mediaid":"Film0001","title":"Collection","collection_count":-7,"teaser":"Teaser"}"#).await.unwrap();
    assert_eq!(collection.metadata.collection_count, Some(-7));
    assert_eq!(collection.metadata.teaser.as_deref(), Some("Teaser"));
    let live = read(r#"{"contentType":"live","mediaid":"Film0001","title":"Live","paywall":"subscription","schedule":{"uninterpreted":true},"description_long":null}"#).await.unwrap();
    assert_eq!(live.metadata.paywall.as_deref(), Some("subscription"));
    assert_eq!(live.metadata.description_long, None);
}

#[tokio::test]
async fn native_detail_episode_association_is_optional_nullable_and_separate_from_identity() {
    // Eight ASCII media/series identifiers are our local admission policy;
    // the native descriptor itself proves required String identity only.
    for (suffix, expected_title) in [
        ("", None),
        (r#", "series_id":null,"series_title":null"#, None),
        (r#", "series_title":"""#, Some("")),
    ] {
        let body = format!(
            r#"{{"contentType":"episode","mediaid":"Film0001","title":"Episode"{suffix}}}"#
        );
        let episode = read(&body).await.unwrap();
        assert_eq!(episode.media.kind, MediaKind::Episode);
        assert_eq!(episode.media.id.as_str(), "Film0001");
        assert_eq!(episode.media.series_id, None);
        assert_eq!(episode.media.series_title.as_deref(), expected_title);
    }
    let episode = read(r#"{"contentType":"episode","mediaid":"Film0001","title":"Episode","series_id":"Series01","series_title":"Series","director":null,"starring":[]}"#).await.unwrap();
    assert_eq!(
        episode.media.series_id.as_ref().unwrap().as_str(),
        "Series01"
    );
    assert_eq!(episode.media.series_title.as_deref(), Some("Series"));
    for field in [
        r#""series_id":"bad""#,
        r#""series_id":42"#,
        r#""series_title":true"#,
        r#""series_title":"bad\nlabel""#,
    ] {
        let body = format!(
            r#"{{"contentType":"episode","mediaid":"Film0001","title":"Episode",{field}}}"#
        );
        assert_eq!(read(&body).await, Err(Error::InvalidResponse));
    }
    let film = read(r#"{"contentType":"film","mediaid":"Film0001","title":"Film","series_id":"bad","series_title":false}"#).await.unwrap();
    assert_eq!(film.media.series_id, None);
    assert_eq!(film.media.series_title, None);
}

#[tokio::test]
async fn native_detail_generic_and_featured_children_keep_first_exact_id_per_list() {
    let detail = read(r#"{"contentType":"collection","mediaid":"Film0001","title":"Collection","collection_count":99,"playlists":[{"type":"GENERIC_PLAYLIST","title":"Related","playlistId":"opaque:id","key":"playlist_related","playlist":[{"contentType":"film","mediaid":"Child001","title":"First"},{"contentType":"supplement","mediaid":"Child001","title":"Duplicate"},{"contentType":"episode","mediaid":"Child002","title":"Second","series_id":"Series01","series_title":""}]},{"type":"GENERIC_PLAYLIST","title":"","playlistId":"","key":"playlist_supplements","playlist":[{"contentType":"supplement","mediaid":"Child001","title":"Own list"}]}],"featured":{"title":"Featured","playlist":[{"contentType":"franchise","mediaid":"Child001","title":"Featured first"},{"contentType":"film","mediaid":"Child001","title":"Featured duplicate"}]}}"#).await.unwrap();
    assert_eq!(detail.metadata.collection_count, Some(99));
    assert_eq!(detail.playlists.len(), 2);
    let NativePlaylist::Generic(first) = &detail.playlists[0] else {
        panic!("expected Generic")
    };
    assert_eq!(first.title, "Related");
    assert_eq!(first.playlist_id, "opaque:id");
    assert_eq!(first.key, NativePlaylistKey::Related);
    assert_eq!(first.raw_child_count, 3);
    assert_eq!(first.children.len(), 2);
    assert_eq!(first.children[0].title, "First");
    assert_eq!(first.children[0].kind, MediaKind::Film);
    assert_eq!(first.children[1].kind, MediaKind::Episode);
    assert_eq!(
        first.children[1].series_id.as_ref().unwrap().as_str(),
        "Series01"
    );
    assert_eq!(first.children[1].series_title.as_deref(), Some(""));
    let NativePlaylist::Generic(second) = &detail.playlists[1] else {
        panic!("expected Generic")
    };
    assert_eq!(second.title, "");
    assert_eq!(second.playlist_id, "");
    assert_eq!(second.key, NativePlaylistKey::Supplements);
    assert_eq!(second.children[0].title, "Own list");
    let featured = detail.featured.as_ref().unwrap();
    assert_eq!(featured.title.as_deref(), Some("Featured"));
    assert_eq!(featured.raw_child_count, 2);
    assert_eq!(featured.children.len(), 1);
    assert_eq!(featured.children[0].kind, MediaKind::Franchise);
    assert_eq!(featured.children[0].title, "Featured first");
    assert!(detail.estimated_bytes() > std::mem::size_of::<NativeDetail>());
}

#[tokio::test]
async fn native_detail_series_selects_first_seasons_then_ordered_generic_tabs() {
    let detail = read(r#"{"contentType":"series","mediaid":"Film0001","title":"Series","playlists":[{"type":"GENERIC_PLAYLIST","title":"Related","playlistId":"related"},{"type":"seasons","title":"Seasons A","playlist":[{"season_number":2,"season_title":"Second season","season_description":"Raw\nseason","episode_count":77,"episodes":[{"mediaid":"Child001","title":"First episode","series_id":"Series01","series_title":"","duration":90.5},{"mediaid":"Child001","title":"Duplicate episode","series_id":null,"series_title":null},{"mediaid":"Child002","title":"Second episode"}]}]},{"type":"seasons","title":"Seasons B","playlist":[{"season_number":-1,"season_title":"","episode_count":99}]},{"type":"GENERIC_PLAYLIST","title":"Supplements","playlistId":"supplements","key":"playlist_supplements"}]}"#).await.unwrap();
    assert_eq!(detail.playlists.len(), 4);
    let NativePlaylist::Seasons(first) = &detail.playlists[1] else {
        panic!("expected Seasons")
    };
    assert_eq!(first.title, "Seasons A");
    assert_eq!(first.seasons[0].number, 2);
    assert_eq!(first.seasons[0].description.as_deref(), Some("Raw\nseason"));
    assert_eq!(first.seasons[0].episode_count, 77);
    assert_eq!(first.seasons[0].raw_episode_count, 3);
    assert_eq!(first.seasons[0].episodes.len(), 2);
    assert_eq!(first.seasons[0].episodes[0].kind, MediaKind::Episode);
    assert_eq!(first.seasons[0].episodes[0].title, "First episode");
    assert_eq!(first.seasons[0].episodes[0].duration, Some(90.5));
    assert_eq!(
        first.seasons[0].episodes[0]
            .series_id
            .as_ref()
            .unwrap()
            .as_str(),
        "Series01"
    );
    assert_eq!(
        first.seasons[0].episodes[0].series_title.as_deref(),
        Some("")
    );
    assert_eq!(first.seasons[0].episodes[1].series_id, None);
    let NativePlaylist::Seasons(later) = &detail.playlists[2] else {
        panic!("expected Seasons")
    };
    assert_eq!(later.seasons[0].episode_count, 99);
    assert_eq!(later.seasons[0].raw_episode_count, 0);
    assert_eq!(later.seasons[0].title, "");
    let displayed: Vec<_> = detail.series_display_playlists().collect();
    assert_eq!(displayed.len(), 3);
    assert!(std::ptr::eq(displayed[0], &detail.playlists[1]));
    assert!(std::ptr::eq(displayed[1], &detail.playlists[0]));
    assert!(std::ptr::eq(displayed[2], &detail.playlists[3]));
}

#[tokio::test]
async fn native_detail_aggregate_limits_reject_before_dedup_without_truncation() {
    let child = serde_json::json!({"contentType":"film","mediaid":"Child001","title":"Child"});
    let generic = |count: usize| serde_json::json!({"type":"GENERIC_PLAYLIST","title":"","playlistId":"","playlist":vec![child.clone();count]});
    let root = |playlists: serde_json::Value| serde_json::json!({"contentType":"series","mediaid":"Film0001","title":"S","playlists":playlists});
    let accepted = read(&root(serde_json::json!([generic(511)])).to_string())
        .await
        .unwrap();
    let NativePlaylist::Generic(first) = &accepted.playlists[0] else {
        panic!("expected Generic")
    };
    assert_eq!(first.raw_child_count, 511);
    assert_eq!(first.children.len(), 1);
    assert_eq!(
        read(&root(serde_json::json!([generic(512)])).to_string()).await,
        Err(Error::InvalidResponse)
    );
    assert_eq!(
        read(&root(serde_json::json!([generic(256), generic(256)])).to_string()).await,
        Err(Error::InvalidResponse)
    );
    let empty = generic(0);
    assert_eq!(
        read(&root(serde_json::json!(vec![empty.clone(); 32])).to_string())
            .await
            .unwrap()
            .playlists
            .len(),
        32
    );
    assert_eq!(
        read(&root(serde_json::json!(vec![empty; 33])).to_string()).await,
        Err(Error::InvalidResponse)
    );
    let season = serde_json::json!({"season_number":1,"season_title":""});
    let seasons = |count: usize| serde_json::json!({"type":"seasons","title":"","playlist":vec![season.clone();count]});
    assert_eq!(
        read(&root(serde_json::json!([seasons(32), seasons(32)])).to_string())
            .await
            .unwrap()
            .playlists
            .len(),
        2
    );
    assert_eq!(
        read(&root(serde_json::json!([seasons(32), seasons(33)])).to_string()).await,
        Err(Error::InvalidResponse)
    );
}

#[tokio::test]
async fn native_detail_raw_unknown_text_and_depth_have_inclusive_global_bounds() {
    // String values include kind/id/title; field names are separately body-capped.
    let allowed = 256 * 1024 - 4 - 8 - 1;
    let root = |unknown: serde_json::Value| serde_json::json!({"contentType":"film","mediaid":"Film0001","title":"F","unknown":unknown});
    assert!(
        read(&root(serde_json::json!("x".repeat(allowed))).to_string())
            .await
            .is_ok()
    );
    assert_eq!(
        read(&root(serde_json::json!("x".repeat(allowed + 1))).to_string()).await,
        Err(Error::InvalidResponse)
    );
    assert_eq!(
        read(
            &root(serde_json::json!([
                "x".repeat(allowed / 2 + 1),
                "y".repeat(allowed / 2 + 1)
            ]))
            .to_string()
        )
        .await,
        Err(Error::InvalidResponse)
    );
    let mut nested = serde_json::json!(null);
    for _ in 0..63 {
        nested = serde_json::json!({"nested":nested});
    }
    assert!(read(&root(nested.clone()).to_string()).await.is_ok());
    nested = serde_json::json!({"nested":nested});
    assert_eq!(
        read(&root(nested).to_string()).await,
        Err(Error::InvalidResponse)
    );
    let mut oversized =
        br#"{"contentType":"film","mediaid":"Film0001","title":"F","ignored":""#.to_vec();
    oversized.resize(512 * 1024, b'x');
    oversized.extend_from_slice(b"\"}");
    assert_eq!(
        read_status(&oversized, 200).await,
        Err(Error::ResponseTooLarge)
    );
    assert_eq!(
        read_status(&oversized, 503).await,
        Err(Error::HttpStatus(503))
    );
    let mut exact_body = br#"{"contentType":"film","mediaid":"Film0001","title":"F"}"#.to_vec();
    exact_body.resize(512 * 1024, b' ');
    assert!(read_status(&exact_body, 200).await.is_ok());
}

#[tokio::test]
async fn native_detail_rejects_owned_schema_errors_and_direct_identity_mismatch() {
    for body in [
        r#"["film","Film0001","Film"]"#,
        r#"{"data":{"contentType":"film","mediaid":"Film0001","title":"Film"}}"#,
        r#"{"contentType":"unknown","mediaid":"Film0001","title":"Film"}"#,
        r#"{"contentType":"film","mediaid":"Other001","title":"Film"}"#,
        r#"{"contentType":"film","mediaid":"Film0001","title":null}"#,
        r#"{"contentType":"film","mediaid":"Film0001","title":""}"#,
        r#"{"contentType":"film","mediaid":"Film0001","title":"Film","description":null}"#,
        r#"{"contentType":"film","mediaid":"Film0001","title":"Film","description":"one","description":"two"}"#,
        r#"{"contentType":"film","contentType":"film","mediaid":"Film0001","title":"Film"}"#,
        r#"{"contentType":"film","mediaid":"Film0001","title":"Film","director":null}"#,
        r#"{"contentType":"series","mediaid":"Film0001","title":"Series","country":null}"#,
        r#"{"contentType":"film","mediaid":"Film0001","title":"Film","starring":[null]}"#,
        r#"{"contentType":"episode","mediaid":"Film0001","title":"Episode","duration":null}"#,
        r#"{"contentType":"film","mediaid":"Film0001","title":"Film","logo":null}"#,
        r#"{"contentType":"live","mediaid":"Film0001","title":"Live","paywall":null}"#,
    ] {
        assert_eq!(read(body).await, Err(Error::InvalidResponse));
    }
    let ignored = read(r#"{"title":"Film","mediaid":"Film0001","rating":{"unproved":true},"paywall":null,"license_end_date_time":false,"source":"private-source","license":"private-license","contentType":"film"}"#).await.unwrap();
    assert_eq!(ignored.metadata, NativeDetailMetadata::default());
    assert!(!format!("{ignored:?} {:?}", ignored.metadata).contains("private"));
}

#[tokio::test]
async fn native_detail_nested_objects_required_fields_and_nonnullable_lists_are_exact() {
    let root = |fragment: &str| {
        format!(
            r#"{{"contentType":"collection","mediaid":"Film0001","title":"Collection",{fragment}}}"#
        )
    };
    for fragment in [
        r#""playlists":{}"#,
        r#""playlists":[["GENERIC_PLAYLIST","Title","id"]]"#,
        r#""playlists":[{"type":"unknown","title":"Title","playlistId":"id"}]"#,
        r#""playlists":[{"type":"GENERIC_PLAYLIST","playlistId":"id"}]"#,
        r#""playlists":[{"type":"GENERIC_PLAYLIST","title":"Title","playlistId":null}]"#,
        r#""playlists":[{"type":"GENERIC_PLAYLIST","title":"Title","title":"Duplicate","playlistId":"id"}]"#,
        r#""playlists":[{"type":"GENERIC_PLAYLIST","title":"Title","playlistId":"id","key":"other"}]"#,
        r#""playlists":[{"type":"GENERIC_PLAYLIST","title":"Title","playlistId":"id","key":null}]"#,
        r#""playlists":[{"type":"GENERIC_PLAYLIST","title":"Title","playlistId":"id","playlist":null}]"#,
        r#""playlists":[{"type":"GENERIC_PLAYLIST","title":"Title","playlistId":"id","playlist":[{"mediaid":"Child001","title":"Missing tagged kind"}]}]"#,
        r#""playlists":[{"type":"seasons","title":"Seasons"}]"#,
        r#""playlists":[{"type":"seasons","title":"Seasons","playlist":null}]"#,
        r#""playlists":[{"type":"seasons","title":"Seasons","playlist":[{"season_title":"Missing number"}]}]"#,
        r#""playlists":[{"type":"seasons","title":"Seasons","playlist":[{"season_number":1,"season_title":null}]}]"#,
        r#""playlists":[{"type":"seasons","title":"Seasons","playlist":[{"season_number":1,"season_title":"","episodes":null}]}]"#,
        r#""playlists":[{"type":"seasons","title":"Seasons","playlist":[{"season_number":1,"season_title":"","episode_count":null}]}]"#,
        r#""playlists":[{"type":"seasons","title":"Seasons","playlist":[{"season_number":1,"season_title":"","episodes":[{"mediaid":"Child001","title":"Episode","series_id":"bad"}]}]}]"#,
        r#""featured":[]"#,
        r#""featured":{"title":null}"#,
        r#""featured":{"playlist":null}"#,
    ] {
        assert_eq!(read(&root(fragment)).await, Err(Error::InvalidResponse));
    }
    let absent = read(&root(r#""playlists":null,"featured":null"#))
        .await
        .unwrap();
    assert!(absent.playlists.is_empty());
    assert!(absent.featured.is_none());
    let empty = read(&root(
        r#""playlists":[{"title":"","playlistId":"","type":"GENERIC_PLAYLIST"}],"featured":{}"#,
    ))
    .await
    .unwrap();
    let NativePlaylist::Generic(generic) = &empty.playlists[0] else {
        panic!("expected Generic")
    };
    assert_eq!(generic.key, NativePlaylistKey::Other);
    assert_eq!(generic.raw_child_count, 0);
    assert_eq!(empty.featured.as_ref().unwrap().title, None);
    for (wire, expected) in [
        (
            "playlist_films_appears_in",
            NativePlaylistKey::FilmsAppearsIn,
        ),
        ("playlist_supplements", NativePlaylistKey::Supplements),
        (
            "playlist_collections_appears_in",
            NativePlaylistKey::CollectionsAppearsIn,
        ),
        (
            "playlist_categories_appears_in",
            NativePlaylistKey::CategoriesAppearsIn,
        ),
        ("playlist_primary", NativePlaylistKey::Primary),
        ("playlist_collections", NativePlaylistKey::Collections),
        ("playlist_related", NativePlaylistKey::Related),
        ("playlist_other", NativePlaylistKey::Other),
    ] {
        let fragment = format!(
            r#""playlists":[{{"type":"GENERIC_PLAYLIST","title":"","playlistId":"","key":"{wire}"}}]"#
        );
        let detail = read(&root(&fragment)).await.unwrap();
        let NativePlaylist::Generic(generic) = &detail.playlists[0] else {
            panic!("expected Generic")
        };
        assert_eq!(generic.key, expected);
    }
}

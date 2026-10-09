use criterion_provider::{
    Catalog, ContentTarget, DiscoveryBlock, DiscoveryRoute, EditorialImage, Error, GalleryLayout,
    ImageLabel, MAX_RESPONSE_BYTES, MediaKind, MediaRoute, RailSource, Request, RequestTransport,
    Response, Slug,
};

fn blocks_fixture() -> serde_json::Value {
    serde_json::from_str::<serde_json::Value>(include_str!(
        "../../../tests/fixtures/provider/discovery-home.json"
    ))
    .unwrap()["blocks"]
        .clone()
}

fn flight_records(blocks: serde_json::Value) -> String {
    format!(
        "baf:I[37,[],\"LanderStoryBlocks\"]\nace:{}\n",
        serde_json::json!(["$", "$Lbaf", null, {"blocks":blocks}])
    )
}

fn envelope(records: &str) -> Vec<u8> {
    format!(
        "<html><script>self.__next_f.push({})</script></html>",
        serde_json::json!([1, records])
    )
    .into_bytes()
}

fn split_envelope(records: &str, chunk_size: usize) -> Vec<u8> {
    let mut html = String::from("<html>");
    let mut start = 0;
    while start < records.len() {
        let mut end = (start + chunk_size).min(records.len());
        while !records.is_char_boundary(end) {
            end -= 1;
        }
        html.push_str(&format!(
            "<script>self.__next_f.push({})</script>",
            serde_json::json!([1, &records[start..end]])
        ));
        start = end;
    }
    html.push_str("</html>");
    html.into_bytes()
}

async fn project(records: &str) -> Result<criterion_provider::DiscoveryPage, Error> {
    Catalog::with_transport(HtmlFixture(envelope(records)))
        .discovery(DiscoveryRoute::Home)
        .await
}

struct HomeFixture;

struct DiscoverFixture;

impl RequestTransport for DiscoverFixture {
    async fn get(&self, request: Request) -> Result<Response, Error> {
        assert_eq!(
            request.url.as_str(),
            "https://www.criterionchannel.com/discover/newly-added"
        );
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/provider/discovery-newly-added.json"
        ))
        .unwrap();
        Ok(Response {
            status: 200,
            content_type: "text/html; charset=utf-8".into(),
            body: envelope(&flight_records(fixture["blocks"].clone())),
        })
    }
}

#[tokio::test]
async fn discover_destination_preserves_the_provided_grid_and_card_order() {
    let page = Catalog::with_transport(DiscoverFixture)
        .discovery(DiscoveryRoute::Discover(Slug::new("newly-added").unwrap()))
        .await
        .unwrap();
    assert_eq!(page.blocks.len(), 1);
    let DiscoveryBlock::Rail {
        id,
        cards,
        presentation,
        ..
    } = &page.blocks[0]
    else {
        panic!("expected discovery grid")
    };
    assert_eq!(*id, 834);
    assert_eq!(presentation.cards_per_view, 4);
    assert_eq!(presentation.layout, GalleryLayout::Grid);
    assert_eq!(cards.len(), 3);
    assert_eq!(cards[0].media.title, "Barry Lyndon");
    assert_eq!(cards[1].media.title, "Jennifer’s Body");
    assert_eq!(cards[2].media.title, "The Gift");
}

#[tokio::test]
async fn discovery_preserves_the_observed_shorts_banner_comma_in_asset_basename() {
    // Factual metadata subset from anonymous /discover/shorts HTML, 2026-10-09,
    // 483998 bytes, SHA256 bf83fefcc131f5fba6b9f55d21d7331803bd01efefacb3fee63934ffa6ccf0ef.
    // One supplied 320-wide desktop/mobile asset each; no asset was fetched.
    let banner: serde_json::Value =
        serde_json::from_str(include_str!("data/discovery-shorts-banner.json")).unwrap();
    let page = project(&flight_records(serde_json::json!([banner])))
        .await
        .unwrap();
    let DiscoveryBlock::Banner { id, artwork, .. } = &page.blocks[0] else {
        panic!("expected observed Shorts banner")
    };
    assert_eq!(*id, 656);
    assert_eq!(
        artwork.desktop[0].image.url().as_str(),
        "https://cc.criterion.com/uploads/storyBlocks/thumbnails/DB_Romvari,S_Banner_Wide_320x0.webp"
    );
    for file in [
        "name,../escape.webp",
        "name,%2Fescape.webp",
        "name,?secret.webp",
        "name,#secret.webp",
        "name,\\escape.webp",
        "name,\n.webp",
    ] {
        assert!(
            EditorialImage::new(
                "https://cc.criterion.com/uploads/storyBlocks/thumbnails/",
                file
            )
            .is_err()
        );
    }
}

struct NewFixture;

struct HtmlFixture(Vec<u8>);

struct ResponseFixture {
    status: u16,
    mime: &'static str,
}

impl RequestTransport for ResponseFixture {
    async fn get(&self, _: Request) -> Result<Response, Error> {
        Ok(Response {
            status: self.status,
            content_type: self.mime.into(),
            body: include_bytes!("../../../tests/fixtures/provider/discovery-home.html").to_vec(),
        })
    }
}

impl RequestTransport for HtmlFixture {
    async fn get(&self, _: Request) -> Result<Response, Error> {
        Ok(Response {
            status: 200,
            content_type: "text/html".into(),
            body: self.0.clone(),
        })
    }
}

impl RequestTransport for NewFixture {
    async fn get(&self, request: Request) -> Result<Response, Error> {
        assert_eq!(request.url.as_str(), "https://www.criterionchannel.com/new");
        Ok(Response {
            status: 200,
            content_type: "text/html".into(),
            body: include_bytes!("../../../tests/fixtures/provider/discovery-new.html").to_vec(),
        })
    }
}

impl RequestTransport for HomeFixture {
    async fn get(&self, request: Request) -> Result<Response, Error> {
        assert_eq!(request.url.as_str(), "https://www.criterionchannel.com/");
        Ok(Response {
            status: 200,
            content_type: "text/html; charset=utf-8".into(),
            body: include_bytes!("../../../tests/fixtures/provider/discovery-home.html").to_vec(),
        })
    }
}

#[tokio::test]
async fn new_preserves_all_known_editorial_and_navigation_blocks() {
    let page = Catalog::with_transport(NewFixture)
        .discovery(DiscoveryRoute::New)
        .await
        .unwrap();
    assert_eq!(page.blocks.len(), 17);
    let DiscoveryBlock::Slideshow { slides, .. } = &page.blocks[0] else {
        panic!("expected leading slideshow")
    };
    assert_eq!(slides[0].title.as_deref(), Some("Dead Ringers"));
}

#[tokio::test]
async fn discovery_rejects_push_looking_text_in_inert_html_contexts() {
    let fixture = include_str!("../../../tests/fixtures/provider/discovery-home.html");
    for tag in [
        "textarea",
        "title",
        "style",
        "xmp",
        "iframe",
        "noembed",
        "noframes",
        "plaintext",
        "noscript",
        "template",
    ] {
        let html = format!("<{tag}>{fixture}</{tag}>");
        assert!(
            matches!(
                Catalog::with_transport(HtmlFixture(html.into_bytes()))
                    .discovery(DiscoveryRoute::Home)
                    .await,
                Err(Error::InvalidResponse)
            ),
            "inert context {tag}"
        );
    }
}

#[tokio::test]
async fn discovery_fails_closed_on_unrecognized_flight_framing() {
    let records = format!(
        "{other}\n{}",
        flight_records(blocks_fixture()),
        other = "ba1:Xchanged transport framing"
    );
    assert!(matches!(
        Catalog::with_transport(HtmlFixture(envelope(&records)))
            .discovery(DiscoveryRoute::Home)
            .await,
        Err(Error::InvalidResponse)
    ));
}

#[tokio::test]
async fn discovery_requires_real_script_tag_names_and_hero_backgrounds() {
    let valid = String::from_utf8(envelope(&flight_records(blocks_fixture()))).unwrap();
    for suffix in ["-decoy", ".decoy", ":decoy"] {
        let html = valid.replace("<script>", &format!("<script{suffix}>"));
        assert!(
            matches!(
                Catalog::with_transport(HtmlFixture(html.into_bytes()))
                    .discovery(DiscoveryRoute::Home)
                    .await,
                Err(Error::InvalidResponse)
            ),
            "tag {suffix}"
        );
    }
    let mut blocks = blocks_fixture();
    blocks[0]["slides"][0]["thumbs"] = serde_json::json!({});
    assert!(matches!(
        Catalog::with_transport(HtmlFixture(envelope(&flight_records(blocks))))
            .discovery(DiscoveryRoute::Home)
            .await,
        Err(Error::InvalidResponse)
    ));
}

#[tokio::test]
async fn discovery_ignores_script_looking_data_in_foreign_cdata() {
    let html = format!(
        "<svg><![CDATA[ignored > {}]]></svg>",
        String::from_utf8(envelope(&flight_records(blocks_fixture()))).unwrap()
    );
    assert!(matches!(
        Catalog::with_transport(HtmlFixture(html.into_bytes()))
            .discovery(DiscoveryRoute::Home)
            .await,
        Err(Error::InvalidResponse)
    ));
}

#[tokio::test]
async fn home_preserves_all_blocks_labels_gallery_semantics_and_account_placeholders() {
    let page = Catalog::with_transport(HomeFixture)
        .discovery(DiscoveryRoute::Home)
        .await
        .unwrap();
    assert_eq!(
        page.blocks
            .iter()
            .map(DiscoveryBlock::id)
            .collect::<Vec<_>>(),
        [
            493, 537, 817, 704, 702, 811, 561, 818, 840, 551, 498, 497, 528, 860, 550, 847, 705,
            820, 861, 783, 532, 724, 726, 529, 538, 822, 748, 708
        ]
    );
    let DiscoveryBlock::Rail {
        target,
        source,
        image_label,
        presentation,
        ..
    } = &page.blocks[1]
    else {
        panic!("popular rail")
    };
    assert!(matches!(source, RailSource::Provided {feed_id: Some(id)} if id == "XfgiRIV9"));
    assert!(
        matches!(target, Some(ContentTarget::Media {route:MediaRoute::Category,id,..}) if id.as_str() == "QGtQj94z")
    );
    assert_eq!(*image_label, ImageLabel::Regalia);
    assert_eq!(presentation.cards_per_view, 4);
    assert_eq!(presentation.layout, GalleryLayout::Rail);
    let DiscoveryBlock::Rail { source, cards, .. } = &page.blocks[10] else {
        panic!("watchlist placeholder")
    };
    assert!(matches!(source, RailSource::Watchlist));
    assert!(cards.is_empty());
    let DiscoveryBlock::Rail { source, cards, .. } = &page.blocks[11] else {
        panic!("continue placeholder")
    };
    assert!(matches!(source, RailSource::ContinueWatching));
    assert!(cards.is_empty());
    let DiscoveryBlock::Rail {
        image_label,
        presentation,
        ..
    } = &page.blocks[14]
    else {
        panic!("portrait rail")
    };
    assert_eq!(*image_label, ImageLabel::Portrait);
    assert_eq!(presentation.aspect_ratio_percent, 150.0);
    let DiscoveryBlock::Rail { cards, .. } = &page.blocks[19] else {
        panic!("animation rail")
    };
    assert_eq!(cards[0].media.kind, MediaKind::Series);
    let DiscoveryBlock::Rail {
        image_label,
        presentation,
        ..
    } = &page.blocks[22]
    else {
        panic!("edition rail")
    };
    assert_eq!(*image_label, ImageLabel::Edition);
    assert_eq!(presentation.cards_per_view, 5);
    assert_eq!(presentation.aspect_ratio_percent, 124.23);
    let DiscoveryBlock::Banner { target, .. } = &page.blocks[27] else {
        panic!("live banner")
    };
    assert!(
        matches!(target,Some(ContentTarget::Media {route:MediaRoute::Live,id,..}) if id.as_str()=="1emmgvqX")
    );
}

#[tokio::test]
async fn discovery_preserves_optional_hero_text_and_explicit_editorial_artwork() {
    let page = Catalog::with_transport(HomeFixture)
        .discovery(DiscoveryRoute::Home)
        .await
        .unwrap();
    let DiscoveryBlock::Slideshow { slides, .. } = &page.blocks[0] else {
        panic!("hero")
    };
    assert_eq!(slides[0].title, None);
    assert_eq!(slides[0].cta.as_deref(), Some("See more"));
    assert_eq!(slides[0].artwork.desktop[0].width, 1920);
    assert_eq!(
        slides[0].artwork.desktop[0].image.url().as_str(),
        "https://cc.criterion.com/uploads/storyBlocks/493/thumbnails/POSSESSIONS_collection_hero_wide_1920x0.webp"
    );
    assert_eq!(
        slides[0].artwork.logo.as_ref().unwrap().url().as_str(),
        "https://cc.criterion.com/uploads/storyBlocks/493/thumbnails/POSSESSIONS_collection_logo_default_760x0.webp"
    );
    let new = Catalog::with_transport(NewFixture)
        .discovery(DiscoveryRoute::New)
        .await
        .unwrap();
    let DiscoveryBlock::Banner { target, .. } = &new.blocks[12] else {
        panic!("original banner")
    };
    assert!(
        matches!(target,Some(ContentTarget::Media {route:MediaRoute::Original,id,..}) if id.as_str()=="k5BwTA40")
    );
    assert!(!format!("{page:?}").contains("POSSESSIONS"));
    assert!(!format!("{:?}", slides[0]).contains("fhQRpxw4"));
    assert!(!format!("{:?}", slides[0].artwork.desktop[0].image).contains("criterion.com"));
}

#[test]
fn typed_content_targets_and_artwork_reject_url_escapes_before_use() {
    for link in [
        "http://www.criterionchannel.com/new",
        "https://criterionchannel.com/new",
        "https://www.criterionchannel.com:443/new",
        "https://www.criterionchannel.com.evil/new",
        "//www.criterionchannel.com/new",
        "/new?token=secret",
        "/new#secret",
        "/films/qvwT6mJ4/../secret",
        "/films/qvwT6mJ4/%2e%2e",
        "/films/qvwT6mJ4/title/extra",
        "/films/short/title",
        "/auth/profile",
        "/login",
        "/api/playback/qvwT6mJ4",
    ] {
        assert!(ContentTarget::parse(link).is_err(), "{link}");
    }
    let target =
        ContentTarget::parse("https://www.criterionchannel.com/collections/fhQRpxw4/possessions")
            .unwrap();
    assert_eq!(
        target.url().unwrap().as_str(),
        "https://www.criterionchannel.com/collections/fhQRpxw4/possessions"
    );
    assert_eq!(
        ContentTarget::parse("/discover/newly-added")
            .unwrap()
            .url()
            .unwrap()
            .path(),
        "/discover/newly-added"
    );
    for base in [
        "http://cc.criterion.com/uploads/storyBlocks/thumbnails/",
        "https://cc.criterion.com.evil/uploads/storyBlocks/thumbnails/",
        "https://cc.criterion.com:443/uploads/storyBlocks/thumbnails/",
        "https://cc.criterion.com/uploads/storyBlocks/../thumbnails/",
        "https://cc.criterion.com/uploads/storyBlocks/%2e%2e/thumbnails/",
        "https://cc.criterion.com/uploads/storyBlocks/thumbnails/?secret=",
    ] {
        assert!(EditorialImage::new(base, "image.webp").is_err());
    }
    for file in [
        "../image.webp",
        "%2e%2e.webp",
        "image.webp?token=secret",
        "image.webp#secret",
        "/image.webp",
        "https://evil/image.webp",
        "image\\name.webp",
        "image.png",
    ] {
        assert!(
            EditorialImage::new(
                "https://cc.criterion.com/uploads/storyBlocks/thumbnails/",
                file
            )
            .is_err()
        );
    }
    assert_eq!(
        ImageLabel::Edition
            .url(&criterion_provider::MediaId::new("qvwT6mJ4").unwrap())
            .unwrap()
            .path(),
        "/v1/media/qvwT6mJ4/images/default_bluray.webp"
    );
}

#[tokio::test]
async fn discovery_resolves_dynamic_named_imports_across_frames_and_ignores_noncomponent_props() {
    let records = format!(
        ":HL[\"/_next/style.css\",\"style\"]\n11:I[8,[],\"DifferentComponent\"]\n12:[\"$\",\"$L11\",null,{{\"blocks\":[]}}]\n{}",
        flight_records(blocks_fixture())
    );
    let page = Catalog::with_transport(HtmlFixture(split_envelope(&records, 777)))
        .discovery(DiscoveryRoute::Home)
        .await
        .unwrap();
    assert_eq!(page.blocks.len(), 28);
}

#[tokio::test]
async fn discovery_rejects_missing_ambiguous_malformed_and_truncated_components() {
    let valid = flight_records(blocks_fixture());
    let variants=vec![valid.replace("LanderStoryBlocks","OtherExport"),format!("1:I[9,[],\"LanderStoryBlocks\"]\n{valid}"),valid.replace("$Lbaf","$Lbad"),format!("{valid}1:[\"$\",\"$Lbaf\",null,{{}}]\n"),valid.trim_end().to_owned(),"baf:I[37,[],\"LanderStoryBlocks\"]\nace:[\"$\",\"$Lbaf\",null,{\"blocks\":\"$unresolved\"}]\n".into(),valid.replace("[37,[]", "[\"module\",[]")];
    for records in variants {
        assert_eq!(project(&records).await, Err(Error::InvalidResponse));
    }
}

#[tokio::test]
async fn discovery_rejects_duplicate_props_including_escaped_key_aliases() {
    let valid = flight_records(blocks_fixture());
    for key in ["blocks", "\\u0062locks"] {
        let records = valid.replace("{\"blocks\":", &format!("{{\"{key}\":[],\"blocks\":"));
        assert_eq!(project(&records).await, Err(Error::InvalidResponse));
    }
}

#[tokio::test]
async fn discovery_rejects_numeric_chunk_aliases_in_records_and_element_references() {
    let valid = flight_records(blocks_fixture());
    for alias in ["0baf", "BAF", "100000000", "80000000"] {
        let duplicate_import = format!("{alias}:I[9,[],\"OtherComponent\"]\n{valid}");
        assert_eq!(
            project(&duplicate_import).await,
            Err(Error::InvalidResponse)
        );
        let duplicate_element = format!("{valid}1:[\"$\",\"$L{alias}\",null,{{}}]\n");
        assert_eq!(
            project(&duplicate_element).await,
            Err(Error::InvalidResponse)
        );
    }
}

#[tokio::test]
async fn discovery_skips_documented_unknown_blocks_and_rejects_empty_or_invalid_known_blocks() {
    let mut blocks = blocks_fixture();
    blocks
        .as_array_mut()
        .unwrap()
        .insert(1, serde_json::json!({"type":999}));
    assert_eq!(
        project(&flight_records(blocks)).await.unwrap().blocks.len(),
        28
    );
    for blocks in [
        serde_json::json!([]),
        serde_json::json!([{"type":999}]),
        serde_json::json!([{"type":20,"id":1}]),
        serde_json::json!([{"type":28,"id":1}]),
    ] {
        assert_eq!(
            project(&flight_records(blocks)).await,
            Err(Error::InvalidResponse)
        );
    }
}

#[tokio::test]
async fn discovery_requires_supplied_playlists_without_inventing_empty_static_rows() {
    let mut blocks = blocks_fixture();
    blocks[1].as_object_mut().unwrap().remove("playlist");
    assert_eq!(
        project(&flight_records(blocks)).await,
        Err(Error::InvalidResponse)
    );
}

#[tokio::test]
async fn discovery_preserves_original_cards_only_with_their_verified_originals_route() {
    // Synthetic row on the observed HTML envelope; Original's route is source-verified.
    let mut blocks = blocks_fixture();
    blocks[1]["playlist"] = serde_json::json!([{
        "mediaid":"ABCDEF02", "title":"Original fixture", "contentType":"original", "duration":71,
        "deeplink":"/originals/ABCDEF02/original-fixture"
    }]);
    let page = project(&flight_records(blocks.clone())).await.unwrap();
    let DiscoveryBlock::Rail { cards, .. } = &page.blocks[1] else {
        panic!("expected supplied rail")
    };
    assert_eq!(cards[0].media.kind, MediaKind::Original);
    assert!(
        matches!(&cards[0].target, ContentTarget::Media { route: MediaRoute::Original, id, .. } if id.as_str() == "ABCDEF02")
    );
    for route in ["films", "supplements", "collections"] {
        blocks[1]["playlist"][0]["deeplink"] =
            serde_json::json!(format!("/{route}/ABCDEF02/original-fixture"));
        assert_eq!(
            project(&flight_records(blocks.clone())).await,
            Err(Error::InvalidResponse)
        );
    }
}

#[tokio::test]
async fn discovery_rejects_card_identity_and_known_metadata_changes() {
    for (key, value) in [
        ("deeplink", serde_json::json!("/films/11111111/the-hitcher")),
        (
            "deeplink",
            serde_json::json!("/collections/qvwT6mJ4/the-hitcher"),
        ),
        ("mediaid", serde_json::json!("../bad")),
        ("duration", serde_json::json!(604801)),
        ("title", serde_json::json!("x".repeat(513))),
    ] {
        let mut blocks = blocks_fixture();
        blocks[1]["playlist"][0][key] = value;
        assert_eq!(
            project(&flight_records(blocks)).await,
            Err(Error::InvalidResponse)
        );
    }
    for (key, value) in [
        ("galleryWrap", serde_json::json!(2)),
        ("galleryPageNum", serde_json::json!(0)),
        ("imageAspectRatio", serde_json::json!(301)),
        ("imageJWLabel", serde_json::json!("../../secret")),
        ("playlistType", serde_json::json!("unverifiedFeed")),
        ("link", serde_json::json!("https://evil.test/")),
        ("header", serde_json::json!("x".repeat(513))),
    ] {
        let mut blocks = blocks_fixture();
        blocks[1][key] = value;
        assert_eq!(
            project(&flight_records(blocks)).await,
            Err(Error::InvalidResponse)
        );
    }
}

#[tokio::test]
async fn discovery_bounds_raw_html_and_flight_frames_separately() {
    let fixture = include_bytes!("../../../tests/fixtures/provider/discovery-home.html");
    let mut body = fixture.to_vec();
    body.extend_from_slice(b"<!--");
    body.resize(MAX_RESPONSE_BYTES - 3, b' ');
    body.extend_from_slice(b"-->");
    assert_eq!(
        Catalog::with_transport(HtmlFixture(body.clone()))
            .discovery(DiscoveryRoute::Home)
            .await
            .unwrap()
            .blocks
            .len(),
        28
    );
    body.push(b' ');
    assert_eq!(
        Catalog::with_transport(HtmlFixture(body))
            .discovery(DiscoveryRoute::Home)
            .await,
        Err(Error::ResponseTooLarge)
    );
    let mut records = flight_records(blocks_fixture());
    records.push_str("ffe:0");
    records.extend(std::iter::repeat_n(' ', 512 * 1024 - records.len() - 1));
    records.push('\n');
    assert_eq!(project(&records).await.unwrap().blocks.len(), 28);
    records.insert(records.len() - 1, ' ');
    assert_eq!(project(&records).await, Err(Error::InvalidResponse));
}

#[tokio::test]
async fn discovery_bounds_json_record_depth_strings_and_tokens_before_projection() {
    let valid = flight_records(blocks_fixture());
    for (depth, okay) in [(32, true), (33, false)] {
        let records = format!("{valid}ffe:{}0{}\n", "[".repeat(depth), "]".repeat(depth));
        assert_eq!(project(&records).await.is_ok(), okay);
    }
    for (len, okay) in [(65536, true), (65537, false)] {
        let records = format!("{valid}ffe:\"{}\"\n", "x".repeat(len));
        assert_eq!(project(&records).await.is_ok(), okay);
    }
    for (len, okay) in [(65536, true), (65537, false)] {
        let records = format!("{valid}ffe:[{}]\n", vec!["0"; len].join(","));
        assert_eq!(project(&records).await.is_ok(), okay);
    }
}

#[tokio::test]
async fn discovery_requires_successful_html_responses_and_retains_safe_errors() {
    for status in [301, 302, 307, 401, 429, 503] {
        assert_eq!(
            Catalog::with_transport(ResponseFixture {
                status,
                mime: "text/html"
            })
            .discovery(DiscoveryRoute::Home)
            .await,
            Err(Error::HttpStatus(status))
        );
    }
    for mime in [
        "application/json",
        "application/xhtml+xml",
        "image/webp",
        "text/html-secret",
        "",
    ] {
        let error = Catalog::with_transport(ResponseFixture { status: 200, mime })
            .discovery(DiscoveryRoute::Home)
            .await
            .unwrap_err();
        assert_eq!(error, Error::InvalidResponse);
        assert_eq!(error.to_string(), "catalog response is invalid");
        assert_eq!(format!("{error:?}"), "InvalidResponse");
        assert!(std::error::Error::source(&error).is_none());
    }
    assert_eq!(
        Catalog::with_transport(ResponseFixture {
            status: 200,
            mime: "TEXT/HTML; charset=UTF-8"
        })
        .discovery(DiscoveryRoute::Home)
        .await
        .unwrap()
        .blocks
        .len(),
        28
    );
}

fn pad_record(mut records: String, total: usize, id: &str) -> String {
    records.push_str(&format!("{id}:0"));
    records.extend(std::iter::repeat_n(' ', total - records.len() - 1));
    records.push('\n');
    records
}

#[tokio::test]
async fn discovery_bounds_combined_flight_record_sizes_and_frame_count() {
    let valid = flight_records(blocks_fixture());
    let records = pad_record(
        pad_record(valid.clone(), 512 * 1024, "ffe"),
        1024 * 1024,
        "ffd",
    );
    assert_eq!(
        Catalog::with_transport(HtmlFixture(split_envelope(&records, 512 * 1024)))
            .discovery(DiscoveryRoute::Home)
            .await
            .unwrap()
            .blocks
            .len(),
        28
    );
    let overflow = format!("{records}\n");
    assert_eq!(
        Catalog::with_transport(HtmlFixture(split_envelope(&overflow, 512 * 1024)))
            .discovery(DiscoveryRoute::Home)
            .await,
        Err(Error::InvalidResponse)
    );
    for (record_len, okay) in [(512 * 1024, true), (512 * 1024 + 1, false)] {
        let total = valid.len() + record_len + 1;
        let records = pad_record(valid.clone(), total, "ffe");
        assert_eq!(
            Catalog::with_transport(HtmlFixture(split_envelope(&records, 256 * 1024)))
                .discovery(DiscoveryRoute::Home)
                .await
                .is_ok(),
            okay
        );
    }
    let base = String::from_utf8(envelope(&valid)).unwrap();
    for (frames, okay) in [(128, true), (129, false)] {
        let html = format!(
            "{base}{}",
            "<script>self.__next_f.push([0])</script>".repeat(frames - 1)
        );
        assert_eq!(
            Catalog::with_transport(HtmlFixture(html.into_bytes()))
                .discovery(DiscoveryRoute::Home)
                .await
                .is_ok(),
            okay
        );
    }
}

#[tokio::test]
async fn discovery_bounds_supplied_cards_slides_navigation_and_block_counts() {
    let fixture = blocks_fixture();
    for (count, okay) in [(512, true), (513, false)] {
        let mut rail = fixture[1].clone();
        rail["playlist"] = serde_json::json!(vec![fixture[1]["playlist"][0].clone(); count]);
        assert_eq!(
            project(&flight_records(serde_json::json!([rail])))
                .await
                .is_ok(),
            okay
        );
    }
    for (count, okay) in [(32, true), (33, false)] {
        let mut slideshow = fixture[0].clone();
        slideshow["slides"] = serde_json::json!(vec![fixture[0]["slides"][0].clone(); count]);
        assert_eq!(
            project(&flight_records(serde_json::json!([slideshow])))
                .await
                .is_ok(),
            okay
        );
    }
    for (count, okay) in [(128, true), (129, false)] {
        let mut nav = fixture[4].clone();
        nav["nav"] = serde_json::json!(vec![fixture[4]["nav"][0].clone(); count]);
        assert_eq!(
            project(&flight_records(serde_json::json!([nav])))
                .await
                .is_ok(),
            okay
        );
        assert_eq!(
            project(&flight_records(serde_json::json!(vec![
                fixture[4].clone();
                count
            ])))
            .await
            .is_ok(),
            okay
        );
    }
}

#[tokio::test]
async fn discovery_preserves_supplied_destination_grid_without_assuming_continuation() {
    let value: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/provider/discovery-newly-added.json"
    ))
    .unwrap();
    let page = project(&flight_records(value["blocks"].clone()))
        .await
        .unwrap();
    let DiscoveryBlock::Rail {
        cards,
        presentation,
        ..
    } = &page.blocks[0]
    else {
        panic!("grid")
    };
    assert_eq!(presentation.layout, GalleryLayout::Grid);
    assert_eq!(presentation.cards_per_view, 4);
    assert_eq!(cards.len(), 3);
    assert_eq!(cards[0].media.title, "Barry Lyndon");
}

#[tokio::test]
async fn home_preserves_first_hero_and_popular_movie_order_from_the_public_envelope() {
    let page = Catalog::with_transport(HomeFixture)
        .discovery(DiscoveryRoute::Home)
        .await
        .unwrap();
    let DiscoveryBlock::Slideshow { id, slides } = &page.blocks[0] else {
        panic!("expected leading slideshow")
    };
    assert_eq!(*id, 493);
    assert_eq!(slides[0].id, 1595);
    assert!(
        matches!(&slides[0].target, Some(ContentTarget::Media { route: MediaRoute::Collection, id, .. }) if id.as_str() == "fhQRpxw4")
    );
    let DiscoveryBlock::Rail { header, cards, .. } = &page.blocks[1] else {
        panic!("expected popular movies")
    };
    assert_eq!(header.as_deref(), Some("Popular Movies"));
    assert_eq!(cards[0].media.title, "The Hitcher");
    assert_eq!(cards[0].media.id.as_str(), "qvwT6mJ4");
    assert_eq!(cards[0].media.duration_seconds, 5851);
}

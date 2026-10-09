use criterion_provider::{
    Catalog, Error, MediaId, Request, RequestTransport, Response, UtcTimestamp,
};

// Admitted factual metadata only: first two schedule entries and both observed
// empty mediaid entries from the 2026-10-09 anonymous HTTPS /api/media/1emmgvqX
// response (218038 bytes, SHA256
// 152ac75fcc4dbef308645c0d1180db76efe64b5bcd579bd30db29f328de02f63).
// Editorial descriptions, source URLs and playback fields are not vendored.
struct LiveFixture(Vec<u8>);

impl RequestTransport for LiveFixture {
    async fn get(&self, request: Request) -> Result<Response, Error> {
        assert_eq!(
            request.url.as_str(),
            "https://www.criterionchannel.com/api/media/1emmgvqX"
        );
        Ok(Response {
            status: 200,
            content_type: "application/json".into(),
            body: self.0.clone(),
        })
    }
}

fn fixture() -> Vec<u8> {
    include_bytes!("data/media-live.json").to_vec()
}

async fn detail(body: Vec<u8>) -> Result<criterion_provider::MediaDetail, Error> {
    Catalog::with_transport(LiveFixture(body))
        .detail(&MediaId::new("1emmgvqX").unwrap())
        .await
}

#[tokio::test]
async fn live_public_detail_preserves_observed_metadata_without_invented_runtime() {
    let result = detail(fixture()).await.unwrap();
    assert_eq!(result.media.title, "Criterion 24/7");
    assert_eq!(format!("{:?}", result.media.kind), "Live");
    assert_eq!(result.media.duration_seconds, 0);
    assert_eq!(result.media.release_date, None);
    assert_eq!(result.description, None);
    assert!(result.directors.is_empty());
    assert!(result.playlists.is_empty());
}

#[tokio::test]
async fn live_schedule_preserves_actual_timestamps_and_unlinked_entries() {
    let result = detail(fixture()).await.unwrap();
    assert_eq!(result.live_schedule.len(), 4);
    let first = &result.live_schedule[0];
    assert_eq!(first.title, "Grenada: The Future Coming Towards Us");
    assert_eq!(first.media_id.as_ref().unwrap().as_str(), "lUeuT11x");
    assert_eq!(first.duration_seconds, 3240);
    assert_eq!(first.starts_at.as_str(), "2026-10-08T10:00:37Z");
    assert_eq!(first.ends_at.as_str(), "2026-10-08T10:54:46Z");
    assert_eq!(result.live_schedule[1].title, "Pressure");
    assert_eq!(result.live_schedule[2].title, "Streetwise");
    assert_eq!(result.live_schedule[3].title, "My Father Is Coming");
    assert!(result.live_schedule[2].media_id.is_none());
    assert!(result.live_schedule[3].media_id.is_none());
}

fn value() -> serde_json::Value {
    serde_json::from_slice(&fixture()).unwrap()
}

async fn rejects(value: serde_json::Value) {
    assert_eq!(
        detail(serde_json::to_vec(&value).unwrap()).await,
        Err(Error::InvalidResponse)
    );
}

#[tokio::test]
async fn live_schedule_requires_both_agreeing_timestamp_aliases() {
    for key in ["startTime", "start_time", "endTime", "end_time"] {
        let mut body = value();
        body["schedule"][0].as_object_mut().unwrap().remove(key);
        rejects(body).await;
        let mut body = value();
        body["schedule"][0][key] = "2026-10-08T10:00:00Z".into();
        rejects(body).await;
    }
}

#[tokio::test]
async fn live_schedule_rejects_invalid_intervals_and_noncanonical_times() {
    for invalid in [
        "2026-02-29T10:00:37Z",
        "2026-10-08T24:00:37Z",
        "2026-10-08T10:60:37Z",
        "2026-10-08T10:00:60Z",
        "2026-10-08T10:00:37+00:00",
        "2026-10-08T10:00:37.0Z",
        "0000-10-08T10:00:37Z",
        "2026-10-08t10:00:37z",
        "2026-10-08T10:54:46Z",
        "2026-10-08T11:00:00Z",
    ] {
        let mut body = value();
        body["schedule"][0]["start_time"] = invalid.into();
        body["schedule"][0]["startTime"] = invalid.into();
        rejects(body).await;
    }
    let mut body = value();
    body["schedule"][1]["start_time"] = "2026-10-08T10:54:45Z".into();
    body["schedule"][1]["startTime"] = "2026-10-08T10:54:45Z".into();
    rejects(body).await;
    let mut body = value();
    body["schedule"].as_array_mut().unwrap().swap(0, 1);
    rejects(body).await;
}

#[tokio::test]
async fn live_schedule_requires_bounded_observed_film_metadata() {
    for (key, replacement) in [
        ("mediaid", serde_json::json!("not-an-id")),
        ("mediaid", serde_json::Value::Null),
        ("title", serde_json::json!(" ")),
        ("title", serde_json::json!("x".repeat(513))),
        ("title", serde_json::json!("control\u{1b}")),
        ("contentType", serde_json::json!("series")),
        ("duration", serde_json::json!(604801)),
        ("duration", serde_json::Value::Null),
    ] {
        let mut body = value();
        body["schedule"][0][key] = replacement;
        rejects(body).await;
    }
    let mut body = value();
    body["schedule"][0]
        .as_object_mut()
        .unwrap()
        .remove("mediaid");
    rejects(body).await;
    for replacement in [serde_json::Value::Null, serde_json::json!({})] {
        let mut body = value();
        body["schedule"] = replacement;
        rejects(body).await;
    }
    let mut body = value();
    body.as_object_mut().unwrap().remove("schedule");
    rejects(body).await;
    let mut body = value();
    body["contentType"] = "film".into();
    rejects(body).await;
}

#[tokio::test]
async fn live_schedule_accepts_the_exact_program_limit_and_rejects_one_more() {
    // Distinct ordered intervals make this count oracle independent of overlap rejection.
    for (count, accepted) in [(256, true), (257, false)] {
        let mut body = value();
        let mut programs = Vec::new();
        for minute in 0..count {
            let mut program = body["schedule"][0].clone();
            let start = format!("2026-10-08T{:02}:{:02}:00Z", minute / 60, minute % 60);
            let end = format!(
                "2026-10-08T{:02}:{:02}:00Z",
                (minute + 1) / 60,
                (minute + 1) % 60
            );
            program["start_time"] = start.clone().into();
            program["startTime"] = start.into();
            program["end_time"] = end.clone().into();
            program["endTime"] = end.into();
            programs.push(program);
        }
        body["schedule"] = programs.into();
        let result = detail(serde_json::to_vec(&body).unwrap()).await;
        assert_eq!(result.is_ok(), accepted);
        if let Ok(detail) = result {
            assert_eq!(detail.live_schedule.len(), count);
        }
    }
}

#[tokio::test]
async fn live_detail_ignores_playback_fields_and_redacts_all_external_debug_data() {
    let mut body = value();
    body["sources"] =
        serde_json::json!([{"file":"https://secret.example/private?token=DO-NOT-LOG"}]);
    body["schedule"][0]["sources"] = body["sources"].clone();
    let result = detail(serde_json::to_vec(&body).unwrap()).await.unwrap();
    let debug = format!(
        "{result:?} {:?} {:?}",
        result.live_schedule, result.live_schedule[0].starts_at
    );
    for external in [
        "DO-NOT-LOG",
        "1emmgvqX",
        "lUeuT11x",
        "Grenada",
        "2026-10-08",
    ] {
        assert!(!debug.contains(external));
    }
    assert_eq!(result.live_schedule.len(), 4);
}

#[test]
fn canonical_utc_timestamps_validate_calendar_boundaries_and_sort_chronologically() {
    let leap = UtcTimestamp::new("2024-02-29T23:59:59Z").unwrap();
    let next = UtcTimestamp::new("2024-03-01T00:00:00Z").unwrap();
    assert!(leap < next);
    assert_eq!(leap.as_str(), "2024-02-29T23:59:59Z");
    for invalid in [
        "1900-02-29T00:00:00Z",
        "2024-04-31T00:00:00Z",
        "2024-13-01T00:00:00Z",
        "2024-00-01T00:00:00Z",
        "é024-02-29T23:59:59Z",
    ] {
        assert_eq!(UtcTimestamp::new(invalid), Err(Error::InvalidRequest));
    }
    assert!(UtcTimestamp::new("2000-02-29T00:00:00Z").is_ok());
}

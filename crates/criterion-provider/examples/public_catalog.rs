use criterion_provider::{BrowseRequest, Catalog, Error, MediaId};

// Explicitly invoked live smoke: no credentials, playback requests or response payload logging.
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Error> {
    let catalog = Catalog::new()?;
    let options = catalog.options().await?;
    println!(
        "options: {} filter groups, {} sort choices",
        options.filter_groups.len(),
        options.sort_options.len()
    );
    let page = catalog
        .browse(&BrowseRequest {
            page_limit: 2,
            ..BrowseRequest::default()
        })
        .await?;
    println!(
        "browse: {} items, continuation present={}",
        page.items.len(),
        page.next_cursor.is_some()
    );
    if let Some(cursor) = page.next_cursor {
        let next = catalog
            .browse(&BrowseRequest {
                page_limit: 2,
                cursor: Some(cursor),
                ..BrowseRequest::default()
            })
            .await?;
        println!("next page: {} items", next.items.len());
    }
    let search = catalog.search("hitcher").await?;
    println!(
        "search: {} items, {} content types",
        search.items.len(),
        search.type_counts.len()
    );
    let detail = catalog.detail(&MediaId::new("qvwT6mJ4")?).await?;
    println!(
        "detail: {} playlists, {} directors",
        detail.playlists.len(),
        detail.directors.len()
    );
    Ok(())
}

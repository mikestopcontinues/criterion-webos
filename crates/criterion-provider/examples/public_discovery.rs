use criterion_provider::{
    Catalog, DiscoveryBlock, DiscoveryRoute, Error, MediaId, MediaKind, Slug,
};

// Explicitly invoked live public smoke. It logs only projected counts; no assets,
// authentication, account, playback, stream or license requests are made.
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Error> {
    let catalog = Catalog::new()?;
    // These destinations were observed in public navigation. No route grid or
    // undocumented API is inferred from arbitrary user text.
    for (label, route) in [
        ("Home", DiscoveryRoute::Home),
        ("New", DiscoveryRoute::New),
        (
            "Newly added",
            DiscoveryRoute::Discover(Slug::new("newly-added")?),
        ),
        (
            "International cinema",
            DiscoveryRoute::Discover(Slug::new("international-cinema")?),
        ),
        ("Shorts", DiscoveryRoute::Discover(Slug::new("shorts")?)),
    ] {
        let page = catalog.discovery(route).await?;
        let mut slides = 0;
        let mut cards = 0;
        let mut nav_items = 0;
        let mut banners = 0;
        for block in &page.blocks {
            match block {
                DiscoveryBlock::Slideshow { slides: items, .. } => slides += items.len(),
                DiscoveryBlock::Rail { cards: items, .. } => cards += items.len(),
                DiscoveryBlock::Navigation { items, .. } => nav_items += items.len(),
                DiscoveryBlock::Banner { .. } => banners += 1,
            }
        }
        println!(
            "{label}: {} blocks, {slides} slides, {cards} cards, {nav_items} navigation items, {banners} banners",
            page.blocks.len()
        );
    }
    let live = catalog.detail(&MediaId::new("1emmgvqX")?).await?;
    if live.media.kind != MediaKind::Live {
        return Err(Error::InvalidResponse);
    }
    println!(
        "24/7: {} public schedule entries, {} without catalog links",
        live.live_schedule.len(),
        live.live_schedule
            .iter()
            .filter(|program| program.media_id.is_none())
            .count()
    );
    Ok(())
}

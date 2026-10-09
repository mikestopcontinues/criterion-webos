use criterion_provider::{Catalog, DiscoveryBlock, DiscoveryRoute, Error};

// Explicitly invoked live public smoke. It logs only projected counts; no assets,
// authentication, account, playback, stream or license requests are made.
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Error> {
    let catalog = Catalog::new()?;
    for route in [DiscoveryRoute::Home, DiscoveryRoute::New] {
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
            "{route:?}: {} blocks, {slides} slides, {cards} cards, {nav_items} navigation items, {banners} banners",
            page.blocks.len()
        );
    }
    Ok(())
}

//! Explicit opt-in: one previously verified public image, no account or GPU.
use criterion_artwork::{ArtworkLoader, ArtworkSource, ImageRole};
use criterion_provider::{ImageLabel, MediaId};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let role = match arguments.as_slice() {
        [argument] if argument == "--live-public-image" => ImageRole::Card,
        [argument] if argument == "--live-public-backdrop" => ImageRole::Backdrop,
        _ => return Err("pass --live-public-image or --live-public-backdrop".into()),
    };
    let source = ArtworkSource::Media {
        id: MediaId::new("qvwT6mJ4")?,
        label: ImageLabel::Landscape,
        role,
    };
    let artwork = ArtworkLoader::new()?.load(&source).await?;
    let [width, height] = artwork.dimensions();
    println!(
        "Public artwork decoded: {width}x{height}, {} RGBA bytes",
        artwork.rgba().len()
    );
    Ok(())
}

use crate::AppUi;
use egui::{ColorImage, TextureHandle, TextureOptions};
use std::collections::VecDeque;
const CACHE_BYTES: usize = 24 * 1024 * 1024;
const CACHE_ENTRIES: usize = 32;
struct ImageEntry {
    key: String,
    image: Option<ColorImage>,
    texture: Option<TextureHandle>,
    bytes: usize,
}
#[derive(Default)]
pub(crate) struct ImageCache {
    entries: VecDeque<ImageEntry>,
    bytes: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageError {
    InvalidKey,
    InvalidDimensions,
}
impl std::fmt::Display for ImageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidKey => "invalid public artwork key",
            Self::InvalidDimensions => "artwork dimensions exceed renderer bounds",
        })
    }
}
impl std::error::Error for ImageError {}
impl AppUi {
    pub fn context(&self) -> &egui::Context {
        &self.context
    }
    /// Admit already decoded public artwork. The runtime owns network origin,
    /// decode limits and stale publication rejection before reaching this seam.
    pub fn admit_image(&mut self, key: &str, image: ColorImage) -> Result<(), ImageError> {
        if key.is_empty() || key.len() > 128 || key.chars().any(char::is_control) {
            return Err(ImageError::InvalidKey);
        }
        let pixels = image.size[0]
            .checked_mul(image.size[1])
            .ok_or(ImageError::InvalidDimensions)?;
        if image.size.contains(&0)
            || image.size.iter().any(|side| *side > 2048)
            || pixels > 1024 * 1024
            || image.pixels.len() != pixels
        {
            return Err(ImageError::InvalidDimensions);
        }
        if let Some(index) = self
            .images
            .entries
            .iter()
            .position(|entry| entry.key == key)
        {
            let old = self.images.entries.remove(index).expect("existing image");
            self.images.bytes -= old.bytes;
        }
        let bytes = pixels * 4;
        while self.images.entries.len() >= CACHE_ENTRIES || self.images.bytes + bytes > CACHE_BYTES
        {
            if let Some(old) = self.images.entries.pop_front() {
                self.images.bytes -= old.bytes;
            } else {
                break;
            }
        }
        self.images.entries.push_back(ImageEntry {
            key: key.to_owned(),
            image: Some(image),
            texture: None,
            bytes,
        });
        self.images.bytes += bytes;
        Ok(())
    }
    pub fn image_cache_len(&self) -> usize {
        self.images.entries.len()
    }
    pub fn image_cache_bytes(&self) -> usize {
        self.images.bytes
    }
    // Upload only cache survivors. Admission never queues evicted decoded data.
    pub(crate) fn flush_images(&mut self) {
        for entry in &mut self.images.entries {
            if let Some(image) = entry.image.take() {
                entry.texture = Some(self.context.load_texture(
                    "public artwork",
                    image,
                    TextureOptions::LINEAR,
                ));
            }
        }
    }
    pub(crate) fn image(&mut self, key: &str) -> Option<egui::TextureId> {
        self.image_info(key).map(|(id, _)| id)
    }
    pub(crate) fn image_info(&mut self, key: &str) -> Option<(egui::TextureId, [usize; 2])> {
        let index = self
            .images
            .entries
            .iter()
            .position(|entry| entry.key == key)?;
        let entry = self.images.entries.remove(index)?;
        let id = entry
            .texture
            .as_ref()
            .map(|texture| (texture.id(), texture.size()));
        self.images.entries.push_back(entry);
        id
    }
}

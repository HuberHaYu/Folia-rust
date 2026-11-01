// Album art for the wall: the embedded front cover, else a cover image beside the file, cropped and shrunk to a tile.

use std::collections::VecDeque;
use std::path::Path;

use image::DynamicImage;
use image::imageops::FilterType;
use lofty::picture::PictureType;
use lofty::prelude::*;

/// Decoded RGBA pixels ready for the GPU.
#[derive(Debug, Clone)]
pub struct Cover {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

const SIDECAR_NAMES: [&str; 6] = ["cover.jpg", "cover.png", "folder.jpg", "folder.png", "front.jpg", "front.png"];

/// The track's cover filling `width`×`height` like CSS `object-fit: cover`, or `None` when it has none.
pub fn cover_for(track_path: &Path, width: u32, height: u32) -> Option<Cover> {
    let bytes = embedded(track_path).or_else(|| sidecar(track_path))?;
    cover_rgba(&bytes, width, height)
        .inspect_err(|err| tracing::debug!(path = %track_path.display(), error = %err, "cannot decode cover"))
        .ok()
}

/// Decodes `bytes` to `width`×`height`, cropped like CSS `object-fit: cover`.
pub fn cover_rgba(bytes: &[u8], width: u32, height: u32) -> Result<Cover, image::ImageError> {
    let image = image::load_from_memory(bytes)?;
    let rgba = fill(image, width.max(1), height.max(1)).into_rgba8();
    Ok(Cover { width: rgba.width(), height: rgba.height(), rgba: rgba.into_raw() })
}

/// Crops the centre to the target's aspect ratio, then shrinks it to the target; never enlarges.
fn fill(image: DynamicImage, width: u32, height: u32) -> DynamicImage {
    let (source_width, source_height) = (image.width(), image.height());
    let scale = (width as f32 / source_width as f32).max(height as f32 / source_height as f32);
    let crop_width = ((width as f32 / scale).round() as u32).clamp(1, source_width);
    let crop_height = ((height as f32 / scale).round() as u32).clamp(1, source_height);
    let cropped =
        image.crop_imm((source_width - crop_width) / 2, (source_height - crop_height) / 2, crop_width, crop_height);
    if scale < 1.0 { cropped.resize_exact(width, height, FilterType::Triangle) } else { cropped }
}

fn embedded(track_path: &Path) -> Option<Vec<u8>> {
    let file = lofty::read_from_path(track_path).ok()?;
    let tag = file.primary_tag().or_else(|| file.first_tag())?;
    let pictures = tag.pictures();
    let picture = pictures.iter().find(|p| p.pic_type() == PictureType::CoverFront).or_else(|| pictures.first())?;
    Some(picture.data().to_vec())
}

fn sidecar(track_path: &Path) -> Option<Vec<u8>> {
    let dir = track_path.parent()?;
    SIDECAR_NAMES.iter().find_map(|name| std::fs::read(dir.join(name)).ok())
}

/// Decoded covers kept for reuse under a byte budget; the least recently used go first once it is passed.
/// `V` is whatever the caller draws from (an image handle); `None` remembers a song without a cover.
pub struct CoverCache<K, V> {
    limit: usize,
    used: usize,
    entries: VecDeque<(K, Option<V>, usize)>,
}

impl<K: PartialEq, V> CoverCache<K, V> {
    pub fn new(limit_bytes: usize) -> Self {
        Self { limit: limit_bytes, used: 0, entries: VecDeque::new() }
    }

    /// The entry for `key`, without counting it as a use.
    pub fn peek(&self, key: &K) -> Option<&Option<V>> {
        self.entries.iter().find(|(k, ..)| k == key).map(|(_, value, _)| value)
    }

    /// Marks `key` as just used so it is kept longest.
    pub fn touch(&mut self, key: &K) {
        if let Some(at) = self.entries.iter().position(|(k, ..)| k == key)
            && let Some(entry) = self.entries.remove(at)
        {
            self.entries.push_back(entry);
        }
    }

    /// Adds or replaces `key`, whose pixels take `bytes`, then drops the oldest until the budget holds.
    pub fn insert(&mut self, key: K, value: Option<V>, bytes: usize) {
        if let Some(at) = self.entries.iter().position(|(k, ..)| *k == key)
            && let Some((_, _, old)) = self.entries.remove(at)
        {
            self.used -= old;
        }
        self.entries.push_back((key, value, bytes));
        self.used += bytes;
        while self.used > self.limit && self.entries.len() > 1 {
            if let Some((_, _, freed)) = self.entries.pop_front() {
                self.used -= freed;
            }
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.used = 0;
    }

    /// Bytes of pixels held now.
    pub fn used_bytes(&self) -> usize {
        self.used
    }
}

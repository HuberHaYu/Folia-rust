// The wall beside an album poster (`bravaisAlbumWall`): one cover laid across each block, or shards of the covers.

use std::collections::HashMap;

use folia_core::Track;
use iced::widget::image::Handle;
use iced::{Color, Rectangle, Size};

/// `SHARD_ZOOM`, `SPOT_COUNT`.
const SHARD_ZOOM: (f32, f32) = (2.0, 3.5);
const SPOT_COUNT: usize = 6;
/// The spots a flat picture falls back to (`FALLBACK_SPOTS`).
const FALLBACK_SPOTS: [(f32, f32); 5] = [(0.5, 0.5), (0.35, 0.4), (0.65, 0.6), (0.4, 0.65), (0.6, 0.35)];
/// `.bravais-tile-art[data-bravais-art="shard"]::after`: black dots at 24%, 1.1px, every 6px; baked once.
pub const DOTS_SIDE: u32 = 240;

/// `getCoverIdentity` for NetEase: the picture's path, whatever `pN` host or size it is asked at.
pub fn cover_identity(url: &str) -> String {
    let url = url.trim();
    let bare = url.split(['?', '#']).next().unwrap_or(url);
    let rest = bare.strip_prefix("https://").or_else(|| bare.strip_prefix("http://"));
    if let Some((host, path)) = rest.and_then(|rest| rest.split_once('/'))
        && host.strip_prefix('p').and_then(|host| host.split_once('.')).is_some_and(|(number, domain)| {
            !number.is_empty()
                && number.chars().all(|c| c.is_ascii_digit())
                && domain.eq_ignore_ascii_case("music.126.net")
        })
    {
        return format!("netease:/{path}");
    }
    bare.to_owned()
}

/// `assignAlbumCoverArt`: how each song of the album is drawn.
#[derive(Debug, Clone, PartialEq)]
pub enum Assignment {
    /// Every song shares one cover (or none): that cover laid across each block.
    Mosaic(String),
    /// Some songs have their own cover: each first appearance whole, the rest shards of their cover (or the album's).
    Shards(Vec<Option<String>>),
    Plain,
}

pub fn assign(tracks: &[Track], album: Option<&str>) -> Assignment {
    let album_id = album.map(cover_identity).filter(|id| !id.is_empty());
    let mut covers: Vec<String> =
        tracks.iter().filter_map(|track| track.cover_url.as_deref()).map(cover_identity).collect();
    covers.sort();
    covers.dedup();
    let single = covers.is_empty() || (covers.len() == 1 && album_id.as_ref().is_none_or(|id| covers.contains(id)));
    if single {
        let url = album.map(str::to_owned).or_else(|| tracks.iter().find_map(|track| track.cover_url.clone()));
        return url.map_or(Assignment::Plain, Assignment::Mosaic);
    }
    let mut seen = std::collections::HashSet::new();
    Assignment::Shards(
        tracks
            .iter()
            .map(|track| match track.cover_url.as_deref() {
                None => album.map(str::to_owned),
                Some(url) if seen.insert(cover_identity(url)) => None,
                Some(url) => Some(url.to_owned()),
            })
            .collect(),
    )
}

/// `findCoverSpots`: the windows of a square picture with the most edge detail, at least a window apart.
pub fn find_spots(rgba: &[u8], side: usize) -> Vec<(f32, f32)> {
    if side < 3 || rgba.len() < side * side * 4 {
        return FALLBACK_SPOTS.to_vec();
    }
    let lum: Vec<f32> = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .take(side * side)
        .map(|p| 0.2126 * f32::from(p[0]) + 0.7152 * f32::from(p[1]) + 0.0722 * f32::from(p[2]))
        .collect();
    let mut energy = vec![0.0f32; side * side];
    for y in 1..side - 1 {
        for x in 1..side - 1 {
            let i = y * side + x;
            energy[i] = (lum[i + 1] - lum[i - 1]).abs() + (lum[i + side] - lum[i - side]).abs();
        }
    }
    let window = ((side as f32 / 5.0).round() as usize).max(4);
    let step = ((window as f32 / 2.0).round() as usize).max(2);
    let mut candidates = Vec::new();
    let mut top = 0;
    while top + window <= side {
        let mut left = 0;
        while left + window <= side {
            let score: f32 = (top..top + window)
                .flat_map(|y| (left..left + window).map(move |x| (x, y)))
                .map(|(x, y)| energy[y * side + x])
                .sum();
            candidates.push((
                (left as f32 + window as f32 / 2.0) / side as f32,
                (top as f32 + window as f32 / 2.0) / side as f32,
                score,
            ));
            left += step;
        }
        top += step;
    }
    candidates.sort_by(|a, b| b.2.total_cmp(&a.2));
    if candidates.first().is_none_or(|first| first.2 <= 0.0) {
        return FALLBACK_SPOTS.to_vec();
    }
    let gap = window as f32 / side as f32;
    let mut spots: Vec<(f32, f32)> = Vec::new();
    for (x, y, score) in candidates {
        if score <= 0.0 {
            break;
        }
        if spots.iter().any(|spot| (spot.0 - x).abs().max((spot.1 - y).abs()) < gap) {
            continue;
        }
        spots.push((x, y));
        if spots.len() >= SPOT_COUNT {
            break;
        }
    }
    spots
}

/// `mosaicPlacement`: the square cover centred on its block, as large as the block's longer side.
pub fn mosaic_square(column: i64, row: i64, block: Size, pitch: Size) -> Rectangle {
    let side = block.width.max(block.height);
    Rectangle {
        x: column as f32 * pitch.width + (block.width - side) / 2.0,
        y: row as f32 * pitch.height + (block.height - side) / 2.0,
        width: side,
        height: side,
    }
}

/// `shardPlacement`: a spot picked by the song's key, the cover zoomed 2–3.5 times its tile's longer side with the
/// spot in the tile's middle, never past the cover's edge. Offsets are relative to the tile.
pub fn shard_placement(spots: &[(f32, f32)], seed: &str, tile: Size) -> Rectangle {
    let mut random = crate::poster::seeded(&format!("shard:{seed}"));
    let list: &[(f32, f32)] = if spots.is_empty() { &FALLBACK_SPOTS } else { spots };
    let spot = list[((random() * list.len() as f64) as usize) % list.len()];
    let size = tile.width.max(tile.height) * (SHARD_ZOOM.0 + (SHARD_ZOOM.1 - SHARD_ZOOM.0) * random() as f32);
    let clamp = |value: f32, extent: f32| value.max(extent - size).min(0.0);
    Rectangle {
        x: clamp(tile.width / 2.0 - spot.0 * size, tile.width),
        y: clamp(tile.height / 2.0 - spot.1 * size, tile.height),
        width: size,
        height: size,
    }
}

/// The shard's duotone (`filter: grayscale(1) contrast(2.2) brightness(1.3)` multiplied onto the accent, then
/// lightened by the block), baked into the picture once since iced draws no blend modes.
pub fn duotone(rgba: &[u8], accent: Color, block: Color) -> Vec<u8> {
    let mut out = Vec::with_capacity(rgba.len());
    for p in rgba.as_chunks::<4>().0 {
        let gray = (0.2126 * f32::from(p[0]) + 0.7152 * f32::from(p[1]) + 0.0722 * f32::from(p[2])) / 255.0;
        let shaped = (((gray - 0.5) * 2.2 + 0.5).clamp(0.0, 1.0) * 1.3).min(1.0);
        let channel = |a: f32, b: f32| ((a * shaped).max(b) * 255.0).round() as u8;
        out.extend_from_slice(&[
            channel(accent.r, block.r),
            channel(accent.g, block.g),
            channel(accent.b, block.b),
            255,
        ]);
    }
    out
}

/// The dot screen laid over shards, baked once into a transparent tile `DOTS_SIDE` px square.
pub fn dots() -> Handle {
    let side = DOTS_SIDE as usize;
    let mut pixels = vec![0u8; side * side * 4];
    for y in 0..side {
        for x in 0..side {
            // `radial-gradient(rgb(0 0 0 / 24%) 1.1px, transparent 1.5px)` every 6px, from each cell's middle.
            let (dx, dy) = ((x % 6) as f32 + 0.5 - 3.0, (y % 6) as f32 + 0.5 - 3.0);
            let reach = (dx * dx + dy * dy).sqrt();
            let alpha = if reach <= 1.1 {
                0.24
            } else if reach >= 1.5 {
                0.0
            } else {
                0.24 * (1.5 - reach) / 0.4
            };
            pixels[(y * side + x) * 4 + 3] = (alpha * 255.0).round() as u8;
        }
    }
    Handle::from_rgba(DOTS_SIDE, DOTS_SIDE, pixels)
}

/// One shard: the duotone picture, its spots, and the song key that picks the spot.
#[derive(Debug, Clone)]
pub struct Shard {
    pub duotone: Handle,
    pub spots: Vec<(f32, f32)>,
    pub seed: String,
}

/// What the wall draws while an album poster is on screen (`has-album-wall`).
#[derive(Debug, Clone, Default)]
pub struct AlbumWall {
    /// The mosaic's cover, and the poster's smaller copy standing in until it arrives.
    pub mosaic: Option<Handle>,
    pub stand_in: Option<Handle>,
    pub mosaic_on: bool,
    /// Shards by tile id.
    pub shards: HashMap<i64, Shard>,
    pub dots: Option<Handle>,
}

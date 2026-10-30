// Finds a track's lyrics: sidecar files next to it first, then lyrics embedded in its tags.

use std::fs;
use std::path::Path;

use encoding_rs::{Encoding, GB18030};
use folia_core::Lyrics;
use folia_lyrics::Format;

use crate::tags;

/// Sidecar formats in the order the original app prefers them; `<stem>.t.<ext>` holds a translation.
const SIDECAR_EXTENSIONS: [&str; 6] = ["lrc", "vtt", "ttml", "qrc", "yrc", "krc"];

pub fn lyrics_for(track_path: &Path) -> Option<Lyrics> {
    sidecar(track_path).or_else(|| embedded(track_path))
}

fn sidecar(track_path: &Path) -> Option<Lyrics> {
    SIDECAR_EXTENSIONS.iter().find_map(|&extension| {
        let content = read_text(&track_path.with_extension(extension))?;
        let translation = read_text(&track_path.with_extension(format!("t.{extension}"))).unwrap_or_default();
        Some(folia_lyrics::parse_file(extension, &content, &translation)).filter(|lyrics| !lyrics.lines.is_empty())
    })
}

fn embedded(track_path: &Path) -> Option<Lyrics> {
    let content = tags::embedded_lyrics(track_path)?;
    Some(folia_lyrics::parse(Format::detect(&content), &content, "", "")).filter(|lyrics| !lyrics.lines.is_empty())
}

/// Reads a text file in UTF-8, UTF-16 with a BOM, or GB18030 as the fallback for legacy Chinese files.
pub fn read_text(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    Some(decode_text(&bytes))
}

pub fn decode_text(bytes: &[u8]) -> String {
    if let Some((encoding, bom_len)) = Encoding::for_bom(bytes) {
        return encoding.decode_without_bom_handling(&bytes[bom_len..]).0.into_owned();
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        Err(_) => GB18030.decode_without_bom_handling(bytes).0.into_owned(),
    }
}

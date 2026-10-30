// Reads a track's tags with lofty, falling back to what the file name says.

use std::path::Path;

use folia_core::Track;
use lofty::prelude::*;

/// Builds a track from the file's tags; `id` is left at zero for the database to assign.
pub fn read(path: &Path) -> Track {
    let from_name = name_metadata(path);
    let mut track =
        Track { path: path.to_owned(), title: from_name.title, artist: from_name.artist, ..Track::default() };

    let file = match lofty::read_from_path(path) {
        Ok(file) => file,
        Err(err) => {
            tracing::debug!(path = %path.display(), error = &err as &dyn std::error::Error, "cannot read tags");
            return track;
        }
    };
    track.duration = file.properties().duration().as_secs_f64();
    let Some(tag) = file.primary_tag().or_else(|| file.first_tag()) else { return track };

    let text = |value: Option<std::borrow::Cow<'_, str>>| value.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty());
    if let Some(title) = text(tag.title()) {
        track.title = title;
    }
    track.artist = text(tag.artist()).or(track.artist);
    track.album = text(tag.album());
    track.album_artist =
        tag.get_string(ItemKey::AlbumArtist).map(str::trim).filter(|v| !v.is_empty()).map(str::to_owned);
    track.track_no = tag.track();
    track.disc_no = tag.disk();
    track
}

/// Lyrics stored inside the file's tags, if any.
pub fn embedded_lyrics(path: &Path) -> Option<String> {
    let file = lofty::read_from_path(path).ok()?;
    let tag = file.primary_tag().or_else(|| file.first_tag())?;
    tag.get_string(ItemKey::Lyrics).filter(|text| !text.trim().is_empty()).map(str::to_owned)
}

struct NameMetadata {
    title: String,
    artist: Option<String>,
}

/// `Artist - Title`, `Title-Artist` or `Title` from the file name, after dropping a leading `01. ` style number.
fn name_metadata(path: &Path) -> NameMetadata {
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let name = strip_track_number(&stem);

    if let Some((artist, title)) = name.split_once(" - ")
        && !title.contains(" - ")
    {
        return NameMetadata { title: title.trim().to_owned(), artist: Some(artist.trim().to_owned()) };
    }
    if let Some((title, artist)) = name.rsplit_once('-')
        && !title.trim().is_empty()
        && !artist.trim().is_empty()
    {
        return NameMetadata { title: title.trim().to_owned(), artist: Some(artist.trim().to_owned()) };
    }
    NameMetadata { title: name.trim().to_owned(), artist: None }
}

/// Drops `01. `, `01 - `, `1-01 ` and similar prefixes that CUE splitting leaves.
fn strip_track_number(name: &str) -> &str {
    let digits = |s: &str| s.bytes().take_while(u8::is_ascii_digit).count();
    let first = digits(name);
    if !(1..=3).contains(&first) {
        return name;
    }
    let mut rest = &name[first..];
    if let Some(after) = rest.strip_prefix(['-', '.'])
        && (1..=3).contains(&digits(after))
    {
        rest = &after[digits(after)..];
    }
    let trimmed = rest.trim_start();
    if let Some(after) = trimmed.strip_prefix(['-', '.']) {
        return after.trim_start();
    }
    if trimmed.len() < rest.len() { trimmed } else { name }
}

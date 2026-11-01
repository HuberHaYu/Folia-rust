// The first page of NetEase's explore feed: daily songs, the home page blocks, daily playlists and the charts.

use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::account::object;
use super::eapi::{self, Client};
use super::library::daily_songs;
use super::{Song, collection, track};
use crate::{CollectionKind, ExploreEntry, ExploreSection, OmniError, Shelf};

/// A shelf hands out this many entries in one read (`BATCH`).
const BATCH: usize = 12;
/// The home page's block of charts is left out: the charts come from NetEase's own list of them.
const CHART_BLOCK: &str = "HOMEPAGE_BLOCK_TOPLIST";

/// Reads the parts side by side in spirit: each may fail alone, all of them failing fails the page.
pub async fn first_page(client: &Client) -> Result<Vec<ExploreSection>, OmniError> {
    let signed_in = client.session().signed_in();
    let home = client.post("/api/homepage/block/page", object(json!({ "refresh": true }))).await;
    let charts = client.post("/api/toplist", Map::new()).await;
    let (daily, lists) = if signed_in {
        (Some(daily_songs(client).await), Some(client.post("/api/v1/discovery/recommend/resource", Map::new()).await))
    } else {
        (None, None)
    };
    let failed = [home.is_err(), charts.is_err(), matches!(daily, Some(Err(_))), matches!(lists, Some(Err(_)))];
    let asked = if signed_in { 4 } else { 2 };
    if failed.iter().filter(|failed| **failed).count() == asked {
        return Err(home.err().or(charts.err()).unwrap_or(OmniError::Response("explore page".to_owned())));
    }

    let mut sections = Vec::new();
    if let Some(Ok(songs)) = daily {
        let entries: Vec<ExploreEntry> = songs.into_iter().take(BATCH).map(ExploreEntry::Song).collect();
        push(&mut sections, "netease-daily-songs".to_owned(), Shelf::DailySongs, entries);
    }
    if let Ok(answer) = &home
        && eapi::check(&answer.body).is_ok()
    {
        for (key, title, entries) in blocks(&answer.body) {
            push(&mut sections, key, Shelf::Block(title), entries);
        }
    }
    if let Some(Ok(answer)) = &lists {
        let rows = answer.body.get("recommend").and_then(Value::as_array).cloned().unwrap_or_default();
        let entries =
            rows.iter().filter_map(|row| collection(row, CollectionKind::Playlist)).map(ExploreEntry::Collection);
        push(&mut sections, "netease-daily-playlists".to_owned(), Shelf::DailyPlaylists, entries.take(BATCH).collect());
    }
    if let Ok(answer) = &charts {
        let rows = answer.body.get("list").and_then(Value::as_array).cloned().unwrap_or_default();
        let entries = rows
            .iter()
            .filter_map(|row| {
                let mut chart = collection(row, CollectionKind::Playlist)?;
                chart.creator = None;
                chart.description =
                    row.get("updateFrequency").and_then(Value::as_str).map(str::to_owned).or(chart.description);
                Some(ExploreEntry::Collection(chart))
            })
            .take(BATCH)
            .collect();
        push(&mut sections, "netease-charts".to_owned(), Shelf::Charts, entries);
    }
    tracing::info!(shelves = sections.len(), signed_in, "netease explore page read");
    Ok(sections)
}

fn push(sections: &mut Vec<ExploreSection>, key: String, shelf: Shelf, entries: Vec<ExploreEntry>) {
    if !entries.is_empty() {
        sections.push(ExploreSection { key, shelf, entries });
    }
}

/// The home page's blocks as shelves under NetEase's headings: songs, playlists and albums only, each once,
/// without the charts block and NetEase's own record of the account's listening.
fn blocks(body: &Value) -> Vec<(String, String, Vec<ExploreEntry>)> {
    let mut shelves: Vec<(String, String, Vec<ExploreEntry>)> = Vec::new();
    let Some(list) = body.pointer("/data/blocks").and_then(Value::as_array) else { return shelves };
    for block in list {
        let code = block.get("blockCode").and_then(Value::as_str).unwrap_or_default();
        let title = block
            .pointer("/uiElement/subTitle/title")
            .or_else(|| block.pointer("/uiElement/mainTitle/title"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        if code == CHART_BLOCK || code.contains("RECENT") || code.contains("HISTORY") || title.starts_with("最近") {
            continue;
        }
        let mut seen = std::collections::HashSet::new();
        let entries: Vec<ExploreEntry> = block
            .get("creatives")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|creative| creative.get("resources").and_then(Value::as_array))
            .flatten()
            .filter_map(entry)
            .filter(|entry| seen.insert(entry_key(entry)))
            .take(BATCH)
            .collect();
        if entries.is_empty() {
            continue;
        }
        let mut key = format!("netease-block-{code}");
        let mut count = 2;
        while shelves.iter().any(|(taken, ..)| *taken == key) {
            key = format!("netease-block-{code}-{count}");
            count += 1;
        }
        shelves.push((key, title.to_owned(), entries));
    }
    shelves
}

/// One resource of a block: a song, a playlist (`list`) or an album; anything else is left out.
fn entry(resource: &Value) -> Option<ExploreEntry> {
    let cover = resource.pointer("/uiElement/image/imageUrl").and_then(Value::as_str).filter(|url| !url.is_empty());
    let kind = resource.get("resourceType").and_then(Value::as_str)?;
    if kind == "song" {
        let raw =
            resource.pointer("/resourceExtInfo/songData").or_else(|| resource.pointer("/resourceExtInfo/song"))?;
        let mut song = track(Song::deserialize(raw).ok()?);
        if song.cover_url.is_none() {
            song.cover_url = cover.map(|url| super::https(url.to_owned()));
        }
        return Some(ExploreEntry::Song(song));
    }
    let id = match resource.get("resourceId")? {
        Value::Number(number) => number.to_string(),
        Value::String(text) => text.clone(),
        _ => return None,
    };
    let name = resource.pointer("/uiElement/mainTitle/title").and_then(Value::as_str)?;
    let artists = resource.pointer("/resourceExtInfo/artists").cloned().unwrap_or(Value::Null);
    let row = json!({ "id": id, "name": name, "picUrl": cover, "artists": artists });
    match kind {
        "list" => collection(&row, CollectionKind::Playlist).map(ExploreEntry::Collection),
        "album" => collection(&row, CollectionKind::Album).map(ExploreEntry::Collection),
        _ => None,
    }
}

fn entry_key(entry: &ExploreEntry) -> String {
    match entry {
        ExploreEntry::Song(track) => format!("song:{}", track.online.as_ref().map_or("", |online| online.id.as_str())),
        ExploreEntry::Collection(collection) => format!("{:?}:{}", collection.kind, collection.id),
    }
}

// The signed-in account's library: its playlists, a collection's songs, saved albums and the radio feed.

use folia_core::Track;
use serde_json::{Map, Value, json};

use super::account::object;
use super::eapi::{self, Client};
use super::{collection, tracks};
use crate::{Collection, CollectionKind, OmniError, RadioFeed};

const PLAYLIST_PAGE: u64 = 50;
/// `FAVORITE_ALBUMS_PAGE_SIZE`.
const ALBUM_PAGE: u64 = 50;
/// `song_detail` takes this many ids at a time.
const DETAIL_BATCH: usize = 500;

/// `user_playlist`, page after page: the account's own playlists and the ones it saved.
pub async fn user_playlists(client: &Client, user: &str) -> Result<Vec<Collection>, OmniError> {
    let mut all = Vec::new();
    let mut offset = 0;
    loop {
        let params = object(json!({ "uid": user, "limit": PLAYLIST_PAGE, "offset": offset, "includeVideo": true }));
        let answer = client.post("/api/user/playlist", params).await?;
        eapi::check(&answer.body)?;
        let rows = answer.body.get("playlist").and_then(Value::as_array).cloned().unwrap_or_default();
        let count = rows.len() as u64;
        all.extend(rows.iter().filter_map(|row| collection(row, CollectionKind::Playlist)));
        let more = answer.body.get("more").and_then(Value::as_bool).unwrap_or(count == PLAYLIST_PAGE);
        if !more || count == 0 {
            return Ok(all);
        }
        offset += count;
    }
}

pub async fn collection_tracks(client: &Client, collection: &Collection) -> Result<Vec<Track>, OmniError> {
    match collection.kind {
        CollectionKind::Playlist => playlist_tracks(client, &collection.id).await,
        CollectionKind::Album => {
            let answer = client.post(&format!("/api/v1/album/{}", digits(&collection.id)?), Map::new()).await?;
            eapi::check(&answer.body)?;
            Ok(tracks(answer.body.get("songs")))
        }
        CollectionKind::DailySongs => daily_songs(client).await,
        CollectionKind::PersonalFm => personal_fm(client).await,
    }
}

/// `playlist_track_all`: the playlist's ids, then their songs in batches, in the playlist's order.
async fn playlist_tracks(client: &Client, id: &str) -> Result<Vec<Track>, OmniError> {
    let answer = client.post("/api/v6/playlist/detail", object(json!({ "id": id, "n": 100000, "s": 8 }))).await?;
    eapi::check(&answer.body)?;
    let ids: Vec<u64> = answer
        .body
        .pointer("/playlist/trackIds")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().filter_map(|row| row.get("id").and_then(Value::as_u64)).collect())
        .unwrap_or_default();
    let mut songs = Vec::with_capacity(ids.len());
    for batch in ids.chunks(DETAIL_BATCH) {
        let wanted: Vec<Value> = batch.iter().map(|id| json!({ "id": id })).collect();
        let answer =
            client.post("/api/v3/song/detail", object(json!({ "c": Value::Array(wanted).to_string() }))).await?;
        eapi::check(&answer.body)?;
        songs.extend(tracks(answer.body.get("songs")));
    }
    Ok(songs)
}

/// `album_sublist`, every page.
pub async fn favorite_albums(client: &Client) -> Result<Vec<Collection>, OmniError> {
    let mut all = Vec::new();
    let mut offset = 0;
    loop {
        let params = object(json!({ "limit": ALBUM_PAGE, "offset": offset, "total": true }));
        let answer = client.post("/api/album/sublist", params).await?;
        eapi::check(&answer.body)?;
        let rows = answer.body.get("data").and_then(Value::as_array).cloned().unwrap_or_default();
        let count = rows.len() as u64;
        all.extend(rows.iter().filter_map(|row| collection(row, CollectionKind::Album)));
        let more = answer.body.get("hasMore").and_then(Value::as_bool).unwrap_or(false);
        if !more || count == 0 {
            return Ok(all);
        }
        offset += count;
    }
}

/// `getHomeFeed`: the three parts each succeed or fail alone; all three failing fails the feed.
pub async fn radio_feed(client: &Client, limit: u32) -> Result<RadioFeed, OmniError> {
    let fm = personal_fm(client).await;
    let daily = daily_songs(client).await;
    let recommended = recommended_playlists(client, limit).await;
    if let (Err(err), Err(_), Err(_)) = (&fm, &daily, &recommended) {
        return Err(OmniError::Response(err.to_string()));
    }
    let partial = fm.is_err() || daily.is_err() || recommended.is_err();
    for err in [fm.as_ref().err(), daily.as_ref().err(), recommended.as_ref().err()].into_iter().flatten() {
        tracing::warn!(error = err as &dyn std::error::Error, "netease home feed part failed");
    }
    Ok(RadioFeed {
        personal_fm: fm.unwrap_or_default(),
        daily_songs: daily.unwrap_or_default(),
        recommended: recommended.unwrap_or_default(),
        partial,
    })
}

/// `personal_fm`: NetEase hands out three songs at a time.
async fn personal_fm(client: &Client) -> Result<Vec<Track>, OmniError> {
    let answer = client.post("/api/v1/radio/get", Map::new()).await?;
    eapi::check(&answer.body)?;
    Ok(tracks(answer.body.get("data")))
}

/// `recommend_songs`.
pub(super) async fn daily_songs(client: &Client) -> Result<Vec<Track>, OmniError> {
    let answer = client.post("/api/v3/discovery/recommend/songs", Map::new()).await?;
    eapi::check(&answer.body)?;
    let list = answer.body.pointer("/data/dailySongs").or_else(|| answer.body.get("recommend"));
    Ok(tracks(list))
}

/// `personalized`: the recommended playlists.
async fn recommended_playlists(client: &Client, limit: u32) -> Result<Vec<Collection>, OmniError> {
    let params = object(json!({ "limit": limit, "total": true, "n": 1000 }));
    let answer = client.post("/api/personalized/playlist", params).await?;
    eapi::check(&answer.body)?;
    let rows = answer.body.get("result").and_then(Value::as_array).cloned().unwrap_or_default();
    Ok(rows.iter().filter_map(|row| collection(row, CollectionKind::Playlist)).collect())
}

/// An id put into a path: digits only.
fn digits(id: &str) -> Result<&str, OmniError> {
    if !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit()) {
        Ok(id)
    } else {
        Err(OmniError::Response(format!("not a NetEase id: {id}")))
    }
}

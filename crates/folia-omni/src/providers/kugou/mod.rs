// KuGou Music over the lite-client gateway: search, song URLs, KRC lyrics, QR sign-in and the account's library.

mod account;
mod gateway;
mod library;
mod lyrics;

use folia_core::{Lyrics, OnlineId, Track};
use serde_json::Value;

use crate::{
    Account, AudioSource, BoxFuture, Collection, CollectionDetail, CollectionKind, ExploreSection, LoginCode,
    LoginState, OmniError, Page, Provider, RadioFeed,
};

const ID: &str = "kugou";
/// `KUGOU_CANONICAL_COVER_SIZE`.
const COVER_SIZE: u32 = 1024;

pub struct Kugou {
    client: gateway::Client,
    /// Keywords whose signed-in first page came back empty: later pages search anonymously too.
    anonymous: std::sync::Mutex<std::collections::VecDeque<String>>,
}

impl Kugou {
    pub fn new(http: reqwest::Client) -> Self {
        Self { client: gateway::Client::new(http), anonymous: std::sync::Mutex::default() }
    }
}

impl Provider for Kugou {
    fn id(&self) -> &'static str {
        ID
    }

    fn display_name(&self) -> &'static str {
        "酷狗音乐"
    }

    fn search_songs<'a>(
        &'a self,
        query: &'a str,
        limit: u32,
        offset: u32,
    ) -> BoxFuture<'a, Result<Page<Track>, OmniError>> {
        Box::pin(library::search(&self.client, &self.anonymous, query, limit, offset))
    }

    fn audio_source<'a>(&'a self, song: &'a OnlineId) -> BoxFuture<'a, Result<AudioSource, OmniError>> {
        Box::pin(library::audio_source(&self.client, SongKey::parse(&song.id)))
    }

    fn lyrics<'a>(&'a self, song: &'a OnlineId) -> BoxFuture<'a, Result<Option<Lyrics>, OmniError>> {
        Box::pin(lyrics::lyrics(&self.client, SongKey::parse(&song.id)))
    }

    fn login_code(&self) -> BoxFuture<'_, Result<LoginCode, OmniError>> {
        Box::pin(account::login_code(&self.client))
    }

    fn check_login<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<LoginState, OmniError>> {
        Box::pin(account::check_login(&self.client, key))
    }

    fn account(&self) -> BoxFuture<'_, Result<Option<Account>, OmniError>> {
        Box::pin(account::account(&self.client))
    }

    fn logout(&self) -> BoxFuture<'_, Result<(), OmniError>> {
        Box::pin(async move {
            self.client.update(|session| {
                session.token = None;
                session.userid = None;
            });
            self.anonymous.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clear();
            Ok(())
        })
    }

    fn session(&self) -> Option<String> {
        let session = self.client.session();
        if !session.signed_in() {
            return None;
        }
        serde_json::to_string(&session).ok()
    }

    fn restore_session(&self, saved: &str) -> Result<(), OmniError> {
        let session: gateway::Session =
            serde_json::from_str(saved).map_err(|err| OmniError::Response(format!("kept session: {err}")))?;
        self.client.set_session(session);
        Ok(())
    }

    fn user_playlists<'a>(&'a self, _user: &'a str) -> BoxFuture<'a, Result<Vec<Collection>, OmniError>> {
        Box::pin(library::user_collections(&self.client, CollectionKind::Playlist))
    }

    fn collection_detail<'a>(
        &'a self,
        collection: &'a Collection,
    ) -> BoxFuture<'a, Result<CollectionDetail, OmniError>> {
        Box::pin(library::collection_detail(&self.client, collection))
    }

    fn set_subscribed<'a>(
        &'a self,
        collection: &'a Collection,
        subscribed: bool,
    ) -> BoxFuture<'a, Result<(), OmniError>> {
        Box::pin(library::set_subscribed(&self.client, collection, subscribed))
    }

    fn favorite_albums(&self) -> BoxFuture<'_, Result<Vec<Collection>, OmniError>> {
        Box::pin(library::user_collections(&self.client, CollectionKind::Album))
    }

    fn radio_feed(&self, limit: u32) -> BoxFuture<'_, Result<RadioFeed, OmniError>> {
        Box::pin(library::radio_feed(&self.client, limit))
    }

    /// The original's KuGou provider has no explore page.
    fn explore(&self) -> BoxFuture<'_, Result<Vec<ExploreSection>, OmniError>> {
        Box::pin(async { Err(OmniError::Unsupported("the explore page")) })
    }

    /// `withKugouCoverSize`: the size in `/stdmusic/<size>/` on `kugou.com` and `kgimg.com`.
    fn sized_cover(&self, url: &str, px: u32) -> Option<String> {
        let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"))?;
        let host = rest.split('/').next()?;
        let ours =
            host == "kugou.com" || host.ends_with(".kugou.com") || host == "kgimg.com" || host.ends_with(".kgimg.com");
        if !ours {
            return None;
        }
        let at = url.find("/stdmusic/")? + "/stdmusic/".len();
        let digits = url[at..].bytes().take_while(u8::is_ascii_digit).count();
        (digits > 0 && url[at + digits..].starts_with('/'))
            .then(|| format!("{}{px}{}", &url[..at], &url[at + digits..]))
    }
}

/// What a KuGou song id carries: the file hash, and the album ids `song_url` and the lyric search take.
pub(super) struct SongKey {
    hash: String,
    album_id: String,
    album_audio_id: String,
}

impl SongKey {
    /// `HASH-albumId-albumAudioId`, the last two possibly empty.
    fn parse(id: &str) -> Self {
        let mut parts = id.split('-');
        let mut next = || parts.next().unwrap_or_default().to_owned();
        Self { hash: next().to_uppercase(), album_id: next(), album_audio_id: next() }
    }
}

/// `valueOf`: the first key present and not null.
fn value_of<'a>(raw: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().find_map(|key| raw.get(*key).filter(|value| !value.is_null()))
}

/// A value as text, numbers included.
fn text_of(raw: &Value, keys: &[&str]) -> Option<String> {
    match value_of(raw, keys)? {
        Value::String(text) if !text.is_empty() => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn number_of(raw: &Value, keys: &[&str]) -> Option<f64> {
    match value_of(raw, keys)? {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

/// `dataOf`.
fn data_of(raw: &Value) -> &Value {
    raw.get("data").filter(|data| !data.is_null()).unwrap_or(raw)
}

/// `listOf`: the first list under the usual keys, breadth first.
fn list_of(raw: &Value) -> Vec<Value> {
    const KEYS: [&str; 21] = [
        "lists",
        "list",
        "info",
        "songs",
        "song_list",
        "songlist",
        "songList",
        "audios",
        "albums",
        "items",
        "records",
        "special",
        "specials",
        "special_list",
        "playlist",
        "playlists",
        "recommend_list",
        "recommendations",
        "data",
        "result",
        "response",
    ];
    let mut queue = std::collections::VecDeque::from([data_of(raw)]);
    while let Some(current) = queue.pop_front() {
        if let Value::Array(rows) = current {
            return rows.clone();
        }
        let Value::Object(map) = current else { continue };
        for key in KEYS {
            match map.get(key) {
                Some(Value::Array(rows)) => return rows.clone(),
                Some(value @ Value::Object(_)) => queue.push_back(value),
                _ => {}
            }
        }
    }
    Vec::new()
}

fn is_song(raw: &Value) -> bool {
    const KEYS: [&str; 11] = [
        "FileHash",
        "fileHash",
        "hash",
        "Hash",
        "songid",
        "songId",
        "SongName",
        "songname",
        "songName",
        "ori_audio_name",
        "audio_name",
    ];
    value_of(raw, &KEYS).is_some()
        || raw.get("base").is_some_and(|base| value_of(base, &["songid", "songId", "songname", "songName"]).is_some())
}

/// `songListOf`: the first list holding songs, through any number of card envelopes.
fn song_list_of(raw: &Value) -> Vec<Value> {
    const KEYS: [&str; 15] = [
        "song_list",
        "songlist",
        "songList",
        "songs",
        "audios",
        "list",
        "lists",
        "info",
        "items",
        "records",
        "song_info",
        "songInfo",
        "data",
        "result",
        "response",
    ];
    let mut queue = std::collections::VecDeque::from([data_of(raw)]);
    while let Some(current) = queue.pop_front() {
        match current {
            Value::Array(rows) => {
                if rows.iter().any(is_song) {
                    return rows.clone();
                }
                queue.extend(rows.iter().filter(|row| row.is_object()));
            }
            Value::Object(map) => {
                if is_song(current) {
                    return vec![current.clone()];
                }
                queue.extend(KEYS.iter().filter_map(|key| map.get(*key)).filter(|v| v.is_object() || v.is_array()));
            }
            _ => {}
        }
    }
    Vec::new()
}

/// `coverOf`: album art first, `{size}` filled, bare file names put on the CDN.
fn cover_of(raw: &Value) -> Option<String> {
    let nested = |outer: &str, keys: &[&str]| raw.get(outer).and_then(|inner| text_of(inner, keys));
    let deep = |a: &str, b: &str| raw.get(a).and_then(|inner| inner.get(b)).and_then(|v| text_of(v, &["union_cover"]));
    let value = nested("album_info", &["sizable_cover", "cover"])
        .or_else(|| nested("albumInfo", &["sizable_cover", "cover"]))
        .or_else(|| deep("audio_info", "trans_param"))
        .or_else(|| deep("audioInfo", "transParam"))
        .or_else(|| nested("trans_param", &["union_cover"]))
        .or_else(|| {
            text_of(
                raw,
                &["sizable_cover", "Image", "image", "img", "pic", "picUrl", "cover", "coverUrl", "sizable_avatar"],
            )
        })?;
    let cover = value.trim().replace("{size}", &COVER_SIZE.to_string());
    let bare = cover.len() > 12
        && cover.bytes().take_while(u8::is_ascii_digit).count() >= 8
        && [".jpg", ".jpeg", ".png", ".webp"].iter().any(|ext| cover.to_ascii_lowercase().ends_with(ext))
        && !cover.contains('/');
    if bare {
        return Some(format!("https://c1.kgimg.com/stdmusic/{COVER_SIZE}/{}/{cover}", &cover[..8]));
    }
    let secure = if let Some(rest) = cover.strip_prefix("//") {
        format!("https://{rest}")
    } else if let Some(rest) = cover.strip_prefix("http://") {
        format!("https://{rest}")
    } else {
        cover
    };
    (!secure.is_empty()).then_some(secure)
}

/// `normalizeArtists`: the singer list, else the names in `SingerName` split on KuGou's separators.
fn artists_of(raw: &Value) -> Vec<String> {
    if let Some(Value::Array(singers)) = value_of(raw, &["Singers", "singers", "singerinfo", "authors", "artists"]) {
        return singers
            .iter()
            .filter_map(|artist| text_of(artist.get("base").unwrap_or(artist), &["name", "author_name", "singername"]))
            .collect();
    }
    text_of(raw, &["SingerName", "singername", "author_name", "Singer"])
        .map(|names| split_names(&names, &['、', ',', '&', '/']))
        .unwrap_or_default()
}

fn split_names(names: &str, separators: &[char]) -> Vec<String> {
    names.split(separators).map(str::trim).filter(|name| !name.is_empty()).map(str::to_owned).collect()
}

/// `normalizeKugouSong`; `None` without a file hash.
fn track(raw: &Value) -> Option<Track> {
    let base = raw.get("base").unwrap_or(&Value::Null);
    let audio = raw.get("audio_info").or_else(|| raw.get("audioInfo")).unwrap_or(&Value::Null);
    let hash = text_of(raw, &["FileHash", "fileHash", "hash", "Hash", "audio_hash", "audioHash", "kgHash"])
        .or_else(|| text_of(audio, &["hash", "hash_128"]))?
        .to_uppercase();
    let album = value_of(raw, &["album_info", "albumInfo", "albuminfo", "album"]).unwrap_or(&Value::Null);
    let album_id = text_of(raw, &["AlbumID", "album_id", "albumId"])
        .or_else(|| text_of(album, &["album_id", "albumId"]))
        .or_else(|| text_of(base, &["album_id", "albumId"]))
        .unwrap_or_default();
    let album_name = text_of(raw, &["AlbumName", "album_name", "albumName", "albumname"])
        .or_else(|| text_of(album, &["name", "album_name", "albumName"]))
        .or_else(|| text_of(base, &["album_name", "albumName"]));
    let duration = number_of(raw, &["Duration", "duration", "timelen", "timeLength", "timelength", "time_length"])
        .or_else(|| number_of(audio, &["duration", "timelength", "duration_128"]))
        .unwrap_or(0.0);
    let seconds = if duration > 0.0 && duration < 10000.0 { duration } else { duration / 1000.0 };
    let album_audio_id = text_of(raw, &["album_audio_id", "albumAudioId"])
        .or_else(|| text_of(base, &["album_audio_id", "albumAudioId"]))
        .or_else(|| text_of(raw, &["MixSongID", "mixsongid", "mixSongId"]))
        .or_else(|| text_of(base, &["MixSongID", "mixsongid", "mixSongId"]))
        .unwrap_or_default();
    let title_keys = [
        "SongName",
        "songname",
        "songName",
        "ori_audio_name",
        "oriAudioName",
        "OriSongName",
        "ori_song_name",
        "oriSongName",
        "FileName",
        "filename",
        "fileName",
        "name",
        "audio_name",
    ];
    let raw_title = text_of(raw, &title_keys)
        .or_else(|| text_of(audio, &["ori_audio_name", "oriAudioName", "audio_name", "audioName"]))
        .or_else(|| text_of(base, &["SongName", "songname", "songName", "audio_name", "audioName"]))
        .unwrap_or_default();
    let (title, artists) = title_and_artists(&raw_title, artists_of(raw));
    let online = OnlineId { provider: ID.to_owned(), id: format!("{hash}-{album_id}-{album_audio_id}") };
    Some(Track {
        id: online.track_id(),
        title,
        artist: (!artists.is_empty()).then(|| artists.join(" / ")),
        album: album_name,
        cover_url: cover_of(raw),
        duration: seconds,
        online: Some(online),
        ..Track::default()
    })
}

/// `normalizeKugouTitleAndArtists`: some lists only give `Artist - Title` in the song's name.
fn title_and_artists(raw_title: &str, artists: Vec<String>) -> (String, Vec<String>) {
    let raw_title = raw_title.trim();
    if !artists.is_empty() {
        let prefix = format!("{} - ", artists.join("、"));
        let title = raw_title.strip_prefix(&prefix).unwrap_or(raw_title);
        return (title.to_owned(), artists);
    }
    let Some((names, title)) = raw_title.split_once(" - ") else { return (raw_title.to_owned(), artists) };
    let names: Vec<String> = split_names(names, &['、', ',', '&', '/'])
        .into_iter()
        .flat_map(|name| {
            let lower = name.to_lowercase();
            match [" feat. ", " feat ", " ft. ", " ft ", " featuring "].iter().find_map(|sep| lower.find(sep)) {
                Some(at) => vec![name[..at].trim().to_owned()],
                None => vec![name],
            }
        })
        .filter(|name| !name.is_empty())
        .collect();
    if names.is_empty() { (raw_title.to_owned(), artists) } else { (title.trim().to_owned(), names) }
}

fn tracks(rows: &[Value]) -> Vec<Track> {
    let mut seen = std::collections::HashSet::new();
    rows.iter().filter_map(track).filter(|track| seen.insert(track.online.clone())).collect()
}

/// `normalizeCollection` for a playlist or an album row.
fn collection(raw: &Value, kind: CollectionKind) -> Option<Collection> {
    let id = match kind {
        CollectionKind::Album => {
            text_of(raw, &["album_id", "AlbumID", "albumId", "musiclib_id", "musicLibId", "list_create_listid"])
        }
        _ => text_of(raw, &["global_collection_id", "globalCollectionId", "specialid", "specialId", "id"]),
    }?;
    let name = text_of(raw, &["name", "listname", "specialname", "album_name", "author_name"])?;
    let artists = artists_of(raw);
    let creator = if artists.is_empty() {
        text_of(raw, &["list_create_username", "nickname", "username"])
    } else {
        Some(artists.join(", "))
    };
    let count = number_of(raw, &["song_count", "count", "m_count", "total", "music_num"]);
    Some(Collection {
        provider: ID.to_owned(),
        id,
        kind,
        name,
        cover_url: cover_of(raw),
        track_count: count.filter(|count| *count >= 0.0).map(|count| count as u32),
        creator,
        description: text_of(raw, &["intro", "description", "brief_desc", "briefDesc", "brief_description", "desc"]),
    })
}

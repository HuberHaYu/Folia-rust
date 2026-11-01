// NetEase Cloud Music: anonymous search, song URLs and lyrics, plus QR sign-in and the account's library over eapi.

mod account;
mod eapi;
mod explore;
mod library;

use folia_core::{Lyrics, OnlineId, Track};
use folia_lyrics::Format;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::{
    Account, AudioSource, BoxFuture, Collection, CollectionKind, ExploreSection, LoginCode, LoginState, OmniError,
    Page, Provider, RadioFeed,
};

const ID: &str = "netease";
const BASE: &str = "https://music.163.com";

pub struct Netease {
    http: reqwest::Client,
    client: eapi::Client,
}

#[derive(Deserialize)]
struct SearchResponse {
    code: i64,
    result: Option<SearchResult>,
}

#[derive(Deserialize)]
struct SearchResult {
    #[serde(default)]
    songs: Vec<Song>,
    #[serde(rename = "songCount")]
    song_count: Option<u64>,
}

/// A song in either of NetEase's shapes: `ar`/`al`/`dt`, or the older `artists`/`album`/`duration`.
#[derive(Deserialize)]
struct Song {
    id: u64,
    name: String,
    #[serde(default, alias = "artists")]
    ar: Vec<Named>,
    #[serde(alias = "album")]
    al: Option<Album>,
    #[serde(default, alias = "duration")]
    dt: u64,
}

#[derive(Deserialize)]
struct Named {
    name: Option<String>,
}

#[derive(Deserialize)]
struct Album {
    name: Option<String>,
    #[serde(rename = "picUrl")]
    pic_url: Option<String>,
}

#[derive(Deserialize)]
struct UrlResponse {
    code: i64,
    #[serde(default)]
    data: Vec<UrlEntry>,
}

#[derive(Deserialize)]
struct UrlEntry {
    url: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
    #[serde(rename = "freeTrialInfo")]
    free_trial: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct LyricResponse {
    code: i64,
    #[serde(rename = "pureMusic", default)]
    pure_music: bool,
    lrc: Option<LyricText>,
    tlyric: Option<LyricText>,
    romalrc: Option<LyricText>,
    yrc: Option<LyricText>,
}

#[derive(Deserialize)]
struct LyricText {
    lyric: Option<String>,
}

impl Netease {
    pub fn new(http: reqwest::Client) -> Self {
        Self { client: eapi::Client::new(http.clone()), http }
    }

    /// Sends an anonymous request to the web API and decodes its JSON answer.
    async fn call<T: DeserializeOwned>(&self, request: reqwest::RequestBuilder) -> Result<T, OmniError> {
        Ok(request.header("Referer", BASE).send().await?.error_for_status()?.json().await?)
    }
}

impl Provider for Netease {
    fn id(&self) -> &'static str {
        ID
    }

    fn display_name(&self) -> &'static str {
        "网易云音乐"
    }

    fn search_songs<'a>(
        &'a self,
        query: &'a str,
        limit: u32,
        offset: u32,
    ) -> BoxFuture<'a, Result<Page<Track>, OmniError>> {
        Box::pin(async move {
            let form = [
                ("s", query.to_owned()),
                ("type", "1".to_owned()),
                ("limit", limit.to_string()),
                ("offset", offset.to_string()),
            ];
            let response: SearchResponse =
                self.call(self.http.post(format!("{BASE}/api/cloudsearch/pc")).form(&form)).await?;
            check(response.code)?;
            let result = response.result.unwrap_or(SearchResult { songs: Vec::new(), song_count: Some(0) });
            Ok(Page { items: result.songs.into_iter().map(track).collect(), total: result.song_count })
        })
    }

    fn audio_source<'a>(&'a self, song: &'a OnlineId) -> BoxFuture<'a, Result<AudioSource, OmniError>> {
        Box::pin(async move {
            let url = format!("{BASE}/api/song/enhance/player/url?ids=[{}]&br=320000", song.id);
            let response: UrlResponse = self.call(self.http.get(url)).await?;
            check(response.code)?;
            let entry = response.data.into_iter().next().ok_or(OmniError::Unavailable)?;
            let url = entry.url.filter(|url| !url.is_empty()).ok_or(OmniError::Unavailable)?;
            let extension = entry.kind.map(|kind| kind.to_ascii_lowercase()).unwrap_or_else(|| "mp3".to_owned());
            Ok(AudioSource { url, extension, preview: entry.free_trial.is_some_and(|info| !info.is_null()) })
        })
    }

    fn lyrics<'a>(&'a self, song: &'a OnlineId) -> BoxFuture<'a, Result<Option<Lyrics>, OmniError>> {
        Box::pin(async move {
            let url = format!("{BASE}/api/song/lyric/v1?id={}&lv=-1&tv=-1&rv=-1&yv=-1&ytv=-1&yrv=-1", song.id);
            let response: LyricResponse = self.call(self.http.get(url)).await?;
            check(response.code)?;
            Ok(lyrics(response))
        })
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
        Box::pin(account::logout(&self.client))
    }

    fn session(&self) -> Option<String> {
        let session = self.client.session();
        if !session.signed_in() {
            return None;
        }
        serde_json::to_string(&session).ok()
    }

    fn restore_session(&self, saved: &str) -> Result<(), OmniError> {
        let session: eapi::Session =
            serde_json::from_str(saved).map_err(|err| OmniError::Response(format!("kept session: {err}")))?;
        self.client.set_session(session);
        Ok(())
    }

    fn user_playlists<'a>(&'a self, user: &'a str) -> BoxFuture<'a, Result<Vec<Collection>, OmniError>> {
        Box::pin(library::user_playlists(&self.client, user))
    }

    fn collection_tracks<'a>(&'a self, collection: &'a Collection) -> BoxFuture<'a, Result<Vec<Track>, OmniError>> {
        Box::pin(library::collection_tracks(&self.client, collection))
    }

    fn favorite_albums(&self) -> BoxFuture<'_, Result<Vec<Collection>, OmniError>> {
        Box::pin(library::favorite_albums(&self.client))
    }

    fn radio_feed(&self, limit: u32) -> BoxFuture<'_, Result<RadioFeed, OmniError>> {
        Box::pin(library::radio_feed(&self.client, limit))
    }

    fn explore(&self) -> BoxFuture<'_, Result<Vec<ExploreSection>, OmniError>> {
        Box::pin(explore::first_page(&self.client))
    }
}

/// Prefers word-timed YRC, falling back to LRC; translations and romanizations are LRC tracks.
/// Instrumentals only carry a placeholder line, so they get no lyrics.
fn lyrics(response: LyricResponse) -> Option<Lyrics> {
    if response.pure_music {
        return None;
    }
    let text = |track: Option<LyricText>| track.and_then(|t| t.lyric).filter(|lyric| !lyric.trim().is_empty());
    let translation = text(response.tlyric).unwrap_or_default();
    let romanization = text(response.romalrc).unwrap_or_default();
    let parsed = match (text(response.yrc), text(response.lrc)) {
        (Some(yrc), _) => folia_lyrics::parse(Format::Yrc, &yrc, &translation, &romanization),
        (None, Some(lrc)) => folia_lyrics::parse(Format::detect(&lrc), &lrc, &translation, &romanization),
        (None, None) => return None,
    };
    (!parsed.lines.is_empty()).then_some(parsed)
}

fn track(song: Song) -> Track {
    let online = OnlineId { provider: ID.to_owned(), id: song.id.to_string() };
    let artists: Vec<String> = song.ar.into_iter().filter_map(|artist| artist.name).collect();
    let album = song.al;
    Track {
        id: online.track_id(),
        title: song.name,
        artist: (!artists.is_empty()).then(|| artists.join(" / ")),
        album: album.as_ref().and_then(|album| album.name.clone()),
        cover_url: album.and_then(|album| album.pic_url).map(https),
        duration: song.dt as f64 / 1000.0,
        online: Some(online),
        ..Track::default()
    }
}

/// The songs of a JSON list, skipping rows that are not songs.
fn tracks(list: Option<&Value>) -> Vec<Track> {
    list.and_then(Value::as_array)
        .map(|rows| rows.iter().filter_map(|row| Song::deserialize(row).ok()).map(track).collect())
        .unwrap_or_default()
}

/// A playlist or album row: `coverImgUrl`/`picUrl`, `trackCount`/`size`, the creator or the artists.
fn collection(row: &Value, kind: CollectionKind) -> Option<Collection> {
    let id = match row.get("id")? {
        Value::Number(number) => number.to_string(),
        Value::String(text) if !text.is_empty() => text.clone(),
        _ => return None,
    };
    let text = |key: &str| row.get(key).and_then(Value::as_str).filter(|value| !value.is_empty()).map(str::to_owned);
    let name = text("name")?;
    let artists: Vec<String> = row
        .get("artists")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter().filter_map(|artist| artist.get("name").and_then(Value::as_str)).map(str::to_owned).collect()
        })
        .unwrap_or_default();
    let creator = row.pointer("/creator/nickname").and_then(Value::as_str).map(str::to_owned);
    let count = row.get("trackCount").or_else(|| row.get("size")).and_then(Value::as_u64);
    Some(Collection {
        provider: ID.to_owned(),
        id,
        kind,
        name,
        cover_url: text("coverImgUrl").or_else(|| text("picUrl")).or_else(|| text("coverUrl")).map(https),
        track_count: count.and_then(|count| u32::try_from(count).ok()),
        creator: if artists.is_empty() { creator } else { Some(artists.join(", ")) },
        description: text("description").or_else(|| text("copywriter")),
    })
}

fn https(url: String) -> String {
    url.strip_prefix("http://").map(|rest| format!("https://{rest}")).unwrap_or(url)
}

fn check(code: i64) -> Result<(), OmniError> {
    if code == 200 { Ok(()) } else { Err(OmniError::Platform(code)) }
}

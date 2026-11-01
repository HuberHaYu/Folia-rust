// NetEase Cloud Music: song search, song URLs and lyrics straight from the platform's web endpoints.

use folia_core::{Lyrics, OnlineId, Track};
use folia_lyrics::Format;
use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::{AudioSource, BoxFuture, OmniError, Page, Provider};

const ID: &str = "netease";
const BASE: &str = "https://music.163.com";

pub struct Netease {
    http: reqwest::Client,
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

#[derive(Deserialize)]
struct Song {
    id: u64,
    name: String,
    #[serde(default)]
    ar: Vec<Named>,
    al: Option<Album>,
    #[serde(default)]
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
        Self { http }
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

fn https(url: String) -> String {
    url.strip_prefix("http://").map(|rest| format!("https://{rest}")).unwrap_or(url)
}

fn check(code: i64) -> Result<(), OmniError> {
    if code == 200 { Ok(()) } else { Err(OmniError::Platform(code)) }
}

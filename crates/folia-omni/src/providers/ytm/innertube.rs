// YouTube Music's InnerTube as a guest (youtubei.js 18.1.0's `YTMUSIC` client), and the mobile app's lyrics page.

use serde_json::{Value, json};

use crate::OmniError;

const ORIGIN: &str = "https://music.youtube.com/youtubei/v1";
/// `Constants.CLIENTS.YTMUSIC`.
const CLIENT_NAME: &str = "WEB_REMIX";
const CLIENT_VERSION: &str = "1.20250219.01.00";
/// `MOBILE_CLIENT`: the web client gives plain lyrics only.
const MOBILE_CLIENT: (&str, &str) = ("ANDROID_MUSIC", "7.21.50");
/// `SearchFilter { filters { musicSearchType { song } } }`, base64 then `encodeURIComponent`.
pub const SONG_FILTER: &str = "EgWKAQIIAQ%3D%3D";

pub struct Client {
    http: reqwest::Client,
}

impl Client {
    pub fn new(http: reqwest::Client) -> Self {
        Self { http }
    }

    /// One InnerTube call as the web music client, in Chinese.
    pub async fn call(&self, endpoint: &str, mut body: Value) -> Result<Value, OmniError> {
        if let Some(map) = body.as_object_mut() {
            map.insert(
                "context".to_owned(),
                json!({ "client": { "clientName": CLIENT_NAME, "clientVersion": CLIENT_VERSION, "hl": "zh-CN", "gl": "US" } }),
            );
        }
        self.post(endpoint, body).await
    }

    /// The mobile app's browse, for a track's timed lyrics.
    pub async fn mobile_browse(&self, browse_id: &str) -> Result<Value, OmniError> {
        let body = json!({ "context": { "client": { "clientName": MOBILE_CLIENT.0, "clientVersion": MOBILE_CLIENT.1 } }, "browseId": browse_id });
        self.post("browse", body).await
    }

    async fn post(&self, endpoint: &str, body: Value) -> Result<Value, OmniError> {
        let response = self
            .http
            .post(format!("{ORIGIN}/{endpoint}?prettyPrint=false"))
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header(reqwest::header::ORIGIN, "https://music.youtube.com")
            .body(body.to_string())
            .send()
            .await?;
        Ok(response.error_for_status()?.json().await?)
    }
}

/// Every object under `key`, depth first, in document order.
pub fn find_all<'a>(value: &'a Value, key: &str, out: &mut Vec<&'a Value>) {
    match value {
        Value::Object(map) => {
            for (name, inner) in map {
                if name == key {
                    out.push(inner);
                } else {
                    find_all(inner, key, out);
                }
            }
        }
        Value::Array(rows) => rows.iter().for_each(|row| find_all(row, key, out)),
        _ => {}
    }
}

pub fn find_first<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    let mut found = Vec::new();
    find_all(value, key, &mut found);
    found.into_iter().next()
}

/// The text of a `runs` node.
pub fn runs_text(node: Option<&Value>) -> Option<String> {
    let node = node?;
    if let Some(simple) = node.get("simpleText").and_then(Value::as_str) {
        return Some(simple.to_owned());
    }
    let text: String =
        node.get("runs")?.as_array()?.iter().filter_map(|run| run.get("text").and_then(Value::as_str)).collect();
    (!text.is_empty()).then_some(text)
}

/// `lyricsBrowseId`: the watch page's lyrics tab.
pub fn lyrics_browse_id(watch: &Value) -> Option<String> {
    let tabs = watch.pointer(
        "/contents/singleColumnMusicWatchNextResultsRenderer/tabbedRenderer/watchNextTabbedResultsRenderer/tabs",
    )?;
    tabs.as_array()?.iter().find_map(|tab| {
        let endpoint = tab.pointer("/tabRenderer/endpoint/browseEndpoint")?;
        let page = endpoint
            .pointer("/browseEndpointContextSupportedConfigs/browseEndpointContextMusicConfig/pageType")?
            .as_str()?;
        (page == "MUSIC_PAGE_TYPE_TRACK_LYRICS")
            .then(|| endpoint.get("browseId")?.as_str().map(str::to_owned))
            .flatten()
    })
}

/// `lrcFromLyricsPage`: timed lines as LRC; the `♪` lines without letters or digits are left out.
pub fn lrc_from_lyrics_page(page: &Value) -> Option<String> {
    let timed = page.pointer(
        "/contents/elementRenderer/newElement/type/componentType/model/timedLyricsModel/lyricsData/timedLyricsData",
    )?;
    let lines: Vec<String> = timed
        .as_array()?
        .iter()
        .filter_map(|entry| {
            let text = entry.get("lyricLine").and_then(Value::as_str)?.split_whitespace().collect::<Vec<_>>().join(" ");
            let start = entry
                .pointer("/cueRange/startTimeMilliseconds")
                .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok()))?;
            text.chars()
                .any(char::is_alphanumeric)
                .then(|| format!("[{:02}:{:06.3}]{text}", start / 60_000, (start % 60_000) as f64 / 1000.0))
        })
        .collect();
    (!lines.is_empty()).then(|| lines.join("\n"))
}

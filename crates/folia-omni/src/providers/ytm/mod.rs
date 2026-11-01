// YouTube Music as a guest of InnerTube: search, timed lyrics, album and public playlist pages. The original signs
// in through Google's web page in a browser it controls and plays with BotGuard tokens; neither exists here.

mod innertube;

use folia_core::{Lyrics, OnlineId, Track};
use folia_lyrics::Format;
use serde_json::{Value, json};

use innertube::{find_all, find_first, runs_text};

use crate::{
    Account, AudioSource, BoxFuture, Collection, CollectionDetail, CollectionKind, ExploreSection, LoginCode,
    LoginState, OmniError, Page, Provider, RadioFeed,
};

const ID: &str = "ytm";

pub struct Ytm {
    client: innertube::Client,
}

impl Ytm {
    pub fn new(http: reqwest::Client) -> Self {
        Self { client: innertube::Client::new(http) }
    }
}

impl Provider for Ytm {
    fn id(&self) -> &'static str {
        ID
    }

    fn display_name(&self) -> &'static str {
        "YouTube Music"
    }

    /// `search_songs`: the songs shelf of a song-filtered search; its continuation token is not an offset, so the
    /// first page is all there is here.
    fn search_songs<'a>(
        &'a self,
        query: &'a str,
        _limit: u32,
        offset: u32,
    ) -> BoxFuture<'a, Result<Page<Track>, OmniError>> {
        Box::pin(async move {
            let query: String = query.trim().chars().take(200).collect();
            if query.is_empty() || offset > 0 {
                return Ok(Page::default());
            }
            let answer =
                self.client.call("search", json!({ "query": query, "params": innertube::SONG_FILTER })).await?;
            let shelf = find_first(&answer, "musicShelfRenderer").unwrap_or(&Value::Null);
            Ok(Page { items: rows(shelf, None), total: None })
        })
    }

    /// YouTube's audio servers want a proof-of-origin token that only Google's BotGuard script makes.
    fn audio_source<'a>(&'a self, _song: &'a OnlineId) -> BoxFuture<'a, Result<AudioSource, OmniError>> {
        Box::pin(async {
            Err(OmniError::Unsupported(
                "playback: YouTube audio needs a BotGuard token, which needs Google's JavaScript",
            ))
        })
    }

    /// `lyrics`: the watch page names the lyrics page; the mobile app's copy of it carries the line times.
    fn lyrics<'a>(&'a self, song: &'a OnlineId) -> BoxFuture<'a, Result<Option<Lyrics>, OmniError>> {
        Box::pin(async move {
            if !is_video_id(&song.id) {
                return Err(OmniError::Response("Invalid track id".to_owned()));
            }
            let watch = self.client.call("next", json!({ "videoId": song.id })).await?;
            let Some(browse_id) = innertube::lyrics_browse_id(&watch) else { return Ok(None) };
            let page = self.client.mobile_browse(&browse_id).await?;
            let Some(lrc) = innertube::lrc_from_lyrics_page(&page) else { return Ok(None) };
            let parsed = folia_lyrics::parse(Format::Lrc, &lrc, "", "");
            Ok((!parsed.lines.is_empty()).then_some(parsed))
        })
    }

    fn login_code(&self) -> BoxFuture<'_, Result<LoginCode, OmniError>> {
        Box::pin(async {
            Err(OmniError::Unsupported(
                "sign-in: the original signs in to Google in a browser window it reads cookies from",
            ))
        })
    }

    fn check_login<'a>(&'a self, _key: &'a str) -> BoxFuture<'a, Result<LoginState, OmniError>> {
        Box::pin(async { Ok(LoginState::Expired) })
    }

    fn account(&self) -> BoxFuture<'_, Result<Option<Account>, OmniError>> {
        Box::pin(async { Ok(None) })
    }

    fn logout(&self) -> BoxFuture<'_, Result<(), OmniError>> {
        Box::pin(async { Ok(()) })
    }

    fn session(&self) -> Option<String> {
        None
    }

    fn restore_session(&self, _saved: &str) -> Result<(), OmniError> {
        Ok(())
    }

    fn user_playlists<'a>(&'a self, _user: &'a str) -> BoxFuture<'a, Result<Vec<Collection>, OmniError>> {
        Box::pin(async { Err(OmniError::SignedOut) })
    }

    fn collection_detail<'a>(
        &'a self,
        collection: &'a Collection,
    ) -> BoxFuture<'a, Result<CollectionDetail, OmniError>> {
        Box::pin(async move {
            match collection.kind {
                CollectionKind::Album => self.album(&collection.id).await,
                CollectionKind::Playlist => self.playlist(&collection.id).await,
                CollectionKind::DailySongs | CollectionKind::PersonalFm => Err(OmniError::SignedOut),
            }
        })
    }

    fn set_subscribed<'a>(
        &'a self,
        _collection: &'a Collection,
        _subscribed: bool,
    ) -> BoxFuture<'a, Result<(), OmniError>> {
        Box::pin(async { Err(OmniError::SignedOut) })
    }

    fn favorite_albums(&self) -> BoxFuture<'_, Result<Vec<Collection>, OmniError>> {
        Box::pin(async { Err(OmniError::SignedOut) })
    }

    /// The original's radio tab is the account's music page and liked songs.
    fn radio_feed(&self, _limit: u32) -> BoxFuture<'_, Result<RadioFeed, OmniError>> {
        Box::pin(async { Err(OmniError::SignedOut) })
    }

    /// The original asks for the explore shelves as the account alone: a guest's are the same for everybody.
    fn explore(&self) -> BoxFuture<'_, Result<Vec<ExploreSection>, OmniError>> {
        Box::pin(async { Err(OmniError::SignedOut) })
    }

    /// `thumbnailUrl`: Google's image hosts take the size after `=`.
    fn sized_cover(&self, url: &str, px: u32) -> Option<String> {
        let rest = url.strip_prefix("https://")?;
        let host = rest.split('/').next()?;
        if !(host.ends_with("googleusercontent.com") || host.ends_with("ggpht.com")) {
            return None;
        }
        let base = url.split_once('=').map_or(url, |(base, _)| base);
        Some(format!("{base}=w{px}-h{px}-l90-rj"))
    }
}

impl Ytm {
    /// `album`: the header's title, artists, picture and year; its rows take the album's name and picture.
    async fn album(&self, id: &str) -> Result<CollectionDetail, OmniError> {
        if !id.starts_with("MPRE") || !is_entity_id(id) {
            return Err(OmniError::Response("Invalid album id".to_owned()));
        }
        let page = self.client.call("browse", json!({ "browseId": id })).await?;
        let header = find_first(&page, "musicResponsiveHeaderRenderer")
            .or_else(|| find_first(&page, "musicDetailHeaderRenderer"));
        let name = header.and_then(|h| runs_text(h.get("title")));
        let cover = header.and_then(|h| thumbnail(h.get("thumbnail")));
        let year = header.and_then(|h| runs_text(h.get("subtitle"))).and_then(|text| {
            text.split(|c: char| !c.is_ascii_digit()).find(|part| part.len() == 4).and_then(|y| y.parse::<i64>().ok())
        });
        let shelf = find_first(&page, "musicShelfRenderer").unwrap_or(&Value::Null);
        let tracks = rows(shelf, Some((name, cover)));
        let published = year.map(|year| days_from_civil(year) * 86_400_000);
        Ok(CollectionDetail { tracks, subscribed: None, owner: None, published })
    }

    /// `public_playlist`: the list as a guest sees it, page after page.
    async fn playlist(&self, id: &str) -> Result<CollectionDetail, OmniError> {
        if !is_entity_id(id) {
            return Err(OmniError::Response("Invalid playlist id".to_owned()));
        }
        let mut page = self.client.call("browse", json!({ "browseId": format!("VL{id}") })).await?;
        let mut songs = Vec::new();
        for _ in 0..50 {
            let shelf = find_first(&page, "musicPlaylistShelfRenderer")
                .or_else(|| find_first(&page, "musicPlaylistShelfContinuation"))
                .or_else(|| find_first(&page, "musicShelfRenderer"));
            songs.extend(rows(shelf.unwrap_or(&page), None));
            let token = find_first(&page, "continuationCommand")
                .and_then(|command| command.get("token"))
                .or_else(|| find_first(&page, "nextContinuationData").and_then(|data| data.get("continuation")))
                .and_then(Value::as_str)
                .map(str::to_owned);
            let Some(token) = token else { break };
            page = self.client.call("browse", json!({ "continuation": token })).await?;
        }
        let mut seen = std::collections::HashSet::new();
        songs.retain(|song| seen.insert(song.online.clone()));
        Ok(CollectionDetail { tracks: songs, ..Default::default() })
    }
}

/// `VIDEO_ID`.
fn is_video_id(id: &str) -> bool {
    id.len() == 11 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// `entityId`.
fn is_entity_id(id: &str) -> bool {
    (2..=128).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// The widest thumbnail of a `musicThumbnailRenderer` or a plain `thumbnails` list.
fn thumbnail(node: Option<&Value>) -> Option<String> {
    let mut lists = Vec::new();
    find_all(node?, "thumbnails", &mut lists);
    lists
        .into_iter()
        .filter_map(Value::as_array)
        .flatten()
        .max_by_key(|thumb| thumb.get("width").and_then(Value::as_u64).unwrap_or(0))
        .and_then(|thumb| thumb.get("url").and_then(Value::as_str))
        .map(str::to_owned)
}

/// `songFromListItem` for every `musicResponsiveListItemRenderer` under `shelf`; album rows get `album`'s name and
/// picture, which they do not carry.
fn rows(shelf: &Value, album: Option<(Option<String>, Option<String>)>) -> Vec<Track> {
    let mut items = Vec::new();
    find_all(shelf, "musicResponsiveListItemRenderer", &mut items);
    items.into_iter().filter_map(|item| song(item, album.as_ref())).collect()
}

fn song(item: &Value, album: Option<&(Option<String>, Option<String>)>) -> Option<Track> {
    let id = item
        .pointer("/playlistItemData/videoId")
        .or_else(|| item.pointer("/overlay/musicItemThumbnailOverlayRenderer/content/musicPlayButtonRenderer/playNavigationEndpoint/watchEndpoint/videoId"))
        .and_then(Value::as_str)
        .filter(|id| is_video_id(id))?;
    let columns: Vec<&Value> =
        item.get("flexColumns").and_then(Value::as_array).map(|c| c.iter().collect()).unwrap_or_default();
    let column =
        |index: usize| columns.get(index).and_then(|c| c.pointer("/musicResponsiveListItemFlexColumnRenderer/text"));
    let title = runs_text(column(0))?;
    let mut artists = Vec::new();
    let mut album_name = album.and_then(|(name, _)| name.clone());
    let mut duration = None;
    for run in columns
        .iter()
        .skip(1)
        .filter_map(|c| c.pointer("/musicResponsiveListItemFlexColumnRenderer/text/runs"))
        .filter_map(Value::as_array)
        .flatten()
    {
        let text = run.get("text").and_then(Value::as_str).unwrap_or_default();
        match run.pointer("/navigationEndpoint/browseEndpoint/browseEndpointContextSupportedConfigs/browseEndpointContextMusicConfig/pageType").and_then(Value::as_str) {
            Some("MUSIC_PAGE_TYPE_ARTIST" | "MUSIC_PAGE_TYPE_USER_CHANNEL") => artists.push(text.to_owned()),
            Some("MUSIC_PAGE_TYPE_ALBUM") => album_name = Some(text.to_owned()),
            _ => {
                if let Some(seconds) = clock(text) {
                    duration = Some(seconds);
                }
            }
        }
    }
    let fixed = item.pointer("/fixedColumns/0/musicResponsiveListItemFixedColumnRenderer/text");
    if let Some(seconds) = runs_text(fixed).and_then(|text| clock(&text)) {
        duration = Some(seconds);
    }
    let cover = thumbnail(item.get("thumbnail")).or_else(|| album.and_then(|(_, cover)| cover.clone()));
    let online = OnlineId { provider: ID.to_owned(), id: id.to_owned() };
    Some(Track {
        id: online.track_id(),
        title,
        artist: (!artists.is_empty()).then(|| artists.join(" / ")),
        album: album_name,
        cover_url: cover,
        duration: duration.unwrap_or(0.0),
        online: Some(online),
        ..Track::default()
    })
}

/// `m:ss` or `h:mm:ss` in seconds.
fn clock(text: &str) -> Option<f64> {
    let parts: Vec<&str> = text.trim().split(':').collect();
    if !(2..=3).contains(&parts.len()) || parts.iter().any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit())) {
        return None;
    }
    Some(parts.iter().fold(0.0, |total, part| total * 60.0 + part.parse::<f64>().unwrap_or(0.0)))
}

/// Days from 1970-01-01 to January 1st of `year`.
fn days_from_civil(year: i64) -> i64 {
    let y = year - 1;
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * 10 + 2) / 5;
    era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468
}

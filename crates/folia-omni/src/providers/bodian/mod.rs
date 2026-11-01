// Bodian (波点音乐) over its desktop API: search, play rights, word-timed lyrics, QR sign-in, lists and radio.

mod api;
mod library;

use folia_core::{Lyrics, OnlineId, Track};
use serde_json::Value;

use crate::{
    Account, AudioSource, BoxFuture, Collection, CollectionDetail, CollectionKind, ExploreSection, LoginCode,
    LoginState, OmniError, Page, Provider, QrContent, RadioFeed,
};

const ID: &str = "bodian";

pub struct Bodian {
    client: api::Client,
}

impl Bodian {
    pub fn new(http: reqwest::Client) -> Self {
        Self { client: api::Client::new(http) }
    }
}

impl Provider for Bodian {
    fn id(&self) -> &'static str {
        ID
    }

    fn display_name(&self) -> &'static str {
        "波点音乐"
    }

    fn search_songs<'a>(
        &'a self,
        query: &'a str,
        limit: u32,
        offset: u32,
    ) -> BoxFuture<'a, Result<Page<Track>, OmniError>> {
        Box::pin(library::search(&self.client, query, limit, offset))
    }

    fn audio_source<'a>(&'a self, song: &'a OnlineId) -> BoxFuture<'a, Result<AudioSource, OmniError>> {
        Box::pin(library::audio_source(&self.client, &song.id))
    }

    fn lyrics<'a>(&'a self, song: &'a OnlineId) -> BoxFuture<'a, Result<Option<Lyrics>, OmniError>> {
        Box::pin(library::lyrics(&self.client, &song.id))
    }

    /// `login_qr_key` and `login_qr_create`: the key goes into the download page the app opens.
    fn login_code(&self) -> BoxFuture<'_, Result<LoginCode, OmniError>> {
        Box::pin(async move {
            let call = api::Call { signed: true, anonymous: true, ..api::Call::default() };
            let answer = self.client.call("/api/ucenter/login/qrCode", call).await?;
            let key = answer
                .pointer("/data/qrCode")
                .and_then(Value::as_str)
                .filter(|key| !key.is_empty())
                .ok_or_else(|| OmniError::Response("Bodian QR key is missing".to_owned()))?
                .to_owned();
            let url = reqwest::Url::parse_with_params(
                "https://bodian-oia.kuwo.cn/bodian/download.html",
                [("pageName", "login_pc"), ("pt", "3"), ("id", key.as_str())],
            )
            .map_err(|err| OmniError::Response(err.to_string()))?;
            Ok(LoginCode { key, content: QrContent::Url(url.into()) })
        })
    }

    fn check_login<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<LoginState, OmniError>> {
        Box::pin(library::check_login(&self.client, key))
    }

    /// `login_status`: the account kept with the session; the API has no endpoint that validates it.
    fn account(&self) -> BoxFuture<'_, Result<Option<Account>, OmniError>> {
        Box::pin(async move {
            let session = self.client.session();
            if !session.signed_in() {
                return Ok(None);
            }
            Ok(Some(Account {
                id: session.uid.unwrap_or_default(),
                nickname: session.nickname.unwrap_or_default(),
                avatar_url: session.avatar_url,
            }))
        })
    }

    /// `logout`: the package forgets the session; nothing is sent to Bodian.
    fn logout(&self) -> BoxFuture<'_, Result<(), OmniError>> {
        Box::pin(async move {
            let device_id = self.client.session().device_id;
            self.client.set_session(api::Session {
                device_id,
                uid: None,
                token: None,
                nickname: None,
                avatar_url: None,
            });
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
        let session: api::Session =
            serde_json::from_str(saved).map_err(|err| OmniError::Response(format!("kept session: {err}")))?;
        self.client.set_session(session);
        Ok(())
    }

    fn user_playlists<'a>(&'a self, _user: &'a str) -> BoxFuture<'a, Result<Vec<Collection>, OmniError>> {
        Box::pin(library::user_playlists(&self.client))
    }

    fn collection_detail<'a>(
        &'a self,
        collection: &'a Collection,
    ) -> BoxFuture<'a, Result<CollectionDetail, OmniError>> {
        Box::pin(library::collection_detail(&self.client, collection))
    }

    /// The original's Bodian provider cannot save playlists or albums.
    fn set_subscribed<'a>(
        &'a self,
        _collection: &'a Collection,
        _subscribed: bool,
    ) -> BoxFuture<'a, Result<(), OmniError>> {
        Box::pin(async { Err(OmniError::Unsupported("saving playlists and albums")) })
    }

    fn favorite_albums(&self) -> BoxFuture<'_, Result<Vec<Collection>, OmniError>> {
        Box::pin(library::favorite_albums(&self.client))
    }

    fn radio_feed(&self, limit: u32) -> BoxFuture<'_, Result<RadioFeed, OmniError>> {
        Box::pin(library::radio_feed(&self.client, limit))
    }

    /// The original's Bodian provider has no explore page.
    fn explore(&self) -> BoxFuture<'_, Result<Vec<ExploreSection>, OmniError>> {
        Box::pin(async { Err(OmniError::Unsupported("the explore page")) })
    }

    /// `withKuwoCoverSize`: `img<n>.kuwo.cn/star/{albumcover,starheads}/<size>/`, 240 or 800, else 800 and up.
    fn sized_cover(&self, url: &str, px: u32) -> Option<String> {
        let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"))?;
        let (host, path) = rest.split_once('/')?;
        let digits = host.strip_prefix("img")?.strip_suffix(".kuwo.cn")?;
        if !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let prefix = ["star/albumcover/", "star/starheads/"].into_iter().find(|prefix| path.starts_with(prefix))?;
        let tail = &path[prefix.len()..];
        let (size, after) = tail.split_once('/')?;
        let current: u32 = size.parse().ok()?;
        let wanted = if px <= 256 {
            240
        } else if px <= 800 {
            800
        } else {
            current.max(800)
        };
        Some(format!("https://{host}/{prefix}{wanted}/{after}"))
    }
}

/// `normalizeBodianSong`.
fn track(raw: &Value) -> Option<Track> {
    let id = text(raw, &["id"])?;
    let artists: Vec<String> = match raw.get("artists").and_then(Value::as_array) {
        Some(list) => list.iter().filter_map(|artist| text(artist, &["name"])).collect(),
        None => text(raw, &["artist"]).into_iter().collect(),
    };
    let online = OnlineId { provider: ID.to_owned(), id };
    Some(Track {
        id: online.track_id(),
        title: text(raw, &["songName", "name"]).unwrap_or_default(),
        artist: (!artists.is_empty()).then(|| artists.join(" / ")),
        album: text(raw, &["album"]),
        cover_url: text(raw, &["albumPic"]).map(|url| secure(&url)),
        duration: raw.get("duration").and_then(Value::as_f64).unwrap_or(0.0).max(0.0),
        online: Some(online),
        ..Track::default()
    })
}

fn tracks(list: Option<&Value>) -> Vec<Track> {
    list.and_then(Value::as_array).map(|rows| rows.iter().filter_map(track).collect()).unwrap_or_default()
}

/// `normalizeBodianCollection`; a playlist's id carries its `source` as `<source>-<id>`.
fn collection(raw: &Value, kind: CollectionKind, source: u32) -> Option<Collection> {
    let id = match kind {
        CollectionKind::Album => text(raw, &["albumId", "id"]),
        _ => text(raw, &["id"]),
    }?;
    let artists: Vec<String> = raw
        .get("artists")
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(|artist| text(artist, &["name"])).collect())
        .unwrap_or_else(|| text(raw, &["artist"]).into_iter().collect());
    let creator = if artists.is_empty() { text(raw, &["creatorName"]) } else { Some(artists.join(", ")) };
    let source = raw.get("sourceType").and_then(Value::as_u64).map_or(source, |s| s as u32);
    Some(Collection {
        provider: ID.to_owned(),
        id: if kind == CollectionKind::Album { id } else { format!("{source}-{id}") },
        kind,
        name: text(raw, &["name"])?,
        cover_url: text(raw, &["pic"]).map(|url| secure(&url)),
        track_count: raw
            .get("musicCount")
            .or_else(|| raw.get("musicCnt"))
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok()),
        creator,
        description: text(raw, &["description", "info", "desc"]),
    })
}

fn text(raw: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| match raw.get(*key)? {
        Value::String(text) if !text.is_empty() => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    })
}

/// Kuwo's image hosts take https.
fn secure(url: &str) -> String {
    match url.strip_prefix("http://") {
        Some(rest) if rest.split('/').next().is_some_and(|host| host.ends_with(".kuwo.cn")) => {
            format!("https://{rest}")
        }
        _ => url.to_owned(),
    }
}

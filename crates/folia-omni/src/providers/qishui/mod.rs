// Qishui (汽水音乐) over the car protocol: search, plaintext audio, KRC lyrics, Douyin QR sign-in, lists and the feed.

mod client;
mod library;

use folia_core::{Lyrics, OnlineId, Track};
use serde_json::Value;

use crate::{
    Account, AudioSource, BoxFuture, Collection, CollectionDetail, CollectionKind, ExploreSection, LoginCode,
    LoginState, OmniError, Page, Provider, RadioFeed,
};

const ID: &str = "qishui";

pub struct Qishui {
    client: client::Client,
    feed: library::Feed,
}

impl Qishui {
    pub fn new(http: reqwest::Client) -> Self {
        Self { client: client::Client::new(http), feed: library::Feed::default() }
    }
}

impl Provider for Qishui {
    fn id(&self) -> &'static str {
        ID
    }

    fn display_name(&self) -> &'static str {
        "汽水音乐"
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

    fn login_code(&self) -> BoxFuture<'_, Result<LoginCode, OmniError>> {
        Box::pin(library::login_code(&self.client))
    }

    fn check_login<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<LoginState, OmniError>> {
        Box::pin(async move {
            let state = library::check_login(&self.client, key).await?;
            if state == LoginState::Confirmed {
                self.feed.clear();
            }
            Ok(state)
        })
    }

    fn account(&self) -> BoxFuture<'_, Result<Option<Account>, OmniError>> {
        Box::pin(library::account(&self.client))
    }

    /// `logout`: the package forgets the credential; nothing is sent.
    fn logout(&self) -> BoxFuture<'_, Result<(), OmniError>> {
        Box::pin(async move {
            self.client.set_credential(None);
            self.feed.clear();
            Ok(())
        })
    }

    fn session(&self) -> Option<String> {
        let session = self.client.session();
        session.credential.as_ref()?;
        serde_json::to_string(&session).ok()
    }

    fn restore_session(&self, saved: &str) -> Result<(), OmniError> {
        let session: client::Session =
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
        Box::pin(library::collection_detail(&self.client, &self.feed, collection))
    }

    /// The car protocol has no saving of playlists or albums.
    fn set_subscribed<'a>(
        &'a self,
        _collection: &'a Collection,
        _subscribed: bool,
    ) -> BoxFuture<'a, Result<(), OmniError>> {
        Box::pin(async { Err(OmniError::Unsupported("saving playlists and albums")) })
    }

    /// Albums are not part of the car protocol.
    fn favorite_albums(&self) -> BoxFuture<'_, Result<Vec<Collection>, OmniError>> {
        Box::pin(async { Err(OmniError::Unsupported("saved albums")) })
    }

    fn radio_feed(&self, limit: u32) -> BoxFuture<'_, Result<RadioFeed, OmniError>> {
        Box::pin(library::radio_feed(&self.client, &self.feed, limit))
    }

    fn has_daily_songs(&self) -> bool {
        false
    }

    fn explore(&self) -> BoxFuture<'_, Result<Vec<ExploreSection>, OmniError>> {
        Box::pin(library::explore(&self.client, &self.feed))
    }

    fn sized_cover(&self, _url: &str, _px: u32) -> Option<String> {
        None
    }
}

/// `qishuiImage`: an https string, or the first https address of `{ urls, uri, template_prefix }`.
fn image(raw: Option<&Value>) -> Option<String> {
    match raw? {
        Value::String(url) => url.starts_with("https://").then(|| url.clone()),
        value => {
            let url = value
                .get("urls")?
                .as_array()?
                .iter()
                .filter_map(Value::as_str)
                .find(|url| url.starts_with("https://"))?;
            match (value.get("uri").and_then(Value::as_str), url.ends_with('/')) {
                (Some(uri), true) => {
                    let template = value
                        .get("template_prefix")
                        .and_then(Value::as_str)
                        .map(|t| format!("~{t}-image.image"))
                        .unwrap_or_default();
                    Some(format!("{url}{uri}{template}"))
                }
                _ => Some(url.to_owned()),
            }
        }
    }
}

/// `publicTrack` then `normalizeQishuiSong`.
fn track(raw: &Value) -> Option<Track> {
    let entity = raw
        .get("track_entity")
        .or_else(|| raw.pointer("/entity/media/track_entity"))
        .or_else(|| raw.pointer("/entity/track"))
        .or_else(|| raw.get("track"))
        .unwrap_or(raw);
    let base = entity.get("base_info").unwrap_or(&Value::Null);
    let id = text(base, "id").or_else(|| text(entity, "id")).filter(|id| id.bytes().all(|b| b.is_ascii_digit()))?;
    let related = entity.get("related_info").unwrap_or(&Value::Null);
    let artists: Vec<String> = related
        .get("artist_links")
        .or_else(|| base.pointer("/song_maker_team/composers"))
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(|artist| text(artist, "name")).collect())
        .unwrap_or_default();
    let online = OnlineId { provider: ID.to_owned(), id };
    Some(Track {
        id: online.track_id(),
        title: text(base, "name").unwrap_or_default(),
        artist: (!artists.is_empty()).then(|| artists.join(" / ")),
        album: related.get("album_link").and_then(|album| text(album, "name")),
        cover_url: image(entity.pointer("/display_info/cover_url")),
        duration: base.get("duration_ms").and_then(Value::as_f64).unwrap_or(0.0).max(0.0) / 1000.0,
        online: Some(online),
        ..Track::default()
    })
}

fn tracks(rows: &[Value]) -> Vec<Track> {
    rows.iter().filter_map(track).collect()
}

/// `normalizeQishuiCollection`; the id carries the owner as `<id>-<owner>`, which `/playlist/detail/` wants.
fn collection(raw: &Value) -> Option<Collection> {
    let item = raw.get("playlist").unwrap_or(raw);
    let base = item.get("base_info").unwrap_or(&Value::Null);
    let id = text(base, "id")?;
    let owner = item.pointer("/relation_info/owner").unwrap_or(&Value::Null);
    let owner_id = text(base, "owner_id").or_else(|| text(owner, "id")).unwrap_or_default();
    Some(Collection {
        provider: ID.to_owned(),
        id: format!("{id}-{owner_id}"),
        kind: CollectionKind::Playlist,
        name: text(base, "title").or_else(|| text(base, "name"))?,
        cover_url: image(item.pointer("/display_info/url_cover")),
        track_count: item
            .pointer("/resource_cnt/track_cnt")
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok()),
        creator: text(owner, "nickname"),
        description: text(base, "desc").or_else(|| text(base, "intro")),
    })
}

fn text(raw: &Value, key: &str) -> Option<String> {
    match raw.get(key)? {
        Value::String(text) if !text.is_empty() => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

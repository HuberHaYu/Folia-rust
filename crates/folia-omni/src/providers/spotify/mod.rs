// Spotify: PKCE sign-in in the browser, pathfinder and spclient for the catalogue, librespot for the audio files.

mod api;
mod audio;
mod library;
mod signin;

use folia_core::{Lyrics, OnlineId, Track};
use serde_json::Value;

use crate::{
    Account, AudioFile, AudioSource, BoxFuture, Collection, CollectionDetail, CollectionKind, ExploreSection,
    LoginCode, LoginState, OmniError, Page, Provider, QrContent, RadioFeed,
};

const ID: &str = "spotify";

pub struct Spotify {
    client: api::Client,
    attempt: std::sync::Mutex<Option<signin::Attempt>>,
    player: audio::Player,
    rng: ring::rand::SystemRandom,
}

impl Spotify {
    pub fn new(http: reqwest::Client) -> Self {
        Self {
            client: api::Client::new(http),
            attempt: std::sync::Mutex::default(),
            player: audio::Player::default(),
            rng: ring::rand::SystemRandom::new(),
        }
    }

    fn attempt(&self) -> std::sync::MutexGuard<'_, Option<signin::Attempt>> {
        self.attempt.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl Provider for Spotify {
    fn id(&self) -> &'static str {
        ID
    }

    fn display_name(&self) -> &'static str {
        "Spotify"
    }

    fn search_songs<'a>(
        &'a self,
        query: &'a str,
        limit: u32,
        offset: u32,
    ) -> BoxFuture<'a, Result<Page<Track>, OmniError>> {
        Box::pin(library::search(&self.client, query, limit, offset))
    }

    /// Spotify's files are encrypted on its CDN: they come whole from `audio_file`.
    fn audio_source<'a>(&'a self, _song: &'a OnlineId) -> BoxFuture<'a, Result<AudioSource, OmniError>> {
        Box::pin(async { Err(OmniError::Unsupported("a direct audio address")) })
    }

    /// `audio`: for Premium accounts only, as Spotify allows every player but its own.
    fn audio_file<'a>(&'a self, song: &'a OnlineId) -> Option<BoxFuture<'a, Result<AudioFile, OmniError>>> {
        Some(Box::pin(async move {
            let session = self.client.session().ok_or(OmniError::SignedOut)?;
            if session.user.premium == Some(false) {
                return Err(OmniError::Unsupported("playback without Spotify Premium"));
            }
            let token = self.client.access_token(false).await?;
            let bytes = self.player.file(&token, &song.id).await?;
            Ok(AudioFile { bytes, extension: "ogg".to_owned() })
        }))
    }

    fn lyrics<'a>(&'a self, song: &'a OnlineId) -> BoxFuture<'a, Result<Option<Lyrics>, OmniError>> {
        Box::pin(library::lyrics(&self.client, &song.id))
    }

    /// `login_start`: the authorize page, answered on the loopback port.
    fn login_code(&self) -> BoxFuture<'_, Result<LoginCode, OmniError>> {
        Box::pin(async move {
            self.attempt().take();
            let attempt = signin::start(&self.rng)?;
            let code = LoginCode { key: attempt.key.clone(), content: QrContent::Browser(attempt.page.clone()) };
            *self.attempt() = Some(attempt);
            Ok(code)
        })
    }

    /// `login_check`: the caught code is exchanged and the account read before the sign-in counts.
    fn check_login<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<LoginState, OmniError>> {
        Box::pin(async move {
            let (outcome, verifier) = {
                let attempt = self.attempt();
                let Some(current) = attempt.as_ref().filter(|attempt| attempt.key == key) else {
                    return Ok(LoginState::Expired);
                };
                if std::time::Instant::now() >= current.until {
                    drop(attempt);
                    self.attempt().take();
                    return Ok(LoginState::Expired);
                }
                let outcome = current.outcome.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
                (outcome, current.verifier.clone())
            };
            match outcome {
                None => Ok(LoginState::Waiting),
                Some(signin::Outcome::Error(reason)) => {
                    self.attempt().take();
                    Ok(LoginState::Failed { code: 0, message: Some(reason) })
                }
                Some(signin::Outcome::Code(code)) => {
                    self.attempt().take();
                    let tokens = signin::exchange(&self.client.http, &code, &verifier).await?;
                    let user = library::read_account(&self.client, Some(&tokens.access_token)).await?;
                    self.client.set_session(Some(api::Session { tokens, user }));
                    self.player.clear().await;
                    tracing::info!("spotify sign-in confirmed");
                    Ok(LoginState::Confirmed)
                }
            }
        })
    }

    fn account(&self) -> BoxFuture<'_, Result<Option<Account>, OmniError>> {
        Box::pin(library::account(&self.client))
    }

    /// `logout`: the sign-in and the audio session go; nothing is sent.
    fn logout(&self) -> BoxFuture<'_, Result<(), OmniError>> {
        Box::pin(async move {
            self.attempt().take();
            self.client.set_session(None);
            self.player.clear().await;
            Ok(())
        })
    }

    fn session(&self) -> Option<String> {
        serde_json::to_string(&self.client.session()?).ok()
    }

    fn restore_session(&self, saved: &str) -> Result<(), OmniError> {
        let session: api::Session =
            serde_json::from_str(saved).map_err(|err| OmniError::Response(format!("kept session: {err}")))?;
        self.client.set_session(Some(session));
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

    fn set_subscribed<'a>(
        &'a self,
        collection: &'a Collection,
        subscribed: bool,
    ) -> BoxFuture<'a, Result<(), OmniError>> {
        Box::pin(library::set_subscribed(&self.client, collection, subscribed))
    }

    fn favorite_albums(&self) -> BoxFuture<'_, Result<Vec<Collection>, OmniError>> {
        Box::pin(library::favorite_albums(&self.client))
    }

    fn radio_feed(&self, limit: u32) -> BoxFuture<'_, Result<RadioFeed, OmniError>> {
        Box::pin(library::radio_feed(&self.client, limit))
    }

    fn explore(&self) -> BoxFuture<'_, Result<Vec<ExploreSection>, OmniError>> {
        Box::pin(library::explore(&self.client))
    }

    fn sized_cover(&self, _url: &str, _px: u32) -> Option<String> {
        None
    }
}

/// `spotifyId`: the 22 base62 characters of a `spotify:<kind>:` URI.
fn spotify_id(uri: &str, kind: &str) -> Option<String> {
    let id = uri.rsplit_once(&format!("{kind}:")).map(|(_, id)| id)?;
    (id.len() == 22 && id.bytes().all(|b| b.is_ascii_alphanumeric())).then(|| id.to_owned())
}

/// `pictureOf`: the widest source.
fn picture(sources: Option<&Value>) -> Option<String> {
    sources?
        .as_array()?
        .iter()
        .filter(|source| source.get("url").and_then(Value::as_str).is_some())
        .max_by_key(|source| source.get("width").and_then(Value::as_u64).unwrap_or(0))
        .and_then(|source| source.get("url").and_then(Value::as_str))
        .map(str::to_owned)
}

fn artists(rows: Option<&Value>) -> Vec<String> {
    rows.and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|row| row.pointer("/profile/name").and_then(Value::as_str))
        .map(str::to_owned)
        .collect()
}

/// `spotifySongOf`.
fn song_of(track: &Value) -> Option<Track> {
    if track.get("__typename").and_then(Value::as_str).is_some_and(|kind| kind != "Track") {
        return None;
    }
    let id = spotify_id(track.get("uri").and_then(Value::as_str)?, "track")?;
    let name = track.get("name").and_then(Value::as_str).filter(|name| !name.is_empty())?;
    let mut names = artists(track.pointer("/artists/items"));
    if names.is_empty() {
        names = artists(track.pointer("/firstArtist/items"));
        names.extend(artists(track.pointer("/otherArtists/items")));
    }
    let album = track.get("albumOfTrack").unwrap_or(&Value::Null);
    let ms = track
        .pointer("/duration/totalMilliseconds")
        .or_else(|| track.pointer("/trackDuration/totalMilliseconds"))
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let online = OnlineId { provider: ID.to_owned(), id };
    Some(Track {
        id: online.track_id(),
        title: name.to_owned(),
        artist: (!names.is_empty()).then(|| names.join(" / ")),
        album: album.get("name").and_then(Value::as_str).filter(|n| !n.is_empty()).map(str::to_owned),
        cover_url: picture(album.pointer("/coverArt/sources")),
        duration: ms.max(0.0) / 1000.0,
        online: Some(online),
        ..Track::default()
    })
}

/// `spotifyPlaylistOf`.
fn playlist_of(list: &Value) -> Option<Collection> {
    if list.get("__typename").and_then(Value::as_str).is_some_and(|kind| kind != "Playlist") {
        return None;
    }
    let id = spotify_id(list.get("uri").and_then(Value::as_str)?, "playlist")?;
    Some(Collection {
        provider: ID.to_owned(),
        id,
        kind: CollectionKind::Playlist,
        name: list.get("name").and_then(Value::as_str).unwrap_or_default().to_owned(),
        cover_url: picture(list.pointer("/images/items/0/sources")),
        track_count: list.pointer("/content/totalCount").and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok()),
        creator: list.pointer("/ownerV2/data/name").and_then(Value::as_str).map(str::to_owned),
        description: list.get("description").and_then(Value::as_str).map(plain_text).filter(|text| !text.is_empty()),
    })
}

/// `spotifyAlbumOf`.
fn album_of(album: &Value) -> Option<Collection> {
    if album.get("__typename").and_then(Value::as_str).is_some_and(|kind| kind != "Album") {
        return None;
    }
    let id = spotify_id(album.get("uri").and_then(Value::as_str)?, "album")?;
    let names = artists(album.pointer("/artists/items"));
    let count =
        album.pointer("/tracksV2/totalCount").or_else(|| album.pointer("/tracks/totalCount")).and_then(Value::as_u64);
    Some(Collection {
        provider: ID.to_owned(),
        id,
        kind: CollectionKind::Album,
        name: album.get("name").and_then(Value::as_str).unwrap_or_default().to_owned(),
        cover_url: picture(album.pointer("/coverArt/sources")),
        track_count: count.and_then(|n| u32::try_from(n).ok()),
        creator: (!names.is_empty()).then(|| names.join(", ")),
        description: None,
    })
}

/// `plainText`: tags dropped, the common entities read.
fn plain_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    [("&amp;", "&"), ("&lt;", "<"), ("&gt;", ">"), ("&quot;", "\""), ("&#x27;", "'"), ("&#39;", "'")]
        .iter()
        .fold(out, |text, (entity, plain)| text.replace(entity, plain))
}

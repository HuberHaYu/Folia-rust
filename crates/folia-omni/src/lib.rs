// Online music: one facade (`Omni`) over every platform provider; only providers talk to platform APIs.

mod providers;
mod qr;
mod reads;
mod session;

use std::future::Future;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use folia_core::{Lyrics, OnlineId, Track};

pub use qr::LoginQr;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, thiserror::Error)]
pub enum OmniError {
    #[error("unknown provider {0}")]
    UnknownProvider(String),
    #[error("request failed")]
    Http(#[from] reqwest::Error),
    #[error("platform answered with code {0}")]
    Platform(i64),
    #[error("unexpected response: {0}")]
    Response(String),
    #[error("song is not playable")]
    Unavailable,
    #[error("not signed in")]
    SignedOut,
    /// The platform has no such capability in the original either.
    #[error("{0} is not supported on this platform")]
    Unsupported(&'static str),
    #[error("cannot draw the sign-in code")]
    Qr(#[from] qrcode::types::QrError),
    #[error("system keyring refused")]
    Keyring(#[from] keyring::Error),
    #[error("cannot write {path}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub total: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AudioSource {
    pub url: String,
    /// File extension of the stream, such as `mp3` or `flac`.
    pub extension: String,
    /// The platform only gave a preview clip, not the whole song.
    pub preview: bool,
}

/// One platform the original lists in its sign-in window, with its mark: a letter on a colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Platform {
    pub id: &'static str,
    pub display_name: &'static str,
    pub short_name: &'static str,
    pub letter: char,
    /// `0xRRGGBB`.
    pub color: u32,
    /// Compiled in with a provider; the others are listed as not configured.
    pub available: bool,
}

/// The original's providers in registration order (`providerRegistry.ts`), with the marks they declare.
const PLATFORMS: [(&str, &str, &str, char, u32); 7] = [
    ("netease", "NetEase Cloud Music", "网易云", '云', 0xdc2626),
    ("kugou", "KuGou Music", "酷狗", 'K', 0x2563eb),
    ("qq", "QQ Music", "QQ音乐", 'Q', 0x16a34a),
    ("bodian", "波点音乐", "波点", '波', 0x0d9488),
    ("ytm", "YouTube Music", "YT Music", 'Y', 0xe11d48),
    ("spotify", "Spotify", "Spotify", 'S', 0x22c55e),
    ("qishui", "汽水音乐", "汽水", '汽', 0x19a58c),
];

/// A QR sign-in session: `key` is what the platform is asked about, `url` what the phone scans.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginCode {
    pub key: String,
    pub url: String,
}

/// What the platform says about a sign-in code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginState {
    Waiting,
    Scanned,
    /// The phone confirmed; the provider now holds the session.
    Confirmed,
    Expired,
    Failed {
        code: i64,
        message: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub id: String,
    pub nickname: String,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CollectionKind {
    Playlist,
    Album,
    /// The account's daily songs.
    DailySongs,
    /// Personal FM: played straight away, not opened.
    PersonalFm,
}

/// A playlist or an album on a platform, as a home card shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct Collection {
    pub provider: String,
    pub id: String,
    pub kind: CollectionKind,
    pub name: String,
    pub cover_url: Option<String>,
    pub track_count: Option<u32>,
    /// The playlist's creator, or the album's artists joined with `, `.
    pub creator: Option<String>,
    pub description: Option<String>,
}

impl Collection {
    /// A stable negative id for the wall, apart from track ids.
    pub fn wall_id(&self) -> i64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        (&self.provider, &self.id, self.kind).hash(&mut hasher);
        -((hasher.finish() >> 1) as i64) - 1
    }
}

/// A collection's songs and the account's relation to it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CollectionDetail {
    pub tracks: Vec<Track>,
    /// Whether the account saved it; `None` where saving does not apply (daily songs, personal FM).
    pub subscribed: Option<bool>,
    /// The playlist creator's user id: an account cannot save its own playlist.
    pub owner: Option<String>,
    /// An album's release time, milliseconds since the Unix epoch (`publishedAt`).
    pub published: Option<i64>,
}

/// The radio tab: personal FM, the daily songs and the recommended playlists (`getHomeFeed`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RadioFeed {
    pub personal_fm: Vec<Track>,
    pub daily_songs: Vec<Track>,
    pub recommended: Vec<Collection>,
    /// One part failed and is empty.
    pub partial: bool,
}

/// Whose heading a shelf of the explore page carries: the platform's, or one the app names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shelf {
    DailySongs,
    DailyPlaylists,
    Charts,
    Block(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExploreEntry {
    Song(Track),
    Collection(Collection),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExploreSection {
    pub key: String,
    pub shelf: Shelf,
    pub entries: Vec<ExploreEntry>,
}

/// What one platform can do. Search results are tracks whose `online` id points back at the platform.
pub trait Provider: Send + Sync {
    fn id(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn search_songs<'a>(
        &'a self,
        query: &'a str,
        limit: u32,
        offset: u32,
    ) -> BoxFuture<'a, Result<Page<Track>, OmniError>>;
    fn audio_source<'a>(&'a self, song: &'a OnlineId) -> BoxFuture<'a, Result<AudioSource, OmniError>>;
    fn lyrics<'a>(&'a self, song: &'a OnlineId) -> BoxFuture<'a, Result<Option<Lyrics>, OmniError>>;
    fn login_code(&self) -> BoxFuture<'_, Result<LoginCode, OmniError>>;
    fn check_login<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<LoginState, OmniError>>;
    /// The signed-in account, or `None` when the session is missing or ran out.
    fn account(&self) -> BoxFuture<'_, Result<Option<Account>, OmniError>>;
    /// Tells the platform and drops the session held in memory, whatever it answers.
    fn logout(&self) -> BoxFuture<'_, Result<(), OmniError>>;
    /// The session to keep, serialised; `None` when nobody is signed in.
    fn session(&self) -> Option<String>;
    fn restore_session(&self, saved: &str) -> Result<(), OmniError>;
    fn user_playlists<'a>(&'a self, user: &'a str) -> BoxFuture<'a, Result<Vec<Collection>, OmniError>>;
    fn collection_detail<'a>(
        &'a self,
        collection: &'a Collection,
    ) -> BoxFuture<'a, Result<CollectionDetail, OmniError>>;
    /// Saves (`true`) or unsaves a playlist or an album for the signed-in account.
    fn set_subscribed<'a>(
        &'a self,
        collection: &'a Collection,
        subscribed: bool,
    ) -> BoxFuture<'a, Result<(), OmniError>>;
    fn favorite_albums(&self) -> BoxFuture<'_, Result<Vec<Collection>, OmniError>>;
    fn radio_feed(&self, limit: u32) -> BoxFuture<'_, Result<RadioFeed, OmniError>>;
    fn explore(&self) -> BoxFuture<'_, Result<Vec<ExploreSection>, OmniError>>;
    /// The address of `url` resized to `px` square on the platform's image CDN, when `url` is on it.
    fn sized_cover(&self, url: &str, px: u32) -> Option<String>;
}

/// Cover downloads at once: what a browser opens to one host, so the first screen of tiles does not queue dozens.
const COVER_DOWNLOADS: usize = 6;

#[derive(Clone)]
pub struct Omni {
    providers: Arc<Vec<Box<dyn Provider>>>,
    http: reqwest::Client,
    cache_dir: PathBuf,
    keyring_service: String,
    covers: Arc<tokio::sync::Semaphore>,
}

impl Omni {
    /// Registers every provider compiled in; downloads go under `cache_dir/audio`, sessions under `keyring_service`.
    pub fn new(cache_dir: PathBuf, keyring_service: String) -> Result<Self, OmniError> {
        let http = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36")
            .build()?;
        let covers = Arc::new(tokio::sync::Semaphore::new(COVER_DOWNLOADS));
        Ok(Self { providers: Arc::new(providers::all(&http)), http, cache_dir, keyring_service, covers })
    }

    /// `(id, display name)` of each provider, in registration order.
    pub fn providers(&self) -> Vec<(&'static str, &'static str)> {
        self.providers.iter().map(|provider| (provider.id(), provider.display_name())).collect()
    }

    /// Every platform the original lists, marked available when a provider for it is compiled in.
    pub fn platforms(&self) -> Vec<Platform> {
        PLATFORMS
            .iter()
            .map(|&(id, display_name, short_name, letter, color)| Platform {
                id,
                display_name,
                short_name,
                letter,
                color,
                available: self.providers.iter().any(|provider| provider.id() == id),
            })
            .collect()
    }

    pub async fn search(&self, provider: &str, query: &str, limit: u32, offset: u32) -> Result<Page<Track>, OmniError> {
        self.provider(provider)?.search_songs(query, limit, offset).await
    }

    pub async fn lyrics(&self, song: &OnlineId) -> Result<Option<Lyrics>, OmniError> {
        self.provider(&song.provider)?.lyrics(song).await
    }

    /// Asks for a sign-in code and draws it as a QR matrix.
    pub async fn login_qr(&self, provider: &str) -> Result<LoginQr, OmniError> {
        let code = self.provider(provider)?.login_code().await?;
        LoginQr::encode(code.key, &code.url)
    }

    pub async fn check_login(&self, provider: &str, key: &str) -> Result<LoginState, OmniError> {
        self.provider(provider)?.check_login(key).await
    }

    pub async fn account(&self, provider: &str) -> Result<Option<Account>, OmniError> {
        self.provider(provider)?.account().await
    }

    pub async fn logout(&self, provider: &str) -> Result<(), OmniError> {
        self.provider(provider)?.logout().await
    }

    /// Reads the kept session from the system keyring into the provider. Blocking: call it off the UI thread.
    pub fn restore_session(&self, provider: &str) -> Result<bool, OmniError> {
        let target = self.provider(provider)?;
        match session::load(&self.keyring_service, provider)? {
            Some(saved) => {
                target.restore_session(&saved)?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// Keeps the provider's session in the system keyring, or removes it when signed out. Blocking.
    pub fn save_session(&self, provider: &str) -> Result<(), OmniError> {
        match self.provider(provider)?.session() {
            Some(value) => session::save(&self.keyring_service, provider, &value),
            None => session::delete(&self.keyring_service, provider),
        }
    }

    /// Removes the kept session from the system keyring. Blocking.
    pub fn forget_session(&self, provider: &str) -> Result<(), OmniError> {
        session::delete(&self.keyring_service, provider)
    }

    pub async fn user_playlists(&self, provider: &str, user: &str) -> Result<Vec<Collection>, OmniError> {
        self.provider(provider)?.user_playlists(user).await
    }

    pub async fn collection_detail(&self, collection: &Collection) -> Result<CollectionDetail, OmniError> {
        self.provider(&collection.provider)?.collection_detail(collection).await
    }

    pub async fn set_subscribed(&self, collection: &Collection, subscribed: bool) -> Result<(), OmniError> {
        self.provider(&collection.provider)?.set_subscribed(collection, subscribed).await
    }

    pub async fn favorite_albums(&self, provider: &str) -> Result<Vec<Collection>, OmniError> {
        self.provider(provider)?.favorite_albums().await
    }

    pub async fn radio_feed(&self, provider: &str, limit: u32) -> Result<RadioFeed, OmniError> {
        self.provider(provider)?.radio_feed(limit).await
    }

    pub async fn explore(&self, provider: &str) -> Result<Vec<ExploreSection>, OmniError> {
        self.provider(provider)?.explore().await
    }

    /// Downloads the song once into the cache and returns the file to play.
    pub async fn fetch_audio(&self, song: &OnlineId) -> Result<PathBuf, OmniError> {
        let dir = self.cache_dir.join("audio");
        let stem = format!("{}-{}", song.provider, song.id);
        if let Some(cached) = cached_file(&dir, &stem) {
            return Ok(cached);
        }

        let source = self.provider(&song.provider)?.audio_source(song).await?;
        let path = dir.join(format!("{stem}.{}", source.extension));
        let io_err = |source| OmniError::Io { path: path.clone(), source };
        std::fs::create_dir_all(&dir).map_err(io_err)?;
        let mut response = self.http.get(&source.url).send().await?.error_for_status()?;
        let partial = path.with_extension("part");
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            bytes.extend_from_slice(&chunk);
        }
        std::fs::write(&partial, &bytes).map_err(io_err)?;
        std::fs::rename(&partial, &path).map_err(io_err)?;
        tracing::info!(song = %stem, bytes = bytes.len(), preview = source.preview, "audio cached");
        Ok(path)
    }

    /// Downloads a cover once into `covers/` and returns the cached file. The platform's CDN is asked for a `px`
    /// square (`getSizedCoverUrl`); `http://` is read as `https://`; at most `COVER_DOWNLOADS` run at once.
    pub async fn fetch_cover(&self, url: &str, px: u32) -> Result<PathBuf, OmniError> {
        let url = https(url);
        let url = self.providers.iter().find_map(|provider| provider.sized_cover(&url, px)).unwrap_or(url);
        let dir = self.cache_dir.join("covers");
        let path = dir.join(cover_cache_key(&url));
        if path.is_file() {
            return Ok(path);
        }
        let _permit =
            self.covers.acquire().await.map_err(|err| OmniError::Response(format!("cover queue closed: {err}")))?;
        if path.is_file() {
            return Ok(path);
        }
        let io_err = |source| OmniError::Io { path: path.clone(), source };
        std::fs::create_dir_all(&dir).map_err(io_err)?;
        let bytes = self.http.get(&url).send().await?.error_for_status()?.bytes().await?;
        let partial = path.with_extension("part");
        std::fs::write(&partial, &bytes).map_err(io_err)?;
        std::fs::rename(&partial, &path).map_err(io_err)?;
        tracing::debug!(bytes = bytes.len(), px, "cover cached");
        Ok(path)
    }

    fn provider(&self, id: &str) -> Result<&dyn Provider, OmniError> {
        self.providers
            .iter()
            .find(|provider| provider.id() == id)
            .map(|provider| provider.as_ref())
            .ok_or_else(|| OmniError::UnknownProvider(id.to_owned()))
    }
}

/// Stable file name for a cover address, after `http://` is rewritten to `https://`.
pub fn cover_cache_key(url: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    https(url).hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn https(url: &str) -> String {
    url.strip_prefix("http://").map(|rest| format!("https://{rest}")).unwrap_or_else(|| url.to_owned())
}

/// A finished download of `stem` in any format.
fn cached_file(dir: &Path, stem: &str) -> Option<PathBuf> {
    std::fs::read_dir(dir).ok()?.filter_map(Result::ok).map(|entry| entry.path()).find(|path| {
        path.file_stem().is_some_and(|name| name == stem) && path.extension().is_some_and(|ext| ext != "part")
    })
}

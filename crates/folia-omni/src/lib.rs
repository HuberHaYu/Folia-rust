// Online music: one facade (`Omni`) over every platform provider; only providers talk to platform APIs.

mod providers;

use std::future::Future;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use folia_core::{Lyrics, OnlineId, Track};

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
}

#[derive(Clone)]
pub struct Omni {
    providers: Arc<Vec<Box<dyn Provider>>>,
    http: reqwest::Client,
    cache_dir: PathBuf,
}

impl Omni {
    /// Registers every provider compiled in; downloads go under `cache_dir/audio`.
    pub fn new(cache_dir: PathBuf) -> Result<Self, OmniError> {
        let http = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36")
            .build()?;
        Ok(Self { providers: Arc::new(providers::all(&http)), http, cache_dir })
    }

    /// `(id, display name)` of each provider, in registration order.
    pub fn providers(&self) -> Vec<(&'static str, &'static str)> {
        self.providers.iter().map(|provider| (provider.id(), provider.display_name())).collect()
    }

    pub async fn search(&self, provider: &str, query: &str, limit: u32, offset: u32) -> Result<Page<Track>, OmniError> {
        self.provider(provider)?.search_songs(query, limit, offset).await
    }

    pub async fn lyrics(&self, song: &OnlineId) -> Result<Option<Lyrics>, OmniError> {
        self.provider(&song.provider)?.lyrics(song).await
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

    /// Downloads a cover once into `covers/` and returns the cached file. `http://` is read as `https://`.
    pub async fn fetch_cover(&self, url: &str) -> Result<PathBuf, OmniError> {
        let url = https(url);
        let dir = self.cache_dir.join("covers");
        let path = dir.join(cover_cache_key(&url));
        if path.is_file() {
            return Ok(path);
        }
        let io_err = |source| OmniError::Io { path: path.clone(), source };
        std::fs::create_dir_all(&dir).map_err(io_err)?;
        let bytes = self.http.get(&url).send().await?.error_for_status()?.bytes().await?;
        let partial = path.with_extension("part");
        std::fs::write(&partial, &bytes).map_err(io_err)?;
        std::fs::rename(&partial, &path).map_err(io_err)?;
        tracing::info!(bytes = bytes.len(), "cover cached");
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

// A playable track, local or from an online platform.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Where an online track lives: a platform id such as `netease` and the platform's song id.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OnlineId {
    pub provider: String,
    pub id: String,
}

impl OnlineId {
    /// A stable negative id, so online tracks never collide with the library's positive row ids.
    pub fn track_id(&self) -> i64 {
        let mut hasher = DefaultHasher::new();
        self.hash(&mut hasher);
        -((hasher.finish() >> 1) as i64) - 1
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub id: i64,
    /// The local file, or the cached download of an online track (empty until downloaded).
    pub path: PathBuf,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub track_no: Option<u32>,
    pub disc_no: Option<u32>,
    /// Length in seconds; zero when the source did not say.
    pub duration: f64,
    #[serde(default)]
    pub online: Option<OnlineId>,
}

impl Track {
    /// `Artist - Title` when the artist is known, otherwise the title alone.
    pub fn display_name(&self) -> String {
        match &self.artist {
            Some(artist) => format!("{artist} - {}", self.title),
            None => self.title.clone(),
        }
    }
}

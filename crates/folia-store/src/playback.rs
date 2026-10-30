// Playback preferences that survive a restart: volume and repeat mode.

use folia_core::Repeat;
use serde::{Deserialize, Serialize};

use crate::Domain;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlaybackSettings {
    pub volume: f32,
    pub repeat: Repeat,
}

impl Default for PlaybackSettings {
    fn default() -> Self {
        Self { volume: 1.0, repeat: Repeat::Off }
    }
}

impl Domain for PlaybackSettings {
    const FILE: &'static str = "playback.toml";
    const SCHEMA_VERSION: u32 = 1;
}

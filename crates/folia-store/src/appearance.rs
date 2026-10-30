// Appearance preferences: theme, lyric animation style and how often it may redraw.

use folia_core::Theme;
use serde::{Deserialize, Serialize};

use crate::Domain;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    #[default]
    Dark,
    Light,
}

impl ThemeMode {
    pub fn theme(self) -> Theme {
        match self {
            ThemeMode::Dark => Theme::MIDNIGHT,
            ThemeMode::Light => Theme::DAYLIGHT,
        }
    }
}

/// Upper bound on visualizer redraws; `Display` follows the monitor's refresh rate.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FrameRate {
    #[serde(rename = "30")]
    Fps30,
    #[serde(rename = "45")]
    Fps45,
    #[default]
    #[serde(rename = "60")]
    Fps60,
    #[serde(rename = "90")]
    Fps90,
    #[serde(rename = "120")]
    Fps120,
    #[serde(rename = "display")]
    Display,
}

impl FrameRate {
    pub const ALL: [FrameRate; 6] =
        [FrameRate::Fps30, FrameRate::Fps45, FrameRate::Fps60, FrameRate::Fps90, FrameRate::Fps120, FrameRate::Display];

    pub fn fps(self) -> Option<u32> {
        match self {
            FrameRate::Fps30 => Some(30),
            FrameRate::Fps45 => Some(45),
            FrameRate::Fps60 => Some(60),
            FrameRate::Fps90 => Some(90),
            FrameRate::Fps120 => Some(120),
            FrameRate::Display => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppearanceSettings {
    pub theme: ThemeMode,
    pub frame_rate: FrameRate,
    pub lyrics_font_scale: f32,
    /// Id of the lyric animation style, as the original app names it.
    pub visualizer: String,
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            theme: ThemeMode::Dark,
            frame_rate: FrameRate::Fps60,
            lyrics_font_scale: 1.0,
            visualizer: "classic".to_owned(),
        }
    }
}

impl Domain for AppearanceSettings {
    const FILE: &'static str = "appearance.toml";
    const SCHEMA_VERSION: u32 = 1;
}

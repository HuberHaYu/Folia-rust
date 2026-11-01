// Playing-page chrome preferences: how far the bottom bar and everything on its baseline is lifted.

use serde::{Deserialize, Serialize};

use crate::Domain;

/// `PLAYER_BOTTOM_BAR_BASE_OFFSET_PX`, the default and least bottom offset.
pub const BOTTOM_BAR_BASE: f32 = 32.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlayerSettings {
    /// `playerBottomBarOffset`: the bar's `bottom` in pixels, at least `BOTTOM_BAR_BASE`.
    pub bottom_bar_offset: f32,
}

impl Default for PlayerSettings {
    fn default() -> Self {
        Self { bottom_bar_offset: BOTTOM_BAR_BASE }
    }
}

impl Domain for PlayerSettings {
    const FILE: &'static str = "player.toml";
    const SCHEMA_VERSION: u32 = 1;
}

/// `resolvePlayerBottomBarMaxOffset`: the expanded bar (132px) must not rise past half the window.
pub fn bottom_bar_max_offset(window_height: f32) -> f32 {
    if !window_height.is_finite() || window_height <= 0.0 {
        return BOTTOM_BAR_BASE;
    }
    (window_height * 0.5 - 132.0).round().max(BOTTOM_BAR_BASE)
}

/// `clampPlayerBottomBarOffset`: rounded into the base and the window's limit.
pub fn clamp_bottom_bar_offset(offset: f32, window_height: f32) -> f32 {
    if !offset.is_finite() {
        return BOTTOM_BAR_BASE;
    }
    offset.round().clamp(BOTTOM_BAR_BASE, bottom_bar_max_offset(window_height))
}

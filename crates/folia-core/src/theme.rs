// Colour themes of the interface and lyrics, with the original app's two built-in presets.

use std::borrow::Cow;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn hex(value: u32) -> Rgb {
        Rgb { r: (value >> 16) as u8, g: (value >> 8) as u8, b: value as u8 }
    }

    /// Linear mix towards `other`; `t` is clamped to `0.0..=1.0`.
    pub fn mix(self, other: Rgb, t: f32) -> Rgb {
        let t = t.clamp(0.0, 1.0);
        let lerp = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t).round() as u8;
        Rgb { r: lerp(self.r, other.r), g: lerp(self.g, other.g), b: lerp(self.b, other.b) }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnimationIntensity {
    Calm,
    #[default]
    Normal,
    Chaotic,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Theme {
    pub name: Cow<'static, str>,
    pub background: Rgb,
    pub primary: Rgb,
    pub accent: Rgb,
    pub secondary: Rgb,
    pub intensity: AnimationIntensity,
}

impl Theme {
    /// "Midnight Default" (午夜墨染), the original dark preset.
    pub const MIDNIGHT: Theme = Theme {
        name: Cow::Borrowed("Midnight Default"),
        background: Rgb::hex(0x09090b),
        primary: Rgb::hex(0xf4f4f5),
        accent: Rgb::hex(0xf4f4f5),
        secondary: Rgb::hex(0x71717a),
        intensity: AnimationIntensity::Normal,
    };

    /// "Daylight Default" (日光素白), the original light preset.
    pub const DAYLIGHT: Theme = Theme {
        name: Cow::Borrowed("Daylight Default"),
        background: Rgb::hex(0xf5f5f4),
        primary: Rgb::hex(0x1c1917),
        accent: Rgb::hex(0xea580c),
        secondary: Rgb::hex(0x44403c),
        intensity: AnimationIntensity::Normal,
    };
}

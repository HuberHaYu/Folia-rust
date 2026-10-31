// The lyric animation styles that exist, with their stored ids and display names.

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Style {
    #[default]
    Classic,
    Partita,
    Still,
    Tilt,
}

impl Style {
    pub const ALL: [Style; 4] = [Style::Classic, Style::Partita, Style::Still, Style::Tilt];

    /// Id used in settings files, the same as the original app's mode id.
    pub fn id(self) -> &'static str {
        match self {
            Style::Classic => "classic",
            Style::Partita => "partita",
            Style::Still => "still",
            Style::Tilt => "tilt",
        }
    }

    pub fn from_id(id: &str) -> Option<Style> {
        Style::ALL.into_iter().find(|style| style.id() == id)
    }

    /// The style a song draws when the random setting is on: stable for one song, spread over all styles.
    pub fn random_for(song_id: i64) -> Style {
        let mixed = (song_id as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15).rotate_left(29);
        Style::ALL[(mixed % Style::ALL.len() as u64) as usize]
    }

    /// i18n key of the style's display name.
    pub fn label_key(self) -> &'static str {
        match self {
            Style::Classic => "ui.visualizerClassic",
            Style::Partita => "ui.visualizerPartita",
            Style::Still => "ui.visualizerStill",
            Style::Tilt => "ui.visualizerTilt",
        }
    }

    /// The most frames per second the style can use; static styles only need to notice line changes.
    pub fn max_fps(self) -> Option<u32> {
        match self {
            Style::Classic | Style::Partita | Style::Tilt => None,
            Style::Still => Some(10),
        }
    }
}

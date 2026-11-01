// The lyric animation styles that exist, with their stored ids and display names.

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Style {
    #[default]
    Classic,
    Partita,
    Still,
    Tilt,
    Claddagh,
    Cadenza,
}

impl Style {
    pub const ALL: [Style; 6] =
        [Style::Classic, Style::Partita, Style::Still, Style::Tilt, Style::Claddagh, Style::Cadenza];

    /// Id used in settings files, the same as the original app's mode id.
    pub fn id(self) -> &'static str {
        match self {
            Style::Classic => "classic",
            Style::Partita => "partita",
            Style::Still => "still",
            Style::Tilt => "tilt",
            Style::Claddagh => "claddagh",
            Style::Cadenza => "cadenza",
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
    /// The style's 24×24 line glyph from its original `entry.tsx`, as SVG; `modeGlyphs.tsx` frames it
    /// with `stroke-width: 1.6` and round caps.
    pub fn glyph_svg(self) -> &'static str {
        match self {
            Style::Classic => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<path d="M3 12h18" opacity="0.35"/><path d="M3 12h8"/></svg>"#
            ),
            Style::Partita => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<path d="M4 7h7"/><path d="M8.5 12h7" opacity="0.7"/><path d="M13 17h7" opacity="0.45"/></svg>"#
            ),
            Style::Still => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<path d="M5 7.5h14" opacity="0.35"/><path d="M3 12h18"/><path d="M6 16.5h12" opacity="0.35"/></svg>"#
            ),
            Style::Tilt => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<path d="M4 9h16"/><path d="M5 16.5l14-3" opacity="0.55"/></svg>"#
            ),
            Style::Claddagh => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<ellipse cx="12" cy="12" rx="9" ry="4.5" transform="rotate(-18 12 12)"/><circle cx="18" cy="9" r="1.5" fill="black" stroke="none"/></svg>"#
            ),
            Style::Cadenza => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<circle cx="9.5" cy="12.5" r="3.5"/><path d="M4 6.5h3.5" opacity="0.45"/><path d="M16 8h4" opacity="0.45"/><path d="M15 17h4.5" opacity="0.45"/></svg>"#
            ),
        }
    }

    pub fn label_key(self) -> &'static str {
        match self {
            Style::Classic => "ui.visualizerClassic",
            Style::Partita => "ui.visualizerPartita",
            Style::Still => "ui.visualizerStill",
            Style::Tilt => "ui.visualizerTilt",
            Style::Claddagh => "ui.visualizerCladdagh",
            Style::Cadenza => "ui.visualizerCadenze",
        }
    }

    /// The most frames per second the style can use; static styles only need to notice line changes.
    pub fn max_fps(self) -> Option<u32> {
        match self {
            Style::Classic | Style::Partita | Style::Tilt | Style::Claddagh | Style::Cadenza => None,
            Style::Still => Some(10),
        }
    }
}

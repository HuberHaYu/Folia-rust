// The lyric animation styles that exist, with their stored ids and display names.

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Style {
    #[default]
    Classic,
}

impl Style {
    pub const ALL: [Style; 1] = [Style::Classic];

    /// Id used in settings files, the same as the original app's mode id.
    pub fn id(self) -> &'static str {
        match self {
            Style::Classic => "classic",
        }
    }

    pub fn from_id(id: &str) -> Option<Style> {
        Style::ALL.into_iter().find(|style| style.id() == id)
    }

    /// i18n key of the style's display name.
    pub fn label_key(self) -> &'static str {
        match self {
            Style::Classic => "ui.visualizerClassic",
        }
    }
}

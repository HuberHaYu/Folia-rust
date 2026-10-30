// Interface languages and the user's language preference.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Language {
    #[serde(rename = "en")]
    En,
    #[serde(rename = "zh-CN")]
    ZhCn,
    #[serde(rename = "zh-TW")]
    ZhTw,
    #[serde(rename = "in")]
    In,
    #[serde(rename = "vi")]
    Vi,
}

impl Language {
    pub const ALL: [Language; 5] = [Language::En, Language::ZhCn, Language::ZhTw, Language::In, Language::Vi];

    pub fn code(self) -> &'static str {
        match self {
            Language::En => "en",
            Language::ZhCn => "zh-CN",
            Language::ZhTw => "zh-TW",
            Language::In => "in",
            Language::Vi => "vi",
        }
    }

    /// Maps an OS locale tag such as `zh-Hant-TW` or `id_ID` to a supported language, English otherwise.
    pub fn from_locale(tag: &str) -> Language {
        let tag = tag.to_ascii_lowercase().replace('_', "-");
        let mut parts = tag.split('-');
        match parts.next() {
            Some("zh") if parts.any(|part| matches!(part, "hant" | "tw" | "hk" | "mo")) => Language::ZhTw,
            Some("zh") => Language::ZhCn,
            Some("id" | "in") => Language::In,
            Some("vi") => Language::Vi,
            _ => Language::En,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LanguagePreference {
    #[default]
    System,
    #[serde(untagged)]
    Fixed(Language),
}

impl LanguagePreference {
    /// Resolves the preference, using `system_locale` when it follows the system.
    pub fn resolve(self, system_locale: Option<&str>) -> Language {
        match self {
            LanguagePreference::Fixed(language) => language,
            LanguagePreference::System => system_locale.map_or(Language::En, Language::from_locale),
        }
    }
}

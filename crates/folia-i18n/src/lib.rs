// Interface text lookup; locales are compiled in and missing keys fall back to zh-CN like the original app.

use std::collections::HashMap;

use folia_core::Language;

const EN: &str = include_str!("../locales/en.toml");
const ZH_CN: &str = include_str!("../locales/zh-CN.toml");
const ZH_TW: &str = include_str!(concat!(env!("OUT_DIR"), "/zh-TW.toml"));
const IN: &str = include_str!("../locales/in.toml");
const VI: &str = include_str!("../locales/vi.toml");

#[derive(Debug, thiserror::Error)]
#[error("locale {code} is malformed")]
pub struct I18nError {
    code: &'static str,
    #[source]
    source: toml::de::Error,
}

#[derive(Debug, Clone)]
pub struct I18n {
    language: Language,
    strings: HashMap<String, String>,
    fallback: HashMap<String, String>,
}

impl I18n {
    pub fn new(language: Language) -> Result<Self, I18nError> {
        Ok(Self { language, strings: load(language)?, fallback: load(Language::ZhCn)? })
    }

    pub fn language(&self) -> Language {
        self.language
    }

    pub fn set_language(&mut self, language: Language) -> Result<(), I18nError> {
        self.strings = load(language)?;
        self.language = language;
        Ok(())
    }

    /// Returns the text for a dotted key such as `options.appLanguage`, or the key itself when no locale has it.
    pub fn t<'a>(&'a self, key: &'a str) -> &'a str {
        self.strings.get(key).or_else(|| self.fallback.get(key)).map_or(key, String::as_str)
    }
}

/// Parses a locale and flattens nested tables into dotted keys.
pub fn load(language: Language) -> Result<HashMap<String, String>, I18nError> {
    let source = match language {
        Language::En => EN,
        Language::ZhCn => ZH_CN,
        Language::ZhTw => ZH_TW,
        Language::In => IN,
        Language::Vi => VI,
    };
    let table: toml::Table = toml::from_str(source).map_err(|source| I18nError { code: language.code(), source })?;

    let mut strings = HashMap::new();
    flatten(&table, "", &mut strings);
    Ok(strings)
}

fn flatten(table: &toml::Table, prefix: &str, out: &mut HashMap<String, String>) {
    for (key, value) in table {
        let path = if prefix.is_empty() { key.clone() } else { format!("{prefix}.{key}") };
        match value {
            toml::Value::Table(child) => flatten(child, &path, out),
            toml::Value::String(text) => {
                out.insert(path, text.clone());
            }
            other => {
                out.insert(path, other.to_string());
            }
        }
    }
}

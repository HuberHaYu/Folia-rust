// App-wide preferences such as the interface language.

use folia_core::LanguagePreference;
use serde::{Deserialize, Serialize};

use crate::Domain;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralSettings {
    pub language: LanguagePreference,
}

impl Domain for GeneralSettings {
    const FILE: &'static str = "general.toml";
    const SCHEMA_VERSION: u32 = 1;
}

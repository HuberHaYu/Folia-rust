// Core data types shared by every Folia crate; depends on no other folia crate.

mod language;
mod lyrics;
mod track;

pub use language::{Language, LanguagePreference};
pub use lyrics::{INTERLUDE_TEXT, Line, Lyrics, Word};
pub use track::Track;

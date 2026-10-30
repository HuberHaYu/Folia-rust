// Timed lyrics shared by the parsers, the player UI and the visualizers; all times are in seconds.

use serde::{Deserialize, Serialize};

/// Text of the interlude placeholder line inserted into long instrumental gaps.
pub const INTERLUDE_TEXT: &str = "......";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Word {
    pub text: String,
    pub start: f64,
    pub end: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Line {
    pub words: Vec<Word>,
    pub start: f64,
    pub end: f64,
    pub text: String,
    pub translation: Option<String>,
    pub romanization: Option<String>,
}

impl Line {
    pub fn is_interlude(&self) -> bool {
        self.text == INTERLUDE_TEXT
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Lyrics {
    pub lines: Vec<Line>,
    pub title: Option<String>,
    pub artist: Option<String>,
    /// True when the source carried real per-word timing rather than estimated word times.
    pub word_timed: bool,
}

impl Lyrics {
    /// Index of the line sung at `time`: the last line that has started, if any.
    pub fn line_at(&self, time: f64) -> Option<usize> {
        self.lines.partition_point(|line| line.start <= time).checked_sub(1)
    }
}

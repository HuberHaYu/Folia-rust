// Lyric parsing: detects a lyric file's format and turns it into `folia_core::Lyrics`.

mod align;
mod clock;
mod lrc;
mod tagged;
mod vtt;
mod words;

use folia_core::Lyrics;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Format {
    Lrc,
    EnhancedLrc,
    Vtt,
    Ttml,
    Yrc,
    Qrc,
    Krc,
}

impl Format {
    /// The format a file extension names outright; `.lrc` and unknown extensions return `None` so the content decides.
    pub fn from_extension(extension: &str) -> Option<Format> {
        match extension.to_ascii_lowercase().as_str() {
            "vtt" => Some(Format::Vtt),
            "ttml" => Some(Format::Ttml),
            "yrc" => Some(Format::Yrc),
            "qrc" => Some(Format::Qrc),
            "krc" => Some(Format::Krc),
            _ => None,
        }
    }

    /// Guesses the format of text-based lyrics from their content.
    pub fn detect(content: &str) -> Format {
        let content = content.trim_start_matches('\u{feff}').trim();
        if is_ttml(content) {
            Format::Ttml
        } else if content.starts_with("WEBVTT") || content.lines().any(|line| vtt::is_timing_line(line.trim())) {
            Format::Vtt
        } else if !clock::scan(content, '<', '>').is_empty() || content.lines().any(has_inline_word_tags) {
            Format::EnhancedLrc
        } else {
            Format::Lrc
        }
    }
}

/// Parses `content` in `format`, matching optional translation and romanization tracks to its lines.
pub fn parse(format: Format, content: &str, translation: &str, romanization: &str) -> Lyrics {
    match format {
        Format::Lrc => lrc::parse(content, translation, romanization),
        Format::EnhancedLrc => lrc::parse_enhanced(content, translation, romanization),
        Format::Vtt => vtt::parse(content, translation, romanization),
        Format::Yrc => tagged::parse_yrc(content, translation, romanization),
        Format::Qrc => tagged::parse_qrc(content, translation, romanization),
        Format::Krc => tagged::parse_krc(content, translation, romanization),
        Format::Ttml => {
            tracing::warn!("TTML lyrics are not supported yet");
            Lyrics::default()
        }
    }
}

/// Detects the format, preferring what the file extension says, then parses.
pub fn parse_file(extension: &str, content: &str, translation: &str) -> Lyrics {
    let format = Format::from_extension(extension).unwrap_or_else(|| Format::detect(content));
    parse(format, content, translation, "")
}

/// Lines of a lyric file without a byte order mark or carriage returns.
fn text_lines(content: &str) -> std::str::Lines<'_> {
    content.trim_start_matches('\u{feff}').lines()
}

fn is_ttml(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    let has_root = lower.match_indices("<tt").any(|(i, _)| matches!(lower.as_bytes().get(i + 3), Some(b' ' | b'>')));
    has_root
        && ["http://www.w3.org/ns/ttml", "itunes:timing=", "ttm:agent=", "ttm:role="]
            .iter()
            .any(|marker| content.contains(marker))
}

/// `[mm:ss.xx]word[mm:ss.xx]…`: a timed line whose text is followed by another tag.
fn has_inline_word_tags(line: &str) -> bool {
    let line = line.trim_start();
    let tags = clock::scan(line, '[', ']');
    match tags.as_slice() {
        [first, second, ..] => {
            let between = &line[first.end..second.start];
            first.start == 0 && !between.is_empty() && !between.contains(['[', ']', '\n'])
        }
        _ => false,
    }
}

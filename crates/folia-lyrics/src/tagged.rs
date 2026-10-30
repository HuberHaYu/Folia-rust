// Word-timed formats whose lines open with `[start_ms,duration_ms]`: NetEase YRC, QQ QRC and Kugou KRC.

use base64::Engine;
use base64::engine::general_purpose::STANDARD_NO_PAD;
use folia_core::{Line, Lyrics, Word};
use serde::Deserialize;

use crate::align::{self, Entry};
use crate::{lrc, text_lines, words};

/// A `(start,duration[,flag])` style word tag; `start..end` is its byte range.
struct WordTag {
    start_ms: u64,
    duration_ms: u64,
    start: usize,
    end: usize,
}

/// YRC: `(start,duration,0)word`, absolute times.
pub fn parse_yrc(content: &str, translation: &str, romanization: &str) -> Lyrics {
    let lines = timed_lines(content, |_, _, rest| words_after_tags(rest, '(', ')', 0));
    word_timed(lines, &lrc::entries(translation), &lrc::entries(romanization))
}

/// KRC: `<offset,duration,0>word`, offsets relative to the line start; may embed translations in `[language:…]`.
pub fn parse_krc(content: &str, translation: &str, romanization: &str) -> Lyrics {
    let mut lines = timed_lines(content, |start_ms, end_ms, rest| {
        let words = words_after_tags(rest, '<', '>', start_ms);
        let plain = rest.trim();
        if words.is_empty() && !plain.is_empty() && !plain.starts_with(['[', '<']) {
            return words::spread(plain, ms(start_ms), ms(end_ms));
        }
        words
    });

    let embedded = embedded_tracks(content);
    lines.sort_by(|a, b| a.start.total_cmp(&b.start));
    for (index, line) in lines.iter_mut().enumerate() {
        line.romanization = embedded.romanization.get(index).filter(|text| !text.is_empty()).cloned();
        line.translation = embedded.translation.get(index).filter(|text| !text.is_empty()).cloned();
    }
    word_timed(lines, &lrc::entries(translation), &lrc::entries(romanization))
}

/// QRC: `word(start,duration)`; the text may sit before or after its tag, decided per line.
pub fn parse_qrc(content: &str, translation: &str, romanization: &str) -> Lyrics {
    let lines = timed_lines(content, |_, _, rest| qrc_words(rest));
    word_timed(lines, &qrc_entries(translation), &qrc_entries(romanization))
}

fn word_timed(lines: Vec<Line>, translation: &[Entry], romanization: &[Entry]) -> Lyrics {
    Lyrics { lines: align::finish(lines, translation, romanization), word_timed: true, ..Lyrics::default() }
}

/// Builds a line for every `[start,duration]` header whose body yields at least one word.
fn timed_lines(content: &str, words_of: impl Fn(u64, u64, &str) -> Vec<Word>) -> Vec<Line> {
    text_lines(content)
        .filter_map(|raw| {
            let (start_ms, duration_ms, rest) = header(raw)?;
            let end_ms = start_ms + duration_ms;
            let words = words_of(start_ms, end_ms, rest);
            if words.is_empty() {
                return None;
            }
            let text = words.iter().map(|word| word.text.as_str()).collect();
            Some(Line { words, start: ms(start_ms), end: ms(end_ms), text, ..Line::default() })
        })
        .collect()
}

fn header(line: &str) -> Option<(u64, u64, &str)> {
    let (body, rest) = line.strip_prefix('[')?.split_once(']')?;
    let (start, duration) = body.split_once(',')?;
    Some((start.parse().ok()?, duration.parse().ok()?, rest))
}

fn word_tags(s: &str, open: char, close: char) -> Vec<WordTag> {
    let mut tags = Vec::new();
    let mut from = 0;
    while let Some(offset) = s[from..].find(open) {
        let start = from + offset;
        let parsed = s[start + 1..].find(close).and_then(|len| {
            let mut numbers = s[start + 1..start + 1 + len].split(',').map(str::parse::<u64>);
            let start_ms = numbers.next()?.ok()?;
            let duration_ms = numbers.next()?.ok()?;
            let extra_ok = numbers.next().is_none_or(|flag| flag.is_ok()) && numbers.next().is_none();
            extra_ok.then_some((start_ms, duration_ms, start + 1 + len + 1))
        });
        match parsed {
            Some((start_ms, duration_ms, end)) => {
                tags.push(WordTag { start_ms, duration_ms, start, end });
                from = end;
            }
            None => from = start + 1,
        }
    }
    tags
}

/// Words whose text follows their tag up to the next tag; tag times are offset by `base_ms`.
fn words_after_tags(rest: &str, open: char, close: char, base_ms: u64) -> Vec<Word> {
    let tags = word_tags(rest, open, close);
    tags.iter()
        .enumerate()
        .map(|(i, tag)| {
            let text_end = tags.get(i + 1).map_or(rest.len(), |next| next.start);
            let start_ms = base_ms + tag.start_ms;
            Word { text: rest[tag.end..text_end].to_owned(), start: ms(start_ms), end: ms(start_ms + tag.duration_ms) }
        })
        .collect()
}

fn qrc_words(rest: &str) -> Vec<Word> {
    let tags = word_tags(rest, '(', ')');
    if tags.is_empty() {
        return Vec::new();
    }
    let mut chunks = Vec::with_capacity(tags.len() + 1);
    let mut cursor = 0;
    for tag in &tags {
        chunks.push(&rest[cursor..tag.start]);
        cursor = tag.end;
    }
    chunks.push(&rest[cursor..]);

    let blank = |s: &str| s.trim().is_empty();
    let (first, last) = (chunks[0], chunks[chunks.len() - 1]);
    let prefers_leading = !blank(first) && blank(last);
    let prefers_trailing = blank(first) && !blank(last);

    tags.iter()
        .enumerate()
        .filter_map(|(i, tag)| {
            let (leading, trailing) = (chunks[i], chunks[i + 1]);
            let text = if prefers_leading {
                leading
            } else if prefers_trailing || (!blank(trailing) && blank(leading)) {
                trailing
            } else if !blank(leading) && blank(trailing) {
                leading
            } else if trailing.len() >= leading.len() {
                trailing
            } else {
                leading
            };
            (!text.is_empty()).then(|| Word {
                text: text.to_owned(),
                start: ms(tag.start_ms),
                end: ms(tag.start_ms + tag.duration_ms),
            })
        })
        .collect()
}

/// QRC alternate tracks are QRC themselves or plain LRC.
fn qrc_entries(track: &str) -> Vec<Entry> {
    if track.trim().is_empty() {
        return Vec::new();
    }
    let lines = timed_lines(track, |_, _, rest| qrc_words(rest));
    if lines.is_empty() {
        return lrc::entries(track);
    }
    align::sorted(lines.into_iter().map(|line| Entry { start: line.start, end: None, text: line.text }).collect())
}

#[derive(Default)]
struct EmbeddedTracks {
    translation: Vec<String>,
    romanization: Vec<String>,
}

#[derive(Deserialize)]
struct LanguageTag {
    #[serde(default)]
    content: Vec<LanguageTrack>,
}

#[derive(Deserialize)]
struct LanguageTrack {
    #[serde(rename = "type")]
    kind: i64,
    #[serde(rename = "lyricContent", default)]
    lines: Vec<serde_json::Value>,
}

/// Decodes the base64 JSON in a KRC `[language:…]` tag: type 0 is romanization, type 1 translation.
fn embedded_tracks(content: &str) -> EmbeddedTracks {
    let Some(encoded) = content.split_once("[language:").and_then(|(_, rest)| rest.split_once(']')).map(|(b64, _)| b64)
    else {
        return EmbeddedTracks::default();
    };
    let decoded = STANDARD_NO_PAD
        .decode(encoded.trim().trim_end_matches('='))
        .map_err(|err| err.to_string())
        .and_then(|bytes| serde_json::from_slice::<LanguageTag>(&bytes).map_err(|err| err.to_string()));
    let tag = match decoded {
        Ok(tag) => tag,
        Err(err) => {
            tracing::warn!(error = %err, "cannot decode KRC language tag");
            return EmbeddedTracks::default();
        }
    };

    let track = |kind: i64| -> Vec<String> {
        tag.content
            .iter()
            .find(|track| track.kind == kind)
            .map(|track| track.lines.iter().map(join_line).collect())
            .unwrap_or_default()
    };
    EmbeddedTracks { romanization: track(0), translation: track(1) }
}

fn join_line(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Array(parts) => {
            parts.iter().map(|part| part.as_str().map_or_else(|| part.to_string(), str::to_owned)).collect::<String>()
        }
        serde_json::Value::String(text) => text.clone(),
        other => other.to_string(),
    }
    .trim()
    .to_owned()
}

fn ms(value: u64) -> f64 {
    value as f64 / 1000.0
}

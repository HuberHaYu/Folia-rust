// LRC and enhanced LRC (per-word `<mm:ss.xx>` or inline `[mm:ss.xx]` tags), plus LRC alternate tracks.

use folia_core::{Line, Lyrics, Word};

use crate::align::{self, Entry};
use crate::clock::{self, Tag};
use crate::{text_lines, words};

struct DraftWord {
    text: String,
    start: f64,
    end: Option<f64>,
}

struct Draft {
    words: Vec<DraftWord>,
    start: f64,
    end: Option<f64>,
    text: String,
}

/// Reads an LRC-style alternate track; word tags are dropped and only line text and times are kept.
pub fn entries(content: &str) -> Vec<Entry> {
    let entries = text_lines(content)
        .map(str::trim)
        .filter(|line| !line.is_empty() && !is_metadata(line))
        .filter_map(draft)
        .filter(|draft| !draft.text.is_empty())
        .map(|draft| Entry { start: draft.start, end: draft.end, text: draft.text })
        .collect();
    align::sorted(entries)
}

/// Plain LRC: one time per line, word times estimated from the text.
pub fn parse(content: &str, translation: &str, romanization: &str) -> Lyrics {
    let entries = align::sorted(text_lines(content).filter_map(simple).collect());
    let lines = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let mut duration = entries.get(index + 1).map_or(5.0, |next| next.start - entry.start);
            let reading = entry.text.chars().count() as f64 * 0.5;
            if duration > reading + 2.0 && duration > 5.0 {
                duration = reading + 2.0;
            }
            let end = entry.start + duration;
            timed_line(&entry.text, entry.start, end, words::spread(&entry.text, entry.start, end))
        })
        .collect();

    Lyrics { lines: align::finish(lines, &entries_of(translation), &entries_of(romanization)), ..Lyrics::default() }
}

/// Enhanced LRC: lines may carry word tags; untagged lines get estimated word times.
pub fn parse_enhanced(content: &str, translation: &str, romanization: &str) -> Lyrics {
    let mut lyrics = Lyrics::default();
    let mut drafts = Vec::new();
    for line in text_lines(content).map(str::trim).filter(|line| !line.is_empty()) {
        if let Some((key, value)) = metadata(line) {
            let slot = if key == "ti" { &mut lyrics.title } else { &mut lyrics.artist };
            if !value.is_empty() {
                *slot = Some(value.to_owned());
            }
        } else if let Some(draft) = draft(line) {
            drafts.push(draft);
        }
    }
    drafts.sort_by(|a, b| a.start.total_cmp(&b.start));
    lyrics.word_timed = drafts.iter().any(|draft| !draft.words.is_empty());

    let lines = drafts
        .iter()
        .enumerate()
        .map(|(index, draft)| {
            let next_start = drafts.get(index + 1).map(|next| next.start);
            let mut end = draft.end.or(next_start).unwrap_or(draft.start + 5.0).max(draft.start + 0.001);
            let words = if draft.words.is_empty() {
                words::spread(&draft.text, draft.start, end)
            } else {
                draft
                    .words
                    .iter()
                    .enumerate()
                    .map(|(i, word)| {
                        let fallback = draft.words.get(i + 1).map_or(end, |next| next.start);
                        let word_end = word.end.unwrap_or(fallback).max(word.start + 0.001);
                        Word { text: word.text.clone(), start: word.start, end: word_end }
                    })
                    .collect()
            };
            if let Some(last) = words.last() {
                end = end.max(last.end);
            }
            timed_line(&draft.text, draft.start, end, words)
        })
        .collect();

    lyrics.lines = align::finish(lines, &entries_of(translation), &entries_of(romanization));
    lyrics
}

fn entries_of(track: &str) -> Vec<Entry> {
    if track.trim().is_empty() { Vec::new() } else { entries(track) }
}

fn timed_line(text: &str, start: f64, end: f64, words: Vec<Word>) -> Line {
    Line { words, start, end, text: text.to_owned(), ..Line::default() }
}

/// A line's draft: angle word tags first, then inline bracket tags, then a plain timed line.
fn draft(line: &str) -> Option<Draft> {
    let body = strip_first_tag(line);
    if body.contains('<')
        && let Some(draft) = precise(body, &clock::scan(body, '<', '>'))
    {
        return Some(draft);
    }
    if line.get(1..).is_some_and(|rest| rest.contains('['))
        && let Some(draft) = precise(line, &clock::scan(line, '[', ']'))
    {
        return Some(draft);
    }
    simple(line).map(|entry| Draft { words: Vec::new(), start: entry.start, end: None, text: entry.text })
}

fn precise(content: &str, tags: &[Tag]) -> Option<Draft> {
    if tags.len() < 2 {
        return None;
    }
    let mut words = Vec::new();
    let mut text = String::new();
    let mut push = |segment: &str, start: f64, end: Option<f64>| {
        text.push_str(segment);
        if !segment.trim().is_empty() {
            words.push(DraftWord { text: segment.to_owned(), start, end });
        }
    };
    for pair in tags.windows(2) {
        push(&content[pair[0].end..pair[1].start], pair[0].time, Some(pair[1].time));
    }
    let last = &tags[tags.len() - 1];
    push(&content[last.end..], last.time, None);

    if text.trim().is_empty() {
        return None;
    }
    let start = words.first().map_or(tags[0].time, |word| word.start);
    let end = words.last().and_then(|word| word.end);
    Some(Draft { words, start, end, text })
}

fn simple(line: &str) -> Option<Entry> {
    let (start, text) = clock::leading(line)?;
    let text = text.trim();
    (!text.is_empty()).then(|| Entry { start, end: None, text: text.to_owned() })
}

fn strip_first_tag(line: &str) -> &str {
    line.strip_prefix('[')
        .and_then(|rest| rest.split_once(']'))
        .filter(|(clock, _)| clock::parse_clock(clock).is_some())
        .map_or(line, |(_, rest)| rest)
}

/// `[ti:Title]` style metadata; returns the lower-cased key and the trimmed value.
fn metadata(line: &str) -> Option<(String, &str)> {
    let body = line.strip_prefix('[')?.strip_suffix(']')?;
    let (key, value) = body.split_once(':')?;
    let key = key.to_ascii_lowercase();
    (matches!(key.as_str(), "ti" | "ar") && !value.contains(']')).then_some((key, value.trim()))
}

fn is_metadata(line: &str) -> bool {
    metadata(line).is_some()
}

// WebVTT subtitles used as line-timed lyrics.

use folia_core::{Line, Lyrics};

use crate::align::{self, Entry};
use crate::words;

pub fn parse(content: &str, translation: &str, romanization: &str) -> Lyrics {
    let cues = entries(content);
    let lines = cues
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let fallback = cues.get(index + 1).map_or(entry.start + 5.0, |next| next.start);
            let end = entry.end.unwrap_or(fallback).max(entry.start + 0.1);
            let words = words::spread(&entry.text, entry.start, end);
            Line { words, start: entry.start, end, text: entry.text.clone(), ..Line::default() }
        })
        .collect();
    Lyrics { lines: align::finish(lines, &entries(translation), &entries(romanization)), ..Lyrics::default() }
}

/// True for a cue timing line such as `00:01.000 --> 00:04.000 align:start`.
pub fn is_timing_line(line: &str) -> bool {
    timing(line).is_some()
}

fn entries(content: &str) -> Vec<Entry> {
    let normalized = content.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    let entries = normalized
        .split("\n\n")
        .filter_map(|block| {
            let lines: Vec<&str> = block.lines().map(str::trim).filter(|line| !line.is_empty()).collect();
            let first = *lines.first()?;
            if first == "WEBVTT" || first.starts_with("NOTE") || first == "STYLE" || first == "REGION" {
                return None;
            }
            let timing_index = lines.iter().position(|line| is_timing_line(line))?;
            let (start, end) = timing(lines[timing_index])?;
            let text = strip_cue_text(&lines[timing_index + 1..].join(" "));
            (!text.is_empty()).then_some(Entry { start, end: Some(end), text })
        })
        .collect();
    align::sorted(entries)
}

fn timing(line: &str) -> Option<(f64, f64)> {
    let (start, rest) = line.split_once("-->")?;
    let end = rest.split_whitespace().next()?;
    Some((timestamp(start.trim())?, timestamp(end)?))
}

/// `hh:mm:ss.mmm` or `mm:ss.mmm`, with two digit fields and a three digit fraction.
fn timestamp(s: &str) -> Option<f64> {
    let (clock, fraction) = s.split_once('.')?;
    let fields: Vec<&str> = clock.split(':').collect();
    let digits = |part: &str, len: usize| part.len() == len && part.bytes().all(|b| b.is_ascii_digit());
    if !(2..=3).contains(&fields.len()) || !fields.iter().all(|field| digits(field, 2)) || !digits(fraction, 3) {
        return None;
    }
    let seconds = fields.iter().try_fold(0.0, |total, field| Some(total * 60.0 + field.parse::<f64>().ok()?))?;
    Some(seconds + fraction.parse::<f64>().ok()? / 1000.0)
}

fn strip_cue_text(text: &str) -> String {
    let mut plain = String::with_capacity(text.len());
    let mut in_tag = false;
    for c in text.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => plain.push(c),
            _ => {}
        }
    }
    let plain = plain.replace("&nbsp;", " ").replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&");
    plain.split_whitespace().collect::<Vec<_>>().join(" ")
}

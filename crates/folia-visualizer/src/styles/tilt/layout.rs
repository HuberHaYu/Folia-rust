// Tilt's line breaking: how many rows a line becomes, which row is italic, and how far it scales down to fit.

use folia_core::{Line, Word};
use folia_lyrics::{RenderHints, is_js_space, split_sentences};

const LOG_OFFSET: f64 = 4.0;
const CHAR_REF: f64 = 20.0;
const LINE_THRESHOLDS: [f64; 3] = [0.45, 1.05, 1.7];
const RESPLIT_THRESHOLD: f32 = 1.6;
/// The most rows one lyric line is allowed to become. The original caps the resplit at 4.
pub const MAX_ROWS: usize = 4;
const SCALE_FLOOR_NORMAL: f32 = 0.55;
const SCALE_FLOOR_TILT: f32 = 0.5;

/// The original default tuning. The settings panel that edits it is not ported.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TiltTuning {
    pub split_probability: f64,
    pub tilt_style_probability: f64,
}

impl Default for TiltTuning {
    fn default() -> Self {
        Self { split_probability: 0.75, tilt_style_probability: 0.35 }
    }
}

/// One row of a tilted line.
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub text: String,
    pub tilt: bool,
    pub short_last: bool,
    /// Index into the line text where this row's trimmed text begins.
    pub char_offset: usize,
}

/// Rows plus the factor applied to every font size so the widest row fits.
#[derive(Debug, Clone, PartialEq)]
pub struct Arrangement {
    pub segments: Vec<Segment>,
    pub scale: f32,
}

/// `sin` fraction used everywhere the original style rolls a layout decision.
pub fn seeded(seed: f64, offset: f64) -> f64 {
    let x = (seed * 1000.0 + offset).sin() * 10000.0;
    x - x.floor()
}

/// How many rows `char_count` characters become.
pub fn line_count(char_count: usize, seed: f64, split_probability: f64) -> usize {
    let normalized = (char_count as f64 + LOG_OFFSET).ln() / (CHAR_REF + LOG_OFFSET).ln();
    let score = normalized * (seeded(seed, 1.0) * 0.6 + 0.7) * split_probability;
    let rows = LINE_THRESHOLDS.iter().position(|threshold| score < *threshold).map_or(MAX_ROWS, |index| index + 1);
    rows.min(MAX_ROWS)
}

/// Splits `text` the way the tilt style does. `measure(text, tilt)` is the unscaled row width.
pub fn arrange(
    text: &str,
    seed: f64,
    tuning: TiltTuning,
    available: f32,
    measure: impl Fn(&str, bool) -> f32,
) -> Arrangement {
    let pieces = if ellipsis_only(text) {
        vec![text.to_owned()]
    } else {
        let count = line_count(text.trim().chars().count(), seed, tuning.split_probability);
        split_sentences(text, count, seed)
    };
    let mut segments = segments_from(&pieces);
    if let Some(index) = pick_tilt(segments.len(), seed, tuning.tilt_style_probability) {
        segments[index].tilt = true;
    }
    if let Some(replaced) = resplit(&segments, text, seed, tuning, available, &measure) {
        return replaced;
    }
    let scale = shrink_to_fit(&segments, available, &measure);
    mark_short_last(&mut segments);
    Arrangement { segments, scale }
}

fn ellipsis_only(text: &str) -> bool {
    let trimmed = text.trim();
    !trimmed.is_empty() && trimmed.chars().all(|c| matches!(c, '.' | '…' | '·' | '。') || is_js_space(c))
}

fn segments_from(pieces: &[String]) -> Vec<Segment> {
    let mut offset = 0;
    let mut segments = Vec::with_capacity(pieces.len());
    for piece in pieces {
        let leading = piece.chars().count() - piece.trim_start().chars().count();
        segments.push(Segment {
            text: piece.trim().to_owned(),
            tilt: false,
            short_last: false,
            char_offset: offset + leading,
        });
        offset += piece.chars().count();
    }
    segments
}

fn pick_tilt(count: usize, seed: f64, probability: f64) -> Option<usize> {
    let candidates: Vec<usize> = (0..count).filter(|i| seeded(seed, 100.0 + *i as f64) < probability).collect();
    let pick = (seeded(seed, 200.0) * candidates.len() as f64).floor() as usize;
    candidates.get(pick).copied()
}

fn resplit(
    segments: &[Segment],
    text: &str,
    seed: f64,
    tuning: TiltTuning,
    available: f32,
    measure: &impl Fn(&str, bool) -> f32,
) -> Option<Arrangement> {
    let (tilt_over, normal_over) = overflow(segments, available, measure);
    if tilt_over.max(normal_over) < RESPLIT_THRESHOLD {
        return None;
    }
    let target_width = available * (RESPLIT_THRESHOLD - 0.15);
    let estimated = measure(text, false);
    let extra = (segments.len() + 1).max((estimated / target_width).ceil() as usize).min(MAX_ROWS);
    if extra <= segments.len() {
        return None;
    }
    let mut segments = segments_from(&split_sentences(text, extra, seed));
    if seeded(seed, 300.0) < tuning.tilt_style_probability && !segments.is_empty() {
        let pick = (seeded(seed, 301.0) * segments.len() as f64).floor() as usize;
        if let Some(segment) = segments.get_mut(pick) {
            segment.tilt = true;
        }
    }
    let scale = resplit_scale(&segments, available, measure);
    mark_short_last(&mut segments);
    Some(Arrangement { segments, scale })
}

fn resplit_scale(segments: &[Segment], available: f32, measure: &impl Fn(&str, bool) -> f32) -> f32 {
    if let Some(tilt) = segments.iter().find(|segment| segment.tilt) {
        let width = measure(&tilt.text, true);
        return if width > available { (available / width).max(SCALE_FLOOR_TILT) } else { 1.0 };
    }
    match widest(segments, false, measure) {
        Some((_, width)) if width > available => (available / width).max(SCALE_FLOOR_NORMAL),
        _ => 1.0,
    }
}

fn shrink_to_fit(segments: &[Segment], available: f32, measure: &impl Fn(&str, bool) -> f32) -> f32 {
    let (tilt_over, normal_over) = overflow(segments, available, measure);
    if tilt_over.max(normal_over) <= 1.0 {
        return 1.0;
    }
    if tilt_over >= normal_over
        && let Some((_, width)) = widest(segments, true, measure)
    {
        return (available / width).max(SCALE_FLOOR_TILT);
    }
    widest(segments, false, measure).map_or(1.0, |(_, width)| (available / width).max(SCALE_FLOOR_NORMAL))
}

fn overflow(segments: &[Segment], available: f32, measure: &impl Fn(&str, bool) -> f32) -> (f32, f32) {
    let over = |tilt| {
        widest(segments, tilt, measure).map_or(0.0, |(_, width)| if width > 0.0 { width / available } else { 0.0 })
    };
    (over(true), over(false))
}

fn widest<'a>(segments: &'a [Segment], tilt: bool, measure: &impl Fn(&str, bool) -> f32) -> Option<(&'a Segment, f32)> {
    segments
        .iter()
        .filter(|segment| segment.tilt == tilt)
        .map(|segment| (segment, measure(&segment.text, tilt)))
        .max_by(|left, right| left.1.total_cmp(&right.1))
}

fn mark_short_last(segments: &mut [Segment]) {
    if segments.len() < 2 {
        return;
    }
    let last = segments.len() - 1;
    let last_len = segments[last].text.trim().chars().count();
    let prev_len = segments[last - 1].text.chars().count();
    if !segments[last].tilt && last_len <= 2 && last_len * 2 <= prev_len {
        segments[last].short_last = true;
    }
}

/// Start and end of each row, taken from the words that overlap it.
pub fn segment_times(line: &Line, segments: &[Segment]) -> Vec<(f64, f64)> {
    let fallback = RenderHints::of(line).render_end;
    segments
        .iter()
        .map(|segment| {
            let words = &line.words[word_range(segment.char_offset, &segment.text, &line.text, &line.words)];
            match (words.first(), words.last()) {
                (Some(first), Some(last)) => (first.start, last.end),
                _ => (line.start, fallback),
            }
        })
        .collect()
}

pub(crate) fn word_range(char_offset: usize, segment: &str, full: &str, words: &[Word]) -> std::ops::Range<usize> {
    if char_offset >= full.chars().count() {
        return 0..words.len();
    }
    let segment_end = char_offset + segment.chars().count();
    let mut pos = 0;
    let mut start_word = 0;
    let mut end_word = words.len();
    for (index, word) in words.iter().enumerate() {
        let Some(word_start) = find_from(full, &word.text, pos) else {
            pos += word.text.chars().count();
            continue;
        };
        let word_end = word_start + word.text.chars().count();
        if word_start <= char_offset && word_end > char_offset {
            start_word = index;
        }
        if word_start < segment_end && word_end >= segment_end {
            end_word = index + 1;
            break;
        }
        pos = word_end;
    }
    start_word..end_word
}

fn find_from(haystack: &str, needle: &str, from_char: usize) -> Option<usize> {
    if needle.is_empty() {
        return Some(from_char);
    }
    let hay: Vec<char> = haystack.chars().collect();
    let needle: Vec<char> = needle.chars().collect();
    hay.get(from_char..)?.windows(needle.len()).position(|window| window == needle.as_slice()).map(|at| at + from_char)
}

/// Last row that has reached its reveal lead, or `None` outside the line's window.
pub fn revealed_through(time: f64, starts: &[f64], line_start: f64, render_end: f64) -> Option<usize> {
    let count = visible_row_count(starts, time, line_start, render_end);
    count.checked_sub(1)
}

/// How many rows are on screen at `time`, including a line frozen at its exit instant.
pub fn visible_row_count(starts: &[f64], time: f64, line_start: f64, render_end: f64) -> usize {
    if time < line_start - 0.1 || time > render_end {
        return 0;
    }
    starts.iter().enumerate().filter(|(_, start)| time >= *start - 0.25).map(|(index, _)| index + 1).max().unwrap_or(0)
}

/// The original per-character pulse: a sine while the character is sung, then a short afterglow floor.
pub fn char_pulse(time: f64, start: f64, end: f64) -> f64 {
    let duration = (end - start).max(0.05).clamp(0.2, 0.9);
    let elapsed = time - start;
    if elapsed < 0.0 {
        return 0.0;
    }
    if elapsed <= duration {
        return (elapsed / duration * std::f64::consts::PI).sin();
    }
    let after = elapsed - duration;
    let ramp = duration * 1.2;
    if after >= ramp { 0.25 } else { 0.25 * after / ramp }
}

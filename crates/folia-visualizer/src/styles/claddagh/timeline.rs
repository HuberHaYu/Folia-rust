// Grapheme times for the Claddagh ring, from the original claddaghTimeline.ts.

use folia_core::Line;

/// One grapheme and the time it is sung.
#[derive(Debug, Clone, PartialEq)]
pub struct Mark {
    pub text: String,
    pub start: f64,
    pub end: f64,
}

/// The original `isCJKChar` ranges, not the whole Unicode CJK set.
pub fn is_cjk(ch: char) -> bool {
    ('\u{4e00}'..='\u{9fa5}').contains(&ch)
        || ('\u{3040}'..='\u{30ff}').contains(&ch)
        || ('\u{ac00}'..='\u{d7af}').contains(&ch)
}

/// CJK counts as 1, everything else as 0.5, matching `getVisualLength`.
pub fn visual_length(text: &str) -> f64 {
    text.chars().map(|ch| if is_cjk(ch) { 1.0 } else { 0.5 }).sum()
}

/// Even slices of `text` across `[start, end]`. Rust `char`s stand in for Intl graphemes.
pub fn even_marks(text: &str, start: f64, end: f64) -> Vec<Mark> {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return Vec::new();
    }
    let unit = (end - start).max(0.0) / chars.len() as f64;
    chars
        .into_iter()
        .enumerate()
        .map(|(index, ch)| Mark {
            text: ch.to_string(),
            start: start + unit * index as f64,
            end: if index + 1 == chars_len(text) { end } else { start + unit * (index as f64 + 1.0) },
        })
        .collect()
}

fn chars_len(text: &str) -> usize {
    text.chars().count()
}

/// Word times mapped onto the line text, including gaps the words do not cover.
pub fn line_marks(line: &Line) -> Vec<Mark> {
    let graphemes: Vec<String> = line.text.chars().map(|ch| ch.to_string()).collect();
    if graphemes.is_empty() {
        return Vec::new();
    }
    if line.words.is_empty() {
        return even_marks(&line.text, line.start, line.end);
    }
    let mut timeline = vec![None; graphemes.len()];
    let mut cursor = 0;
    let mut last = line.start;
    for word in &line.words {
        let word_chars: Vec<String> = word.text.chars().map(|ch| ch.to_string()).collect();
        if word_chars.is_empty() {
            continue;
        }
        let matched = find_sequence(&graphemes, &word_chars, cursor);
        let start = if matched >= 0 { matched as usize } else { cursor };
        let end = (start + word_chars.len()).min(graphemes.len());
        for gap in cursor..start {
            timeline[gap] = Some(Mark { text: graphemes[gap].clone(), start: word.start, end: word.start });
        }
        let word_marks = even_marks(&word.text, word.start, word.end);
        for (local, slot) in (start..end).enumerate() {
            let Some(timing) = word_marks.get(local) else { continue };
            timeline[slot] = Some(Mark { text: graphemes[slot].clone(), start: timing.start, end: timing.end });
            last = last.max(timing.end);
        }
        cursor = cursor.max(end);
    }
    for (index, slot) in timeline.iter_mut().enumerate() {
        if slot.is_none() {
            *slot = Some(Mark { text: graphemes[index].clone(), start: last, end: last });
        }
    }
    timeline.into_iter().flatten().collect()
}

fn find_sequence(source: &[String], target: &[String], from: usize) -> isize {
    if target.is_empty() {
        return from as isize;
    }
    let last = source.len().saturating_sub(target.len());
    for index in from..=last {
        if source[index..index + target.len()] == *target {
            return index as isize;
        }
    }
    -1
}

/// Gives zero-length gaps at least 60ms, borrowing from the neighbours, as `adjustCladdaghTimeline`.
pub fn adjust(marks: &mut [Mark], line_start: f64, line_end: f64) {
    let n = marks.len();
    let mut i = 0;
    while i < n {
        if marks[i].start != marks[i].end {
            i += 1;
            continue;
        }
        let mut j = i;
        while j < n && marks[j].start == marks[j].end {
            j += 1;
        }
        let gap_count = j - i;
        let mut gap_start = if i > 0 { marks[i - 1].end } else { line_start };
        let mut gap_end = if j < n { marks[j].start } else { line_end };
        let min_needed = gap_count as f64 * 0.06;
        let mut duration = gap_end - gap_start;
        if duration < min_needed {
            let deficit = min_needed - duration;
            if i > 0 && j < n {
                let half = deficit / 2.0;
                let prev_steal = half.min((marks[i - 1].end - marks[i - 1].start - 0.04).max(0.0));
                let next_steal = (deficit - prev_steal).min((marks[j].end - marks[j].start - 0.04).max(0.0));
                gap_start -= prev_steal;
                gap_end += next_steal;
                marks[i - 1].end = gap_start;
                marks[j].start = gap_end;
            } else if i > 0 {
                let prev_steal = deficit.min((marks[i - 1].end - marks[i - 1].start - 0.04).max(0.0));
                gap_start -= prev_steal;
                marks[i - 1].end = gap_start;
            } else if j < n {
                let next_steal = deficit.min((marks[j].end - marks[j].start - 0.04).max(0.0));
                gap_end += next_steal;
                marks[j].start = gap_end;
            }
            duration = gap_end - gap_start;
        }
        let unit = if duration > 0.0 { duration / gap_count as f64 } else { 0.0 };
        for (k, mark) in marks.iter_mut().enumerate().take(j).skip(i) {
            let at = (k - i) as f64;
            mark.start = gap_start + unit * at;
            mark.end = gap_start + unit * (at + 1.0);
        }
        i = j;
    }
}

pub fn clamp(value: f64, min: f64, max: f64) -> f64 {
    value.max(min).min(max)
}

/// Fractional grapheme index at `time`, matching `getFractionalActiveIndex`.
pub fn fractional_index(marks: &[Mark], time: f64, render_end: Option<f64>) -> f64 {
    fractional_index_at(marks.len(), time, render_end, |index| (marks[index].start, marks[index].end))
}

/// Same index, reading times through `at` so a cached placement does not build a new list.
pub fn fractional_index_at(len: usize, time: f64, render_end: Option<f64>, at: impl Fn(usize) -> (f64, f64)) -> f64 {
    if len == 0 {
        return 0.0;
    }
    let first = at(0);
    if len == 1 {
        let end = render_end.filter(|end| end.is_finite()).unwrap_or(first.1);
        let duration = (end - first.0).max(0.2);
        if time <= first.0 {
            return 0.0;
        }
        return (time - first.0) / duration;
    }
    if time <= first.0 {
        return 0.0;
    }
    let last = len - 1;
    let last_time = at(last);
    if time >= last_time.0 {
        if let Some(end) = render_end.filter(|end| end.is_finite() && *end > last_time.0) {
            let progress = clamp((time - last_time.0) / (end - last_time.0), 0.0, 1.0);
            return last as f64 + progress * 2.0;
        }
        let prev = at(last - 1);
        let item_dur = last_time.1 - last_time.0;
        let gap_dur = last_time.0 - prev.0;
        let step = if item_dur > 0.0 {
            item_dur
        } else if gap_dur > 0.0 {
            gap_dur
        } else {
            0.5
        };
        return last as f64 + ((time - last_time.0) / step).min(1.8);
    }
    for i in 0..last {
        let (from, _) = at(i);
        let (to, _) = at(i + 1);
        if time >= from && time < to {
            if to == from {
                return i as f64;
            }
            return i as f64 + (time - from) / (to - from);
        }
    }
    last as f64
}

/// Ring angle contributed by the sung grapheme, in radians along the arc.
pub fn word_offset(nominal: &[f64], marks: &[Mark], time: f64, render_end: Option<f64>) -> f64 {
    if nominal.is_empty() || nominal.len() != marks.len() {
        return 0.0;
    }
    let index = fractional_index(marks, time, render_end);
    let last = nominal.len() - 1;
    if index <= last as f64 {
        let at = index.floor() as usize;
        let frac = index - at as f64;
        let left = nominal[at];
        let right = nominal[at.min(last).saturating_add(1).min(last)];
        return left + (right - left) * frac;
    }
    let step = nominal[last] - nominal[last.saturating_sub(1)];
    nominal[last] + step * (index - last as f64)
}

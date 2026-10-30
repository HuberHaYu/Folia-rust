// Alternate tracks (translation, romanization) and the finishing steps every parser shares.

use folia_core::Line;

use crate::words;

const MATCH_WINDOW: f64 = 1.0;

/// One timed text entry of an alternate track.
#[derive(Debug, Clone)]
pub struct Entry {
    pub start: f64,
    pub end: Option<f64>,
    pub text: String,
}

/// Pairs each start time with an entry within ±1 s; closest pairs are settled first and each side is used once.
pub fn match_entries(starts: &[f64], entries: &[Entry]) -> Vec<Option<String>> {
    let mut matched = vec![None; starts.len()];
    let mut candidates = Vec::new();
    for (line, &start) in starts.iter().enumerate() {
        let first = entries.partition_point(|entry| entry.start < start - MATCH_WINDOW);
        for (index, entry) in entries.iter().enumerate().skip(first) {
            if entry.start >= start + MATCH_WINDOW {
                break;
            }
            let diff = (entry.start - start).abs();
            if diff < MATCH_WINDOW {
                candidates.push((diff, line, index));
            }
        }
    }
    candidates.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));

    let mut entry_taken = vec![false; entries.len()];
    for (_, line, index) in candidates {
        if matched[line].is_none() && !entry_taken[index] {
            entry_taken[index] = true;
            matched[line] = Some(entries[index].text.clone());
        }
    }
    matched
}

/// Sorts lines by start, attaches alternate tracks where a line has none yet, and inserts interludes.
pub fn finish(mut lines: Vec<Line>, translation: &[Entry], romanization: &[Entry]) -> Vec<Line> {
    lines.sort_by(|a, b| a.start.total_cmp(&b.start));
    let starts: Vec<f64> = lines.iter().map(|line| line.start).collect();
    let translations = match_entries(&starts, translation);
    let romanizations = match_entries(&starts, romanization);
    for ((line, t), r) in lines.iter_mut().zip(translations).zip(romanizations) {
        line.translation = line.translation.take().or(t);
        line.romanization = line.romanization.take().or(r);
    }
    words::insert_interludes(lines)
}

/// Sorts entries by start time, keeping the file order of equal times.
pub fn sorted(mut entries: Vec<Entry>) -> Vec<Entry> {
    entries.sort_by(|a, b| a.start.total_cmp(&b.start));
    entries
}

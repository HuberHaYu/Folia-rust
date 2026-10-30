// Estimated word timing for line-timed lyrics and the interlude lines placed in long gaps.

use folia_core::{INTERLUDE_TEXT, Line, Word};

const INTERLUDE_GAP: f64 = 3.0;
const MIN_WORD: f64 = 0.05;

/// Spreads a line's words over `start..end`, weighting CJK characters equally and longer Latin words more.
pub fn spread(text: &str, start: f64, end: f64) -> Vec<Word> {
    let mut tokens: Vec<(String, f64)> = Vec::new();
    for token in text.split_whitespace() {
        if token.chars().any(is_cjk) {
            tokens.extend(token.chars().map(|c| (c.to_string(), if is_cjk_punctuation(c) { 0.0 } else { 1.0 })));
        } else {
            tokens.push((token.to_owned(), 1.0 + token.chars().count() as f64 * 0.15));
        }
    }

    let total: f64 = tokens.iter().map(|(_, weight)| weight).sum();
    let per_weight = (end - start).max(0.1) * 0.9 / if total > 0.0 { total } else { 1.0 };
    let mut cursor = start;
    let mut words: Vec<Word> = tokens
        .into_iter()
        .map(|(text, weight)| {
            let duration = weight * per_weight;
            let word = Word { text, start: cursor, end: cursor + duration.max(MIN_WORD) };
            cursor += if weight > 0.0 { duration } else { MIN_WORD };
            word
        })
        .collect();

    if let Some(last_end) = words.last().map(|word| word.end)
        && last_end > end
    {
        let scale = (end - start) / (last_end - start);
        for word in &mut words {
            word.start = start + (word.start - start) * scale;
            word.end = start + (word.end - start) * scale;
        }
    }
    words
}

/// Inserts an interlude line before a late first line and into every gap longer than three seconds.
pub fn insert_interludes(lines: Vec<Line>) -> Vec<Line> {
    let mut out = Vec::with_capacity(lines.len() + 4);
    if let Some(first) = lines.first()
        && first.start > INTERLUDE_GAP
    {
        out.push(interlude(0.5, first.start - 0.5));
    }

    let mut lines = lines.into_iter().peekable();
    while let Some(line) = lines.next() {
        let gap_end = lines.peek().filter(|next| next.start - line.end > INTERLUDE_GAP).map(|next| next.start);
        let gap_start = line.end;
        out.push(line);
        if let Some(gap_end) = gap_end {
            out.push(interlude(gap_start + 0.05, gap_end - 0.05));
        }
    }
    out
}

fn interlude(start: f64, end: f64) -> Line {
    let step = (end - start) / 6.0;
    let words = (0..6)
        .map(|i| Word { text: ".".to_owned(), start: start + i as f64 * step, end: start + (i + 1) as f64 * step })
        .collect();
    Line { words, start, end, text: INTERLUDE_TEXT.to_owned(), ..Line::default() }
}

fn is_cjk(c: char) -> bool {
    matches!(c, '\u{4e00}'..='\u{9fa5}' | '\u{3040}'..='\u{30ff}' | '\u{ac00}'..='\u{d7af}')
}

fn is_cjk_punctuation(c: char) -> bool {
    matches!(c, '，' | '。' | '！' | '？' | '、' | '：' | '；' | '"' | '\'' | '）')
}

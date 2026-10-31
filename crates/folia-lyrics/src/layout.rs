// Display grouping of a line's timed words: CJK words joined by a word segmenter, punctuation and contractions kept attached.

use std::sync::OnceLock;

use folia_core::{Line, Word};
use icu_segmenter::WordSegmenter;
use icu_segmenter::options::WordBreakInvariantOptions;

/// A group of consecutive timed words that a visualizer places as one unit.
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutUnit {
    pub text: String,
    /// Index range into `line.words`.
    pub words: std::ops::Range<usize>,
    pub start: f64,
    pub end: f64,
    /// A CJK word made of several timed characters, which keep their own timing when drawn.
    pub semantic: bool,
    /// Punctuation or a contraction was attached to it.
    pub sticky: bool,
}

/// Semantic grouping (when asked) followed by sticky punctuation, as the original visualizers do.
pub fn layout_units(line: &Line, semantic: bool) -> Vec<LayoutUnit> {
    let units = if semantic { semantic_units(line) } else { None };
    attach_punctuation(&line.words, units.unwrap_or_else(|| single_units(&line.words)))
}

/// The words to draw: sticky units become one word, CJK words keep their per-character words.
pub fn display_words(line: &Line, units: &[LayoutUnit]) -> Vec<Word> {
    units
        .iter()
        .flat_map(|unit| {
            if unit.sticky && !unit.semantic {
                vec![Word { text: unit.text.clone(), start: unit.start, end: unit.end }]
            } else {
                line.words[unit.words.clone()].to_vec()
            }
        })
        .collect()
}

fn single_units(words: &[Word]) -> Vec<LayoutUnit> {
    words.iter().enumerate().map(|(i, word)| unit(words, i..i + 1, word.text.clone(), false)).collect()
}

fn unit(words: &[Word], range: std::ops::Range<usize>, text: String, semantic: bool) -> LayoutUnit {
    LayoutUnit {
        start: words[range.start].start,
        end: words[range.end - 1].end,
        words: range,
        text,
        semantic,
        sticky: false,
    }
}

/// Aligns segmenter words onto the timed words; `None` when any segment does not line up exactly.
fn semantic_units(line: &Line) -> Option<Vec<LayoutUnit>> {
    if line.words.is_empty() || !line.text.chars().any(is_cjk) {
        return None;
    }
    let text: String = line.words.iter().map(|word| word.text.as_str()).collect();
    let mut units: Vec<LayoutUnit> = Vec::new();
    let mut next_word = 0;
    let mut previous = 0;
    for (boundary, kind) in segmenter().segment_str(&text).iter_with_word_type() {
        let segment = &text[previous..boundary];
        previous = boundary;
        if segment.is_empty() || segment.trim().is_empty() {
            next_word += consume_whitespace(&line.words[next_word..], segment);
            continue;
        }

        let first = next_word;
        let mut collected = String::new();
        while next_word < line.words.len() && collected.len() < segment.len() {
            collected.push_str(&line.words[next_word].text);
            next_word += 1;
            if !segment.starts_with(collected.as_str()) {
                return None;
            }
        }
        if collected != segment {
            return None;
        }

        let range = first..next_word;
        match units.last_mut() {
            Some(previous_unit) if !kind.is_word_like() => {
                previous_unit.text.push_str(segment);
                previous_unit.words.end = range.end;
                previous_unit.end = line.words[range.end - 1].end;
            }
            _ => {
                let semantic = kind.is_word_like() && segment.chars().any(is_cjk) && range.len() > 1;
                units.push(unit(&line.words, range, segment.to_owned(), semantic));
            }
        }
    }
    (next_word == line.words.len() && !units.is_empty()).then_some(units)
}

/// Whitespace segments only consume timed words that are whitespace themselves.
fn consume_whitespace(words: &[Word], segment: &str) -> usize {
    let mut consumed = 0;
    let mut length = 0;
    while let Some(word) = words.get(consumed) {
        if !word.text.trim().is_empty() || length + word.text.len() > segment.len() {
            break;
        }
        length += word.text.len();
        consumed += 1;
    }
    consumed
}

/// Attaches trailing punctuation and contraction pieces (`It` `’` `s`) to the unit before them.
fn attach_punctuation(words: &[Word], units: Vec<LayoutUnit>) -> Vec<LayoutUnit> {
    let mut merged: Vec<LayoutUnit> = Vec::with_capacity(units.len());
    let mut iter = units.into_iter().peekable();
    while let Some(current) = iter.next() {
        let Some(previous) = merged.last_mut() else {
            merged.push(current);
            continue;
        };
        let trimmed = current.text.trim();
        let attach_pair = is_apostrophe(trimmed)
            && ends_with_word_char(&previous.text)
            && iter.peek().is_some_and(|next| is_contraction_suffix(next.text.trim()));
        if attach_pair {
            append(previous, current);
            if let Some(next) = iter.next() {
                append(previous, next);
            }
        } else if (is_direct_contraction(trimmed) && ends_with_word_char(&previous.text))
            || (is_contraction_suffix(trimmed) && previous.text.trim_end().ends_with(['\'', '’']))
            || (is_trailing_punctuation(trimmed) && ends_with_word_char(&previous.text))
        {
            append(previous, current);
        } else {
            merged.push(current);
        }
    }
    for unit in &mut merged {
        let last_is_punctuation =
            unit.words.len() > 1 && is_trailing_punctuation(words[unit.words.end - 1].text.trim());
        if last_is_punctuation || (unit.words.len() > 1 && !unit.semantic && has_inline_contraction(&unit.text)) {
            unit.sticky = true;
        }
    }
    merged
}

fn append(target: &mut LayoutUnit, unit: LayoutUnit) {
    target.text.push_str(&unit.text);
    target.words.end = unit.words.end;
    target.end = unit.end;
    target.sticky = true;
}

pub(crate) fn segmenter() -> &'static icu_segmenter::WordSegmenterBorrowed<'static> {
    static SEGMENTER: OnceLock<icu_segmenter::WordSegmenterBorrowed<'static>> = OnceLock::new();
    SEGMENTER.get_or_init(|| WordSegmenter::new_dictionary(WordBreakInvariantOptions::default()))
}

const CONTRACTIONS: [&str; 8] = ["s", "t", "m", "d", "ll", "re", "ve", "em"];

fn is_contraction_suffix(text: &str) -> bool {
    CONTRACTIONS.iter().any(|suffix| suffix.eq_ignore_ascii_case(text))
}

fn is_direct_contraction(text: &str) -> bool {
    text.strip_prefix(['\'', '’']).is_some_and(is_contraction_suffix)
}

fn is_apostrophe(text: &str) -> bool {
    matches!(text, "'" | "’")
}

fn ends_with_word_char(text: &str) -> bool {
    text.trim_end().chars().last().is_some_and(char::is_alphanumeric)
}

fn is_trailing_punctuation(text: &str) -> bool {
    const MARKS: &str = ",.;:!?，。！？、：；）】》」』〉〕］)}]\"'’”";
    !text.is_empty() && text.chars().all(|c| MARKS.contains(c))
}

/// `word'suffix` inside one unit, such as a parser word `it's`.
fn has_inline_contraction(text: &str) -> bool {
    text.match_indices(['\'', '’']).any(|(i, mark)| {
        let before = text[..i].chars().last().is_some_and(char::is_alphanumeric);
        let after: String = text[i + mark.len()..].chars().take_while(|c| c.is_alphabetic()).collect();
        before && is_contraction_suffix(&after)
    })
}

pub(crate) fn is_cjk(c: char) -> bool {
    matches!(c, '\u{4e00}'..='\u{9fa5}' | '\u{3040}'..='\u{30ff}' | '\u{ac00}'..='\u{d7af}')
}

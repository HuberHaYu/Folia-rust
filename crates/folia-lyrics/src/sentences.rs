// Sentence breaks for the tilt style, matching `SentenceLayout.splitIntoSentences` for a positive target count.

use crate::layout::{is_cjk, segmenter};

/// Splits `text` into `target_count` pieces. Counts below 2 return the text unchanged.
pub fn split_sentences(text: &str, target_count: usize, time_seed: f64) -> Vec<String> {
    if target_count <= 1 {
        return vec![text.to_owned()];
    }
    let mut sentences = vec![text.to_owned()];
    for level in 1..=5 {
        if sentences.len() >= target_count {
            break;
        }
        let next = sentences.iter().flat_map(|sentence| split_by_level(sentence, level)).collect();
        sentences = next;
    }
    if sentences.len() < target_count {
        secondary_split(&mut sentences, target_count, time_seed);
    }
    if sentences.len() > target_count {
        merge_sentences(&mut sentences, target_count);
    }
    sentences
}

fn split_by_level(text: &str, level: usize) -> Vec<String> {
    match level {
        1 => split_by_punctuation(text),
        2 => split_by_brackets(text),
        3 => split_by_western_words(text),
        4 => split_cjk_by_space(text),
        _ => split_by_special(text),
    }
}

fn is_sentence_punct(c: char) -> bool {
    matches!(c, '，' | '。' | '；' | '！' | '？' | '、' | '…' | '·' | '.' | ',' | ';' | '!' | '?')
}

/// JS `\s`: the whitespace a lyric line can actually contain.
pub fn is_js_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n'
            | '\u{000b}'
            | '\u{000c}'
            | '\r'
            | ' '
            | '\u{00a0}'
            | '\u{1680}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202f}'
            | '\u{205f}'
            | '\u{3000}'
            | '\u{feff}'
    ) || ('\u{2000}'..='\u{200a}').contains(&c)
}

fn split_by_punctuation(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    if !chars.iter().copied().any(is_sentence_punct) {
        return vec![text.to_owned()];
    }
    let mut result = Vec::new();
    let mut last = 0;
    let mut i = 0;
    while i < chars.len() {
        if !is_sentence_punct(chars[i]) {
            i += 1;
            continue;
        }
        let punct_start = i;
        while i < chars.len() && is_sentence_punct(chars[i]) {
            i += 1;
        }
        while i < chars.len() && is_js_space(chars[i]) {
            i += 1;
        }
        push_punct_piece(&mut result, &chars, last, punct_start, i);
        last = i;
    }
    if last < chars.len() {
        result.push(chars[last..].iter().collect());
    }
    result.retain(|piece| !piece.is_empty());
    result
}

fn push_punct_piece(result: &mut Vec<String>, chars: &[char], last: usize, punct_start: usize, end: usize) {
    let punct_with_spaces: String = chars[punct_start..end].iter().collect();
    let before: String = chars[last..punct_start].iter().collect();
    if !before.is_empty() {
        result.push(before + &punct_with_spaces);
    } else if let Some(previous) = result.last_mut() {
        previous.push_str(&punct_with_spaces);
    } else {
        result.push(punct_with_spaces);
    }
}

fn split_by_western_words(text: &str) -> Vec<String> {
    if !text.chars().any(is_cjk) {
        return vec![text.to_owned()];
    }
    let chars: Vec<char> = text.chars().collect();
    let mut ranges = Vec::new();
    let mut from = 0;
    while let Some((start, end)) = find_western_block(&chars, from) {
        ranges.push((start, end));
        from = end;
    }
    let multi = ranges.iter().any(|&(start, end)| alnum_runs(&chars[start..end]) > 1);
    if !multi {
        return vec![text.to_owned()];
    }
    let mut parts = Vec::new();
    let mut last = 0;
    for (start, end) in ranges {
        let before: String = chars[last..start].iter().collect();
        let block: String = chars[start..end].iter().collect();
        if should_skip_boundary(text, &before) {
            parts.push(before + &block);
        } else if start > last {
            parts.push(before);
            parts.push(block);
        } else {
            parts.push(block);
        }
        last = end;
    }
    if last < chars.len() {
        parts.push(chars[last..].iter().collect());
    }
    if parts.len() > 1 { parts } else { vec![text.to_owned()] }
}

fn alnum_runs(chars: &[char]) -> usize {
    let mut runs = 0;
    let mut inside = false;
    for c in chars {
        if c.is_ascii_alphanumeric() {
            if !inside {
                runs += 1;
            }
            inside = true;
        } else {
            inside = false;
        }
    }
    runs
}

fn find_western_block(chars: &[char], from: usize) -> Option<(usize, usize)> {
    let mut i = from;
    while i < chars.len() {
        if let Some(end) = match_western_block(chars, i) {
            return Some((i, end));
        }
        i += 1;
    }
    None
}

fn match_western_block(chars: &[char], start: usize) -> Option<usize> {
    let mut at = match_western_word(chars, start)?;
    let mut words = 1;
    loop {
        let spaced = skip_js_space(chars, at);
        if spaced == at {
            break;
        }
        let Some(next) = match_western_word(chars, spaced) else { break };
        at = next;
        words += 1;
    }
    if words < 2 {
        return None;
    }
    if at < chars.len() && is_western_trail_punct(chars[at]) {
        at += 1;
    }
    Some(skip_js_space(chars, at))
}

fn match_western_word(chars: &[char], start: usize) -> Option<usize> {
    if start >= chars.len() || !chars[start].is_ascii_alphanumeric() {
        return None;
    }
    let mut end = start + 1;
    while end < chars.len() && chars[end].is_ascii_alphanumeric() {
        end += 1;
    }
    let ext = end;
    while end < chars.len() && (chars[end].is_ascii_alphanumeric() || chars[end] == '\'' || chars[end] == '-') {
        end += 1;
    }
    while end > ext && !chars[end - 1].is_ascii_alphanumeric() {
        end -= 1;
    }
    Some(end)
}

fn skip_js_space(chars: &[char], mut at: usize) -> usize {
    while at < chars.len() && is_js_space(chars[at]) {
        at += 1;
    }
    at
}

fn is_western_trail_punct(c: char) -> bool {
    matches!(c, '.' | ',' | ';' | ':' | '!' | '?' | '。' | '，' | '；' | '：' | '！' | '？')
}

fn should_skip_boundary(full: &str, before: &str) -> bool {
    let Some(sep) = before.chars().next_back() else { return false };
    if sep == '-' {
        return true;
    }
    matches!(sep, ':' | '：' | '/' | '／' | '|' | '｜') && full.chars().filter(|c| *c == sep).count() >= 2
}

const PAIRS: [(char, char); 11] = [
    ('「', '」'),
    ('『', '』'),
    ('《', '》'),
    ('【', '】'),
    ('｛', '｝'),
    ('［', '］'),
    ('[', ']'),
    ('（', '）'),
    ('(', ')'),
    ('"', '"'),
    ('\'', '\''),
];

fn split_by_brackets(text: &str) -> Vec<String> {
    let result = extract_pairs(text);
    if result.len() > 1 { result } else { vec![text.to_owned()] }
}

fn extract_pairs(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let Some((open_at, close_end)) = outermost_pair(&chars) else {
        return vec![text.to_owned()];
    };
    let mut result = Vec::new();
    if open_at > 0 {
        result.extend(extract_pairs(&chars[..open_at].iter().collect::<String>()));
    }
    if close_end > open_at {
        result.push(chars[open_at..close_end].iter().collect());
    }
    if close_end < chars.len() {
        result.extend(extract_pairs(&chars[close_end..].iter().collect::<String>()));
    }
    result.retain(|piece| !piece.is_empty());
    result
}

/// Earliest opener wins. Quote closers are the next same mark; other closers are the first in the text,
/// because the original regular expressions are not global and ignore the search offset.
fn outermost_pair(chars: &[char]) -> Option<(usize, usize)> {
    let mut best: Option<(usize, usize)> = None;
    for (open, close) in PAIRS {
        let Some(open_at) = chars.iter().position(|c| *c == open) else { continue };
        let from = if open == close { open_at + 1 } else { 0 };
        let Some(close_at) = chars[from..].iter().position(|c| *c == close) else { continue };
        let close_end = from + close_at + 1;
        let replace = best
            .is_none_or(|(best_open, best_end)| open_at < best_open || (open_at == best_open && close_end > best_end));
        if replace {
            best = Some((open_at, close_end));
        }
    }
    best
}

fn split_cjk_by_space(text: &str) -> Vec<String> {
    if !text.chars().any(is_cjk) {
        return vec![text.to_owned()];
    }
    let mut segments: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut in_cjk = false;
    for ch in text.chars() {
        if is_cjk(ch) {
            current.push(ch);
            in_cjk = true;
            continue;
        }
        if ch.is_ascii_alphanumeric() {
            current.push(ch);
            continue;
        }
        if is_js_space(ch) {
            push_cjk_space(&mut segments, &mut current, &mut in_cjk, ch);
        } else if current.is_empty() {
            push_or_start(&mut segments, &mut current, ch);
        } else {
            current.push(ch);
        }
    }
    finish_cjk_segment(&mut segments, current);
    if segments.len() > 1 { segments } else { vec![text.to_owned()] }
}

fn push_cjk_space(segments: &mut Vec<String>, current: &mut String, in_cjk: &mut bool, ch: char) {
    let breaks = ch == '\u{3000}' || (*in_cjk && current.chars().next_back().is_some_and(is_cjk));
    if breaks && !current.is_empty() {
        current.push(ch);
        segments.push(std::mem::take(current));
        *in_cjk = false;
    } else if current.is_empty() {
        push_or_start(segments, current, ch);
    } else {
        current.push(ch);
    }
}

fn push_or_start(segments: &mut [String], current: &mut String, ch: char) {
    if current.is_empty()
        && let Some(last) = segments.last_mut()
    {
        last.push(ch);
    } else {
        current.push(ch);
    }
}

fn finish_cjk_segment(segments: &mut Vec<String>, current: String) {
    if current.is_empty() {
        return;
    }
    if segments.last().is_some() && current.chars().all(is_js_space) {
        if let Some(last) = segments.last_mut() {
            last.push_str(&current);
        }
    } else {
        segments.push(current);
    }
}

fn is_special(c: char) -> bool {
    matches!(c, '：' | ':' | '/' | '／' | '\\' | '|' | '｜' | '~' | '～')
}

fn split_by_special(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut spans = Vec::new();
    let mut start = 0;
    for (i, ch) in chars.iter().enumerate() {
        if is_special(*ch) {
            spans.push((start, i));
            start = i + 1;
        }
    }
    spans.push((start, chars.len()));
    if spans.iter().filter(|(a, b)| a != b).count() <= 1 {
        return vec![text.to_owned()];
    }
    let mut result: Vec<String> = Vec::new();
    let mut last = 0;
    for (start, end) in spans {
        if start > last {
            let special: String = chars[last..start].iter().collect();
            if let Some(previous) = result.last_mut() {
                previous.push_str(&special);
            } else {
                result.push(special);
            }
        }
        if start == end {
            last = start;
        } else {
            result.push(chars[start..end].iter().collect());
            last = end;
        }
    }
    if last < chars.len() {
        let tail: String = chars[last..].iter().collect();
        if let Some(previous) = result.last_mut() {
            previous.push_str(&tail);
        } else if !tail.is_empty() {
            result.push(tail);
        }
    }
    result.retain(|piece| !piece.is_empty());
    result
}

fn secondary_split(sentences: &mut Vec<String>, target_count: usize, time_seed: f64) {
    while sentences.len() < target_count {
        let candidates: Vec<usize> =
            sentences.iter().enumerate().filter(|(_, text)| text.chars().count() > 2).map(|(i, _)| i).collect();
        if candidates.is_empty() {
            break;
        }
        if let Some((index, at)) = best_word_split(sentences, &candidates) {
            let (first, second) = split_at_char(&sentences[index], at);
            replace_one(sentences, index, first, second);
            continue;
        }
        let hash = sentences.iter().flat_map(|text| text.chars()).map(|c| f64::from(c as u32)).sum::<f64>();
        let seed = hash + sentences.len() as f64 + time_seed;
        let pick = (frac_sin(seed) * candidates.len() as f64).floor() as usize;
        let index = candidates[pick.min(candidates.len() - 1)];
        let mid = sentences[index].chars().count() / 2;
        let (first, second) = split_at_char(&sentences[index], mid);
        replace_one(sentences, index, first, second);
    }
}

fn best_word_split(sentences: &[String], candidates: &[usize]) -> Option<(usize, usize)> {
    let mut best: Option<(usize, usize, f64)> = None;
    for &index in candidates {
        let Some((at, score)) = word_split(&sentences[index]) else { continue };
        if best.as_ref().is_none_or(|(_, _, held)| score > *held) {
            best = Some((index, at, score));
        }
    }
    best.map(|(index, at, _)| (index, at))
}

fn word_split(text: &str) -> Option<(usize, f64)> {
    let starts = word_starts(text);
    if starts.len() < 2 {
        return None;
    }
    let mid = text.chars().count() as f64 / 2.0;
    let mut gap = 1;
    let mut nearest = f64::INFINITY;
    for (g, start) in starts.iter().enumerate().skip(1) {
        let distance = (*start as f64 - mid).abs();
        if distance < nearest {
            nearest = distance;
            gap = g;
        }
    }
    let at = starts[gap];
    let balance = 1.0 - (at as f64 - mid).abs() / mid;
    Some((at, balance * 10.0 + starts.len() as f64))
}

fn word_starts(text: &str) -> Vec<usize> {
    let mut starts = Vec::new();
    let mut previous = 0;
    let mut char_at = 0;
    for (boundary, kind) in segmenter().segment_str(text).iter_with_word_type() {
        if boundary < previous {
            continue;
        }
        let segment = &text[previous..boundary];
        let chars = segment.chars().count();
        if kind.is_word_like() && !segment.is_empty() {
            starts.push(char_at);
        }
        char_at += chars;
        previous = boundary;
    }
    starts
}

fn split_at_char(text: &str, char_index: usize) -> (String, String) {
    for (seen, (byte, _)) in text.char_indices().enumerate() {
        if seen == char_index {
            return (text[..byte].to_owned(), text[byte..].to_owned());
        }
    }
    (text.to_owned(), String::new())
}

fn replace_one(sentences: &mut Vec<String>, index: usize, first: String, second: String) {
    sentences.remove(index);
    sentences.insert(index, second);
    sentences.insert(index, first);
}

fn merge_sentences(sentences: &mut Vec<String>, target_count: usize) {
    while sentences.len() > target_count {
        let mut best = 0;
        let mut shortest = usize::MAX;
        for i in 0..sentences.len() - 1 {
            let combined = sentences[i].chars().count() + sentences[i + 1].chars().count();
            if combined < shortest {
                shortest = combined;
                best = i;
            }
        }
        let merged = format!("{}{}", sentences[best], sentences[best + 1]);
        sentences.remove(best + 1);
        sentences[best] = merged;
    }
}

fn frac_sin(seed: f64) -> f64 {
    let x = seed.sin() * 10000.0;
    x - x.floor()
}

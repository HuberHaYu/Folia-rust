// TTML lyrics (Apple Music / AMLL style): timed syllable spans, inline translation and romanization, head translations.

use std::collections::HashMap;

use folia_core::{Line, Lyrics, Word};
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use crate::{align, words};

#[derive(Default)]
struct Syllable {
    text: String,
    start: f64,
    end: f64,
    space_after: bool,
}

#[derive(Default)]
struct DraftLine {
    key: Option<String>,
    start: Option<f64>,
    end: Option<f64>,
    syllables: Vec<Syllable>,
    plain: String,
    translation: String,
    romanization: String,
}

/// Where text inside the document currently goes.
#[derive(Clone, PartialEq)]
enum Target {
    Ignore,
    Line,
    Syllable,
    Translation,
    Romanization,
    HeadText(HeadKind, String),
}

#[derive(Clone, Copy, PartialEq)]
enum HeadKind {
    Translation,
    Romanization,
}

#[derive(Default)]
struct Document {
    line_timed: bool,
    lines: Vec<DraftLine>,
    head_translations: HashMap<String, String>,
    head_romanizations: HashMap<String, String>,
}

pub fn parse(content: &str) -> Lyrics {
    match read(content.trim_start_matches('\u{feff}')) {
        Ok(document) => build(document),
        Err(err) => {
            tracing::warn!(error = &err as &dyn std::error::Error, "cannot parse TTML lyrics");
            Lyrics::default()
        }
    }
}

fn read(content: &str) -> Result<Document, quick_xml::Error> {
    let mut reader = Reader::from_str(content);
    let mut document = Document::default();
    let mut stack: Vec<Target> = Vec::new();
    let mut line: Option<DraftLine> = None;
    let mut head_kind = None;

    loop {
        match reader.read_event()? {
            Event::Start(element) => {
                let target = open(&element, &mut document, &mut line, &mut head_kind, stack.last());
                stack.push(target);
            }
            Event::Empty(element) => {
                open(&element, &mut document, &mut line, &mut head_kind, stack.last());
            }
            Event::End(element) => {
                stack.pop();
                if element.local_name().as_ref() == "p"
                    && let Some(finished) = line.take()
                {
                    document.lines.push(finished);
                }
            }
            Event::Text(text) => push_text(&text.xml10_content(), stack.last(), &mut line, &mut document),
            Event::CData(data) => push_text(&data.xml10_content(), stack.last(), &mut line, &mut document),
            Event::GeneralRef(reference) => {
                let resolved = match reference.resolve_char_ref()? {
                    Some(c) => c.to_string(),
                    None => entity(&reference.xml10_content()).to_owned(),
                };
                push_text(&resolved, stack.last(), &mut line, &mut document);
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(document)
}

/// Handles an opening tag and returns where the text inside it goes.
fn open(
    element: &BytesStart<'_>,
    document: &mut Document,
    line: &mut Option<DraftLine>,
    head_kind: &mut Option<HeadKind>,
    parent: Option<&Target>,
) -> Target {
    let attr = |name: &str| attribute(element, name);
    match element.local_name().as_ref() {
        "tt" => {
            document.line_timed = attr("timing").is_some_and(|timing| timing.eq_ignore_ascii_case("line"));
            Target::Ignore
        }
        "translations" | "translation" => {
            *head_kind = Some(HeadKind::Translation);
            Target::Ignore
        }
        "transliterations" | "transliteration" => {
            *head_kind = Some(HeadKind::Romanization);
            Target::Ignore
        }
        "text" => match (*head_kind, attr("for")) {
            (Some(kind), Some(key)) => Target::HeadText(kind, key),
            _ => Target::Ignore,
        },
        "p" => {
            *line = Some(DraftLine {
                key: attr("key"),
                start: attr("begin").and_then(|t| time(&t)),
                end: attr("end").and_then(|t| time(&t)),
                ..DraftLine::default()
            });
            Target::Line
        }
        "span" => match (attr("role").as_deref(), parent) {
            (Some("x-translation"), _) => Target::Translation,
            (Some("x-roman"), _) => Target::Romanization,
            (Some(_), _) => Target::Ignore,
            (None, Some(Target::HeadText(kind, key))) => Target::HeadText(*kind, key.clone()),
            (None, Some(Target::Line)) => {
                let (Some(start), Some(end)) =
                    (attr("begin").and_then(|t| time(&t)), attr("end").and_then(|t| time(&t)))
                else {
                    return Target::Line;
                };
                if let Some(line) = line {
                    line.syllables.push(Syllable { start, end, ..Syllable::default() });
                }
                Target::Syllable
            }
            (None, Some(other)) => other.clone(),
            (None, None) => Target::Ignore,
        },
        _ => Target::Ignore,
    }
}

fn push_text(text: &str, target: Option<&Target>, line: &mut Option<DraftLine>, document: &mut Document) {
    match target {
        Some(Target::HeadText(kind, key)) => {
            let map = match kind {
                HeadKind::Translation => &mut document.head_translations,
                HeadKind::Romanization => &mut document.head_romanizations,
            };
            map.entry(key.clone()).or_default().push_str(text);
        }
        Some(target) => {
            let Some(line) = line else { return };
            match target {
                Target::Syllable => {
                    if let Some(syllable) = line.syllables.last_mut() {
                        syllable.text.push_str(text);
                    }
                }
                Target::Line if text.trim().is_empty() => {
                    if let Some(syllable) = line.syllables.last_mut() {
                        syllable.space_after = true;
                    }
                }
                Target::Line => line.plain.push_str(text),
                Target::Translation => line.translation.push_str(text),
                Target::Romanization => line.romanization.push_str(text),
                _ => {}
            }
        }
        None => {}
    }
}

fn build(document: Document) -> Lyrics {
    let word_timed = !document.line_timed && document.lines.iter().any(|line| !line.syllables.is_empty());
    let lines = document
        .lines
        .into_iter()
        .filter_map(|draft| {
            let start = draft.start.or_else(|| draft.syllables.first().map(|s| s.start))?;
            let end = draft.end.or_else(|| draft.syllables.last().map(|s| s.end)).unwrap_or(start).max(start);
            let text = if draft.syllables.is_empty() {
                draft.plain.trim().to_owned()
            } else {
                syllable_text(&draft.syllables)
            };
            if text.is_empty() {
                return None;
            }
            let words =
                if draft.syllables.is_empty() { words::spread(&text, start, end) } else { group(&draft.syllables) };
            let from_head = |map: &HashMap<String, String>| draft.key.as_ref().and_then(|key| map.get(key)).cloned();
            let non_empty = |text: String| Some(text.trim().to_owned()).filter(|text| !text.is_empty());
            Some(Line {
                words,
                start,
                end,
                text,
                translation: non_empty(draft.translation).or_else(|| from_head(&document.head_translations)),
                romanization: non_empty(draft.romanization).or_else(|| from_head(&document.head_romanizations)),
            })
        })
        .collect();
    Lyrics { lines: align::finish(lines, &[], &[]), word_timed, ..Lyrics::default() }
}

fn syllable_text(syllables: &[Syllable]) -> String {
    let mut text = String::new();
    for syllable in syllables {
        text.push_str(&syllable.text);
        if syllable.space_after {
            text.push(' ');
        }
    }
    text.trim().to_owned()
}

/// Joins syllables of one Latin word into a single word, as the original does, so layouts do not break inside words.
fn group(syllables: &[Syllable]) -> Vec<Word> {
    let mut words: Vec<Word> = Vec::new();
    let mut joining = false;
    for (i, syllable) in syllables.iter().enumerate() {
        match words.last_mut() {
            Some(word) if joining => {
                word.text.push_str(&syllable.text);
                word.end = syllable.end;
            }
            _ => words.push(Word { text: syllable.text.clone(), start: syllable.start, end: syllable.end }),
        }
        joining = syllables.get(i + 1).is_some_and(|next| {
            !syllable.space_after && western_joinable(&syllable.text) && western_joinable(&next.text)
        });
    }
    words.retain(|word| !word.text.is_empty());
    words
}

fn western_joinable(text: &str) -> bool {
    let text = text.trim();
    let latin = |c: char| c.is_alphabetic() && (u32::from(c) < 0x250 || ('\u{1e00}'..='\u{1eff}').contains(&c));
    !text.is_empty()
        && !text
            .chars()
            .any(|c| matches!(c, '\u{4e00}'..='\u{9fa5}' | '\u{3040}'..='\u{30ff}' | '\u{ac00}'..='\u{d7af}'))
        && text.chars().any(|c| latin(c) || c.is_numeric() || matches!(c, '\'' | '’' | '`' | '-'))
}

/// `hh:mm:ss.fff`, `mm:ss.fff`, `ss.fff`, or a number with an `s` / `ms` suffix, in seconds.
fn time(value: &str) -> Option<f64> {
    let value = value.trim();
    if let Some(ms) = value.strip_suffix("ms") {
        return ms.parse::<f64>().ok().map(|ms| ms / 1000.0);
    }
    if let Some(seconds) = value.strip_suffix('s') {
        return seconds.parse().ok();
    }
    value.split(':').try_fold(0.0, |total, part| Some(total * 60.0 + part.parse::<f64>().ok()?))
}

fn attribute(element: &BytesStart<'_>, local: &str) -> Option<String> {
    element
        .attributes()
        .flatten()
        .find(|attr| attr.key.local_name().as_ref() == local)
        .and_then(|attr| attr.normalized_value(quick_xml::XmlVersion::Implicit1_0).ok().map(|value| value.into_owned()))
}

fn entity(name: &str) -> &'static str {
    match name {
        "amp" => "&",
        "lt" => "<",
        "gt" => ">",
        "quot" => "\"",
        "apos" => "'",
        _ => "",
    }
}

// The sample song the style pictures play, from the original `PreviewPlaceholder.ts` (`default`).

use folia_core::{Line, Lyrics, Word};

/// `loopDuration` of the sample.
pub const PREVIEW_LOOP: f64 = 14.4;
/// `posterTime`: 60% into the second line, where a picture that is not running stands.
pub const POSTER_TIME: f64 = 4.2 + (7.2 - 4.2) * 0.6;
/// A picture is drawn on a stage this large and scaled down, so it lays out as on the playing page.
pub const STAGE_WIDTH: f32 = 1200.0;
pub const STAGE_HEIGHT: f32 = 800.0;

/// The four lines of the sample, with words timed evenly like `createCharacterWords` / `createTokenWords`.
pub fn preview_lyrics() -> Lyrics {
    let translation = "编织那没有诗意，却能将你带到现实的神文之诗。";
    let lines = vec![
        line(0.7, 3.6, "詩情を持たずとも、あなたを現実へと導くその神文の詩を紡ぐ。", None, translation),
        line(
            4.2,
            7.2,
            "Weave that prosaic divine poem that leads you to reality.",
            Some(&["Weave", "that", "prosaic", "divine", "poem", "that", "leads", "you", "to", "reality."]),
            translation,
        ),
        line(
            7.8,
            10.9,
            "Tisse ce poème divin sans poésie qui te mène au réel.",
            Some(&["Tisse", "ce", "poème", "divin", "sans", "poésie", "qui", "te", "mène", "au", "réel."]),
            translation,
        ),
        line(11.5, 14.4, translation, None, translation),
    ];
    Lyrics { lines, title: Some("神文之诗".to_owned()), word_timed: true, ..Lyrics::default() }
}

fn line(start: f64, end: f64, text: &str, tokens: Option<&[&str]>, translation: &str) -> Line {
    let pieces: Vec<String> = match tokens {
        // Words carry their own spacing in this crate's lyrics, so each token but the last keeps a space.
        Some(tokens) => {
            let last = tokens.len().saturating_sub(1);
            tokens
                .iter()
                .enumerate()
                .map(|(at, token)| if at < last { format!("{token} ") } else { (*token).to_owned() })
                .collect()
        }
        None => text.chars().map(String::from).collect(),
    };
    let count = pieces.len().max(1) as f64;
    let span = end - start;
    let words = pieces
        .into_iter()
        .enumerate()
        .map(|(index, text)| Word {
            text,
            start: start + span * index as f64 / count,
            end: start + span * (index + 1) as f64 / count,
        })
        .collect();
    Line { words, start, end, text: text.to_owned(), translation: Some(translation.to_owned()), romanization: None }
}

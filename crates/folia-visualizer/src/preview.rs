// The sample song the style pictures play, from the original `PreviewPlaceholder.ts` (`default`).

use std::cell::RefCell;
use std::rc::Rc;

use folia_core::{Line, Lyrics, Theme, Word};
use iced::widget::canvas::{self, Cache, Geometry};
use iced::{Rectangle, Renderer, mouse};

use crate::{Scene, SceneState, Style};

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

/// What a kept picture shows, as `pictureImages.ts` names it: the style, the theme, the settings and the moment.
#[derive(PartialEq)]
struct PictureKey {
    style: Style,
    theme: Theme,
    font_scale: u32,
    time: u64,
}

/// The images of the small pictures, kept while the app runs so a picker opened again shows them at once.
#[derive(Default)]
pub struct Pictures {
    images: RefCell<Vec<(PictureKey, Rc<Cache>)>>,
}

impl Pictures {
    /// `KEPT`: more than every style in two themes; the oldest go first.
    const KEPT: usize = 64;

    /// `scene` drawn once into its kept image, then shown from it.
    pub fn frozen<'a>(&self, scene: Scene<'a>) -> Frozen<'a> {
        let mut images = self.images.borrow_mut();
        let found = images.iter().position(|(key, _)| {
            key.style == scene.style
                && key.theme == *scene.theme
                && key.font_scale == scene.font_scale.to_bits()
                && key.time == scene.time.to_bits()
        });
        let entry = match found {
            Some(index) => images.remove(index),
            None => {
                let key = PictureKey {
                    style: scene.style,
                    theme: scene.theme.clone(),
                    font_scale: scene.font_scale.to_bits(),
                    time: scene.time.to_bits(),
                };
                (key, Rc::new(Cache::new()))
            }
        };
        let image = entry.1.clone();
        images.push(entry);
        if images.len() > Self::KEPT {
            images.remove(0);
        }
        Frozen { scene, image }
    }
}

/// A picture shown from its kept image; the image is redrawn only when the widget changes size.
pub struct Frozen<'a> {
    scene: Scene<'a>,
    image: Rc<Cache>,
}

impl<Message> canvas::Program<Message> for Frozen<'_> {
    type State = SceneState;

    fn draw(
        &self,
        state: &SceneState,
        renderer: &Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let target = bounds.size();
        vec![self.image.draw(renderer, target, |frame| self.scene.paint(state, frame, target))]
    }
}

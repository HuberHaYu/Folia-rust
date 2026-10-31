// Classic (流光): one line at a time; each word springs in, glows while sung and drifts once passed.

mod layout;

pub use layout::{ClassicTuning, Justify, font_px, justify, random, word_configs, wrap};

use std::rc::Rc;

use folia_core::{Line, Word};
use folia_lyrics::RenderHints;
use iced::widget::canvas::Frame;
use iced::{Point, Size};

use crate::styles::glow::{self, WordConfig};
use crate::{LayoutCache, Scene, measure};

const LINE_HEIGHT: f32 = 1.22;
const MAX_LINE_WIDTH: f32 = 1152.0;
const PADDING: f32 = 32.0;

pub struct LineLayout {
    /// Display words: CJK words grouped and punctuation attached, as the original's semantic layout.
    words: Vec<Word>,
    configs: Vec<WordConfig>,
    centers: Vec<Point>,
    font_px: f32,
}

pub fn draw(scene: &Scene<'_>, cache: &mut LayoutCache<LineLayout>, frame: &mut Frame, size: Size) {
    let tuning = ClassicTuning::default();
    let theme = scene.theme;
    let center = Point::new(size.width / 2.0, size.height / 2.0);
    let (breath_y, breath_scale) = glow::breathing(scene.time, theme.intensity, tuning.breathing);

    glow::visible_lines(&scene.lyrics.lines, scene.time, |line, opacity, scale, active| {
        let layout = cache.get_or_insert(line, size, scene.font_scale, || build(line, scene, size, tuning));
        let hints = RenderHints::of(line);
        frame.with_save(|frame| {
            glow::scale_around(frame, center, scale * breath_scale, breath_y);
            for ((word, config), at) in layout.words.iter().zip(&layout.configs).zip(&layout.centers) {
                let pose = glow::word_pose(
                    config,
                    word.start,
                    word.end,
                    hints,
                    theme.intensity,
                    tuning.word_rotation,
                    scene.time,
                );
                glow::draw_word(frame, theme, &word.text, *at, &pose, opacity, layout.font_px);
            }
        });
        if active {
            glow::draw_translation(frame, theme, line, size, scene.font_scale, opacity);
        }
    });
}

fn build(line: &Line, scene: &Scene<'_>, size: Size, tuning: ClassicTuning) -> Rc<LineLayout> {
    let font_px = font_px(size.width, scene.font_scale);
    let words = folia_lyrics::display_words(line, &folia_lyrics::layout_units(line, true));
    let widths: Vec<f32> =
        words.iter().map(|word| measure(&word.text, font_px, crate::BOLD, f32::INFINITY).width).collect();
    let seed = line.start;
    let interlude = line.is_interlude();
    let configs = word_configs(seed, scene.theme.intensity, interlude, &widths, font_px, tuning);
    let margins: Vec<f32> = configs.iter().map(|config| config.margin_right as f32).collect();
    let max_width = (size.width - PADDING * 2.0).clamp(1.0, MAX_LINE_WIDTH);
    let left = (size.width - max_width) / 2.0;
    let justify = justify(seed, scene.theme.intensity, interlude);
    let placed = wrap(&widths, &margins, font_px * LINE_HEIGHT, max_width, left, size.height / 2.0, justify);
    let centers = placed.iter().map(|p| Point::new(p.x, p.y)).collect();
    Rc::new(LineLayout { words, configs, centers, font_px })
}

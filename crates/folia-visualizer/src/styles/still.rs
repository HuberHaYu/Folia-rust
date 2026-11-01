// Still (静止): the previous, current and next lines as static text under a vignette; redrawn only when the line changes.

use std::cell::Cell;

use folia_core::{Line, Lyrics, Theme};
use iced::font::Weight;
use iced::widget::canvas::{Cache, Frame, Geometry, Path, Stroke, Text};
use iced::{Color, Font, Point, Renderer, Size, Vector};

use crate::{Scene, color, measure};

const MAX_WIDTH: f32 = 896.0;
const SIDE_PADDING: f32 = 48.0;
const BOTTOM_PADDING: f32 = 64.0;
const GAP: f32 = 32.0;
const EMPTY_HEIGHT: f32 = 80.0;
const VIGNETTE_RINGS: usize = 48;

/// What a frame showed: the line index, canvas size and font scale (as bits).
type FrameKey = (Option<usize>, u32, u32, u32);

/// Geometry of the last drawn frame and what it showed.
#[derive(Default)]
pub struct StillCache {
    cache: Cache,
    key: Cell<Option<FrameKey>>,
}

/// One text block: content, pixel size, weight, colour and opacity.
struct Block<'a> {
    content: &'a str,
    size: f32,
    weight: Weight,
    color: Color,
    gap_above: f32,
}

/// Draws into `target`; the layout is made for `size` and scaled down to cover `target` when they differ.
pub fn draw(scene: &Scene<'_>, state: &StillCache, renderer: &Renderer, target: Size, size: Size) -> Geometry {
    let index = current_index(scene.lyrics, scene.time);
    let key = Some((index, target.width.to_bits(), target.height.to_bits(), scene.font_scale.to_bits()));
    if state.key.replace(key) != key {
        state.cache.clear();
    }
    let scale = (target.width / size.width).max(target.height / size.height);
    state.cache.draw(renderer, target, |frame| {
        frame.fill_rectangle(Point::ORIGIN, target, color(scene.theme.background, 1.0));
        if (scale - 1.0).abs() > f32::EPSILON {
            frame.translate(iced::Vector::new(
                (target.width - size.width * scale) / 2.0,
                (target.height - size.height * scale) / 2.0,
            ));
            frame.scale(scale);
        }
        vignette(frame, size);
        draw_lines(frame, scene, index, size);
    })
}

/// The line on screen: the latest one that has started, or the first before any has.
fn current_index(lyrics: &Lyrics, time: f64) -> Option<usize> {
    if lyrics.lines.is_empty() {
        return None;
    }
    Some(lyrics.line_at(time).unwrap_or(0))
}

fn draw_lines(frame: &mut Frame, scene: &Scene<'_>, index: Option<usize>, size: Size) {
    let Some(index) = index else { return };
    let max_width = (size.width - SIDE_PADDING * 2.0).clamp(1.0, MAX_WIDTH);
    let lines: Vec<Option<&Line>> = [-1_isize, 0, 1]
        .iter()
        .map(|offset| index.checked_add_signed(*offset).and_then(|i| scene.lyrics.lines.get(i)))
        .collect();

    let groups: Vec<Vec<Block<'_>>> = lines
        .iter()
        .enumerate()
        .map(|(slot, line)| line.map(|line| blocks(line, slot == 1, scene)).unwrap_or_default())
        .collect();
    let heights: Vec<f32> = groups
        .iter()
        .map(|group| {
            if group.is_empty() {
                return EMPTY_HEIGHT;
            }
            group
                .iter()
                .map(|block| block.gap_above + measure(block.content, block.size, font(block.weight), max_width).height)
                .sum()
        })
        .collect();

    let total: f32 = heights.iter().sum::<f32>() + GAP * 2.0;
    let mut y = (size.height - BOTTOM_PADDING - total) / 2.0;
    for (group, height) in groups.iter().zip(&heights) {
        let mut block_y = y;
        for block in group {
            block_y += block.gap_above;
            frame.fill_text(Text {
                content: block.content.to_owned(),
                position: Point::new(size.width / 2.0, block_y),
                max_width,
                color: block.color,
                size: block.size.into(),
                font: font(block.weight),
                align_x: iced::widget::text::Alignment::Center,
                align_y: iced::alignment::Vertical::Top,
                ..Text::default()
            });
            block_y += measure(block.content, block.size, font(block.weight), max_width).height;
        }
        y += height + GAP;
    }
}

/// The line's text and translation with the original sizes: 2.5rem / 1.5rem current, 1.875rem / 1.25rem around it.
fn blocks<'a>(line: &'a Line, current: bool, scene: &Scene<'_>) -> Vec<Block<'a>> {
    let theme: &Theme = scene.theme;
    let scale = scene.font_scale;
    let (text_size, text_weight, text_alpha) =
        if current { (40.0, Weight::Bold, 1.0) } else { (30.0, Weight::Semibold, 0.3) };
    let (sub_size, sub_weight, sub_alpha, sub_gap) =
        if current { (24.0, Weight::Medium, 0.8, 12.0) } else { (20.0, Weight::Normal, 0.3, 8.0) };

    let mut blocks = Vec::with_capacity(2);
    if !line.text.is_empty() {
        let color = color(theme.primary, text_alpha);
        blocks.push(Block { content: &line.text, size: text_size * scale, weight: text_weight, color, gap_above: 0.0 });
    }
    if let Some(translation) = &line.translation {
        let color = color(theme.secondary, sub_alpha);
        blocks.push(Block {
            content: translation,
            size: sub_size * scale,
            weight: sub_weight,
            color,
            gap_above: sub_gap,
        });
    }
    blocks
}

fn font(weight: Weight) -> Font {
    Font { weight, ..Font::DEFAULT }
}

/// `radial-gradient(ellipse at center, transparent, #000)` at 65% opacity, drawn as thin elliptical rings.
fn vignette(frame: &mut Frame, size: Size) {
    let reach = std::f32::consts::SQRT_2;
    let step = reach / VIGNETTE_RINGS as f32;
    frame.with_save(|frame| {
        frame.translate(Vector::new(size.width / 2.0, size.height / 2.0));
        frame.scale_nonuniform(Vector::new(size.width / 2.0, size.height / 2.0));
        for ring in 0..VIGNETTE_RINGS {
            let radius = (ring as f32 + 0.5) * step;
            let alpha = radius / reach * 0.65;
            let stroke = Stroke::default().with_width(step).with_color(Color { a: alpha, ..Color::BLACK });
            frame.stroke(&Path::circle(Point::ORIGIN, radius), stroke);
        }
    });
}

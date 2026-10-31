// Tilt (倾诉): a line split into one to four centered rows, one of them large and italic, revealed in time order.

mod layout;

pub use layout::{
    Arrangement, MAX_ROWS, Segment, TiltTuning, arrange, char_pulse, line_count, revealed_through, seeded,
    segment_times, visible_row_count,
};

use std::rc::Rc;

use folia_core::Line;
use iced::advanced::text::Shaping;
use iced::font::{Style, Weight};
use iced::widget::canvas::{Frame, Path, Text};
use iced::{Font, Point, Size, Vector};
use icu_segmenter::GraphemeClusterSegmenter;

use crate::styles::glow;
use crate::{Ease, LayoutCache, Scene, color, measure, tween};

const REVEAL: Ease = Ease::new(0.25, 0.46, 0.45, 0.94);
const COLUMN_ENTER: f64 = 0.3;
const COLUMN_EXIT: f64 = 0.45;
const ROW_ENTER_NORMAL: f64 = 0.55;
const ROW_ENTER_TILT: f64 = 0.6;
const CHAR_ENTER: f64 = 0.5;
/// Blink's italic slope, also the CSS default oblique angle. Shear is `tan` of this, top toward the right.
const SYNTHETIC_ITALIC_DEGREES: f32 = 14.0;

pub struct TiltLayout {
    segments: Vec<DrawnSegment>,
}

struct DrawnSegment {
    glyphs: Vec<Glyph>,
    font_px: f32,
    tilt: bool,
    width: f32,
    height: f32,
    stagger: f32,
    start: f64,
    timings: Vec<CharTiming>,
}

struct Glyph {
    text: String,
    x: f32,
    width: f32,
    space: bool,
    index: usize,
    /// Upright outlines sheared by [`synthetic_shear`], for scripts with no italic face.
    sheared: Vec<Path>,
}

struct CharTiming {
    index: usize,
    start: f64,
    end: f64,
}

pub fn draw(scene: &Scene<'_>, cache: &mut LayoutCache<TiltLayout>, frame: &mut Frame, size: Size) {
    let lines = &scene.lyrics.lines;
    let active = folia_lyrics::active_line(lines, scene.time);
    let leaving =
        lines.iter().enumerate().rev().filter(|(index, line)| Some(*index) != active && line.start <= scene.time);
    for (_, line) in leaving.take(2) {
        let exit_at = glow::exit_at(lines, line);
        if scene.time >= exit_at && scene.time < exit_at + COLUMN_EXIT {
            let opacity = tween(1.0, 0.0, scene.time - exit_at, COLUMN_EXIT, Ease::IN_OUT);
            let moment = Moment { time: scene.time, line_start: line.start, opacity, exiting: true, exit_at };
            paint(scene, cache, frame, size, line, moment);
        }
    }
    if let Some(index) = active {
        let line = &lines[index];
        let opacity = tween(0.0, 1.0, scene.time - line.start, COLUMN_ENTER, Ease::LINEAR);
        let moment = Moment { time: scene.time, line_start: line.start, opacity, exiting: false, exit_at: scene.time };
        paint(scene, cache, frame, size, line, moment);
    }
}

fn paint(
    scene: &Scene<'_>,
    cache: &mut LayoutCache<TiltLayout>,
    frame: &mut Frame,
    size: Size,
    line: &Line,
    moment: Moment,
) {
    if moment.opacity <= 0.002 {
        return;
    }
    let layout = cache.get_or_insert(line, size, scene.font_scale, || Rc::new(build(line, scene, size)));
    let gap = if size.width >= 640.0 { 16.0 } else { 12.0 };
    let total = layout.segments.iter().map(|segment| segment.height).sum::<f32>()
        + gap * layout.segments.len().saturating_sub(1) as f32;
    let mut top = size.height * 0.5 - total / 2.0;
    let reveal_time = if moment.exiting { moment.exit_at } else { moment.time };
    let limit = reveal_limit(line, &layout.segments, reveal_time);
    for (index, segment) in layout.segments.iter().enumerate() {
        if index < limit {
            draw_segment(frame, scene, segment, size.width / 2.0, top, moment);
        }
        top += segment.height + gap;
    }
}

fn reveal_limit(line: &Line, segments: &[DrawnSegment], time: f64) -> usize {
    let mut starts = [0.0; MAX_ROWS];
    let count = segments.len().min(MAX_ROWS);
    for (slot, segment) in segments.iter().take(count).enumerate() {
        starts[slot] = segment.start;
    }
    visible_row_count(&starts[..count], time, line.start, folia_lyrics::RenderHints::of(line).render_end)
}

#[derive(Clone, Copy)]
struct Moment {
    time: f64,
    line_start: f64,
    opacity: f64,
    exiting: bool,
    exit_at: f64,
}

fn draw_segment(frame: &mut Frame, scene: &Scene<'_>, segment: &DrawnSegment, center_x: f32, top: f32, moment: Moment) {
    let anim_start = moment.line_start.max(segment.start - 0.25);
    let (row_opacity, row_y, row_scale) = row_pose(moment, anim_start, segment.tilt);
    let rgb = if segment.tilt { scene.theme.accent } else { scene.theme.primary };
    let center_y = top + segment.height / 2.0;
    let moment = Moment { opacity: moment.opacity * row_opacity, ..moment };
    frame.with_save(|frame| {
        frame.translate(Vector::new(center_x, center_y + row_y));
        frame.scale(row_scale);
        frame.translate(Vector::new(-center_x, -center_y));
        for glyph in &segment.glyphs {
            draw_glyph(frame, segment, glyph, Point::new(center_x, center_y), moment, rgb);
        }
    });
}

fn draw_glyph(
    frame: &mut Frame,
    segment: &DrawnSegment,
    glyph: &Glyph,
    center: Point,
    moment: Moment,
    rgb: folia_core::Rgb,
) {
    let anim_start = moment.line_start.max(segment.start - 0.25);
    let delay = if glyph.space { 0.0 } else { glyph.index as f64 * if segment.tilt { 0.05 } else { 0.04 } };
    let elapsed = moment.time - anim_start - delay;
    let alpha = (moment.opacity * tween(0.0, 1.0, elapsed, CHAR_ENTER, REVEAL)) as f32;
    if alpha <= 0.002 {
        return;
    }
    let y = if glyph.space || !segment.tilt { 0.0 } else { char_y(segment, glyph, elapsed) };
    let scale = pulse_scale(segment, glyph, moment.time);
    let x = center.x - segment.width / 2.0 + glyph.x + glyph.width / 2.0;
    let font = glyph_font(&glyph.text, segment.tilt);
    frame.with_save(|frame| {
        frame.translate(Vector::new(x, center.y + y));
        frame.scale(scale);
        if glyph.sheared.is_empty() {
            frame.fill_text(Text {
                content: glyph.text.clone(),
                position: Point::ORIGIN,
                color: color(rgb, alpha),
                size: segment.font_px.into(),
                font,
                align_x: iced::widget::text::Alignment::Center,
                align_y: iced::alignment::Vertical::Center,
                shaping: Shaping::Advanced,
                ..Text::default()
            });
        } else {
            let fill = color(rgb, alpha);
            for path in &glyph.sheared {
                frame.fill(path, fill);
            }
        }
    });
}

/// Enter pose, or the row's own exit (`y` -12, or -16 and scale 0.95) once the column is leaving.
fn row_pose(moment: Moment, anim_start: f64, tilt: bool) -> (f64, f32, f32) {
    if moment.exiting {
        let (opacity, y, scale) = leaving_row(moment.time - moment.exit_at, tilt);
        return (opacity, y as f32, scale as f32);
    }
    let time = moment.time;
    let duration = if tilt { ROW_ENTER_TILT } else { ROW_ENTER_NORMAL };
    let from_y = if tilt { 24.0 } else { 20.0 };
    let from_scale = if tilt { 0.92 } else { 1.0 };
    let elapsed = time - anim_start;
    (
        tween(0.0, 1.0, elapsed, duration, REVEAL),
        tween(from_y, 0.0, elapsed, duration, REVEAL) as f32,
        tween(from_scale, 1.0, elapsed, duration, REVEAL) as f32,
    )
}

/// Row exit from the shown pose. `elapsed` is seconds since the column's exit instant.
pub fn leaving_row(elapsed: f64, tilt: bool) -> (f64, f64, f64) {
    let duration = if tilt { ROW_ENTER_TILT } else { ROW_ENTER_NORMAL };
    let y_to = if tilt { -16.0 } else { -12.0 };
    let scale_to = if tilt { 0.95 } else { 1.0 };
    (
        tween(1.0, 0.0, elapsed, duration, REVEAL),
        tween(0.0, y_to, elapsed, duration, REVEAL),
        tween(1.0, scale_to, elapsed, duration, REVEAL),
    )
}

fn char_y(segment: &DrawnSegment, glyph: &Glyph, elapsed: f64) -> f32 {
    let direction = if glyph.index.is_multiple_of(2) { -1.0 } else { 1.0 };
    let from = direction * f64::from(segment.stagger) * 2.0;
    let to = direction * f64::from(segment.stagger);
    tween(from, to, elapsed, CHAR_ENTER, REVEAL) as f32
}

fn pulse_scale(segment: &DrawnSegment, glyph: &Glyph, time: f64) -> f32 {
    if glyph.space {
        return 1.0;
    }
    let Some(timing) = segment.timings.iter().find(|timing| timing.index == glyph.index) else {
        return 1.0;
    };
    let factor = if segment.tilt { 0.18 } else { 0.15 };
    (1.0 + char_pulse(time, timing.start, timing.end) * factor) as f32
}

fn build(line: &Line, scene: &Scene<'_>, size: Size) -> TiltLayout {
    let viewport = size.width.max(1.0);
    let available = viewport.max(320.0) * 0.85;
    let base = (viewport * 0.06875).max(90.0 * scene.font_scale);
    let arrangement = arrange(&line.text, line.start, TiltTuning::default(), available, |text, tilt| {
        if tilt {
            graphemes(text)
                .iter()
                .map(|grapheme| measure(grapheme, base, glyph_font(grapheme, true), f32::INFINITY).width)
                .sum()
        } else {
            measure(text, base, font_normal(), f32::INFINITY).width
        }
    });
    let times = segment_times(line, &arrangement.segments);
    let base_scale = scene.font_scale * arrangement.scale;
    let stagger = (viewport * 0.06875 * base_scale).min(90.0 * base_scale) / 6.0;
    let segments = arrangement
        .segments
        .iter()
        .zip(times)
        .map(|(segment, (start, end))| drawn_segment(line, segment, start, end, viewport, base_scale, stagger))
        .collect();
    TiltLayout { segments }
}

fn drawn_segment(
    line: &Line,
    segment: &Segment,
    start: f64,
    end: f64,
    viewport: f32,
    base_scale: f32,
    stagger: f32,
) -> DrawnSegment {
    let boost = if segment.short_last && !segment.tilt { 1.18 } else { 1.0 };
    let font_px = display_px(viewport, base_scale, boost);
    let spacing = font_px * if segment.tilt { 0.15 } else { 0.08 };
    let min_space = font_px * if segment.tilt { 0.35 } else { 0.25 };
    let mut glyphs = Vec::new();
    let mut x = 0.0;
    let mut non_space = 0;
    for grapheme in graphemes(&segment.text) {
        let space = is_space(&grapheme);
        let font = glyph_font(&grapheme, segment.tilt);
        let mut width = measure(&grapheme, font_px, font, f32::INFINITY).width;
        if space {
            width = width.max(min_space);
        }
        let sheared =
            if segment.tilt && needs_shear(&grapheme) { sheared_outlines(&grapheme, font_px) } else { Vec::new() };
        glyphs.push(Glyph { text: grapheme, x, width, space, index: non_space, sheared });
        if !space {
            non_space += 1;
        }
        x += width + spacing;
    }
    let words = &line.words[layout::word_range(segment.char_offset, &segment.text, &line.text, &line.words)];
    DrawnSegment {
        glyphs,
        font_px,
        tilt: segment.tilt,
        width: x,
        height: font_px * if segment.tilt { 1.25 } else { 1.35 },
        stagger,
        start,
        timings: char_timings(&segment.text, start, end, words),
    }
}

fn display_px(viewport: f32, base_scale: f32, boost: f32) -> f32 {
    let min = 50.0 * base_scale * boost;
    let preferred = viewport * 0.06875 * base_scale * boost;
    let max = 90.0 * base_scale * boost;
    preferred.clamp(min, max)
}

fn char_timings(segment: &str, start: f64, end: f64, words: &[folia_core::Word]) -> Vec<CharTiming> {
    let count = graphemes(segment).into_iter().filter(|grapheme| !is_space(grapheme)).count();
    if count == 0 {
        return Vec::new();
    }
    let mut timings = Vec::new();
    if !words.is_empty() {
        for word in words {
            let pieces = graphemes(&word.text).into_iter().filter(|grapheme| !is_space(grapheme)).count();
            if pieces == 0 {
                continue;
            }
            let each = (word.end - word.start).max(0.05) / pieces as f64;
            for n in 0..pieces {
                if timings.len() == count {
                    break;
                }
                timings.push(CharTiming {
                    index: timings.len(),
                    start: word.start + n as f64 * each,
                    end: word.start + (n as f64 + 1.0) * each,
                });
            }
        }
    }
    if timings.len() == count {
        return timings;
    }
    let each = (end - start).max(0.3) / count as f64;
    (0..count)
        .map(|index| CharTiming { index, start: start + index as f64 * each, end: start + (index as f64 + 1.0) * each })
        .collect()
}

fn graphemes(text: &str) -> Vec<String> {
    let bounds: Vec<usize> = GraphemeClusterSegmenter::new().segment_str(text).collect();
    bounds.windows(2).filter(|pair| pair[0] != pair[1]).map(|pair| text[pair[0]..pair[1]].to_owned()).collect()
}

fn is_space(text: &str) -> bool {
    !text.is_empty() && text.chars().all(folia_lyrics::is_js_space)
}

fn needs_shear(text: &str) -> bool {
    !is_space(text)
        && text.chars().any(|c| {
            matches!(
                c,
                '\u{4e00}'..='\u{9fff}'
                    | '\u{3400}'..='\u{4dbf}'
                    | '\u{3040}'..='\u{30ff}'
                    | '\u{ac00}'..='\u{d7af}'
                    | '\u{3000}'..='\u{303f}'
                    | '\u{f900}'..='\u{faff}'
                    | '\u{ff00}'..='\u{ffef}'
            )
        })
}

fn glyph_font(text: &str, tilt: bool) -> Font {
    if !tilt {
        font_normal()
    } else if needs_shear(text) {
        font_upright()
    } else {
        font_italic()
    }
}

/// Outlines of `text` in the upright face, sheared around the origin so the top leans right.
fn sheared_outlines(text: &str, font_px: f32) -> Vec<Path> {
    let mut outlines = Vec::new();
    let sample = Text {
        content: text.to_owned(),
        position: Point::ORIGIN,
        color: iced::Color::WHITE,
        size: font_px.into(),
        font: font_upright(),
        align_x: iced::widget::text::Alignment::Center,
        align_y: iced::alignment::Vertical::Center,
        shaping: Shaping::Advanced,
        ..Text::default()
    };
    sample.draw_with(|path, _| outlines.push(path));
    let shear = synthetic_shear();
    outlines.into_iter().map(|path| path.transform(&shear)).collect()
}

fn synthetic_shear() -> lyon_path::math::Transform {
    let shear = SYNTHETIC_ITALIC_DEGREES.to_radians().tan();
    lyon_path::math::Transform::new(1.0, 0.0, -shear, 1.0, 0.0, 0.0)
}

fn font_normal() -> Font {
    Font { weight: Weight::Normal, ..Font::DEFAULT }
}

fn font_upright() -> Font {
    Font { weight: Weight::Light, style: Style::Normal, ..Font::DEFAULT }
}

fn font_italic() -> Font {
    Font { weight: Weight::Light, style: Style::Italic, ..Font::DEFAULT }
}

// Classic (流光): one line at a time; each word springs in, glows while sung and drifts once passed.

mod layout;

pub use layout::{ClassicTuning, Justify, WordConfig, font_px, justify, random, word_configs, wrap};

use std::rc::Rc;

use folia_core::{AnimationIntensity, Line, Theme};
use folia_lyrics::{RenderHints, Reveal, Transition, active_line};
use iced::widget::canvas::{Frame, Text};
use iced::{Point, Radians, Size, Vector};

use crate::anim::{Ease, keyframes, spring, tween};
use crate::{LayoutCache, Scene, color, measure};

const LINE_HEIGHT: f32 = 1.22;
const MAX_LINE_WIDTH: f32 = 1152.0;
const PADDING: f32 = 32.0;

/// A word's animated state at one instant.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    pub opacity: f64,
    pub scale: f64,
    pub x: f64,
    pub y: f64,
    pub rotate: f64,
    /// 0 is the theme's primary colour, 1 the accent colour.
    pub color: f64,
    pub glow: f64,
}

pub struct LineLayout {
    texts: Vec<String>,
    configs: Vec<WordConfig>,
    centers: Vec<Point>,
    font_px: f32,
}

pub fn draw(scene: &Scene<'_>, cache: &mut LayoutCache<LineLayout>, frame: &mut Frame, size: Size) {
    let lines = &scene.lyrics.lines;
    let time = scene.time;
    let tuning = ClassicTuning::default();
    let center = Point::new(size.width / 2.0, size.height / 2.0);
    let (breath_y, breath_scale) = breathing(time, scene.theme.intensity, tuning.breathing);

    let active = active_line(lines, time);
    let leaving = lines.iter().enumerate().rev().filter(|(i, line)| Some(*i) != active && line.start <= time).take(2);
    for (_, line) in leaving {
        let (exit_at, duration) = exit_timing(lines, line);
        if time >= exit_at && time < exit_at + duration {
            let (opacity, scale) = exit_pose(RenderHints::of(line).transition, time - exit_at, duration);
            let layout = cache.get_or_insert(line, size, scene.font_scale, || build(line, scene, size, tuning));
            draw_line(frame, scene.theme, line, &layout, time, center, opacity, scale * breath_scale, breath_y, tuning);
        }
    }
    if let Some(index) = active {
        let line = &lines[index];
        let (opacity, scale) = enter_pose(RenderHints::of(line).transition, time - line.start);
        let layout = cache.get_or_insert(line, size, scene.font_scale, || build(line, scene, size, tuning));
        draw_line(frame, scene.theme, line, &layout, time, center, opacity, scale * breath_scale, breath_y, tuning);
        draw_translation(frame, scene, line, size, opacity);
    }
}

fn build(line: &Line, scene: &Scene<'_>, size: Size, tuning: ClassicTuning) -> Rc<LineLayout> {
    let font_px = font_px(size.width, scene.font_scale);
    let texts: Vec<String> = line.words.iter().map(|word| word.text.clone()).collect();
    let widths: Vec<f32> = texts.iter().map(|text| measure(text, font_px, crate::BOLD, f32::INFINITY).width).collect();
    let seed = line.start;
    let interlude = line.is_interlude();
    let configs = word_configs(seed, scene.theme.intensity, interlude, &widths, font_px, tuning);
    let margins: Vec<f32> = configs.iter().map(|config| config.margin_right as f32).collect();
    let max_width = (size.width - PADDING * 2.0).clamp(1.0, MAX_LINE_WIDTH);
    let left = (size.width - max_width) / 2.0;
    let justify = justify(seed, scene.theme.intensity, interlude);
    let placed = wrap(&widths, &margins, font_px * LINE_HEIGHT, max_width, left, size.height / 2.0, justify);
    let centers = placed.iter().map(|p| Point::new(p.x, p.y)).collect();
    Rc::new(LineLayout { texts, configs, centers, font_px })
}

#[allow(clippy::too_many_arguments)]
fn draw_line(
    frame: &mut Frame,
    theme: &Theme,
    line: &Line,
    layout: &LineLayout,
    time: f64,
    center: Point,
    line_opacity: f64,
    line_scale: f64,
    breath_y: f64,
    tuning: ClassicTuning,
) {
    let hints = RenderHints::of(line);
    frame.with_save(|frame| {
        frame.translate(Vector::new(center.x, center.y + breath_y as f32));
        frame.scale(line_scale as f32);
        frame.translate(Vector::new(-center.x, -center.y));
        for (i, word) in line.words.iter().enumerate() {
            let config = layout.configs[i];
            let pose = word_pose(&config, word.start, word.end, hints, theme.intensity, tuning, time);
            let alpha = (pose.opacity * line_opacity) as f32;
            if alpha <= 0.002 {
                continue;
            }
            let body = color(theme.primary.mix(theme.accent, pose.color as f32), alpha);
            let glow = (pose.glow * pose.opacity * line_opacity * 0.45) as f32;
            frame.with_save(|frame| {
                let at = layout.centers[i];
                frame.translate(Vector::new(at.x + pose.x as f32, at.y + pose.y as f32));
                frame.rotate(Radians(pose.rotate.to_radians() as f32));
                frame.scale(pose.scale as f32);
                if glow > 0.01 {
                    frame.with_save(|frame| {
                        frame.scale(1.06_f32);
                        frame.fill_text(word_text(&layout.texts[i], layout.font_px, color(theme.accent, glow)));
                    });
                }
                frame.fill_text(word_text(&layout.texts[i], layout.font_px, body));
            });
        }
    });
}

fn word_text(content: &str, size: f32, color: iced::Color) -> Text {
    Text {
        content: content.to_owned(),
        position: Point::ORIGIN,
        color,
        size: size.into(),
        font: crate::BOLD,
        align_x: iced::widget::text::Alignment::Center,
        align_y: iced::alignment::Vertical::Center,
        ..Text::default()
    }
}

/// A word's pose at `time`: waiting until its lookahead, active until its active end, passed after.
#[allow(clippy::too_many_arguments)]
pub fn word_pose(
    config: &WordConfig,
    start: f64,
    end: f64,
    hints: RenderHints,
    intensity: AnimationIntensity,
    tuning: ClassicTuning,
    time: f64,
) -> Pose {
    let waiting = Pose {
        opacity: 0.0,
        scale: 0.5,
        x: config.x + config.y.sin() * 100.0,
        y: config.y + config.x.cos() * 50.0,
        rotate: if tuning.word_rotation { config.rotate + 20.0 } else { 0.0 },
        color: 0.0,
        glow: 0.0,
    };
    let active_at = start - hints.word_lookahead();
    let passed_at = hints.word_active_end(start, end);
    let duration = (passed_at - start).max(match hints.reveal {
        Reveal::Instant => 0.08,
        Reveal::Fast => 0.12,
        Reveal::Normal => 0.1,
    });
    if time < active_at {
        return waiting;
    }

    let active = |elapsed: f64| Pose {
        opacity: tween(0.0, 1.0, elapsed, 0.1, Ease::OUT),
        scale: spring(waiting.scale, config.scale * 1.4, elapsed, 200.0, 20.0),
        x: spring(waiting.x, config.x, elapsed, 200.0, 20.0),
        y: spring(waiting.y, config.y, elapsed, 200.0, 20.0),
        rotate: spring(waiting.rotate, config.rotate, elapsed, 200.0, 20.0),
        color: tween(0.0, 1.0, elapsed, duration, Ease::LINEAR),
        glow: glow(hints.reveal, elapsed, duration),
    };
    if time <= passed_at {
        return active(time - active_at);
    }

    let from = active(passed_at - active_at);
    let elapsed = time - passed_at;
    let color_duration = match hints.reveal {
        Reveal::Instant => 0.12,
        Reveal::Fast => 0.24,
        Reveal::Normal => 0.8,
    };
    let rest = if intensity == AnimationIntensity::Chaotic { 0.9 } else { 0.82 };
    Pose {
        opacity: tween(from.opacity, rest, elapsed, 0.5, Ease::OUT),
        scale: tween(from.scale, config.scale, elapsed, 0.5, Ease::OUT),
        x: tween(from.x, config.x, elapsed, 0.5, Ease::OUT),
        y: tween(from.y, config.y, elapsed, 0.5, Ease::OUT),
        rotate: tween(from.rotate, config.rotate + config.passed_rotate, elapsed, 5.0, Ease::LINEAR),
        color: tween(from.color, 0.0, elapsed, color_duration, Ease::IN_OUT),
        glow: tween(from.glow, 0.0, elapsed, 0.3, Ease::OUT),
    }
}

fn glow(reveal: Reveal, elapsed: f64, duration: f64) -> f64 {
    match reveal {
        Reveal::Instant => keyframes(&[0.0, 1.0, 0.0], &[0.0, 0.35, 1.0], elapsed, duration.min(0.12), Ease::OUT),
        Reveal::Fast => keyframes(&[0.0, 1.0, 0.0], &[0.0, 0.4, 1.0], elapsed, duration.clamp(0.12, 0.2), Ease::IN_OUT),
        Reveal::Normal => keyframes(&[0.0, 1.0, 1.0], &[0.0, 0.9, 1.0], elapsed, duration, Ease::IN_OUT),
    }
}

/// When a line leaves the screen and how long its exit takes.
fn exit_timing(lines: &[Line], line: &Line) -> (f64, f64) {
    let hints = RenderHints::of(line);
    let next_start = lines.iter().map(|other| other.start).find(|&start| start > line.start).unwrap_or(f64::INFINITY);
    let duration = match hints.transition {
        Transition::Normal => 0.3,
        Transition::Fast => 0.16,
        Transition::None => 0.12,
    };
    (hints.render_end.min(next_start), duration)
}

fn enter_pose(transition: Transition, elapsed: f64) -> (f64, f64) {
    match transition {
        Transition::None => (1.0, 1.0),
        Transition::Fast => (tween(0.35, 1.0, elapsed, 0.16, Ease::OUT), tween(0.96, 1.0, elapsed, 0.16, Ease::OUT)),
        Transition::Normal => (tween(0.0, 1.0, elapsed, 0.3, Ease::OUT), spring(0.9, 1.0, elapsed, 500.0, 25.0)),
    }
}

fn exit_pose(transition: Transition, elapsed: f64, duration: f64) -> (f64, f64) {
    let (ease, scale) = match transition {
        Transition::Normal => (Ease::OUT, 1.1),
        Transition::Fast => (Ease::IN_OUT, 1.04),
        Transition::None => (Ease::OUT, 1.02),
    };
    (tween(1.0, 0.0, elapsed, duration, ease), tween(1.0, scale, elapsed, duration, ease))
}

/// The whole line's slow float: returns a vertical offset in pixels and a scale.
fn breathing(time: f64, intensity: AnimationIntensity, multiplier: f64) -> (f64, f64) {
    if multiplier <= 0.0 {
        return (0.0, 1.0);
    }
    let (distance, period) = match intensity {
        AnimationIntensity::Calm => (10.0, 8.5),
        AnimationIntensity::Normal => (14.0, 7.0),
        AnimationIntensity::Chaotic => (18.0, 5.8),
    };
    let d = distance * multiplier;
    let phase = time.rem_euclid(period);
    let times = [0.0, 0.25, 0.5, 0.75, 1.0];
    let y = keyframes(&[0.0, -d, 0.0, d * 0.45, 0.0], &times, phase, period, Ease::IN_OUT);
    let scale = keyframes(
        &[1.0, 1.0 + 0.01 * multiplier, 1.0, 1.0 - 0.005 * multiplier, 1.0],
        &times,
        phase,
        period,
        Ease::IN_OUT,
    );
    (y, scale)
}

fn draw_translation(frame: &mut Frame, scene: &Scene<'_>, line: &Line, size: Size, opacity: f64) {
    let Some(translation) = &line.translation else { return };
    frame.fill_text(Text {
        content: translation.clone(),
        position: Point::new(size.width / 2.0, size.height * 0.86),
        color: color(scene.theme.secondary, opacity as f32),
        size: (20.0 * scene.font_scale).into(),
        align_x: iced::widget::text::Alignment::Center,
        align_y: iced::alignment::Vertical::Center,
        ..Text::default()
    });
}

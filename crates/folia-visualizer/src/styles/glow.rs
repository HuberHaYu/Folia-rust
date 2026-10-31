// The original's GlowWord shared by classic and partita: word poses, line enter/exit, breathing float and word drawing.

use folia_core::{AnimationIntensity, Line, Theme};
use folia_lyrics::{RenderHints, Reveal, Transition, active_line};
use iced::widget::canvas::{Frame, Text};
use iced::{Point, Radians, Size, Vector};

use crate::anim::{Ease, keyframes, spring, tween};
use crate::color;

/// One word's resting geometry; offsets in pixels, angles in degrees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WordConfig {
    pub x: f64,
    pub y: f64,
    pub rotate: f64,
    pub scale: f64,
    pub passed_rotate: f64,
    pub margin_right: f64,
}

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

/// A word's pose at `time`: waiting until its lookahead, active until its active end, passed after.
pub fn word_pose(
    config: &WordConfig,
    start: f64,
    end: f64,
    hints: RenderHints,
    intensity: AnimationIntensity,
    rotation: bool,
    time: f64,
) -> Pose {
    let waiting = Pose {
        opacity: 0.0,
        scale: 0.5,
        x: config.x + config.y.sin() * 100.0,
        y: config.y + config.x.cos() * 50.0,
        rotate: if rotation { config.rotate + 20.0 } else { 0.0 },
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

/// Calls `draw` for every line on screen at `time` with its container opacity and scale:
/// the lines still leaving, then the active line entering.
pub fn visible_lines<'a>(lines: &'a [Line], time: f64, mut draw: impl FnMut(&'a Line, f64, f64, bool)) {
    let active = active_line(lines, time);
    let leaving = lines.iter().enumerate().rev().filter(|(i, line)| Some(*i) != active && line.start <= time).take(2);
    for (_, line) in leaving {
        let (exit_at, duration) = exit_timing(lines, line);
        if time >= exit_at && time < exit_at + duration {
            let (opacity, scale) = exit_pose(RenderHints::of(line).transition, time - exit_at, duration);
            draw(line, opacity, scale, false);
        }
    }
    if let Some(index) = active {
        let line = &lines[index];
        let (opacity, scale) = enter_pose(RenderHints::of(line).transition, time - line.start);
        draw(line, opacity, scale, true);
    }
}

/// When a line starts leaving: its render end, or the next line's start when that is sooner.
pub fn exit_at(lines: &[Line], line: &Line) -> f64 {
    let end = RenderHints::of(line).render_end;
    let next = lines.iter().map(|other| other.start).find(|start| *start > line.start);
    next.map_or(end, |start| end.min(start))
}

/// How long the shared glow exit runs after [`exit_at`].
fn exit_timing(lines: &[Line], line: &Line) -> (f64, f64) {
    let duration = match RenderHints::of(line).transition {
        Transition::Normal => 0.3,
        Transition::Fast => 0.16,
        Transition::None => 0.12,
    };
    (exit_at(lines, line), duration)
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

/// The whole line's slow float: a vertical offset in pixels and a scale.
pub fn breathing(time: f64, intensity: AnimationIntensity, multiplier: f64) -> (f64, f64) {
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

/// Draws one word at `at` with its pose: a soft enlarged halo while it glows, then the body.
pub fn draw_word(
    frame: &mut Frame,
    theme: &Theme,
    text: &str,
    at: Point,
    pose: &Pose,
    line_opacity: f64,
    font_px: f32,
) {
    let alpha = (pose.opacity * line_opacity) as f32;
    if alpha <= 0.002 {
        return;
    }
    let body = color(theme.primary.mix(theme.accent, pose.color as f32), alpha);
    let glow = (pose.glow * pose.opacity * line_opacity * 0.45) as f32;
    frame.with_save(|frame| {
        frame.translate(Vector::new(at.x + pose.x as f32, at.y + pose.y as f32));
        frame.rotate(Radians(pose.rotate.to_radians() as f32));
        frame.scale(pose.scale as f32);
        if glow > 0.01 {
            frame.with_save(|frame| {
                frame.scale(1.06_f32);
                frame.fill_text(word_text(text, font_px, color(theme.accent, glow)));
            });
        }
        frame.fill_text(word_text(text, font_px, body));
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

/// Scales the frame by `scale` around `center` after shifting it down by `offset_y`.
pub fn scale_around(frame: &mut Frame, center: Point, scale: f64, offset_y: f64) {
    frame.translate(Vector::new(center.x, center.y + offset_y as f32));
    frame.scale(scale as f32);
    frame.translate(Vector::new(-center.x, -center.y));
}

/// The active line's translation under the lyrics, in the theme's secondary colour.
pub fn draw_translation(frame: &mut Frame, theme: &Theme, line: &Line, size: Size, font_scale: f32, opacity: f64) {
    let Some(translation) = &line.translation else { return };
    frame.fill_text(Text {
        content: translation.clone(),
        position: Point::new(size.width / 2.0, size.height * 0.86),
        color: color(theme.secondary, opacity as f32),
        size: (20.0 * font_scale).into(),
        align_x: iced::widget::text::Alignment::Center,
        align_y: iced::alignment::Vertical::Center,
        ..Text::default()
    });
}

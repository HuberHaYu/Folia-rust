// Cadenza envelopes from cadenzaEnvelopes.ts. Steps in scale, rotation and alpha use the
// original exponential smoothing as a closed form from each status change.

use folia_core::{AnimationIntensity, Line};
use folia_lyrics::{RenderHints, Reveal};

pub const FONT_SCALE: f32 = 1.12;
pub const WIDTH_RATIO: f64 = 0.72;
pub const MOTION: f64 = 1.0;
pub const GLOW: f64 = 1.0;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Waiting,
    Active,
    Passed,
}

pub struct Timing {
    pub hints: RenderHints,
    pub last_word_end: f64,
    pub enter: f64,
    pub exit: f64,
    pub hold: f64,
}

pub struct Motion {
    pub scale: f64,
    pub rotation: f64,
    pub alpha: f64,
    pub glow: f64,
    pub mix: f64,
    pub drift_x: f64,
    pub drift_y: f64,
}

/// The placement fields `motion` needs. Kept together so the chase stays one call.
pub struct WordMotion {
    pub scale: f64,
    pub passed_rotate: f64,
    pub drift_x: f64,
    pub drift_y: f64,
    pub start: f64,
    pub end: f64,
    pub line_start: f64,
    pub motion_amount: f64,
}

pub fn timing_of(line: &Line) -> Timing {
    let hints = RenderHints::of(line);
    let phases = hints.phases(line);
    Timing {
        hints,
        last_word_end: line.words.last().map_or(line.end, |word| word.end),
        enter: phases.enter,
        exit: phases.exit,
        hold: phases.hold,
    }
}

pub fn graphemes(text: &str) -> Vec<String> {
    text.chars().map(|ch| ch.to_string()).collect()
}

pub fn is_cjk_text(text: &str) -> bool {
    text.chars().any(|ch| {
        ('\u{4e00}'..='\u{9fa5}').contains(&ch)
            || ('\u{3040}'..='\u{30ff}').contains(&ch)
            || ('\u{ac00}'..='\u{d7af}').contains(&ch)
    })
}

pub fn clamp(value: f64, min: f64, max: f64) -> f64 {
    value.max(min).min(max)
}

fn ease_out_cubic(value: f64) -> f64 {
    1.0 - (1.0 - clamp(value, 0.0, 1.0)).powi(3)
}

fn ease_in_out_quad(value: f64) -> f64 {
    let t = clamp(value, 0.0, 1.0);
    if t < 0.5 { 2.0 * t * t } else { 1.0 - (-2.0 * t + 2.0).powi(2) / 2.0 }
}

fn mix(from: f64, to: f64, amount: f64) -> f64 {
    from + (to - from) * amount
}

pub fn status(time: f64, timing: &Timing, start: f64, end: f64) -> Status {
    let active_end = if timing.hints.reveal == Reveal::Instant { timing.hints.render_end } else { end };
    if time >= start - lookahead(timing.hints.reveal) && time <= active_end {
        Status::Active
    } else if time > active_end {
        Status::Passed
    } else {
        Status::Waiting
    }
}

fn lookahead(reveal: Reveal) -> f64 {
    match reveal {
        Reveal::Fast => 0.045,
        Reveal::Instant => 0.0,
        Reveal::Normal => 0.18,
    }
}

pub fn glow_envelope(time: f64, timing: &Timing, start: f64, end: f64) -> f64 {
    match timing.hints.reveal {
        Reveal::Instant => instant_glow(time, start, timing.hints.render_end),
        Reveal::Fast => fast_glow(time, start, end),
        Reveal::Normal => normal_glow(time, start, end),
    }
}

fn instant_glow(time: f64, start: f64, active_end: f64) -> f64 {
    if time < start || time > active_end {
        return 0.0;
    }
    keyframed_glow(clamp((time - start) / 0.067, 0.0, 1.0))
}

fn fast_glow(time: f64, start: f64, end: f64) -> f64 {
    if time < start {
        return 0.0;
    }
    if time <= end {
        let progress = clamp((time - start) / (end - start).max(0.045), 0.0, 1.0);
        return if progress < 0.14 {
            ease_out_cubic(progress / 0.14)
        } else if progress < 0.82 {
            1.0
        } else {
            mix(1.0, 0.92, (progress - 0.82) / 0.18)
        };
    }
    (1.0 - clamp((time - end) / 0.14, 0.0, 1.0)).powi(2)
}

fn normal_glow(time: f64, start: f64, end: f64) -> f64 {
    if time < start {
        return 0.0;
    }
    if time <= end {
        let progress = clamp((time - start) / (end - start).max(0.1), 0.0, 1.0);
        return if progress < 0.18 {
            ease_out_cubic(progress / 0.18)
        } else if progress < 0.9 {
            1.0
        } else {
            mix(1.0, 0.9, (progress - 0.9) / 0.1)
        };
    }
    0.9 * (1.0 - clamp((time - end) / 0.9, 0.0, 1.0)).powi(2)
}

fn keyframed_glow(progress: f64) -> f64 {
    if progress <= 0.0 || progress >= 1.0 {
        return 0.0;
    }
    if progress < 0.3 { ease_out_cubic(progress / 0.3) } else { 1.0 - clamp((progress - 0.3) / 0.7, 0.0, 1.0) }
}

pub fn body_mix(time: f64, timing: &Timing, start: f64, end: f64) -> f64 {
    if timing.hints.reveal == Reveal::Instant {
        return if time < start {
            0.0
        } else if time <= timing.hints.render_end {
            1.0
        } else {
            0.0
        };
    }
    if time < start {
        return 0.0;
    }
    if time <= end {
        let floor = if timing.hints.reveal == Reveal::Fast { 0.045 } else { 0.01 };
        return clamp((time - start) / (end - start).max(floor), 0.0, 1.0);
    }
    let fade = if timing.hints.reveal == Reveal::Fast { 0.12 } else { 0.8 };
    1.0 - clamp((time - end) / fade, 0.0, 1.0)
}

pub fn char_glow(time: f64, end: f64, glyph_start: f64, glyph_end: f64) -> f64 {
    let duration = (glyph_end - glyph_start).max(0.001);
    let active = if time - glyph_start <= 0.0 { 0.0 } else { keyframed_glow((time - glyph_start) / (duration * 6.0)) };
    if time <= end { active } else { active * (1.0 - clamp((time - end) / 0.9, 0.0, 1.0)).powi(2) }
}

pub fn passed_drift(time: f64, end: f64) -> f64 {
    if time <= end { 0.0 } else { ease_in_out_quad(clamp((time - end) / 5.0, 0.0, 1.0)) }
}

pub fn line_fade(time: f64, line: &Line, timing: &Timing) -> (f64, f64) {
    if timing.hints.transition == folia_lyrics::Transition::None {
        return (1.0, 1.0);
    }
    let pass = timing.last_word_end.max(line.start) + timing.hold;
    if timing.hints.transition == folia_lyrics::Transition::Fast {
        return fast_line(time, line.start, timing.hints.render_end, timing.enter, timing.exit, pass);
    }
    let pre = 0.1_f64.min(timing.enter * 0.35);
    let enter = ease_out_cubic(clamp((time - (line.start - pre)) / (timing.enter + pre), 0.0, 1.0));
    let mut opacity = enter;
    let mut scale = mix(0.9, 1.0, enter);
    let exit_start = pass.max(timing.hints.render_end - timing.exit);
    let exit = ease_out_cubic(clamp((time - exit_start) / timing.exit, 0.0, 1.0));
    if exit > 0.0 {
        opacity *= 1.0 - exit;
        scale = mix(scale, 1.1, exit);
    }
    (clamp(opacity, 0.0, 1.0), scale)
}

fn fast_line(time: f64, start: f64, render_end: f64, enter: f64, exit: f64, pass: f64) -> (f64, f64) {
    let entered = ease_out_cubic(clamp((time - start) / enter, 0.0, 1.0));
    let mut opacity = mix(0.65, 1.0, entered);
    let mut scale = mix(0.97, 1.0, entered);
    let exit_start = (start + enter + 0.01).max(pass).max(render_end - exit);
    let leaving = ease_out_cubic(clamp((time - exit_start) / exit, 0.0, 1.0));
    if leaving > 0.0 {
        opacity = mix(opacity, 0.0, leaving);
        scale = mix(scale, 1.03, leaving);
    }
    (clamp(opacity, 0.0, 1.0), scale)
}

/// Scale, rotation and alpha chased with the original rates. Glow and colour follow their curves.
pub fn motion(word: &WordMotion, time: f64, timing: &Timing, intensity: AnimationIntensity) -> Motion {
    let WordMotion { scale, passed_rotate, drift_x, drift_y, start, end, line_start, motion_amount } = *word;
    let reveal = timing.hints.reveal;
    let active_at = start - lookahead(reveal);
    let passed_at = if reveal == Reveal::Instant { timing.hints.render_end } else { end };
    let now = status(time, timing, start, end);
    let pulse = if now == Status::Active && reveal != Reveal::Instant {
        1.0 + (time * 10.0 + start * 5.0).sin() * 0.04 * motion_amount
    } else {
        1.0
    };
    let waiting_scale = (scale * 0.5).max(0.5);
    let active_scale = if reveal == Reveal::Instant { scale } else { scale * 1.3 };
    let rest = if reveal == Reveal::Instant {
        0.0
    } else if intensity == AnimationIntensity::Chaotic {
        0.9
    } else {
        0.82
    };
    let drift = if reveal == Reveal::Instant { 0.0 } else { passed_drift(time, end) };
    let born_active = reveal == Reveal::Instant && line_start >= start;
    let born_scale = if born_active { scale } else { waiting_scale };
    let scale_at = chase(
        Chase {
            origin: line_start,
            active_at,
            passed_at,
            born: born_scale,
            waiting: waiting_scale,
            active: active_scale,
            passed: scale,
            rate: 11.0,
        },
        time,
    );
    let born_rotation = if born_active { 0.0 } else { (if line_start < active_at { 20.0 } else { 0.0 }) + 16.0 };
    Motion {
        scale: if now == Status::Active && reveal != Reveal::Instant { scale_at * pulse } else { scale_at },
        rotation: chase(
            Chase {
                origin: line_start,
                active_at,
                passed_at,
                born: born_rotation,
                waiting: 20.0,
                active: 0.0,
                passed: passed_rotate * drift,
                rate: 11.0,
            },
            time,
        ),
        alpha: chase(
            Chase {
                origin: line_start,
                active_at,
                passed_at,
                born: if born_active { 1.0 } else { 0.0 },
                waiting: 0.0,
                active: 1.0,
                passed: rest,
                rate: 14.0,
            },
            time,
        ),
        glow: glow_envelope(time, timing, start, end),
        mix: body_mix(time, timing, start, end),
        drift_x: if now == Status::Passed { drift_x * drift } else { 0.0 },
        drift_y: if now == Status::Passed { drift_y * drift } else { 0.0 },
    }
}

struct Chase {
    origin: f64,
    active_at: f64,
    passed_at: f64,
    born: f64,
    waiting: f64,
    active: f64,
    passed: f64,
    rate: f64,
}

/// Exponential chase. `born` is the value when the line appears; targets then step at the status changes.
fn chase(chase: Chase, time: f64) -> f64 {
    let Chase { origin, active_at, passed_at, born, waiting, active, passed, rate } = chase;
    let mut value = born;
    let mut cursor = origin;
    if active_at > cursor {
        if time <= active_at {
            return lag(value, waiting, time - cursor, rate);
        }
        value = lag(value, waiting, active_at - cursor, rate);
        cursor = active_at;
    }
    if time <= passed_at {
        return lag(value, active, time - cursor, rate);
    }
    value = lag(value, active, (passed_at - cursor).max(0.0), rate);
    lag(value, passed, time - passed_at, rate)
}

fn lag(from: f64, to: f64, elapsed: f64, rate: f64) -> f64 {
    if elapsed <= 0.0 { from } else { to + (from - to) * (-rate * elapsed).exp() }
}

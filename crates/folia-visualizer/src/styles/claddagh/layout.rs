// Ellipse placement of one Claddagh line, from claddaghLayout.ts and CladdaghRingLine.tsx.

use folia_core::Line;

use super::timeline::{self, Mark};
use crate::measure;

const MAX_ARC: f64 = 4.25;
const LETTER_EM: f32 = 0.04;
const TRACKING_EM: f32 = 0.18;
const BACK_FOLLOW: f64 = 0.28;
const BACK_ORBIT: f64 = 0.52;
const ENTRY_LEAD: f64 = 0.34;
const SPRING_K: f64 = 55.0 / 0.9;
const SPRING_C: f64 = 14.0 / 0.9;

/// One grapheme after it has been measured onto the ring.
#[derive(Debug, Clone)]
pub struct Placed {
    pub text: String,
    pub start: f64,
    pub end: f64,
    pub nominal: f64,
    pub arc_scale: f64,
}

/// Where one glyph sits this frame. Coordinates are pixels from the ring centre.
#[derive(Debug, Clone, Copy)]
pub struct Glyph {
    pub x: f32,
    pub y: f32,
    pub opacity: f32,
    pub scale: f32,
    pub rotation: f32,
}

pub fn placed(line: &Line, font_px: f32, radius: f32, spacing_scale: f64, letter_offset: f32) -> Vec<Placed> {
    let mut marks = timeline::line_marks(line);
    timeline::adjust(&mut marks, line.start, line.end);
    place_marks(&marks, font_px, radius, spacing_scale, letter_offset)
}

pub fn place_marks(marks: &[Mark], font_px: f32, radius: f32, spacing_scale: f64, letter_offset: f32) -> Vec<Placed> {
    if marks.is_empty() {
        return Vec::new();
    }
    let font = iced::Font { weight: iced::font::Weight::Bold, ..iced::Font::DEFAULT };
    let mut offsets = Vec::with_capacity(marks.len());
    let mut prefix = String::new();
    let mut fallback = 0.0_f32;
    for (index, mark) in marks.iter().enumerate() {
        if index > 0 {
            fallback += fallback_width(&marks[index - 1].text, font_px);
        }
        prefix.push_str(&mark.text);
        let measured = measure(&prefix, font_px, font, 10_000.0).width;
        let natural = if measured > 0.0 { measured } else { fallback };
        let gaps = index as f32;
        let width = natural + gaps * font_px * (LETTER_EM + TRACKING_EM) + gaps * letter_offset;
        let previous = offsets.last().copied().unwrap_or(0.0_f32);
        offsets.push(previous.max(width));
    }
    let scale = spacing_scale.max(0.1);
    let safe_radius = (radius as f64).max(f64::from(font_px) * 2.0).max(1.0);
    let total = f64::from(*offsets.last().unwrap_or(&0.0)) * scale;
    let span = total / safe_radius;
    let arc = if span > MAX_ARC { MAX_ARC / span } else { 1.0 };
    marks
        .iter()
        .enumerate()
        .map(|(index, mark)| {
            let here = f64::from(offsets[index]) * scale;
            let next = f64::from(*offsets.get(index + 1).unwrap_or(&offsets[index])) * scale;
            let start_angle = ((here + next) / 2.0) / safe_radius;
            Placed {
                text: mark.text.clone(),
                start: mark.start,
                end: mark.end,
                nominal: (start_angle - span / 2.0) * arc,
                arc_scale: arc,
            }
        })
        .collect()
}

fn fallback_width(text: &str, font_px: f32) -> f32 {
    if text.chars().all(char::is_whitespace) {
        font_px * 0.36
    } else if text.chars().all(timeline::is_cjk) {
        font_px
    } else {
        font_px * 0.62
    }
}

/// Sung-word angle of a placed line. Reads the cached placement and does not allocate.
pub fn sung_offset(placed: &[Placed], time: f64, render_end: Option<f64>) -> f64 {
    if placed.is_empty() {
        return 0.0;
    }
    let index = timeline::fractional_index_at(placed.len(), time, render_end, |at| (placed[at].start, placed[at].end));
    let last = placed.len() - 1;
    if index <= last as f64 {
        let at = index.floor() as usize;
        let frac = index - at as f64;
        let left = placed[at].nominal;
        let right = placed[(at + 1).min(last)].nominal;
        return left + (right - left) * frac;
    }
    let step = placed[last].nominal - placed[last.saturating_sub(1)].nominal;
    placed[last].nominal + step * (index - last as f64)
}

pub fn render_end(lines: &[Line], index: usize) -> Option<f64> {
    let line = lines.get(index)?;
    let own = folia_lyrics::RenderHints::of(line).render_end;
    let next = lines.get(index + 1).map(|next| next.start - ENTRY_LEAD);
    Some(own.max(next.unwrap_or(line.end + 10.0)))
}

pub fn playback_progress(placed: &[Placed], time: f64, render_end: Option<f64>) -> f64 {
    if placed.is_empty() {
        return 0.0;
    }
    if placed.len() == 1 {
        let end = render_end.filter(|end| end.is_finite()).unwrap_or(placed[0].end);
        let duration = (end - placed[0].start).max(0.001);
        return timeline::clamp((time - placed[0].start) / duration, 0.0, 1.0);
    }
    let index = timeline::fractional_index_at(placed.len(), time, render_end, |at| (placed[at].start, placed[at].end));
    timeline::clamp(index / (placed.len() - 1) as f64, 0.0, 1.0)
}

/// Line the ring centers on. After the first line has started this is the last
/// one already started: the original uses the render window, then the last line
/// whose end has passed, and `render_end` is never earlier than `end`.
pub fn center_line(lines: &[Line], time: f64) -> Option<usize> {
    lines.iter().rposition(|line| line.start <= time)
}

/// How far the ring has turned. Each line starts from the angle the previous
/// line had reached, folded from the start of the song. A fixed lookback would
/// treat a spring that is still moving as finished and step a run of short
/// lines forward, so the fold stops only when the index jumps by more than one.
pub fn ring_angle(lines: &[Line], time: f64) -> f64 {
    let Some(current) = center_line(lines, time) else {
        return 0.0;
    };
    let mut angle = 0.0;
    for index in 1..=current {
        let target = index as f64 * std::f64::consts::PI;
        let boundary = lines[index].start;
        if lines[index - 1].start >= boundary {
            angle = target;
            continue;
        }
        let until = if index == current { time } else { lines[index + 1].start };
        angle = crate::anim::spring(angle, target, until - boundary, SPRING_K, SPRING_C);
    }
    angle
}

pub fn word_shift(line_index: usize, center: usize, own: f64, active: f64, active_progress: f64, ring: f64) -> f64 {
    if line_index < center {
        return own;
    }
    let line_diff = (ring - center as f64 * std::f64::consts::PI).abs() / std::f64::consts::PI;
    let follow = if line_index > center { 1.0 } else { timeline::clamp(line_diff, 0.0, 1.0) };
    let orbit = std::f64::consts::PI * BACK_ORBIT * (1.0 - (1.0 - active_progress).powf(1.35));
    own + (active * BACK_FOLLOW + orbit) * follow
}

/// Inputs that are the same for every grapheme of one line.
pub struct RingMoment {
    pub line_index: usize,
    pub center: usize,
    pub ring: f64,
    pub word_offset: f64,
    pub rx: f32,
    pub focus_scale: f32,
    pub tilt_deg: f32,
    pub power: f32,
    pub chorus: bool,
    pub length_fade: f32,
    pub length_scale: f32,
    pub upcoming: f32,
    pub font_px: f32,
}

/// One glyph on the ellipse. `power` is the beat; this port passes 0 until audio bands exist.
pub fn glyph(item: &Placed, index: usize, moment: &RingMoment) -> Glyph {
    let RingMoment {
        line_index,
        center,
        ring,
        word_offset,
        rx,
        focus_scale,
        tilt_deg,
        power,
        chorus,
        length_fade,
        length_scale,
        upcoming,
        font_px,
    } = *moment;
    let line_diff = (ring - center as f64 * std::f64::consts::PI).abs() / std::f64::consts::PI;
    let theta = line_index as f64 * std::f64::consts::PI + item.nominal;
    let psi = theta - ring - word_offset;
    let delta = psi * f64::from(rx);
    let curve = if rx > 0.0 { delta / f64::from(rx) } else { 0.0 };
    let local_cos = curve.cos();
    let depth = (local_cos + 1.0) / 2.0;
    let spacing = 0.35 + 0.65 * depth.powf(1.2);
    let raw_x = curve.sin() * f64::from(rx) * spacing;
    let mut raw_y = local_cos * f64::from(rx) * 0.09;
    if chorus {
        let stagger = f64::from(font_px) * (0.06 + f64::from(power) * 0.12);
        raw_y += if index.is_multiple_of(2) { stagger } else { -stagger };
    }
    let tilt = -f64::from(tilt_deg) * std::f64::consts::PI / 180.0;
    let (cos_t, sin_t) = (tilt.cos(), tilt.sin());
    let x = raw_x * cos_t - raw_y * sin_t;
    let y = raw_x * sin_t + raw_y * cos_t;
    let tangent_x = curve.cos() * f64::from(rx);
    let tangent_y = -curve.sin() * f64::from(rx) * 0.09;
    let rot_tx = tangent_x * cos_t - tangent_y * sin_t;
    let rot_ty = tangent_x * sin_t + tangent_y * cos_t;
    let tangent = readable_angle(rot_ty.atan2(rot_tx).to_degrees());
    let active = (1.0 - line_diff).max(0.0);
    let dist = (delta.abs() / (f64::from(rx) * 0.48).max(1.0)).min(1.0);
    let focus = active * (1.0 - dist).powf(1.8);
    let distance_opacity = 0.68 + 0.32 * depth.powf(1.9);
    let mut opacity = (0.35 + 0.65 * depth.powf(1.5) * (0.35 + 0.65 * focus)) * distance_opacity;
    opacity *= timeline::clamp(2.0 - line_diff, 0.0, 1.0);
    if line_index < center {
        opacity *= (1.0 - line_diff).max(0.0);
    }
    opacity *= f64::from(length_fade);
    if line_index > center {
        opacity *= 0.18 + 0.82 * f64::from(upcoming);
    }
    if line_diff > 0.02 {
        let progress = timeline::clamp((line_diff - 0.3) / 0.6, 0.0, 1.0);
        let threshold = 1.0 - 1.2 * progress;
        if local_cos > threshold {
            opacity *= timeline::clamp(1.0 - (local_cos - threshold) / 0.15, 0.0, 1.0);
        }
    }
    let scale = (0.22 + 0.98 * depth.powf(1.5))
        * (1.0 + f64::from(focus_scale) * focus)
        * f64::from(length_scale)
        * item.arc_scale;
    let rotation = timeline::clamp(tangent * (0.4 + 0.6 * depth), -38.0, 38.0);
    Glyph { x: x as f32, y: y as f32, opacity: opacity as f32, scale: scale as f32, rotation: rotation as f32 }
}

fn readable_angle(mut degrees: f64) -> f64 {
    while degrees > 90.0 {
        degrees -= 180.0;
    }
    while degrees < -90.0 {
        degrees += 180.0;
    }
    degrees
}

pub fn length_factors(lines: &[Line], line_index: usize, center: usize, line_diff: f64) -> (f32, f32) {
    if line_index <= center {
        return (1.0, 1.0);
    }
    let current = lines.get(center).map(visual_of).unwrap_or(0.0);
    if current <= 10.0 {
        return (1.0, 1.0);
    }
    let target = lines.get(line_index).map(visual_of).unwrap_or(0.0);
    let fade = timeline::clamp((current - 10.0) / 8.0, 0.0, 1.0);
    let target_strength = timeline::clamp((target - 5.0) / 5.0, 0.4, 1.0);
    let combined = fade * target_strength;
    let progress = timeline::clamp((line_diff - 0.4) / 0.5, 0.0, 1.0);
    let opacity = 1.0 - (1.0 - (1.0 - combined)) * progress;
    let scale = 1.0 - (1.0 - (1.0 - combined * 0.25)) * progress;
    (opacity as f32, scale as f32)
}

fn visual_of(line: &Line) -> f64 {
    timeline::visual_length(&line.text)
}

pub fn entry_progress(line: &Line, time: f64, upcoming: bool) -> f32 {
    if !upcoming {
        return 0.0;
    }
    timeline::clamp((time - (line.start - ENTRY_LEAD)) / ENTRY_LEAD, 0.0, 1.0) as f32
}

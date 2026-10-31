// Cadenza: one active line broken into words and scattered around a larger hero word.

mod envelopes;
mod place;

use std::rc::Rc;

use folia_core::{AnimationIntensity, Line};
use iced::widget::canvas::{Frame, Text};
use iced::{Point, Size, Vector};

use crate::styles::glow::scale_around;
use crate::{Scene, color};

pub use envelopes::{Motion, Status, Timing, WordMotion, glow_envelope, line_fade, motion, status, timing_of};
pub use place::{Placement, font_px, layout};

pub struct LineLayout {
    placements: Vec<Placement>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Key {
    start: u64,
    width: u32,
    height: u32,
    font: u32,
    intensity: u8,
}

#[derive(Default)]
pub struct Cache {
    entries: Vec<(Key, Rc<LineLayout>)>,
}

impl Cache {
    pub fn layout(
        &mut self,
        line: &Line,
        size: Size,
        font_scale: f32,
        intensity: AnimationIntensity,
    ) -> Rc<LineLayout> {
        let key = Key {
            start: line.start.to_bits(),
            width: size.width.to_bits(),
            height: size.height.to_bits(),
            font: font_scale.to_bits(),
            intensity: match intensity {
                AnimationIntensity::Calm => 0,
                AnimationIntensity::Normal => 1,
                AnimationIntensity::Chaotic => 2,
            },
        };
        if let Some((_, hit)) = self.entries.iter().find(|(held, _)| *held == key) {
            return Rc::clone(hit);
        }
        let layout = Rc::new(LineLayout { placements: layout(line, size.width, size.height, font_scale, intensity) });
        if self.entries.len() == 4 {
            self.entries.remove(0);
        }
        self.entries.push((key, Rc::clone(&layout)));
        layout
    }
}

/// Draws the active line. Beat energy is 0, so the per-word float stays still; the cloud still lifts.
pub fn draw(scene: &Scene<'_>, cache: &mut Cache, frame: &mut Frame, size: Size) {
    let Some(index) = folia_lyrics::active_line(&scene.lyrics.lines, scene.time) else {
        return;
    };
    let line = &scene.lyrics.lines[index];
    let layout = cache.layout(line, size, scene.font_scale, scene.theme.intensity);
    let timing = timing_of(line);
    let (opacity, line_scale) = line_fade(scene.time, line, &timing);
    if opacity <= 0.01 {
        return;
    }
    let origin = Point::new(size.width / 2.0, size.height * 0.42);
    let lift = (scene.time * 2.3).sin() * 3.0;
    frame.with_save(|frame| {
        scale_around(frame, origin, line_scale, 0.0);
        let px = font_px(size.width, line.text.chars().count(), line.words.len().max(1), scene.font_scale);
        for bucket in buckets(&layout.placements, scene.time, &timing) {
            for index in bucket {
                let ctx = Paint { scene, timing: &timing, size, lift, opacity, line_start: line.start, font_px: px };
                paint(frame, &layout.placements[index], &ctx);
            }
        }
    });
}

fn buckets(placements: &[Placement], time: f64, timing: &Timing) -> [Vec<usize>; 3] {
    let mut order = [Vec::new(), Vec::new(), Vec::new()];
    for (index, placement) in placements.iter().enumerate() {
        let bucket = match status(time, timing, placement.start, placement.end) {
            Status::Waiting => 0,
            Status::Passed => 1,
            Status::Active => 2,
        };
        order[bucket].push(index);
    }
    order
}

struct Paint<'a> {
    scene: &'a Scene<'a>,
    timing: &'a Timing,
    size: Size,
    lift: f64,
    opacity: f64,
    line_start: f64,
    font_px: f32,
}

fn paint(frame: &mut Frame, placement: &Placement, paint: &Paint<'_>) {
    let Paint { scene, timing, size, lift, opacity, line_start, font_px } = *paint;
    let posed = motion(
        &WordMotion {
            scale: placement.scale,
            passed_rotate: placement.passed_rotate,
            drift_x: placement.passed_drift_x,
            drift_y: placement.passed_drift_y,
            start: placement.start,
            end: placement.end,
            line_start,
            motion_amount: envelopes::MOTION,
        },
        scene.time,
        timing,
        scene.theme.intensity,
    );
    if posed.alpha * opacity < 0.015 && posed.glow * opacity < 0.015 {
        return;
    }
    let left = f64::from(size.width) / 2.0 + placement.x + posed.drift_x;
    let baseline = f64::from(size.height) * 0.42 + lift + placement.y + posed.drift_y;
    let anchor = Point::new((left + placement.width / 2.0) as f32, (baseline - placement.height * 0.42) as f32);
    frame.with_save(|frame| {
        frame.translate(Vector::new(anchor.x, anchor.y));
        frame.rotate((posed.rotation as f32).to_radians());
        frame.scale(posed.scale as f32);
        paint_glow(frame, scene, placement, timing, posed.glow * opacity, font_px);
        let body = scene.theme.primary.mix(scene.theme.accent, posed.mix as f32);
        fill(frame, &placement.text, 0.0, color(body, (posed.alpha * opacity) as f32), font_px);
    });
}

fn paint_glow(frame: &mut Frame, scene: &Scene<'_>, placement: &Placement, timing: &Timing, glow: f64, font_px: f32) {
    if glow <= 0.02 {
        return;
    }
    let split = timing.hints.reveal == folia_lyrics::Reveal::Normal && !placement.cjk && placement.glyphs.len() > 1;
    let tint = color(scene.theme.accent, 1.0);
    if !split {
        frame.with_save(|frame| {
            frame.scale(1.08_f32);
            fill(frame, &placement.text, 0.0, fade(tint, glow * 0.55), font_px);
        });
        return;
    }
    for (index, glyph) in placement.glyphs.iter().enumerate() {
        let next = placement.glyphs.get(index + 1).map(|next| next.offset);
        let width = next.unwrap_or(placement.width as f32) - glyph.offset;
        let intensity = envelopes::char_glow(scene.time, placement.end, glyph.start, glyph.end);
        let x = glyph.offset + width / 2.0 - placement.width as f32 / 2.0;
        fill(frame, &glyph.text, x, fade(tint, glow * intensity * 0.7 * envelopes::GLOW), font_px);
    }
}

fn fade(mut tint: iced::Color, alpha: f64) -> iced::Color {
    tint.a = (alpha as f32).clamp(0.0, 1.0);
    tint
}

fn fill(frame: &mut Frame, text: &str, x: f32, color: iced::Color, font_px: f32) {
    frame.fill_text(Text {
        content: text.to_owned(),
        position: Point::new(x, 0.0),
        color,
        size: font_px.into(),
        font: crate::BOLD,
        align_x: iced::widget::text::Alignment::Center,
        align_y: iced::alignment::Vertical::Center,
        shaping: iced::widget::text::Shaping::Advanced,
        ..Text::default()
    });
}

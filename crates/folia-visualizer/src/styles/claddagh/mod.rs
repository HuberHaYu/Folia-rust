// Claddagh: graphemes on a tilted ellipse. A frame is a function of the playback time.

mod layout;
mod timeline;

use std::rc::Rc;

use folia_core::Theme;
use iced::widget::canvas::{Frame, Text};
use iced::{Point, Size, Vector};

use crate::styles::claddagh::layout::{Glyph, Placed};
use crate::{LayoutCache, Scene, color};

pub use layout::{RingMoment, glyph, placed, ring_angle};
pub use timeline::{adjust, line_marks, visual_length, word_offset};

const FONT_PX: f32 = 72.0;
const FOCUS: f32 = 0.65;
const TILT_DEG: f32 = 45.0;
const RADIUS_SCALE: f32 = 1.0;

pub struct LineLayout {
    placed: Vec<Placed>,
}

/// Draws the ring for `scene.time`. Beat power is 0: audio bands are not wired yet.
pub fn draw(scene: &Scene<'_>, cache: &mut LayoutCache<LineLayout>, frame: &mut Frame, size: Size) {
    let lines = &scene.lyrics.lines;
    let Some(center) = layout::center_line(lines, scene.time) else {
        return;
    };
    let font_px = FONT_PX * scene.font_scale;
    let rx = (size.width * 0.44).min(560.0) * RADIUS_SCALE;
    let ring = ring_angle(lines, scene.time);
    draw_axis(frame, size, scene.theme);
    let first = center.saturating_sub(1);
    let last = (center + 2).min(lines.len().saturating_sub(1));
    let ctx = DrawCtx { scene, size, center, rx, font_px, ring };
    for index in first..=last {
        draw_line(&ctx, cache, frame, index);
    }
}

struct DrawCtx<'a> {
    scene: &'a Scene<'a>,
    size: Size,
    center: usize,
    rx: f32,
    font_px: f32,
    ring: f64,
}

fn draw_axis(frame: &mut Frame, size: Size, theme: &Theme) {
    let angle = (90.0 - TILT_DEG).to_radians();
    frame.with_save(|frame| {
        frame.translate(Vector::new(size.width / 2.0, size.height / 2.0));
        frame.rotate(angle);
        frame.fill_rectangle(Point::new(-150.0, -2.0), Size::new(300.0, 4.0), color(theme.primary, 0.2));
    });
}

fn draw_line(ctx: &DrawCtx<'_>, cache: &mut LayoutCache<LineLayout>, frame: &mut Frame, index: usize) {
    let DrawCtx { scene, size, center, rx, font_px, ring } = *ctx;
    let line = &scene.lyrics.lines[index];
    let layout = cache.get_or_insert(line, size, scene.font_scale, || {
        Rc::new(LineLayout { placed: layout::placed(line, font_px, rx, 1.0, 0.0) })
    });
    let active = cache.get_or_insert(&scene.lyrics.lines[center], size, scene.font_scale, || {
        Rc::new(LineLayout { placed: layout::placed(&scene.lyrics.lines[center], font_px, rx, 1.0, 0.0) })
    });
    let end = layout::render_end(&scene.lyrics.lines, center);
    let own_end = layout::render_end(&scene.lyrics.lines, index);
    let active_offset = layout::sung_offset(&active.placed, scene.time, end);
    let own_offset = layout::sung_offset(&layout.placed, scene.time, own_end);
    let progress = layout::playback_progress(&active.placed, scene.time, end);
    let shift = layout::word_shift(index, center, own_offset, active_offset, progress, ring);
    let line_diff = (ring - center as f64 * std::f64::consts::PI).abs() / std::f64::consts::PI;
    let (fade, length_scale) = layout::length_factors(&scene.lyrics.lines, index, center, line_diff);
    let upcoming = layout::entry_progress(line, scene.time, index > center);
    let origin = Point::new(size.width / 2.0, size.height / 2.0);
    for (glyph_index, item) in layout.placed.iter().enumerate() {
        let pose = layout::glyph(
            item,
            glyph_index,
            &RingMoment {
                line_index: index,
                center,
                ring,
                word_offset: shift,
                rx,
                focus_scale: FOCUS,
                tilt_deg: TILT_DEG,
                power: 0.0,
                chorus: false,
                length_fade: fade,
                length_scale,
                upcoming,
                font_px,
            },
        );
        paint(frame, origin, item, pose, font_px, scene);
    }
}

fn paint(frame: &mut Frame, origin: Point, item: &Placed, pose: Glyph, font_px: f32, scene: &Scene<'_>) {
    if pose.opacity <= 0.004 || pose.scale <= 0.01 {
        return;
    }
    let sung = sung_amount(item, scene.time);
    let body = if sung >= 1.0 { scene.theme.accent } else { scene.theme.primary.mix(scene.theme.accent, sung) };
    let alpha = (0.55 + 0.45 * sung) * pose.opacity;
    frame.with_save(|frame| {
        frame.translate(Vector::new(origin.x + pose.x, origin.y + pose.y));
        frame.rotate(pose.rotation.to_radians());
        frame.scale(pose.scale);
        frame.fill_text(Text {
            content: item.text.clone(),
            position: Point::ORIGIN,
            color: color(body, alpha),
            size: font_px.into(),
            font: iced::Font { weight: iced::font::Weight::Bold, ..iced::Font::DEFAULT },
            align_x: iced::widget::text::Alignment::Center,
            align_y: iced::alignment::Vertical::Center,
            shaping: iced::widget::text::Shaping::Advanced,
            ..Text::default()
        });
    });
}

fn sung_amount(item: &Placed, time: f64) -> f32 {
    if time >= item.end {
        1.0
    } else if time > item.start {
        let duration = item.end - item.start;
        let progress = if duration > 0.0 { (time - item.start) / duration } else { 1.0 };
        (progress * 15.0).min(1.0) as f32
    } else {
        0.0
    }
}

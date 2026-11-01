// First-screen splash from index.html `#app-splash` and `src/utils/appSplash.ts`.
// The cover fades once a frame has been painted after the library result; frames stop with it.

use std::time::Instant;

use folia_visualizer::Ease;
use iced::font::{Family, Weight};
use iced::widget::canvas::{self, Frame, Path, Text};
use iced::{Color, Font, Point, Rectangle, Renderer, Size, Vector, mouse};

/// CSS `ease`: `cubic-bezier(0.25, 0.1, 0.25, 1)`.
const EASE: Ease = Ease::new(0.25, 0.1, 0.25, 1.0);
/// CSS `ease-in-out` on the wave keyframes.
const WAVE: Ease = Ease::IN_OUT;

const FADE_IN_DELAY_MS: f64 = 180.0;
const FADE_IN_MS: f64 = 420.0;
const FADE_OUT_MS: f64 = 320.0;
const LETTER_BEAT_MS: f64 = 380.0;
const BARS: usize = 14;
const LETTERS: [char; 5] = ['F', 'o', 'l', 'i', 'a'];
const GAP: f32 = 28.0;
const BACKGROUND: Color = Color::from_rgb8(0x09, 0x09, 0x0b);
/// `randomGlow`'s third tier also has a 70–110px ring. Two rings keep one glyph size in the atlas.
const GLOW_LAYERS: usize = 6;
/// BASE, then the five generic families. `system-ui` is Segoe UI, not the sans-serif fallback.
const FAMILIES: [Family; 6] =
    [Family::SansSerif, Family::Serif, Family::Monospace, Family::Cursive, Family::Fantasy, Family::Name("Segoe UI")];

#[derive(Clone, Copy)]
struct Bar {
    peak: f64,
    floor: f64,
    width: f32,
    alpha: f64,
    glow: f32,
    duration_ms: f64,
    delay_ms: f64,
}

#[derive(Clone, Copy)]
struct Face {
    family: u8,
    weight: u8,
    italic: bool,
    scale_x: f64,
    scale_y: f64,
    /// Blur radius in px and the ring alpha, before the eight-way split. A zero radius is unused.
    rings: [(f32, f32); 2],
}

impl Face {
    const fn base() -> Face {
        Face { family: 0, weight: 1, italic: false, scale_x: 1.0, scale_y: 1.0, rings: [(8.0, 0.55), (28.0, 0.3)] }
    }
}

/// Opacity of the word and the wave, from the first painted frame.
/// A leave that starts before 180ms keeps the content hidden for the whole cover.
fn content_opacity(elapsed_ms: f64, leave_ms: Option<f64>) -> f64 {
    if leave_ms.is_some_and(|leave| leave < FADE_IN_DELAY_MS) {
        return 0.0;
    }
    if elapsed_ms <= FADE_IN_DELAY_MS {
        0.0
    } else {
        EASE.at(((elapsed_ms - FADE_IN_DELAY_MS) / FADE_IN_MS).clamp(0.0, 1.0))
    }
}

/// Opacity of the whole cover. It holds until the library result has been painted, then fades over 320ms.
fn cover_opacity(elapsed_ms: f64, leave_ms: Option<f64>) -> f64 {
    let Some(leave) = leave_ms else {
        return 1.0;
    };
    if elapsed_ms <= leave { 1.0 } else { 1.0 - EASE.at(((elapsed_ms - leave) / FADE_OUT_MS).clamp(0.0, 1.0)) }
}

/// `scaleY` of one bar. `delay_ms` is negative, the same as a CSS animation delay.
fn bar_scale(floor: f64, peak: f64, duration_ms: f64, delay_ms: f64, elapsed_ms: f64) -> f64 {
    let duration = duration_ms.max(1.0);
    let shifted = elapsed_ms - delay_ms;
    let wrapped = shifted.rem_euclid(duration);
    let local = wrapped / duration;
    if local < 0.5 {
        floor + (peak - floor) * WAVE.at(local / 0.5)
    } else {
        peak + (floor - peak) * WAVE.at((local - 0.5) / 0.5)
    }
}

/// How many letters change on this beat, and which ones. One letter, or two when the roll is under 0.25.
fn swaps_on_beat(seed: u64, beat: u64) -> [Option<usize>; 2] {
    let count = if unit(seed, beat, 0) < 0.25 { 2 } else { 1 };
    let mut picks = [None, None];
    for (slot, pick_slot) in picks.iter_mut().enumerate().take(count) {
        *pick_slot = Some(pick(seed, beat, slot as u64, LETTERS.len()));
    }
    picks
}

fn face_at(seed: u64, index: usize, elapsed_ms: f64) -> Face {
    let mut face = Face::base();
    let beats = (elapsed_ms / LETTER_BEAT_MS).floor().max(0.0) as u64;
    for beat in 0..beats {
        for (slot, pick) in swaps_on_beat(seed, beat).into_iter().enumerate() {
            if pick == Some(index) {
                face = roll_face(seed, beat, slot as u64);
            }
        }
    }
    face
}

fn bars_from(seed: u64) -> [Bar; BARS] {
    let mut bars =
        [Bar { peak: 1.0, floor: 0.18, width: 5.0, alpha: 0.85, glow: 14.0, duration_ms: 1100.0, delay_ms: 0.0 }; BARS];
    for (index, bar) in bars.iter_mut().enumerate() {
        let envelope = (((index as f64 + 0.5) / BARS as f64) * std::f64::consts::PI).sin();
        let span = lerp(0.38, 0.72, unit(seed, index as u64, 1));
        bar.peak = (0.32 + envelope * span).min(1.0);
        bar.floor = lerp(0.08, 0.24, unit(seed, index as u64, 2));
        bar.width = lerp(3.5, 6.5, unit(seed, index as u64, 3)) as f32;
        bar.alpha = lerp(0.55, 1.0, unit(seed, index as u64, 4));
        bar.glow = lerp(6.0, 26.0, unit(seed, index as u64, 5)).round() as f32;
        bar.duration_ms = lerp(820.0, 1500.0, unit(seed, index as u64, 6)).round();
        bar.delay_ms = -lerp(0.0, 1500.0, unit(seed, index as u64, 7)).round();
    }
    bars
}

fn roll_face(seed: u64, beat: u64, slot: u64) -> Face {
    let weights = [0u8, 1, 2];
    let scales = [0.86, 0.94, 1.0, 1.06, 1.12];
    let size = lerp(0.86, 1.12, unit(seed, beat, 10 + slot));
    Face {
        family: (unit(seed, beat, 30 + slot) * FAMILIES.len() as f64).floor() as u8,
        weight: weights[(unit(seed, beat, 40 + slot) * weights.len() as f64).floor() as usize],
        italic: unit(seed, beat, 50 + slot) < 0.25,
        scale_x: size * scales[(unit(seed, beat, 60 + slot) * scales.len() as f64).floor() as usize],
        scale_y: size,
        rings: glow_rings(seed, beat, slot, unit(seed, beat, 20 + slot)),
    }
}

/// One or two blur radii from `randomGlow`. The third tier drops its 70–110px ring.
fn glow_rings(seed: u64, beat: u64, slot: u64, roll: f64) -> [(f32, f32); 2] {
    let radius = |channel: u64, from: f64, to: f64| lerp(from, to, unit(seed, beat, channel + slot)) as f32;
    if roll < 0.3 {
        [(radius(80, 2.0, 6.0), 0.35), (0.0, 0.0)]
    } else if roll < 0.75 {
        [(radius(80, 6.0, 14.0), 0.7), (radius(90, 20.0, 36.0), 0.35)]
    } else {
        [(radius(80, 10.0, 18.0), 0.95), (radius(90, 34.0, 54.0), 0.7)]
    }
}

fn pick(seed: u64, beat: u64, slot: u64, len: usize) -> usize {
    (unit(seed, beat, 70 + slot) * len as f64).floor() as usize
}

fn unit(seed: u64, a: u64, b: u64) -> f64 {
    let mixed = seed.wrapping_add(a.wrapping_mul(0x9E37)).wrapping_mul(0xBF58).wrapping_add(b.wrapping_mul(0x94D0));
    let x = mixed.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    (x >> 11) as f64 / (1u64 << 53) as f64
}

fn lerp(from: f64, to: f64, t: f64) -> f64 {
    from + (to - from) * t
}

fn millis(later: Instant, earlier: Instant) -> f64 {
    later.saturating_duration_since(earlier).as_secs_f64() * 1000.0
}

pub(crate) struct Splash {
    seed: u64,
    bars: [Bar; BARS],
    /// Measured only if the word may appear. A fast library result never loads these faces.
    widths: Option<[f32; 5]>,
    started: Option<Instant>,
    leave: Option<Instant>,
    library_seen: bool,
}

impl Splash {
    pub(crate) fn new() -> Splash {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|time| time.as_nanos() as u64)
            .unwrap_or(1);
        Splash { seed, bars: bars_from(seed), widths: None, started: None, leave: None, library_seen: false }
    }

    /// The home page received its first library result. Success and failure both count, and only the first one.
    pub(crate) fn library_ready(&mut self) {
        self.library_seen = true;
    }

    /// One painted frame. The clock starts here; the fade starts on the first frame after the library result.
    pub(crate) fn on_frame(&mut self, now: Instant) {
        if self.started.is_none() {
            self.started = Some(now);
        }
        if self.library_seen && self.leave.is_none() {
            self.leave = Some(now);
        }
        let content_possible = match (self.started, self.leave) {
            (Some(start), Some(leave)) => millis(leave, start) >= FADE_IN_DELAY_MS,
            (Some(_), None) => true,
            (None, _) => false,
        };
        if content_possible && self.widths.is_none() {
            self.widths = Some(letter_widths());
        }
    }

    pub(crate) fn showing(&self, now: Instant) -> bool {
        let (elapsed, leave) = self.sample(now);
        leave.is_none_or(|leave| elapsed < leave + FADE_OUT_MS)
    }

    /// Cover color, including the fade. The fast path paints this without a canvas.
    pub(crate) fn cover_color(&self, now: Instant) -> Color {
        let (elapsed, leave) = self.sample(now);
        Color { a: cover_opacity(elapsed, leave) as f32, ..BACKGROUND }
    }

    /// Word and wave opacity. Zero when the library was ready before 180ms, so no glyphs are built.
    pub(crate) fn word(&self, now: Instant) -> f32 {
        let (elapsed, leave) = self.sample(now);
        (content_opacity(elapsed, leave) * cover_opacity(elapsed, leave)) as f32
    }

    /// Opaque cover eats clicks. The fade lets them through, matching `pointer-events: none`.
    pub(crate) fn blocks_pointer(&self) -> bool {
        self.leave.is_none()
    }

    pub(crate) fn program(&self, now: Instant) -> SplashProgram<'_> {
        let (elapsed_ms, leave_ms) = self.sample(now);
        SplashProgram { splash: self, elapsed_ms, leave_ms }
    }

    fn sample(&self, now: Instant) -> (f64, Option<f64>) {
        let Some(start) = self.started else {
            return (0.0, None);
        };
        (millis(now, start), self.leave.map(|leave| millis(leave, start)))
    }
}

fn letter_widths() -> [f32; 5] {
    let mut widths = [0.7; 5];
    for (index, letter) in LETTERS.iter().enumerate() {
        let text = letter.to_string();
        let mut widest = 0.0_f32;
        for family in FAMILIES {
            for italic in [false, true] {
                let font = face_font(family, Weight::ExtraBold, italic);
                widest = widest.max(folia_visualizer::measure(&text, 100.0, font, 10_000.0).width);
            }
        }
        if widest > 0.0 {
            widths[index] = (widest / 100.0) * 1.26 + 0.04;
        }
    }
    widths
}

fn face_font(family: Family, weight: Weight, italic: bool) -> Font {
    let style = if italic { iced::font::Style::Italic } else { iced::font::Style::Normal };
    crate::typeface::with(Font { family, style, ..Font::DEFAULT }, weight)
}

pub(crate) struct SplashProgram<'a> {
    splash: &'a Splash,
    elapsed_ms: f64,
    leave_ms: Option<f64>,
}

impl canvas::Program<super::Message> for SplashProgram<'_> {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let cover = cover_opacity(self.elapsed_ms, self.leave_ms) as f32;
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), Color { a: cover, ..BACKGROUND });
        let content = (content_opacity(self.elapsed_ms, self.leave_ms) * f64::from(cover)) as f32;
        if content > 0.004 {
            draw_mark(&mut frame, self.splash, bounds.size(), self.elapsed_ms, content);
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(&self, _state: &(), _bounds: Rectangle, _cursor: mouse::Cursor) -> mouse::Interaction {
        if self.leave_ms.is_some() { mouse::Interaction::None } else { mouse::Interaction::Idle }
    }
}

fn draw_mark(frame: &mut Frame, splash: &Splash, size: Size, elapsed_ms: f64, alpha: f32) {
    let font_px = (size.width * 0.17).clamp(64.0, 200.0);
    let wave_h = (size.width * 0.075).clamp(42.0, 92.0);
    let gap = (size.width * 0.011).clamp(6.0, 11.0);
    let tracking = font_px * 0.06;
    let Some(widths) = splash.widths else {
        return;
    };
    let mut boxes = [0.0_f32; 5];
    for (index, em) in widths.iter().enumerate() {
        boxes[index] = em * font_px;
    }
    let title_w = boxes.iter().sum::<f32>() + tracking * (boxes.len().saturating_sub(1) as f32);
    let wave_w = splash.bars.iter().map(|bar| bar.width).sum::<f32>() + gap * (BARS.saturating_sub(1) as f32);
    let block_h = font_px + GAP + wave_h;
    let top = (size.height - block_h) / 2.0;
    let mut x = (size.width - title_w) / 2.0 + tracking;
    for (index, letter) in LETTERS.iter().enumerate() {
        let face = face_at(splash.seed, index, elapsed_ms);
        draw_letter(frame, *letter, face, Point::new(x, top), boxes[index], font_px, alpha);
        x += boxes[index] + tracking;
    }
    let mut bar_x = (size.width - wave_w) / 2.0;
    let bar_top = top + font_px + GAP;
    for bar in &splash.bars {
        let scale = bar_scale(bar.floor, bar.peak, bar.duration_ms, bar.delay_ms, elapsed_ms) as f32;
        let height = wave_h * scale;
        let y = bar_top + (wave_h - height) / 2.0;
        draw_bar(frame, Point::new(bar_x, y), Size::new(bar.width, height), bar.glow, alpha, bar.alpha as f32);
        bar_x += bar.width + gap;
    }
}

fn draw_letter(frame: &mut Frame, letter: char, face: Face, origin: Point, width: f32, font_px: f32, alpha: f32) {
    let pivot = Point::new(origin.x + width / 2.0, origin.y + font_px * 0.78);
    let center = Point::new(0.0, font_px * (0.5 - 0.78));
    frame.with_save(|frame| {
        frame.translate(Vector::new(pivot.x, pivot.y));
        // Uniform scales other than 1 are rasterized into the glyph atlas, which never shrinks.
        let mut scale_x = face.scale_x as f32;
        let scale_y = face.scale_y as f32;
        if (scale_x - scale_y).abs() < 0.0001 && (scale_x - 1.0).abs() > 0.0001 {
            scale_x *= 1.001;
        }
        if (scale_x - 1.0).abs() > 0.0001 || (scale_y - 1.0).abs() > 0.0001 {
            frame.scale_nonuniform(Vector::new(scale_x, scale_y));
        }
        // Eight copies on the blur radius alone leave ghosts, so each radius is also stamped at 1/3 and 2/3.
        for (radius, ring_alpha) in face.rings {
            if radius <= 0.0 || ring_alpha <= 0.0 {
                continue;
            }
            let copy_alpha = alpha * ring_alpha / 24.0;
            if copy_alpha <= 0.004 {
                continue;
            }
            for step in 0..8 {
                let angle = step as f32 * std::f32::consts::FRAC_PI_4;
                let (sin, cos) = angle.sin_cos();
                for band in [radius / 3.0, radius * 2.0 / 3.0, radius] {
                    let at = Point::new(center.x + band * cos, center.y + band * sin);
                    fill_letter(frame, letter, face, at, font_px, copy_alpha);
                }
            }
        }
        fill_letter(frame, letter, face, center, font_px, alpha);
    });
}

/// Soft stand-in for `box-shadow: 0 0 var(--glow) rgba(255,255,255,0.35)`.
fn draw_bar(frame: &mut Frame, at: Point, size: Size, glow: f32, content: f32, bar_alpha: f32) {
    const SIGMA: f32 = 0.45;
    let mut weights = [0.0_f32; GLOW_LAYERS];
    let mut sum = 0.0_f32;
    for (index, weight) in weights.iter_mut().enumerate() {
        let distance = (index as f32 + 1.0) / GLOW_LAYERS as f32;
        let z = distance / SIGMA;
        *weight = (-0.5 * z * z).exp();
        sum += *weight;
    }
    for index in (0..GLOW_LAYERS).rev() {
        let spread = glow * (index as f32 + 1.0) / GLOW_LAYERS as f32;
        let layer = content * 0.35 * weights[index] / sum;
        if layer <= 0.004 {
            continue;
        }
        let path = Path::rounded_rectangle(
            Point::new(at.x - spread, at.y - spread),
            Size::new(size.width + spread * 2.0, size.height + spread * 2.0),
            iced::border::Radius::from(spread),
        );
        frame.fill(&path, Color { a: layer, ..Color::WHITE });
    }
    frame.fill_rectangle(at, size, Color { a: content * bar_alpha, ..Color::WHITE });
}

fn fill_letter(frame: &mut Frame, letter: char, face: Face, at: Point, font_px: f32, alpha: f32) {
    if alpha <= 0.004 {
        return;
    }
    let family = FAMILIES.get(usize::from(face.family)).copied().unwrap_or(Family::SansSerif);
    let weight = match face.weight {
        0 => Weight::Normal,
        2 => Weight::ExtraBold,
        _ => Weight::Semibold,
    };
    frame.fill_text(Text {
        content: letter.to_string(),
        position: at,
        color: Color { a: alpha, ..Color::WHITE },
        size: font_px.into(),
        font: face_font(family, weight, face.italic),
        align_x: iced::widget::text::Alignment::Center,
        align_y: iced::alignment::Vertical::Center,
        shaping: iced::widget::text::Shaping::Advanced,
        ..Text::default()
    });
}

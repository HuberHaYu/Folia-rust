// Pure timing for the now-playing chrome, the capsule hover, and the subtitle rows.

use folia_core::Line;

/// How long a real pointer move keeps the chrome up.
pub const CHROME_HOLD_MS: u64 = 3_000;
/// How long the chrome stays after the pointer leaves the window.
pub const CHROME_LEAVE_MS: u64 = 300;
/// Hover time before the capsule expands, from the original 20ms delay.
pub const CAPSULE_EXPAND_MS: u64 = 20;
/// Time outside the capsule before it collapses, from the original 100ms delay.
pub const CAPSULE_COLLAPSE_MS: u64 = 100;
/// Expand/collapse animation, from the original 0.26s ease-out.
pub const CAPSULE_FADE_MS: u64 = 260;
/// Widest expanded capsule, the original `32rem`.
pub const CAPSULE_MAX: f32 = 512.0;

/// Deadline for the now-playing chrome. `until_ms` is `None` once it has hidden.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChromeClock {
    pub until_ms: Option<u64>,
    pub pointer: Option<(f32, f32)>,
}

impl ChromeClock {
    /// Shown from `now_ms` for one hold period. Used when the player page opens.
    pub fn shown(now_ms: u64) -> Self {
        Self { until_ms: Some(now_ms.saturating_add(CHROME_HOLD_MS)), pointer: None }
    }

    pub fn visible(self, now_ms: u64, held: bool) -> bool {
        held || self.until_ms.is_some_and(|until| now_ms < until)
    }

    /// A pointer sample. The same coordinates do not extend the deadline: a layout change
    /// can resend the last position when the chrome hides, and that must not show it again.
    pub fn pointer(self, now_ms: u64, x: f32, y: f32) -> Self {
        let moved = self.pointer != Some((x, y));
        let mut next = self;
        next.pointer = Some((x, y));
        if moved {
            next.until_ms = Some(now_ms.saturating_add(CHROME_HOLD_MS));
        }
        next
    }

    /// A click, a wheel, or entering the page. Extends the deadline without moving the pointer.
    pub fn reveal(self, now_ms: u64) -> Self {
        Self { until_ms: Some(now_ms.saturating_add(CHROME_HOLD_MS)), ..self }
    }

    pub fn leave(self, now_ms: u64, held: bool) -> Self {
        let mut next = self;
        next.pointer = None;
        if !held {
            next.until_ms = Some(now_ms.saturating_add(CHROME_LEAVE_MS));
        }
        next
    }

    /// Drops the deadline once `now_ms` has reached it. A held pointer keeps the chrome up.
    pub fn tick(self, now_ms: u64, held: bool) -> Self {
        if held || self.until_ms.is_none_or(|until| now_ms < until) {
            return self;
        }
        Self { until_ms: None, ..self }
    }
}

/// Hover state of the floating capsule. `since_ms` is when `inside` last changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapsuleHover {
    pub expanded: bool,
    pub inside: bool,
    pub since_ms: u64,
}

impl CapsuleHover {
    pub fn collapsed(now_ms: u64) -> Self {
        Self { expanded: false, inside: false, since_ms: now_ms }
    }

    pub fn set_inside(self, now_ms: u64, inside: bool) -> Self {
        if inside == self.inside {
            return self;
        }
        Self { inside, since_ms: now_ms, expanded: self.expanded }
    }

    /// Applies the 20ms expand delay and the 100ms collapse delay.
    pub fn step(self, now_ms: u64) -> Self {
        let elapsed = now_ms.saturating_sub(self.since_ms);
        if self.inside && !self.expanded && elapsed >= CAPSULE_EXPAND_MS {
            return Self { expanded: true, ..self };
        }
        if !self.inside && self.expanded && elapsed >= CAPSULE_COLLAPSE_MS {
            return Self { expanded: false, ..self };
        }
        self
    }
}

/// Width animation of the capsule. `value` is the linear progress at `now_ms`, before easing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CapsuleFade {
    pub from: f32,
    pub to: f32,
    pub started_ms: u64,
}

impl CapsuleFade {
    pub fn settled(value: f32, now_ms: u64) -> Self {
        Self { from: value, to: value, started_ms: now_ms }
    }

    /// Starts a new 260ms run when `to` changes. The same target leaves the clock alone.
    pub fn retarget(self, now_ms: u64, to: f32) -> Self {
        if (self.to - to).abs() < 0.001 {
            return self;
        }
        Self { from: self.value(now_ms), to, started_ms: now_ms }
    }

    pub fn value(self, now_ms: u64) -> f32 {
        let span = self.to - self.from;
        if span.abs() < 0.001 {
            return self.to;
        }
        let t = (now_ms.saturating_sub(self.started_ms) as f32 / CAPSULE_FADE_MS as f32).clamp(0.0, 1.0);
        self.from + span * t
    }

    pub fn running(self, now_ms: u64) -> bool {
        (self.value(now_ms) - self.to).abs() > 0.001
    }
}

/// Hover opens the capsule. A track that is loaded and not playing stays open, as the original does.
pub fn capsule_open(hovered: bool, playing: bool, has_duration: bool) -> bool {
    hovered || (has_duration && !playing)
}

/// Cubic ease-out over the unit interval, standing in for the original `easeOut` curve.
pub fn ease_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t) * (1.0 - t) * (1.0 - t)
}

/// Capsule width. At 768px and wider the collapsed bar is 60% of the expanded one, otherwise 80%.
pub fn capsule_width(window_width: f32, openness: f32) -> f32 {
    let full = (window_width - 32.0).clamp(280.0, CAPSULE_MAX);
    let factor = if window_width >= 768.0 { 0.6 } else { 0.8 };
    let collapsed = full * factor;
    collapsed + (full - collapsed) * openness.clamp(0.0, 1.0)
}

/// A string the subtitle overlay will draw. Markers such as `......` are not readable.
pub fn readable(text: &str) -> bool {
    text.chars().any(|ch| ch.is_alphanumeric())
}

/// What the bottom subtitle shows at `time`: the current line's translation, and the next readable line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Subtitle<'a> {
    pub translation: Option<&'a str>,
    pub upcoming: Option<&'a str>,
}

pub fn subtitle_at(lines: &[Line], time: f64) -> Subtitle<'_> {
    let Some(index) = lines.iter().rposition(|line| line.start <= time) else {
        return Subtitle { translation: None, upcoming: lines.iter().find_map(|line| line_text(&line.text)) };
    };
    Subtitle {
        translation: lines[index].translation.as_deref().and_then(line_text),
        upcoming: lines.iter().skip(index + 1).find_map(|line| line_text(&line.text)),
    }
}

fn line_text(text: &str) -> Option<&str> {
    readable(text).then_some(text)
}

/// `clamp(1.125rem, 2.6vw, 1.25rem)` times the lyric font scale.
pub fn translation_px(width: f32, scale: f32) -> f32 {
    let scale = positive(scale);
    (width * 0.026 * scale).clamp(18.0 * scale, 20.0 * scale)
}

/// `clamp(0.875rem, 2vw, 1rem)` times the lyric font scale.
pub fn upcoming_px(width: f32, scale: f32) -> f32 {
    let scale = positive(scale);
    (width * 0.02 * scale).clamp(14.0 * scale, 16.0 * scale)
}

/// Parent opacity of the subtitle block, from the overlay's default `opacity`.
pub const OVERLAY_ALPHA: f32 = 0.6;
/// Size of a row under the first, from `SECONDARY_TRACK_SIZE_FACTOR`.
pub const SECONDARY_TRACK_SIZE: f32 = 0.88;
/// Extra opacity of that row, multiplied by [`OVERLAY_ALPHA`].
pub const SECONDARY_TRACK_ALPHA: f32 = 0.82;
/// Gap under the first row, in em, from `SECONDARY_TRACK_GAP_EM`.
pub const SECONDARY_TRACK_GAP: f32 = 0.25;

/// Size and opacity of the next-line row. Under a translation it steps down; alone it uses the upcoming size.
pub fn upcoming_look(width: f32, scale: f32, under_translation: bool) -> (f32, f32) {
    if under_translation {
        (translation_px(width, scale) * SECONDARY_TRACK_SIZE, OVERLAY_ALPHA * SECONDARY_TRACK_ALPHA)
    } else {
        (upcoming_px(width, scale), OVERLAY_ALPHA)
    }
}

/// Vertical gap between the translation and the next line.
pub fn row_gap(first_px: f32) -> f32 {
    first_px * SECONDARY_TRACK_GAP
}

fn positive(scale: f32) -> f32 {
    if scale > 0.0 { scale } else { 1.0 }
}

/// `text-shadow: 0 1px 2px` alpha, before the overlay's own opacity.
pub const SHADOW_ALPHA: f32 = 0.24;
/// Downward offset of that shadow. The 2px blur radius is not drawn.
pub const SHADOW_OFFSET_Y: f32 = 1.0;
/// How far `blur-2xl` (40px) pushes the ellipse edge out.
pub const GLOW_BLUR_PX: f32 = 40.0;

const GLOW_PAD_X: f32 = 6.0;
const GLOW_PAD_Y: f32 = 2.0;
const GLOW_INSET_X: f32 = 40.0;
const GLOW_INSET_Y: f32 = 24.0;
const GLOW_ELLIPSE_X: f32 = 1.15;
const GLOW_ELLIPSE_Y: f32 = 1.30;
const GLOW_MAX_TEXT: f32 = 896.0;

/// Center and semi-axes of the subtitle ellipse, as fractions of the window. Y grows downward.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlowEllipse {
    pub center: [f32; 2],
    pub radius: [f32; 2],
}

/// Alpha of the unblurred radial stops, with the outer edge spread by `blur_t` ellipse-radii.
pub fn glow_coverage(t: f32, blur_t: f32) -> f32 {
    if t <= 0.62 {
        return 0.96 + (0.78 - 0.96) * (t.max(0.0) / 0.62);
    }
    let end = 1.0 + blur_t.max(0.0);
    let x = ((t - 0.62) / (end - 0.62)).clamp(0.0, 1.0);
    let s = x * x * (3.0 - 2.0 * x);
    0.78 * (1.0 - s)
}

/// The original glow box: text, `px-1.5 py-0.5`, then `-inset-x-10 -inset-y-6`, then a 115% × 130% ellipse.
pub fn glow_ellipse(
    window_w: f32,
    window_h: f32,
    bottom: f32,
    translation_px: f32,
    upcoming_px: f32,
    translation: Option<&str>,
    upcoming: Option<&str>,
) -> GlowEllipse {
    let mut width = 0.0_f32;
    let mut height = 0.0_f32;
    let mut first = 0.0_f32;
    if let Some(text) = translation {
        first = translation_px;
        height += translation_px;
        width = width.max(subtitle_line_width(text, translation_px));
    }
    if let Some(text) = upcoming {
        if height > 0.0 {
            height += first * SECONDARY_TRACK_GAP;
        }
        height += upcoming_px;
        width = width.max(subtitle_line_width(text, upcoming_px));
    }
    width = width.min(GLOW_MAX_TEXT);
    let box_w = width + (GLOW_PAD_X + GLOW_INSET_X) * 2.0;
    let box_h = height + (GLOW_PAD_Y + GLOW_INSET_Y) * 2.0;
    let (w, h) = (window_w.max(1.0), window_h.max(1.0));
    let center_y = h - bottom - (height + GLOW_PAD_Y * 2.0) * 0.5;
    GlowEllipse {
        center: [0.5, center_y / h],
        radius: [box_w * GLOW_ELLIPSE_X * 0.5 / w, box_h * GLOW_ELLIPSE_Y * 0.5 / h],
    }
}

/// Width of one subtitle row. `text()` draws with `Font::DEFAULT`, normal weight, at `px`.
pub fn subtitle_line_width(text: &str, px: f32) -> f32 {
    folia_visualizer::measure(text, px, iced::Font::DEFAULT, GLOW_MAX_TEXT).width
}

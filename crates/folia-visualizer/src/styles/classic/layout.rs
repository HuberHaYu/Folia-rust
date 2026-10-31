// Classic word placement: the original's seeded per-word offsets, spacing and line wrapping.

use folia_core::AnimationIntensity;

use crate::styles::glow::WordConfig;

/// Classic style tuning with the original defaults.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClassicTuning {
    pub word_rotation: bool,
    pub breathing: f64,
    pub word_spacing: f64,
}

impl Default for ClassicTuning {
    fn default() -> Self {
        Self { word_rotation: true, breathing: 1.0, word_spacing: 0.7 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Justify {
    Start,
    Center,
    End,
    Around,
    Between,
}

/// A word box placed by the wrap layout; `x`/`y` is its centre before animation offsets.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placed {
    pub x: f32,
    pub y: f32,
}

/// The original's tiny deterministic generator: fractional part of `sin(seed + offset) * 10000`.
pub fn random(seed: f64, offset: f64) -> f64 {
    let x = (seed + offset).sin() * 10000.0;
    x - x.floor()
}

/// `clamp(2.25rem, 6vw, 4.5rem)` scaled by the lyrics font scale, in pixels.
pub fn font_px(width: f32, font_scale: f32) -> f32 {
    let rem = 16.0;
    (6.0 * font_scale * width / 100.0).clamp(2.25 * font_scale * rem, 4.5 * font_scale * rem)
}

pub fn justify(seed: f64, intensity: AnimationIntensity, interlude: bool) -> Justify {
    const OPTIONS: [Justify; 5] = [Justify::Start, Justify::Center, Justify::End, Justify::Around, Justify::Between];
    if interlude || intensity == AnimationIntensity::Calm {
        return Justify::Center;
    }
    OPTIONS[(seed % OPTIONS.len() as f64).floor() as usize % OPTIONS.len()]
}

pub fn word_configs(
    seed: f64,
    intensity: AnimationIntensity,
    interlude: bool,
    widths: &[f32],
    font_px: f32,
    tuning: ClassicTuning,
) -> Vec<WordConfig> {
    let chaotic = intensity == AnimationIntensity::Chaotic;
    let (spread, rotate) = match intensity {
        AnimationIntensity::Chaotic => (60.0, 30.0),
        AnimationIntensity::Calm => (0.0, 0.0),
        AnimationIntensity::Normal => (20.0, 5.0),
    };
    let font_px = f64::from(font_px);
    let scale_of =
        |word_seed: f64| if chaotic { 0.8 + random(word_seed, 4.0) * 0.6 } else { 1.1 + random(word_seed, 4.0) * 0.2 };
    let x_of = |word_seed: f64| (random(word_seed, 1.0) - 0.5) * spread * 2.0;

    (0..widths.len())
        .map(|i| {
            let word_seed = seed + i as f64;
            if interlude {
                return WordConfig {
                    x: 0.0,
                    y: (random(word_seed, 2.0) - 0.5) * 15.0,
                    rotate: 0.0,
                    scale: 1.5,
                    passed_rotate: 0.0,
                    margin_right: 48.0,
                };
            }
            let scale = scale_of(word_seed);
            let x = x_of(word_seed);

            // Leave room for this word and the next one growing to their active scale.
            let (next_width, next_scale, next_x) = match widths.get(i + 1) {
                Some(&width) => (f64::from(width), scale_of(word_seed + 1.0) * 1.4, x_of(word_seed + 1.0)),
                None => (0.0, 1.0, 0.0),
            };
            let overflow = f64::from(widths[i]) * (scale * 1.4 - 1.0) / 2.0 + next_width * (next_scale - 1.0) / 2.0;
            let wanted = (overflow + x - next_x + 0.05 * font_px) * tuning.word_spacing;
            let minimum = if chaotic { 0.08 } else { 0.12 } * font_px * tuning.word_spacing;

            let rotation = |offset: f64, range: f64| {
                if tuning.word_rotation { (random(word_seed, offset) - 0.5) * range } else { 0.0 }
            };
            WordConfig {
                x,
                y: (random(word_seed, 2.0) - 0.5) * spread * 2.0,
                rotate: rotation(3.0, rotate * 2.0),
                scale,
                passed_rotate: rotation(8.0, 45.0),
                margin_right: wanted.max(minimum),
            }
        })
        .collect()
}

/// Wraps word boxes into rows no wider than `max_width` and centres the block on `center_y`.
pub fn wrap(
    widths: &[f32],
    margins: &[f32],
    row_height: f32,
    max_width: f32,
    left: f32,
    center_y: f32,
    justify: Justify,
) -> Vec<Placed> {
    let mut rows: Vec<std::ops::Range<usize>> = Vec::new();
    let mut start = 0;
    let mut used = 0.0;
    for i in 0..widths.len() {
        let outer = widths[i] + margins[i];
        if i > start && used + outer > max_width {
            rows.push(start..i);
            start = i;
            used = 0.0;
        }
        used += outer;
    }
    if start < widths.len() {
        rows.push(start..widths.len());
    }

    let top = center_y - rows.len() as f32 * row_height / 2.0;
    let mut placed = Vec::with_capacity(widths.len());
    for (row_index, row) in rows.into_iter().enumerate() {
        let count = row.len() as f32;
        let content: f32 = row.clone().map(|i| widths[i] + margins[i]).sum();
        let free = (max_width - content).max(0.0);
        let (mut x, gap) = match justify {
            Justify::Start => (left, 0.0),
            Justify::Center => (left + free / 2.0, 0.0),
            Justify::End => (left + free, 0.0),
            Justify::Around => (left + free / count / 2.0, free / count),
            Justify::Between if count > 1.0 => (left, free / (count - 1.0)),
            Justify::Between => (left, 0.0),
        };
        let y = top + (row_index as f32 + 0.5) * row_height;
        for i in row {
            placed.push(Placed { x: x + widths[i] / 2.0, y });
            x += widths[i] + margins[i] + gap;
        }
    }
    placed
}

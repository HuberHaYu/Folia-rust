// The album poster (`BravaisSeamAlbumPoster`): its colours from the cover, its composition, and the canvas pieces.

use iced::widget::canvas::{self, Cache, Frame, Geometry, Path, Program, Stroke, Text};
use iced::widget::image::Handle;
use iced::{Color, Point, Radians, Rectangle, Renderer, Size, Theme, Vector, mouse};

/// `--bravais-album-*` with the CSS fallbacks for when the cover gives no colours.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub paper: Color,
    pub ink: Color,
    pub block: Color,
    pub title: Color,
    pub extrude: [Color; 4],
    pub accent: Color,
}

pub const FALLBACK: Palette = Palette {
    paper: rgb(0xef, 0xe9, 0xdc),
    ink: rgb(0x14, 0x12, 0x10),
    block: rgb(0x22, 0x30, 0x5f),
    title: rgb(0xec, 0xdc, 0xae),
    extrude: [rgb(0xb9, 0xa6, 0x79), rgb(0x8a, 0x7c, 0x5b), rgb(0x4a, 0x45, 0x60), rgb(0x18, 0x22, 0x4a)],
    accent: rgb(0xe4, 0x46, 0x2b),
};

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a: 1.0 }
}

type Rgb = [f32; 3];

const ALBUM_PAPER: Rgb = [239.0, 233.0, 220.0];
const ALBUM_INK: Rgb = [20.0, 18.0, 16.0];
/// `TEMPERA_TONE_STOPS`, `THEME_HUE_MIX`, `MIN_INK_CONTRAST`.
const TONE_STOPS: [f32; 4] = [0.12, 0.3, 0.52, 0.72];
const THEME_HUE_MIX: f32 = 0.32;
const MIN_INK_CONTRAST: f32 = 96.0;

/// `extractRepresentativeColorsFromPixels`: a weighted median cut of the 4-bit histogram, largest first.
pub fn representative(rgba: &[u8], count: usize) -> Vec<Rgb> {
    let mut histogram: std::collections::HashMap<u16, [f64; 4]> = std::collections::HashMap::new();
    for pixel in rgba.as_chunks::<4>().0 {
        if pixel[3] < 128 {
            continue;
        }
        let key = (u16::from(pixel[0] >> 4) << 8) | (u16::from(pixel[1] >> 4) << 4) | u16::from(pixel[2] >> 4);
        let entry = histogram.entry(key).or_insert([0.0; 4]);
        entry[0] += f64::from(pixel[0]);
        entry[1] += f64::from(pixel[1]);
        entry[2] += f64::from(pixel[2]);
        entry[3] += 1.0;
    }
    let mut keys: Vec<u16> = histogram.keys().copied().collect();
    keys.sort_unstable();
    let colors: Vec<[f64; 4]> = keys
        .iter()
        .filter_map(|key| histogram.get(key))
        .map(|sum| [(sum[0] / sum[3]).round(), (sum[1] / sum[3]).round(), (sum[2] / sum[3]).round(), sum[3]])
        .collect();
    if colors.is_empty() || count == 0 {
        return Vec::new();
    }
    let range = |bucket: &[[f64; 4]], channel: usize| {
        let (low, high) = bucket
            .iter()
            .fold((255.0f64, 0.0f64), |(low, high), color| (low.min(color[channel]), high.max(color[channel])));
        high - low
    };
    let weight = |bucket: &[[f64; 4]]| bucket.iter().map(|color| color[3]).sum::<f64>();
    let mut buckets = vec![colors];
    let wanted = count.min(buckets[0].len());
    while buckets.len() < wanted {
        let best = buckets
            .iter()
            .enumerate()
            .filter(|(_, bucket)| bucket.len() >= 2)
            .map(|(index, bucket)| {
                let widest = range(bucket, 0).max(range(bucket, 1)).max(range(bucket, 2));
                (index, widest * weight(bucket).sqrt())
            })
            .fold(None, |best: Option<(usize, f64)>, candidate| match best {
                Some(kept) if kept.1 >= candidate.1 => Some(kept),
                _ => Some(candidate),
            });
        let Some((index, _)) = best else { break };
        let mut bucket = buckets.remove(index);
        let channel =
            (0..3).fold(
                0,
                |widest, candidate| if range(&bucket, candidate) > range(&bucket, widest) { candidate } else { widest },
            );
        bucket.sort_by(|a, b| a[channel].total_cmp(&b[channel]));
        let midpoint = weight(&bucket) / 2.0;
        let mut accumulated = 0.0;
        let mut split = 1;
        for (place, color) in bucket.iter().enumerate().take(bucket.len() - 1) {
            accumulated += color[3];
            split = place + 1;
            if accumulated >= midpoint {
                break;
            }
        }
        let right = bucket.split_off(split);
        buckets.insert(index, right);
        buckets.insert(index, bucket);
    }
    let mut averaged: Vec<(Rgb, f64)> = buckets
        .iter()
        .map(|bucket| {
            let total = weight(bucket);
            let channel =
                |c: usize| (bucket.iter().map(|color| color[c] * color[3]).sum::<f64>() / total).round() as f32;
            ([channel(0), channel(1), channel(2)], total)
        })
        .collect();
    averaged.sort_by(|a, b| b.1.total_cmp(&a.1));
    averaged.into_iter().map(|(color, _)| color).collect()
}

fn luminance(color: Rgb) -> f32 {
    0.2126 * color[0] + 0.7152 * color[1] + 0.0722 * color[2]
}

fn saturation(color: Rgb) -> f32 {
    let max = color[0].max(color[1]).max(color[2]);
    let min = color[0].min(color[1]).min(color[2]);
    if max == 0.0 { 0.0 } else { (max - min) / max }
}

/// `mixColors`: per channel, rounded as the `rgba()` string rounds.
fn mix(from: Rgb, to: Rgb, amount: f32) -> Rgb {
    let amount = amount.clamp(0.0, 1.0);
    [0, 1, 2].map(|c| (from[c] + (to[c] - from[c]) * amount).round())
}

/// `ensureInkContrast`.
fn ink_contrast(paper: Rgb, ink: Rgb) -> Rgb {
    if (luminance(ink) - luminance(paper)).abs() >= MIN_INK_CONTRAST {
        return ink;
    }
    let target = if luminance(paper) < 128.0 { [244.0, 244.0, 242.0] } else { [20.0, 20.0, 20.0] };
    let tinted = mix(target, ink, 0.14);
    if (luminance(tinted) - luminance(paper)).abs() >= MIN_INK_CONTRAST { tinted } else { target }
}

/// `matchLuminance`: the tinted step pulled back onto the neutral step's brightness.
fn match_luminance(color: Rgb, target: Rgb) -> Rgb {
    let current = luminance(color);
    if current <= 0.5 {
        return target;
    }
    let gain = luminance(target) / current;
    let scaled = color.map(|c| (c * gain).min(255.0).round());
    if (luminance(scaled) - luminance(target)).abs() > 1.5 { target } else { scaled }
}

/// `resolveTemperaPalette` in gradient mode: the four tone steps between paper and ink, tinted by the cover.
fn tone_ladder(paper: Rgb, ink: Rgb, cover: &[Rgb], theme: [Rgb; 4]) -> [Rgb; 4] {
    let ink = ink_contrast(paper, ink);
    let hues: Vec<Rgb> = if cover.len() >= 2 {
        cover
            .iter()
            .take(4)
            .enumerate()
            .map(|(index, color)| mix(*color, theme[index % theme.len()], THEME_HUE_MIX))
            .collect()
    } else {
        theme.to_vec()
    };
    let mut ranked = hues;
    ranked.sort_by(|a, b| luminance(*a).total_cmp(&luminance(*b)));
    if luminance(ink) < luminance(paper) {
        ranked.reverse();
    }
    TONE_STOPS.iter().enumerate().fold([[0.0; 3]; 4], |mut out, (index, stop)| {
        let rung = mix(paper, ink, *stop);
        out[index] =
            ranked.get(index % ranked.len().max(1)).map_or(rung, |hue| match_luminance(mix(rung, *hue, 0.78), rung));
        out
    })
}

/// `resolveBravaisAlbumPalette`: the darkest step pressed toward ink is the block; the colour most saturated and
/// most unlike the block is the accent. Fewer than two colours give none.
pub fn album_palette(colors: &[Rgb]) -> Option<Palette> {
    if colors.len() < 2 {
        return None;
    }
    // `posterTheme` with the paper and ink swapped in: accent and secondary are the first two cover colours.
    let theme =
        [colors[0], colors.get(1).copied().unwrap_or(colors[0]), ALBUM_INK, ink_contrast(ALBUM_PAPER, ALBUM_INK)];
    let tones = tone_ladder(ALBUM_PAPER, ALBUM_INK, colors, theme);
    let darkest =
        tones.iter().copied().fold(tones[0], |dark, tone| if luminance(tone) < luminance(dark) { tone } else { dark });
    let block = mix(darkest, ALBUM_INK, 0.25);
    let distance = |color: Rgb| {
        ((color[0] - block[0]).powi(2) + (color[1] - block[1]).powi(2) + (color[2] - block[2]).powi(2)).sqrt() / 441.0
    };
    let accent = tones
        .iter()
        .copied()
        .filter(|tone| *tone != darkest)
        .chain(colors.iter().copied())
        .map(|color| (color, saturation(color) * distance(color)))
        .fold(None, |best: Option<(Rgb, f32)>, candidate| match best {
            Some(kept) if kept.1 >= candidate.1 => Some(kept),
            _ => Some(candidate),
        })
        .map_or(colors[0], |(color, _)| color);
    let title = mix(ALBUM_PAPER, accent, 0.12);
    let color = |c: Rgb| Color::from_rgb(c[0] / 255.0, c[1] / 255.0, c[2] / 255.0);
    Some(Palette {
        paper: color(mix(ALBUM_PAPER, tones[0], 0.06)),
        ink: color(ALBUM_INK),
        block: color(block),
        title: color(title),
        extrude: [
            color(mix(title, block, 0.25)),
            color(mix(title, block, 0.45)),
            color(mix(title, block, 0.65)),
            color(mix(block, ALBUM_INK, 0.3)),
        ],
        accent: color(accent),
    })
}

/// `resolveAlbumComposition`: fixed for an album, from its key.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Composition {
    pub cover_deg: f32,
    pub tape_x: f32,
    pub tape_deg: f32,
    /// Where the diagonal meets the left and right edges, in percent of the poster's height.
    pub divider: (f32, f32),
    pub underline: f32,
    /// The hero word's move in em and its turn in degrees.
    pub hero: (f32, f32, f32),
    pub ghost_right: bool,
}

/// `hashPosterSeed` (FNV-1a over UTF-16 units) feeding `createRandom` (mulberry32): the same seed, the same numbers.
pub fn seeded(seed: &str) -> impl FnMut() -> f64 + use<> {
    let mut state =
        seed.encode_utf16().fold(2_166_136_261u32, |hash, unit| (hash ^ u32::from(unit)).wrapping_mul(16_777_619));
    move || {
        state = state.wrapping_add(0x6d2b_79f5);
        let mut value = state;
        value = (value ^ (value >> 15)).wrapping_mul(value | 1);
        value ^= value.wrapping_add((value ^ (value >> 7)).wrapping_mul(value | 61));
        f64::from(value ^ (value >> 14)) / 4_294_967_296.0
    }
}

pub fn composition(seed: &str) -> Composition {
    let mut random = seeded(&format!("album:{seed}"));
    let between = |min: f64, max: f64, random: &mut dyn FnMut() -> f64| (min + (max - min) * random()) as f32;
    let high = between(44.0, 50.0, &mut random);
    let low = between(56.0, 62.0, &mut random);
    let left_high = random() < 0.5;
    let magnitude = between(2.5, 6.0, &mut random);
    let cover_deg = if random() < 0.5 { -magnitude } else { magnitude };
    let tape_x = between(28.0, 58.0, &mut random);
    let tape_deg = between(-5.0, 5.0, &mut random);
    let underline = between(38.0, 62.0, &mut random);
    let (hero_x, hero_y) = (between(0.04, 0.12, &mut random), between(0.12, 0.22, &mut random));
    let turn = between(2.0, 7.0, &mut random);
    let hero = (hero_x, hero_y, if random() < 0.5 { -turn } else { turn });
    let ghost_right = random() >= 0.5;
    Composition {
        cover_deg,
        tape_x,
        tape_deg,
        divider: if left_high { (high, low) } else { (low, high) },
        underline,
        hero,
        ghost_right,
    }
}

/// `--bravais-poster-pad` on the album poster.
pub const PAD: f32 = 40.0;
/// The title's face: 900, drawn at the heaviest weight the fonts have.
pub const TITLE_FONT: iced::Font = crate::typeface::font(iced::font::Weight::Black);

/// `resolvePosterWidth`: 46% of the window, between 520 and 680.
pub fn width(viewport: f32) -> f32 {
    (viewport * 0.46).clamp(520.0, 680.0).round()
}

/// `titleColumns`: CJK counts a column a character, the rest 0.62; the longest Latin word may not break.
pub fn title_columns(title: &str) -> f32 {
    let wide = |c: char| matches!(c as u32, 0x3040..=0x30ff | 0x3400..=0x9fff | 0xac00..=0xd7af | 0xf900..=0xfaff);
    let columns = |text: &str| text.chars().map(|c| if wide(c) { 1.02 } else { 0.62 }).sum::<f32>();
    let longest = title.split_whitespace().filter(|word| !word.chars().any(wide)).map(columns).fold(0.0, f32::max);
    2.0f32.max(longest).max(columns(title) / 3.0)
}

/// `splitHeroWord`: the longest word is the hero when the title has two words or more.
pub fn hero_split(title: &str) -> Vec<(String, bool)> {
    let mut parts: Vec<(String, bool)> = Vec::new();
    let mut word = String::new();
    let mut gap = String::new();
    for c in title.chars() {
        if c.is_alphanumeric() {
            if !gap.is_empty() {
                parts.push((std::mem::take(&mut gap), false));
            }
            word.push(c);
        } else {
            if !word.is_empty() {
                parts.push((std::mem::take(&mut word), true));
            }
            gap.push(c);
        }
    }
    if !word.is_empty() {
        parts.push((word, true));
    }
    if !gap.is_empty() {
        parts.push((gap, false));
    }
    let words = parts.iter().filter(|(_, is_word)| *is_word).count();
    let hero = parts
        .iter()
        .enumerate()
        .filter(|(_, (_, is_word))| *is_word)
        .fold(None, |best: Option<(usize, usize)>, (index, (text, _))| {
            let length = text.chars().count();
            match best {
                Some(kept) if kept.1 >= length => Some(kept),
                _ => Some((index, length)),
            }
        })
        .map(|(index, _)| index)
        .filter(|_| words >= 2);
    parts.into_iter().enumerate().map(|(index, (text, _))| (text, Some(index) == hero)).collect()
}

/// The poster's ground: the block, the dotted paper down to the diagonal, the hatch along it, the dots low on the
/// right, and the outlined watermark letter (`.bravais-poster-ground`).
pub struct Ground<'a> {
    pub palette: Palette,
    pub composition: Composition,
    pub ghost: String,
    pub cache: &'a Cache,
    pub entry: crate::poster_motion::Entry,
}

/// What the ground is drawn from.
#[derive(Debug, Clone, Copy)]
pub struct Band<'a> {
    pub palette: Palette,
    pub composition: Composition,
    pub ghost: &'a str,
    pub entry: crate::poster_motion::Entry,
}

impl Ground<'_> {
    fn band(&self) -> Band<'_> {
        Band { palette: self.palette, composition: self.composition, ghost: &self.ghost, entry: self.entry }
    }
}

impl<Message> Program<Message> for Ground<'_> {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        // During the entry the ground changes every frame and is drawn afresh; at rest it comes from the cache.
        if self.entry.0.is_some() {
            let mut frame = Frame::new(renderer, bounds.size());
            draw_ground(&mut frame, bounds.size(), self.band());
            return vec![frame.into_geometry()];
        }
        vec![self.cache.draw(renderer, bounds.size(), |frame| draw_ground(frame, bounds.size(), self.band()))]
    }
}

fn faded(color: Color, alpha: f32) -> Color {
    Color { a: color.a * alpha, ..color }
}

/// Draws the poster's ground into `size` at the frame's origin; the wall draws it too while the layer tilts.
pub fn draw_ground(frame: &mut Frame, size: Size, band: Band<'_>) {
    let Palette { paper, ink, block, .. } = band.palette;
    let (w, h) = (size.width, size.height);
    frame.fill(&Path::rectangle(Point::ORIGIN, size), block);
    // `bravais-album-paper-in`: the paper's lower edge comes up from the bottom to the diagonal.
    let shut = band.entry.paper();
    let (left, right) = (h * band.composition.divider.0 / 100.0, h * band.composition.divider.1 / 100.0);
    let (left, right) = (h + (left - h) * shut, h + (right - h) * shut);
    let edge = |x: f32| left + (right - left) * (x / w.max(1.0));
    // `.bravais-album-paper`: down to the diagonal, a 1px ink dot at 20% every 9px.
    let paper_shape = Path::new(|path| {
        path.move_to(Point::ORIGIN);
        path.line_to(Point::new(w, 0.0));
        path.line_to(Point::new(w, right));
        path.line_to(Point::new(0.0, left));
        path.close();
    });
    frame.fill(&paper_shape, paper);
    let dots = Path::new(|path| {
        let mut y = 4.5;
        while y < left.max(right) {
            let mut x = 4.5;
            while x < w {
                if y < edge(x) - 1.0 {
                    path.circle(Point::new(x, y), 1.0);
                }
                x += 9.0;
            }
            y += 9.0;
        }
    });
    frame.fill(&dots, faded(ink, 0.2));
    // `.bravais-album-hatch`: 2px ink stripes every 9px at −58°, in a band 3.2% high under the diagonal.
    let strip = h * 0.032;
    let (dx, dy) = (58f32.to_radians().cos(), 58f32.to_radians().sin());
    let step = 9.0 / dy;
    let hatch = Path::new(|path| {
        let mut x0 = -strip / dy * dx - 9.0;
        while x0 < w + strip {
            let top = edge(x0.clamp(0.0, w));
            let start = Point::new(x0, top);
            let end = Point::new(x0 - strip / dy * dx, top + strip);
            if start.x >= 0.0 && start.x <= w || end.x >= 0.0 && end.x <= w {
                path.move_to(start);
                path.line_to(end);
            }
            x0 += step;
        }
    });
    let hatch_alpha = band.entry.hatch();
    frame.with_clip(Rectangle::new(Point::ORIGIN, size), |frame| {
        frame.stroke(&hatch, Stroke::default().with_color(faded(ink, hatch_alpha)).with_width(2.0));
    });
    let dots_alpha = band.entry.dots();
    // `.bravais-album-dots`: right −8%, bottom 6%, 64% × 34%, 1.4px paper dots at 18% every 10px, fading out.
    let (area_w, area_h) = (w * 0.64, h * 0.34);
    let (area_x, area_y) = (w * 1.08 - area_w, h * 0.94 - area_h);
    let center = Point::new(area_x + area_w / 2.0, area_y + area_h / 2.0);
    let mut y = area_y + 5.0;
    while y < area_y + area_h {
        let mut x = area_x + 5.0;
        while x < area_x + area_w {
            // `mask-image: radial-gradient(closest-side, #000 40%, transparent)`.
            let reach = ((x - center.x) / (area_w / 2.0)).hypot((y - center.y) / (area_h / 2.0));
            let mask = if reach <= 0.4 { 1.0 } else { ((1.0 - reach) / 0.6).clamp(0.0, 1.0) };
            if mask > 0.0 && x < w {
                frame.fill(&Path::circle(Point::new(x, y), 1.4), faded(paper, 0.18 * mask * dots_alpha));
            }
            x += 10.0;
        }
        y += 10.0;
    }
    // `.bravais-album-ghost`: min(420px, 52% high), its outline in paper at 16%.
    let size_px = 420f32.min(h * 0.52);
    let ghost_x = if band.composition.ghost_right { w + size_px * 0.18 } else { -size_px * 0.12 };
    frame.fill_text(Text {
        content: band.ghost.to_owned(),
        position: Point::new(ghost_x, h + size_px * 0.12),
        color: faded(paper, 0.07 * dots_alpha),
        size: size_px.into(),
        font: crate::typeface::font(iced::font::Weight::Black),
        align_x: if band.composition.ghost_right {
            iced::widget::text::Alignment::Right
        } else {
            iced::widget::text::Alignment::Left
        },
        align_y: iced::alignment::Vertical::Bottom,
        ..Text::default()
    });
}

/// `.bravais-album-cover`: the cover turned on the page, a hard ink shadow 12px down-right, and a strip of tape.
pub struct Cover<M> {
    /// Tells where the cover is on screen when that changes: the flying cover lands there.
    pub report: Option<fn(Rectangle) -> M>,
    pub image: Option<Handle>,
    pub palette: Palette,
    pub composition: Composition,
    pub entry: crate::poster_motion::Entry,
    /// Hidden while the flying cover is on its way (`data-flight="pending"`).
    pub hidden: bool,
    /// The cover's side; the canvas leaves room round it for the turn, the shadow and the tape.
    pub side: f32,
}

pub const COVER_MARGIN: f32 = 28.0;

impl<Message> Program<Message> for Cover<Message> {
    type State = Option<Rectangle>;

    fn update(
        &self,
        state: &mut Option<Rectangle>,
        _event: &canvas::Event,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        if *state == Some(bounds) {
            return None;
        }
        *state = Some(bounds);
        self.report.map(|report| canvas::Action::publish(report(bounds)))
    }

    fn draw(
        &self,
        _state: &Option<Rectangle>,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        if self.hidden {
            return vec![frame.into_geometry()];
        }
        // `bravais-album-cover-in`: it falls 28px from 1.14 into place as it shows.
        let (opacity, drop, grow) = self.entry.cover();
        let side = self.side * grow;
        let angle = Radians(self.composition.cover_deg.to_radians());
        let center = Point::new(COVER_MARGIN + self.side / 2.0, COVER_MARGIN + self.side / 2.0 + drop);
        frame.with_save(|frame| {
            frame.translate(Vector::new(center.x, center.y));
            frame.rotate(angle);
            // The figure fades as one (`opacity` on the element): the shadow shows only outside the cover, and the
            // block only where no picture covers it.
            let (h, o) = (side / 2.0, 12.0);
            let shadow = Path::new(|path| {
                path.move_to(Point::new(h, -h + o));
                path.line_to(Point::new(h + o, -h + o));
                path.line_to(Point::new(h + o, h + o));
                path.line_to(Point::new(-h + o, h + o));
                path.line_to(Point::new(-h + o, h));
                path.line_to(Point::new(h, h));
                path.close();
            });
            frame.fill(&shadow, faded(self.palette.ink, opacity));
            if self.image.is_none() {
                frame.fill(
                    &Path::rectangle(Point::new(-h, -h), Size::new(side, side)),
                    faded(self.palette.block, opacity),
                );
            }
        });
        if let Some(image) = &self.image {
            let rect = Rectangle::new(Point::new(center.x - side / 2.0, center.y - side / 2.0), Size::new(side, side));
            frame.draw_image(rect, canvas::Image::new(image.clone()).rotation(angle).opacity(opacity));
        }
        // `.bravais-album-tape`: 30% wide, 24px, 12px above the top edge at `tape_x`, turned on its own; it slaps on last.
        let (tape_alpha, tape_scale) = self.entry.tape();
        frame.with_save(|frame| {
            frame.translate(Vector::new(center.x, center.y));
            frame.rotate(angle);
            frame.translate(Vector::new(-side / 2.0 + side * self.composition.tape_x / 100.0, -side / 2.0));
            frame.rotate(Radians(self.composition.tape_deg.to_radians()));
            frame.scale(tape_scale);
            let (tape_w, tape_h) = (side * 0.3, 24.0);
            let lift = Path::rectangle(Point::new(-tape_w / 2.0, -tape_h / 2.0 + 1.0), Size::new(tape_w, tape_h));
            frame.fill(&lift, Color::from_rgba(0.0, 0.0, 0.0, 0.18 * tape_alpha));
            let tape = Path::rectangle(Point::new(-tape_w / 2.0, -tape_h / 2.0), Size::new(tape_w, tape_h));
            frame.fill(&tape, Color::from_rgba8(214, 212, 204, 0.8 * tape_alpha));
        });
        vec![frame.into_geometry()]
    }
}

/// `.bravais-album-underline`: a 9px accent bar turned −1°.
pub struct Underline {
    pub color: Color,
    pub width: f32,
    /// `scale: 0 1` growing to 1 from the left edge.
    pub drawn: f32,
}

impl<Message> Program<Message> for Underline {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        frame.translate(Vector::new(self.width / 2.0, bounds.height / 2.0));
        frame.rotate(Radians((-1f32).to_radians()));
        frame.fill(
            &Path::rectangle(Point::new(-self.width / 2.0, -4.5), Size::new(self.width * self.drawn, 9.0)),
            self.color,
        );
        vec![frame.into_geometry()]
    }
}

/// `new Date(time).getFullYear()` in UTC: there is no local time zone to read here.
pub fn year_of(millis: i64) -> i64 {
    let days = millis.div_euclid(86_400_000);
    // Howard Hinnant's civil-from-days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    yoe + era * 400 + i64::from(month <= 2)
}

// The album poster's entry (`bravaisAlbumPoster.css` keyframes) and its title, laid out and drawn in a canvas.

use folia_visualizer::Ease;
use iced::widget::canvas::{Frame, Geometry, Program, Text};
use iced::{Color, Font, Point, Radians, Rectangle, Renderer, Theme, Vector, mouse};

/// The entry is over this long after it starts (the tape's 820ms + 300ms is the last part).
pub const ENTRY_MS: f32 = 1120.0;

const EASE_OUT: Ease = Ease::new(0.0, 0.0, 0.58, 1.0);
const PAPER_EASE: Ease = Ease::new(0.65, 0.0, 0.25, 1.0);
const COVER_EASE: Ease = Ease::new(0.2, 0.9, 0.3, 1.08);
const TAPE_EASE: Ease = Ease::new(0.3, 0.7, 0.3, 1.2);
const RISE_EASE: Ease = Ease::new(0.2, 0.8, 0.3, 1.0);
const WORD_EASE: Ease = Ease::new(0.2, 0.9, 0.3, 1.1);
const UNDERLINE_EASE: Ease = Ease::new(0.6, 0.0, 0.2, 1.0);

/// Progress of a keyframe run of `ms` after `delay`, eased (`animation: … both`): 0 before, 1 after.
fn run(at: Option<f32>, delay: f32, ms: f32, ease: Ease) -> f32 {
    match at {
        None => 1.0,
        Some(at) => ease.at(f64::from(((at - delay) / ms).clamp(0.0, 1.0))) as f32,
    }
}

/// Where the poster's entry is: milliseconds since it began, `None` once it is over.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Entry(pub Option<f32>);

impl Entry {
    /// `bravais-album-paper-in`: the paper's lower edge from the poster's bottom up to the diagonal.
    pub fn paper(self) -> f32 {
        run(self.0, 0.0, 620.0, PAPER_EASE)
    }

    /// `bravais-album-fade-in` on the hatch (360ms after 480ms).
    pub fn hatch(self) -> f32 {
        run(self.0, 480.0, 360.0, EASE_OUT)
    }

    /// The same fade on the dots and the watermark (600ms after 520ms).
    pub fn dots(self) -> f32 {
        run(self.0, 520.0, 600.0, EASE_OUT)
    }

    /// `bravais-album-cover-in`: opacity up by 60%, 28px down and 1.14 to 1 over the whole run.
    pub fn cover(self) -> (f32, f32, f32) {
        let Some(at) = self.0 else { return (1.0, 0.0, 1.0) };
        let t = ((at - 120.0) / 560.0).clamp(0.0, 1.0);
        let opacity = COVER_EASE.at(f64::from((t / 0.6).min(1.0))) as f32;
        let moved = COVER_EASE.at(f64::from(t)) as f32;
        (opacity.clamp(0.0, 1.0), -28.0 * (1.0 - moved), 1.14 + (1.0 - 1.14) * moved)
    }

    /// `bravais-album-tape-in`: from nothing at 1.7, to whole at 0.94 by 70%, then to 1.
    pub fn tape(self) -> (f32, f32) {
        let Some(at) = self.0 else { return (1.0, 1.0) };
        let t = ((at - 820.0) / 300.0).clamp(0.0, 1.0);
        if t <= 0.7 {
            let p = TAPE_EASE.at(f64::from(t / 0.7)) as f32;
            (p.clamp(0.0, 1.0), 1.7 + (0.94 - 1.7) * p)
        } else {
            let p = TAPE_EASE.at(f64::from((t - 0.7) / 0.3)) as f32;
            (1.0, 0.94 + (1.0 - 0.94) * p)
        }
    }

    /// The scrap's fade (500ms after 700ms).
    pub fn scrap(self) -> f32 {
        run(self.0, 700.0, 500.0, EASE_OUT)
    }

    /// The top bar's fade (400ms after 200ms).
    pub fn top(self) -> f32 {
        run(self.0, 200.0, 400.0, EASE_OUT)
    }

    /// `bravais-album-rise-in` on the kicker (380ms after 420ms): opacity and how far below it still is.
    pub fn kicker(self) -> (f32, f32) {
        let p = run(self.0, 420.0, 380.0, RISE_EASE);
        (p.clamp(0.0, 1.0), 14.0 * (1.0 - p))
    }

    /// The rise of the bottom block's `n`-th child: 560, 620, 680, then 740ms in.
    pub fn bottom(self, n: usize) -> (f32, f32) {
        let delay = 560.0 + 60.0 * n.min(3) as f32;
        let p = run(self.0, delay, 420.0, RISE_EASE);
        (p.clamp(0.0, 1.0), 14.0 * (1.0 - p))
    }

    /// `bravais-album-underline-in`: drawn out from the left.
    pub fn underline(self) -> f32 {
        run(self.0, 620.0, 420.0, UNDERLINE_EASE)
    }

    /// `bravais-album-extrude-in`: the extrusion grows from nothing (420ms after 640ms).
    pub fn extrude(self) -> f32 {
        run(self.0, 640.0, 420.0, EASE_OUT)
    }

    /// `bravais-album-word-in` of the span at `index`: 520ms after `300 + 70·index`, from one of three sides.
    fn word(self, index: usize) -> (f32, Vector, f32) {
        let p = run(self.0, 300.0 + 70.0 * index as f32, 520.0, WORD_EASE);
        // `:nth-child(3n + 2)` and `:nth-child(3n)`, counted from 1.
        let (x, y, deg) = match (index + 1) % 3 {
            2 => (0.4, -0.45, 7.0),
            0 => (0.0, 0.6, 4.0),
            _ => (-0.5, 0.35, -8.0),
        };
        let rest = 1.0 - p;
        (p.clamp(0.0, 1.0), Vector::new(x * rest, y * rest), deg * rest)
    }
}

/// One span of the title placed on its line.
#[derive(Debug, Clone)]
pub struct Placed {
    pub text: String,
    pub hero: bool,
    pub space: bool,
    pub index: usize,
    pub at: Point,
    pub size: f32,
    pub width: f32,
}

/// The title's spans wrapped into the column, and its height.
#[derive(Debug, Clone, Default)]
pub struct TitleLayout {
    pub spans: Vec<Placed>,
    pub height: f32,
    pub width: f32,
    pub size: f32,
}

/// Wraps the words at `width` like the browser (a word moves to the next line whole; spaces at a line's start go).
pub fn layout_title(words: &[(String, bool)], size: f32, width: f32, font: Font) -> TitleLayout {
    let line = size * 1.02;
    let mut spans = Vec::with_capacity(words.len());
    let (mut x, mut y) = (0.0f32, 0.0f32);
    for (index, (text, hero)) in words.iter().enumerate() {
        let space = text.trim().is_empty();
        let span_size = if *hero { size * 0.72 } else { size };
        let advance = folia_visualizer::measure(text, span_size, font, f32::INFINITY).width;
        if space {
            if x > 0.0 {
                spans.push(Placed {
                    text: text.clone(),
                    hero: false,
                    space,
                    index,
                    at: Point::new(x, y),
                    size: span_size,
                    width: advance,
                });
                x += advance;
            }
            continue;
        }
        if x > 0.0 && x + advance > width {
            x = 0.0;
            y += line;
        }
        // A word wider than the column breaks by characters (`overflow-wrap: anywhere`).
        if advance > width {
            for c in text.chars() {
                let piece = c.to_string();
                let w = folia_visualizer::measure(&piece, span_size, font, f32::INFINITY).width;
                if x > 0.0 && x + w > width {
                    x = 0.0;
                    y += line;
                }
                spans.push(Placed {
                    text: piece,
                    hero: *hero,
                    space: false,
                    index,
                    at: Point::new(x, y),
                    size: span_size,
                    width: w,
                });
                x += w;
            }
            continue;
        }
        spans.push(Placed {
            text: text.clone(),
            hero: *hero,
            space,
            index,
            at: Point::new(x, y),
            size: span_size,
            width: advance,
        });
        x += advance;
    }
    TitleLayout { spans, height: y + line, width, size }
}

/// `.bravais-album-title`: cream words over four steps of extrusion; the hero word moved and turned by the
/// composition; during the entry every word flies in and the extrusion grows behind it.
pub struct Title {
    pub layout: std::rc::Rc<TitleLayout>,
    pub color: Color,
    pub extrude: [Color; 4],
    pub hero: (f32, f32, f32),
    pub entry: Entry,
    pub font: Font,
}

impl<Message> Program<Message> for Title {
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
        let size = self.layout.size;
        let grow = self.entry.extrude();
        for span in self.layout.spans.iter().filter(|span| !span.space) {
            let (alpha, from, turn) = self.entry.word(span.index);
            if alpha <= 0.0 {
                continue;
            }
            let (mut offset, mut deg) = (Vector::new(from.x * size, from.y * size), turn);
            if span.hero {
                // `translate(hero.x em, hero.y em) rotate(hero.deg)` on the 0.72em hero word.
                offset += Vector::new(self.hero.0 * span.size, self.hero.1 * span.size);
                deg += self.hero.2;
            }
            // Inline blocks turn about their middle (`transform-origin: 50% 50%`).
            let half = Vector::new(span.width / 2.0, span.size * 1.02 / 2.0);
            frame.with_save(|frame| {
                frame.translate(Vector::new(span.at.x, span.at.y + (size * 1.02 - span.size * 1.02)) + offset + half);
                frame.rotate(Radians(deg.to_radians()));
                frame.translate(Vector::new(-half.x, -half.y));
                for (step, color) in self.extrude.iter().enumerate().rev() {
                    let shift = size * 0.025 * (step + 1) as f32 * grow;
                    frame.fill_text(Text {
                        content: span.text.clone(),
                        position: Point::new(shift, shift),
                        color: Color { a: color.a * alpha * grow, ..*color },
                        size: span.size.into(),
                        font: self.font,
                        line_height: iced::widget::text::LineHeight::Relative(1.02),
                        ..Text::default()
                    });
                }
                frame.fill_text(Text {
                    content: span.text.clone(),
                    position: Point::ORIGIN,
                    color: Color { a: self.color.a * alpha, ..self.color },
                    size: span.size.into(),
                    font: self.font,
                    line_height: iced::widget::text::LineHeight::Relative(1.02),
                    ..Text::default()
                });
            });
        }
        vec![frame.into_geometry()]
    }
}

/// `BRAVAIS_POSTER_TILT_DEG` and its timing: in after 120ms over 950ms, back over 500ms, `[0.45, 0, 0.2, 1]`.
const TILT_DEG: f32 = -10.0;
const TILT_IN_DELAY_MS: f32 = 120.0;
const TILT_IN_MS: f32 = 950.0;
const TILT_OUT_MS: f32 = 500.0;
const TILT_EASE: Ease = Ease::new(0.45, 0.0, 0.2, 1.0);

/// The whole layer turning with a poster on screen (`useBravaisPosterTilt`), in degrees.
#[derive(Debug, Clone, Copy)]
pub struct Tilt {
    from: f32,
    to: f32,
    at: std::time::Instant,
    delay: f32,
    ms: f32,
}

impl Tilt {
    pub fn new(now: std::time::Instant) -> Self {
        Self { from: 0.0, to: 0.0, at: now, delay: 0.0, ms: TILT_OUT_MS }
    }

    /// Tilts while `on`, straightens otherwise; a change starts from where it is.
    pub fn set(&mut self, on: bool, now: std::time::Instant) {
        let goal = if on { TILT_DEG } else { 0.0 };
        if goal == self.to {
            return;
        }
        self.from = self.deg(now);
        self.to = goal;
        self.at = now;
        (self.delay, self.ms) = if on { (TILT_IN_DELAY_MS, TILT_IN_MS) } else { (0.0, TILT_OUT_MS) };
    }

    pub fn deg(&self, now: std::time::Instant) -> f32 {
        let elapsed = now.saturating_duration_since(self.at).as_secs_f32() * 1000.0 - self.delay;
        let t = (elapsed / self.ms).clamp(0.0, 1.0);
        self.from + (self.to - self.from) * TILT_EASE.at(f64::from(t)) as f32
    }

    pub fn running(&self, now: std::time::Instant) -> bool {
        now.saturating_duration_since(self.at).as_secs_f32() * 1000.0 < self.delay + self.ms && self.from != self.to
    }
}

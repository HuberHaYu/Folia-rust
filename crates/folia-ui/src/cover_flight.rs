// The cover flying between a wall tile and the album poster (`bravaisCoverFlight`).

use std::time::Instant;

use iced::widget::canvas::{self, Frame, Geometry, Path, Program};
use iced::widget::image::Handle;
use iced::{Color, Point, Radians, Rectangle, Renderer, Size, Theme, Vector, mouse};

/// `FLIGHT_MS`, `BACK_FLIGHT_MS`; a lost landing fades in 150ms, a flight back fades off its tile in 200ms.
const FLIGHT_MS: f32 = 640.0;
const BACK_FLIGHT_MS: f32 = 560.0;
const ABANDON_FADE_MS: f32 = 150.0;
const LAND_FADE_MS: f32 = 200.0;
/// A landing that does not come within this long past the flight is given up (`elapsed < duration + 600`).
const LANDING_WAIT_MS: f32 = 600.0;
/// The poster cover's ink shadow, 12px down-right in its own frame.
const POSTER_SHADOW: f32 = 12.0;

/// A turned rectangle: its middle, size and angle in radians (`BravaisQuad`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quad {
    pub center: Point,
    pub size: Size,
    pub angle: f32,
}

impl Quad {
    pub fn of(rect: Rectangle, angle: f32) -> Self {
        Self { center: rect.center(), size: rect.size(), angle }
    }

    /// `lerpQuad`: the angle goes the short way round.
    fn lerp(self, to: Quad, t: f32) -> Quad {
        let mut turn = to.angle - self.angle;
        while turn > std::f32::consts::PI {
            turn -= std::f32::consts::TAU;
        }
        while turn < -std::f32::consts::PI {
            turn += std::f32::consts::TAU;
        }
        let mix = |a: f32, b: f32| a + (b - a) * t;
        Quad {
            center: Point::new(mix(self.center.x, to.center.x), mix(self.center.y, to.center.y)),
            size: Size::new(mix(self.size.width, to.size.width), mix(self.size.height, to.size.height)),
            angle: self.angle + turn * t,
        }
    }
}

/// `easeInOut` of the flight: a cubic.
fn ease_in_out(t: f32) -> f32 {
    if t < 0.5 { 4.0 * t * t * t } else { 1.0 - (-2.0 * t + 2.0).powi(3) / 2.0 }
}

/// One flight: out to the poster (landing by swapping in the poster's cover) or back to the wall (landing by fading
/// over the tile).
pub struct Flight {
    pub image: Handle,
    from: Quad,
    pub back: bool,
    started: Instant,
    /// The landing spot as last measured, and when the flight ended (landed or given up).
    last: Quad,
    ended: Option<(Instant, bool)>,
    ink: Color,
}

impl Flight {
    pub fn new(image: Handle, from: Quad, back: bool, ink: Color, now: Instant) -> Self {
        Self { image, from, back, started: now, last: from, ended: None, ink }
    }

    fn duration(&self) -> f32 {
        if self.back { BACK_FLIGHT_MS } else { FLIGHT_MS }
    }

    fn elapsed(&self, now: Instant) -> f32 {
        now.saturating_duration_since(self.started).as_secs_f32() * 1000.0
    }

    /// One frame: the landing spot as measured now (`None` while there is none). Returns true once the cover has
    /// landed this frame (`onDone`), so the poster's own cover shows in the same frame.
    pub fn step(&mut self, target: Option<Quad>, now: Instant) -> bool {
        if self.ended.is_some() {
            return false;
        }
        if let Some(target) = target {
            self.last = target;
        }
        let elapsed = self.elapsed(now);
        if elapsed >= self.duration() {
            if target.is_some() {
                self.ended = Some((now, true));
                return true;
            }
            if elapsed >= self.duration() + LANDING_WAIT_MS {
                self.ended = Some((now, false));
            }
        }
        false
    }

    /// The poster's cover stays hidden until the flight out has landed (`data-flight="pending"`).
    pub fn pending(&self) -> bool {
        !self.back && self.ended.is_none()
    }

    /// Whether anything of the flight is still drawn.
    pub fn alive(&self, now: Instant) -> bool {
        match self.ended {
            None => true,
            // Out: the swap leaves nothing to draw. Back: the fade off the tile; given up: the fade where it is.
            Some((at, landed)) => {
                let fade = if landed { if self.back { LAND_FADE_MS } else { 0.0 } } else { ABANDON_FADE_MS };
                now.saturating_duration_since(at).as_secs_f32() * 1000.0 < fade
            }
        }
    }

    /// Where the cover is drawn and how much of it shows.
    fn drawn(&self, now: Instant) -> (Quad, f32, f32) {
        let t = ease_in_out((self.elapsed(now) / self.duration()).min(1.0));
        let quad = self.from.lerp(self.last, t);
        let opacity = match self.ended {
            Some((at, landed)) => {
                let fade = if landed { LAND_FADE_MS } else { ABANDON_FADE_MS };
                1.0 - (now.saturating_duration_since(at).as_secs_f32() * 1000.0 / fade).min(1.0)
            }
            None => 1.0,
        };
        (quad, t, opacity)
    }
}

/// Draws the flying cover over everything: lifted a little mid-way, its hard ink shadow growing toward the poster's.
pub struct Overlay<'a> {
    pub flight: &'a Flight,
    pub now: Instant,
}

impl<Message> Program<Message> for Overlay<'_> {
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
        let (quad, t, opacity) = self.flight.drawn(self.now);
        if opacity <= 0.0 {
            return vec![frame.into_geometry()];
        }
        // `scale(1 + sin(πt) · 0.05)`.
        let lift = (std::f32::consts::PI * t).sin();
        let size = Size::new(quad.size.width * (1.0 + lift * 0.05), quad.size.height * (1.0 + lift * 0.05));
        // The hard shadow runs from none (the tile) to the poster's 12px ink, fading out and back in across the middle.
        let (from_offset, to_offset) = if self.flight.back { (POSTER_SHADOW, 0.0) } else { (0.0, POSTER_SHADOW) };
        let offset = from_offset + (to_offset - from_offset) * t;
        let shadow_alpha = if t < 0.5 { 1.0 - t * 2.0 } else { t * 2.0 - 1.0 };
        let shown_alpha = if (t < 0.5) == self.flight.back { shadow_alpha } else { 0.0 };
        frame.with_save(|frame| {
            frame.translate(Vector::new(quad.center.x, quad.center.y));
            frame.rotate(Radians(quad.angle));
            if offset > 0.0 && shown_alpha > 0.0 {
                let shadow = Path::rectangle(Point::new(-size.width / 2.0 + offset, -size.height / 2.0 + offset), size);
                frame.fill(&shadow, Color { a: self.flight.ink.a * shown_alpha * opacity, ..self.flight.ink });
            }
        });
        // `background: center / cover`: the square cover over the longer side.
        let side = size.width.max(size.height);
        let square =
            Rectangle::new(Point::new(quad.center.x - side / 2.0, quad.center.y - side / 2.0), Size::new(side, side));
        let image = canvas::Image::new(self.flight.image.clone()).rotation(Radians(quad.angle)).opacity(opacity);
        if quad.angle.abs() < 0.001 {
            let clip =
                Rectangle::new(Point::new(quad.center.x - size.width / 2.0, quad.center.y - size.height / 2.0), size);
            frame.with_clip(clip, |clipped| clipped.draw_image(square, image));
        } else {
            frame.draw_image(square, image);
        }
        vec![frame.into_geometry()]
    }
}

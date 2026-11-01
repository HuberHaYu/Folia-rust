// Layer changes on the wall (`bravaisShift.ts`, `flipPlan.ts`, `bravaisWallWave.ts`, `BravaisTile.tsx`): tiles turn
// over one by one from where the change began, or the whole wall lifts away and lands again.

use std::time::Instant;

use folia_visualizer::Ease;
use iced::{Point, Rectangle};

/// World distance between neighbouring cells (`getPitch`: cell 128 + gap 8).
const PITCH: f32 = 136.0;

// `flipPlan.ts` and `BRAVAIS_TILE_FLIP_OUT_MS` / `IN_MS`.
const FLIP_STAGGER_MS_PER_CELL: f32 = 18.0;
const FLIP_MAX_DELAY_MS: f32 = 420.0;
const FLIP_OUT_MS: f32 = 150.0;
const FLIP_IN_MS: f32 = 210.0;
const FLIP_OUT_EASE: Ease = Ease::new(0.55, 0.0, 0.9, 0.45);
const FLIP_IN_EASE: Ease = Ease::new(0.2, 0.7, 0.25, 1.0);

// `bravaisWallWave.ts` and the lift keyframes of `BravaisTile.tsx`.
const WAVE_LIFT: f32 = 90.0;
const WAVE_LIFTED_SCALE: f32 = 0.88;
const WAVE_STAGGER_MS_PER_CELL: f32 = 30.0;
const WAVE_MAX_DELAY_MS: f32 = 340.0;
const WAVE_OUT_MS: f32 = 280.0;
const WAVE_IN_MS: f32 = 420.0;
const WAVE_EXIT_TOTAL_MS: f32 = WAVE_MAX_DELAY_MS + WAVE_OUT_MS;
const WAVE_OUT_EASE: Ease = Ease::new(0.4, 0.0, 1.0, 1.0);
const WAVE_IN_EASE: Ease = Ease::new(0.2, 0.9, 0.3, 1.04);

/// The kind of change (`resolveShiftKind`): a push, back or data update turns tiles over from a point; a home tab
/// switch (`replace` between home layers) sends the whole wall away and back.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    /// The pressed tile for a push, the seam for anything else; in screen coordinates.
    Flip(Point),
    Wave,
}

/// One tile at a moment of a shift. The turn about the vertical axis is drawn as a squeeze across the tile's middle:
/// iced has no perspective transform, and the squeeze is the turn seen head on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    pub squeeze: f32,
    /// Screen pixels the tile is raised by.
    pub lift: f32,
    pub scale: f32,
    pub alpha: f32,
}

impl Pose {
    pub const REST: Pose = Pose { squeeze: 1.0, lift: 0.0, scale: 1.0, alpha: 1.0 };

    fn turned(degrees: f32) -> Self {
        Self { squeeze: degrees.to_radians().cos().max(0.0), ..Self::REST }
    }

    fn lifted(progress: f32, wall_scale: f32) -> Self {
        Self {
            squeeze: 1.0,
            lift: WAVE_LIFT * progress * wall_scale,
            scale: 1.0 - (1.0 - WAVE_LIFTED_SCALE) * progress,
            alpha: (1.0 - progress).clamp(0.0, 1.0),
        }
    }
}

/// A shift in progress. Rectangles are the tiles' screen rectangles in the wall's own coordinates.
#[derive(Debug, Clone, Copy)]
pub struct Shift {
    kind: Kind,
    started: Instant,
    /// The wall's zoom: staggers count cells in world units.
    scale: f32,
    end_ms: f32,
}

impl Shift {
    pub fn new(kind: Kind, scale: f32, now: Instant, rects: impl Iterator<Item = Rectangle>) -> Self {
        let mut shift = Self { kind, started: now, scale: scale.max(0.01), end_ms: 0.0 };
        shift.end_ms = rects
            .map(|rect| match kind {
                Kind::Flip(_) => shift.flip_delay(rect) + FLIP_OUT_MS + FLIP_IN_MS,
                Kind::Wave => WAVE_EXIT_TOTAL_MS + shift.wave_stagger(rect) + WAVE_IN_MS,
            })
            .fold(0.0, f32::max);
        shift
    }

    pub fn is_wave(&self) -> bool {
        self.kind == Kind::Wave
    }

    /// Lets tiles laid out during the shift finish on its schedule.
    pub fn reach(&mut self, rects: impl Iterator<Item = Rectangle>) {
        let later = Self::new(self.kind, self.scale, self.started, rects);
        self.end_ms = self.end_ms.max(later.end_ms);
    }

    pub fn running(&self, now: Instant) -> bool {
        self.elapsed(now) < self.end_ms
    }

    /// A tile of the outgoing layer; `None` once it has turned away or lifted off.
    pub fn leaving(&self, rect: Rectangle, now: Instant) -> Option<Pose> {
        let t = self.elapsed(now);
        match self.kind {
            Kind::Flip(_) => {
                let delay = self.flip_delay(rect);
                match t - delay {
                    t if t < 0.0 => Some(Pose::REST),
                    t if t < FLIP_OUT_MS => Some(Pose::turned(90.0 * ease(FLIP_OUT_EASE, t / FLIP_OUT_MS))),
                    _ => None,
                }
            }
            Kind::Wave => {
                let delay = WAVE_MAX_DELAY_MS - self.wave_stagger(rect);
                match t - delay {
                    t if t < 0.0 => Some(Pose::REST),
                    t if t < WAVE_OUT_MS => Some(Pose::lifted(ease(WAVE_OUT_EASE, t / WAVE_OUT_MS), self.scale)),
                    _ => None,
                }
            }
        }
    }

    /// A tile of the incoming layer; `None` until it turns in or lands.
    pub fn arriving(&self, rect: Rectangle, now: Instant) -> Option<Pose> {
        let t = self.elapsed(now);
        match self.kind {
            Kind::Flip(_) => match t - self.flip_delay(rect) - FLIP_OUT_MS {
                t if t < 0.0 => None,
                t if t < FLIP_IN_MS => Some(Pose::turned(90.0 * (1.0 - ease(FLIP_IN_EASE, t / FLIP_IN_MS)))),
                _ => Some(Pose::REST),
            },
            Kind::Wave => match t - WAVE_EXIT_TOTAL_MS - self.wave_stagger(rect) {
                t if t < 0.0 => None,
                t if t < WAVE_IN_MS => Some(Pose::lifted(1.0 - ease(WAVE_IN_EASE, t / WAVE_IN_MS), self.scale)),
                _ => Some(Pose::REST),
            },
        }
    }

    fn elapsed(&self, now: Instant) -> f32 {
        now.saturating_duration_since(self.started).as_secs_f32() * 1000.0
    }

    /// `getFlipDelay`: 18ms for each cell between the tile and the origin, at most 420ms.
    fn flip_delay(&self, rect: Rectangle) -> f32 {
        let Kind::Flip(origin) = self.kind else { return 0.0 };
        let cells = rect.center().distance(origin) / self.scale / PITCH;
        (cells * FLIP_STAGGER_MS_PER_CELL).min(FLIP_MAX_DELAY_MS)
    }

    /// `getWaveStagger`: 30ms for each cell along the diagonal from the viewport's top-left corner, at most 340ms.
    fn wave_stagger(&self, rect: Rectangle) -> f32 {
        let cells = (rect.x.max(0.0) + rect.y.max(0.0)) / self.scale / PITCH;
        (cells * WAVE_STAGGER_MS_PER_CELL).min(WAVE_MAX_DELAY_MS)
    }
}

fn ease(curve: Ease, t: f32) -> f32 {
    curve.at(f64::from(t.clamp(0.0, 1.0))) as f32
}

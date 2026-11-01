// Wall camera input from `wallPanMotion.ts` and `useWallPointerPan.ts`: drag, release inertia and wheel glide.

use std::time::Instant;

use iced::{Point, Vector};

/// `DRAG_THRESHOLD_PX`: a press becomes a drag once it has moved this far.
pub const DRAG_THRESHOLD: f32 = 7.0;
/// `BRAVAIS_OVERSCAN`: world distance laid out (and fetched) past the viewport.
pub const OVERSCAN: f32 = 420.0;
/// `CULL_EDGE_MARGIN_PX`: the layout is cut again once the camera has used up all but this much of the overscan.
pub const CULL_EDGE_MARGIN: f32 = 180.0;

// `WALL_PAN_INERTIA`.
const INERTIA_MIN_SPEED: f32 = 40.0;
const INERTIA_MAX_SPEED: f32 = 4000.0;
const INERTIA_POWER: f32 = 0.3;
const INERTIA_TIME_CONSTANT_MS: f32 = 280.0;
const INERTIA_REST_DELTA: f32 = 0.5;
const STALE_RELEASE_MS: f32 = 80.0;

// `WALL_WHEEL_SMOOTHING`.
const WHEEL_TIME_CONSTANT_MS: f32 = 80.0;
const WHEEL_REST_PX: f32 = 0.5;
const WHEEL_MAX_FRAME_MS: f32 = 50.0;
const WHEEL_MIN_NOTCH_PX: f32 = 50.0;
const WHEEL_GESTURE_GAP_MS: f32 = 120.0;
/// Chromium's pixels for one wheel notch: about 100 on Windows and macOS, about 53 on Linux.
const WHEEL_NOTCH_PX: f32 = if cfg!(target_os = "linux") { 53.0 } else { 100.0 };

/// Browsers time-stamp each pointer event; iced does not, and moves handled in one batch arrive microseconds apart.
/// The release speed is therefore measured over the moves of the last few frames instead of the last pair.
const VELOCITY_WINDOW_MS: f32 = 48.0;
const SAMPLES: usize = 8;

/// Where a wheel event came from (`createWallWheelClassifier`): a notched wheel glides, a trackpad follows at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WheelSource {
    Wheel,
    Trackpad,
}

/// The pointer on the wall between press and release, and the wheel gesture in progress.
#[derive(Debug, Default)]
pub struct Pointer {
    pressed_at: Option<Point>,
    last: Option<Point>,
    dragged: bool,
    samples: [Option<(Instant, Point)>; SAMPLES],
    next: usize,
    gesture: Option<(Instant, WheelSource)>,
    pub shift: bool,
}

/// What a pointer event does to the camera.
pub enum Gesture {
    None,
    /// Screen pixels the pointer has dragged the wall by.
    Drag(Vector),
    /// A press and release without a drag, where it was pressed.
    Click(Point),
    /// The end of a drag.
    Release(Release),
}

#[derive(Debug, Clone, Copy)]
pub struct Release {
    /// Pointer velocity in screen px/s.
    pub velocity: Vector,
    pub stale: bool,
}

impl Pointer {
    pub fn press(&mut self, at: Point, now: Instant) {
        *self = Self { shift: self.shift, gesture: self.gesture, ..Self::default() };
        self.pressed_at = Some(at);
        self.last = Some(at);
        self.sample(at, now);
    }

    pub fn pressed(&self) -> bool {
        self.pressed_at.is_some()
    }

    /// The camera follows the pointer from where it was pressed once it has moved past the threshold.
    pub fn moved(&mut self, at: Point, now: Instant) -> Gesture {
        let (Some(from), Some(last)) = (self.pressed_at, self.last) else { return Gesture::None };
        self.sample(at, now);
        self.last = Some(at);
        if self.dragged {
            return Gesture::Drag(at - last);
        }
        if from.distance(at) <= DRAG_THRESHOLD {
            return Gesture::None;
        }
        self.dragged = true;
        Gesture::Drag(at - from)
    }

    pub fn release(&mut self, now: Instant) -> Gesture {
        let Some(from) = self.pressed_at.take() else { return Gesture::None };
        if !std::mem::take(&mut self.dragged) {
            return Gesture::Click(from);
        }
        let newest = self.samples[(self.next + SAMPLES - 1) % SAMPLES];
        let stale = newest.is_none_or(|(at, _)| ms(now, at) > STALE_RELEASE_MS);
        Gesture::Release(Release { velocity: self.velocity(), stale })
    }

    /// Classifies a wheel event and turns it into the screen distance the camera moves (`wallWheelDelta`).
    pub fn wheel(&mut self, delta: iced::mouse::ScrollDelta, now: Instant) -> (WheelSource, Vector) {
        let same = self.gesture.is_some_and(|(at, _)| ms(now, at) <= WHEEL_GESTURE_GAP_MS);
        let previous = if same { self.gesture.map(|(_, source)| source) } else { None };
        let (source, x, y) = match delta {
            iced::mouse::ScrollDelta::Lines { x, y } => (WheelSource::Wheel, -x * WHEEL_NOTCH_PX, -y * WHEEL_NOTCH_PX),
            iced::mouse::ScrollDelta::Pixels { x, y } => {
                let single_axis = (x == 0.0) != (y == 0.0);
                let notch = x.abs().max(y.abs()) >= WHEEL_MIN_NOTCH_PX;
                let wheel = single_axis && notch && previous != Some(WheelSource::Trackpad);
                (if wheel { WheelSource::Wheel } else { WheelSource::Trackpad }, -x, -y)
            }
        };
        self.gesture = Some((now, source));
        let delta = if self.shift && x == 0.0 { Vector::new(y, 0.0) } else { Vector::new(x, y) };
        (source, delta)
    }

    fn sample(&mut self, at: Point, now: Instant) {
        self.samples[self.next] = Some((now, at));
        self.next = (self.next + 1) % SAMPLES;
    }

    /// Distance over time from the oldest move inside the window to the newest.
    fn velocity(&self) -> Vector {
        let Some((newest_at, newest)) = self.samples[(self.next + SAMPLES - 1) % SAMPLES] else { return Vector::ZERO };
        let mut oldest = None;
        for step in 2..=SAMPLES {
            let Some((at, point)) = self.samples[(self.next + SAMPLES - step) % SAMPLES] else { break };
            if ms(newest_at, at) > VELOCITY_WINDOW_MS {
                break;
            }
            oldest = Some((at, point));
        }
        let Some((oldest_at, oldest)) = oldest else { return Vector::ZERO };
        let seconds = ms(newest_at, oldest_at) / 1000.0;
        if seconds <= 0.0 {
            return Vector::ZERO;
        }
        (newest - oldest) * (1.0 / seconds)
    }
}

/// Release inertia (framer `inertia`): the camera runs on `power × speed` px, closing in with a 280ms time constant.
#[derive(Debug, Clone, Copy)]
pub struct Inertia {
    from: Vector,
    /// Unit direction the camera travels in, in world units per screen pixel.
    along: Vector,
    amplitude: f32,
    started: Instant,
}

impl Inertia {
    /// `None` for a stale or slow release, which does not coast.
    pub fn start(release: Release, from: Vector, scale: f32, now: Instant) -> Option<Self> {
        let speed = release.velocity.x.hypot(release.velocity.y);
        if release.stale || speed < INERTIA_MIN_SPEED {
            return None;
        }
        let amplitude = speed.min(INERTIA_MAX_SPEED) * INERTIA_POWER;
        let along = release.velocity * (-1.0 / speed / scale.max(0.01));
        Some(Self { from, along, amplitude, started: now })
    }

    /// The camera at `now`, and whether it is still moving.
    pub fn at(&self, now: Instant) -> (Vector, bool) {
        let remaining = self.amplitude * (-ms(now, self.started) / INERTIA_TIME_CONSTANT_MS).exp();
        let moving = remaining >= INERTIA_REST_DELTA;
        let travelled = if moving { self.amplitude - remaining } else { self.amplitude };
        (self.from + self.along * travelled, moving)
    }
}

/// Wheel notches add to a distance still to go, which the camera closes in on each frame (`stepWallWheel`).
#[derive(Debug, Clone, Copy, Default)]
pub struct Glide {
    /// Screen pixels the camera has yet to move.
    pending: Vector,
    last: Option<Instant>,
}

impl Glide {
    pub fn add(&mut self, delta: Vector) {
        self.pending += delta;
    }

    pub fn pending(&self) -> Vector {
        self.pending
    }

    /// This frame's screen step; `None` once the glide has landed.
    pub fn step(&mut self, now: Instant) -> Option<Vector> {
        if self.pending == Vector::ZERO {
            return None;
        }
        let dt = self.last.map_or(1000.0 / 60.0, |last| ms(now, last)).clamp(0.0, WHEEL_MAX_FRAME_MS);
        self.last = Some(now);
        let factor = 1.0 - (-dt / WHEEL_TIME_CONSTANT_MS).exp();
        let step = self.pending * factor;
        let after = self.pending - step;
        if after.x.hypot(after.y) < WHEEL_REST_PX {
            let rest = self.pending;
            self.pending = Vector::ZERO;
            return Some(rest);
        }
        self.pending = after;
        Some(step)
    }
}

fn ms(later: Instant, earlier: Instant) -> f32 {
    later.saturating_duration_since(earlier).as_secs_f32() * 1000.0
}

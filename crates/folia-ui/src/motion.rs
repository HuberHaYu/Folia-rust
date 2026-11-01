// Wall-clock tweens for chrome that eases between two values, like a CSS transition.

use std::time::Instant;

use folia_visualizer::Ease;

/// Tailwind's default `transition` curve.
pub const CSS_TRANSITION: Ease = Ease::new(0.4, 0.0, 0.2, 1.0);

/// A value that eases from where it was towards `goal` whenever the goal changes.
#[derive(Debug, Clone, Copy)]
pub struct Tween {
    from: f32,
    goal: f32,
    at: Instant,
    ms: f64,
    ease: Ease,
}

impl Tween {
    pub fn new(value: f32, ms: f64, ease: Ease, now: Instant) -> Self {
        Self { from: value, goal: value, at: now, ms, ease }
    }

    pub fn value(&self, now: Instant) -> f32 {
        let elapsed = now.saturating_duration_since(self.at).as_secs_f64() * 1000.0;
        folia_visualizer::tween(f64::from(self.from), f64::from(self.goal), elapsed, self.ms, self.ease) as f32
    }

    /// Starts easing towards `goal` from the current value; the same goal keeps the running tween.
    pub fn set(&mut self, goal: f32, now: Instant) {
        if (goal - self.goal).abs() < f32::EPSILON {
            return;
        }
        self.from = self.value(now);
        self.goal = goal;
        self.at = now;
    }

    /// `set` with this transition's own duration, for an enter and an exit that differ.
    pub fn set_with(&mut self, goal: f32, ms: f64, ease: Ease, now: Instant) {
        if (goal - self.goal).abs() < f32::EPSILON {
            return;
        }
        self.from = self.value(now);
        self.goal = goal;
        self.at = now;
        self.ms = ms;
        self.ease = ease;
    }

    /// Jumps to `from` and eases back to the goal, for content that enters again.
    pub fn restart(&mut self, from: f32, now: Instant) {
        self.from = from;
        self.at = now;
    }

    pub fn running(&self, now: Instant) -> bool {
        (self.from - self.goal).abs() > 0.0001
            && now.saturating_duration_since(self.at).as_secs_f64() * 1000.0 < self.ms
    }
}

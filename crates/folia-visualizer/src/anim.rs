// Time-based animation curves matching the framer-motion transitions the original styles use.

/// `cubic-bezier(x1, y1, x2, y2)` easing as CSS defines it.
#[derive(Debug, Clone, Copy)]
pub struct Ease(f64, f64, f64, f64);

impl Ease {
    pub const LINEAR: Ease = Ease(0.0, 0.0, 1.0, 1.0);
    pub const OUT: Ease = Ease(0.0, 0.0, 0.58, 1.0);
    pub const IN_OUT: Ease = Ease(0.42, 0.0, 0.58, 1.0);

    pub const fn new(x1: f64, y1: f64, x2: f64, y2: f64) -> Ease {
        Ease(x1, y1, x2, y2)
    }

    /// Eased progress for linear progress `t` in `0.0..=1.0`.
    pub fn at(self, t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        let Ease(x1, y1, x2, y2) = self;
        if x1 == y1 && x2 == y2 {
            return t;
        }
        let bezier = |a: f64, b: f64, s: f64| 3.0 * a * s * (1.0 - s).powi(2) + 3.0 * b * s * s * (1.0 - s) + s.powi(3);
        let (mut low, mut high) = (0.0, 1.0);
        for _ in 0..20 {
            let mid = (low + high) / 2.0;
            if bezier(x1, x2, mid) < t {
                low = mid;
            } else {
                high = mid;
            }
        }
        bezier(y1, y2, (low + high) / 2.0)
    }
}

/// A tween from `from` to `to` that started `elapsed` seconds ago.
pub fn tween(from: f64, to: f64, elapsed: f64, duration: f64, ease: Ease) -> f64 {
    if duration <= 0.0 || elapsed >= duration {
        return to;
    }
    from + (to - from) * ease.at(elapsed.max(0.0) / duration)
}

/// A spring released at rest from `from` towards `to` `elapsed` seconds ago (unit mass).
pub fn spring(from: f64, to: f64, elapsed: f64, stiffness: f64, damping: f64) -> f64 {
    let t = elapsed.max(0.0);
    let offset = from - to;
    let omega = stiffness.sqrt();
    let zeta = damping / (2.0 * omega);
    let decay = (-zeta * omega * t).exp();
    let displacement = if zeta < 1.0 {
        let omega_d = omega * (1.0 - zeta * zeta).sqrt();
        offset * decay * ((omega_d * t).cos() + zeta * omega / omega_d * (omega_d * t).sin())
    } else {
        offset * decay * (1.0 + omega * t)
    };
    to + displacement
}

/// Evaluates keyframes `values` placed at `times` (fractions of `duration`), easing each segment.
pub fn keyframes(values: &[f64], times: &[f64], elapsed: f64, duration: f64, ease: Ease) -> f64 {
    let progress = if duration > 0.0 { (elapsed / duration).clamp(0.0, 1.0) } else { 1.0 };
    for (i, pair) in times.windows(2).enumerate() {
        if progress <= pair[1] {
            let span = pair[1] - pair[0];
            let local = if span > 0.0 { (progress - pair[0]) / span } else { 1.0 };
            return values[i] + (values[i + 1] - values[i]) * ease.at(local);
        }
    }
    values.last().copied().unwrap_or_default()
}

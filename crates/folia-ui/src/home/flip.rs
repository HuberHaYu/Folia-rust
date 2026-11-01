// The seam's content turning over when the layer or the home tab changes (`bravaisSeamMotion.ts`, `useBravaisSeam.ts`):
// out to 90° in 160ms `ease-in`, the content swapped, back from −90° in 240ms `cubic-bezier(.2,.7,.25,1)`. Widgets
// cannot be squeezed across in iced, so the turn shows as the content's opacity following the cosine of its angle.

use std::time::Instant;

use folia_store::HomeTab;
use folia_visualizer::Ease;

use super::LayerSeam;

const OUT_MS: f32 = 160.0;
const IN_MS: f32 = 240.0;
const OUT_EASE: Ease = Ease::new(0.42, 0.0, 1.0, 1.0);
const IN_EASE: Ease = Ease::new(0.2, 0.7, 0.25, 1.0);

/// The seam content as it was before the change, shown until the turn reaches 90°.
pub struct SeamFlip {
    at: Instant,
    pub tab: HomeTab,
    pub layer: Option<LayerSeam>,
    pub spine: bool,
    pub focus_title: bool,
}

impl SeamFlip {
    pub fn new(tab: HomeTab, layer: Option<LayerSeam>, spine: bool, focus_title: bool, now: Instant) -> Self {
        Self { at: now, tab, layer, spine, focus_title }
    }

    /// Whether the outgoing content is still turning away.
    pub fn leaving(&self, now: Instant) -> bool {
        self.elapsed(now) < OUT_MS
    }

    pub fn running(&self, now: Instant) -> bool {
        self.elapsed(now) < OUT_MS + IN_MS
    }

    /// The content's opacity: the cosine of how far it has turned.
    pub fn opacity(&self, now: Instant) -> f32 {
        let t = self.elapsed(now);
        let degrees = if t < OUT_MS {
            90.0 * OUT_EASE.at(f64::from(t / OUT_MS)) as f32
        } else {
            90.0 * (1.0 - IN_EASE.at(f64::from(((t - OUT_MS) / IN_MS).min(1.0))) as f32)
        };
        degrees.to_radians().cos().max(0.0)
    }

    fn elapsed(&self, now: Instant) -> f32 {
        now.saturating_duration_since(self.at).as_secs_f32() * 1000.0
    }
}

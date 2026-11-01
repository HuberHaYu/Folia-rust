// Home ↔ player cross-fade (`App.tsx`: the home layer eases its opacity over 0.25s `easeInOut` above the visualizer).
// iced has no opacity for a widget tree, so the page being left is captured once and faded out over the new one.

use std::time::Instant;

use folia_visualizer::Ease;
use iced::advanced::image::Handle;
use iced::widget::image;
use iced::{ContentFit, Element, Length};

use crate::motion::Tween;

const FADE_MS: f64 = 250.0;
/// framer-motion's `easeInOut`.
const EASE_IN_OUT: Ease = Ease::new(0.42, 0.0, 0.58, 1.0);

/// The page that was left, fading out over the one that took its place.
pub struct PageFade {
    shot: Handle,
    shown: Tween,
}

impl PageFade {
    pub fn new(shot: iced::window::Screenshot, now: Instant) -> Self {
        let mut shown = Tween::new(1.0, FADE_MS, EASE_IN_OUT, now);
        shown.set(0.0, now);
        Self { shot: Handle::from_rgba(shot.size.width, shot.size.height, shot.rgba), shown }
    }

    pub fn running(&self, now: Instant) -> bool {
        self.shown.running(now)
    }

    pub fn view<'a, M: 'a>(&self, now: Instant) -> Element<'a, M> {
        image(self.shot.clone())
            .width(Length::Fill)
            .height(Length::Fill)
            .content_fit(ContentFit::Fill)
            .opacity(self.shown.value(now))
            .into()
    }
}

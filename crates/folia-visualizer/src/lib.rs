// Lyric animation styles drawn on an iced canvas; every frame is a pure function of playback time.

mod anim;
mod registry;
pub mod styles;

pub use anim::{Ease, keyframes, spring, tween};
pub use registry::Style;

use std::cell::RefCell;
use std::rc::Rc;

use folia_core::{Line, Lyrics, Rgb, Theme};
use iced::advanced::text::{self as core_text, Paragraph as _};
use iced::widget::canvas::{self, Frame, Geometry};
use iced::{Color, Font, Rectangle, Renderer, Size, mouse};

pub(crate) const BOLD: Font = Font { weight: iced::font::Weight::Bold, ..Font::DEFAULT };

/// Everything a style needs to draw one frame.
pub struct Scene<'a> {
    pub style: Style,
    pub lyrics: &'a Lyrics,
    /// Playback position in seconds.
    pub time: f64,
    pub theme: &'a Theme,
    pub font_scale: f32,
}

/// Per-widget state: measured line layouts survive between frames.
#[derive(Default)]
pub struct SceneState {
    classic: RefCell<LayoutCache<styles::classic::LineLayout>>,
    partita: RefCell<LayoutCache<styles::partita::PartitaLayout>>,
    tilt: RefCell<LayoutCache<styles::tilt::TiltLayout>>,
    still: styles::still::StillCache,
}

impl<Message> canvas::Program<Message> for Scene<'_> {
    type State = SceneState;

    fn draw(
        &self,
        state: &SceneState,
        renderer: &Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let size = bounds.size();
        match self.style {
            Style::Classic | Style::Partita | Style::Tilt => {
                let mut frame = Frame::new(renderer, size);
                frame.fill_rectangle(iced::Point::ORIGIN, size, color(self.theme.background, 1.0));
                match self.style {
                    Style::Partita => styles::partita::draw(self, &mut state.partita.borrow_mut(), &mut frame, size),
                    Style::Tilt => styles::tilt::draw(self, &mut state.tilt.borrow_mut(), &mut frame, size),
                    _ => styles::classic::draw(self, &mut state.classic.borrow_mut(), &mut frame, size),
                }
                vec![frame.into_geometry()]
            }
            Style::Still => vec![styles::still::draw(self, &state.still, renderer, size)],
        }
    }
}

/// Keeps the layouts of the few lines on screen, keyed by line start, canvas size and font scale.
pub struct LayoutCache<T> {
    entries: Vec<(LayoutKey, Rc<T>)>,
}

#[derive(Clone, Copy, PartialEq)]
struct LayoutKey {
    start: u64,
    width: u32,
    height: u32,
    font_scale: u32,
}

impl<T> Default for LayoutCache<T> {
    fn default() -> Self {
        Self { entries: Vec::new() }
    }
}

impl<T> LayoutCache<T> {
    const CAPACITY: usize = 4;

    pub fn get_or_insert(&mut self, line: &Line, size: Size, font_scale: f32, build: impl FnOnce() -> Rc<T>) -> Rc<T> {
        let key = LayoutKey {
            start: line.start.to_bits(),
            width: size.width.to_bits(),
            height: size.height.to_bits(),
            font_scale: font_scale.to_bits(),
        };
        if let Some((_, layout)) = self.entries.iter().find(|(k, _)| *k == key) {
            return layout.clone();
        }
        let layout = build();
        if self.entries.len() == Self::CAPACITY {
            self.entries.remove(0);
        }
        self.entries.push((key, layout.clone()));
        layout
    }
}

pub fn color(rgb: Rgb, alpha: f32) -> Color {
    Color::from_rgba8(rgb.r, rgb.g, rgb.b, alpha.clamp(0.0, 1.0))
}

/// Size of `content` set in `font` at `size` pixels, wrapping at word boundaries past `max_width`.
pub fn measure(content: &str, size: f32, font: Font, max_width: f32) -> Size {
    let paragraph = <Renderer as core_text::Renderer>::Paragraph::with_text(core_text::Text {
        content,
        bounds: Size::new(max_width, f32::INFINITY),
        size: size.into(),
        line_height: core_text::LineHeight::Relative(1.22),
        font,
        align_x: core_text::Alignment::Default,
        align_y: iced::alignment::Vertical::Top,
        shaping: core_text::Shaping::Advanced,
        wrapping: core_text::Wrapping::Word,
    });
    paragraph.min_bounds()
}

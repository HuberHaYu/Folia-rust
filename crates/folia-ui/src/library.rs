// Home page: the cover wall. Import and rescan live in the seam menu.

use std::path::PathBuf;

use folia_core::{Theme, Track};
use folia_i18n::I18n;
use folia_library::Library;
use folia_render::{Glow, Veil, Wash};
use std::time::Instant;

use iced::widget::{canvas, column, container, shader, stack, text};
use iced::{Alignment, Color, Element, Length, Task};

use crate::background::{blocking, error_chain};
use crate::wall::{self, Wall};

pub struct LibraryPage {
    db_path: PathBuf,
    pub tracks: Vec<Track>,
    pub busy: bool,
    error: Option<String>,
    wall: Wall,
    hint_at: Option<Instant>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Import,
    Picked(Option<PathBuf>),
    Reload,
    Loaded(Result<Vec<Track>, String>),
    Wall(wall::Message),
    Frame,
}

impl LibraryPage {
    /// Shows what the index already holds; scanning only happens on import or reload.
    pub fn new(db_path: PathBuf) -> (Self, Task<Message>) {
        let task = load(db_path.clone(), None, false);
        let page = Self { db_path, tracks: Vec::new(), busy: true, error: None, wall: Wall::new(), hint_at: None };
        (page, task)
    }

    /// The seam the wall opens for the home chrome, and where it is on screen.
    pub fn set_seam(&mut self, width: f32) {
        self.wall.set_seam(width);
    }

    /// The seam the wall measured. A canvas that has not reported a size yet is closed.
    pub fn seam(&self) -> crate::wall::Seam {
        self.wall.seam()
    }

    pub fn set_horizontal(&mut self, horizontal: bool) {
        self.wall.set_horizontal(horizontal);
    }

    pub fn replan(&mut self, open: f32) {
        self.wall.replan(open);
    }

    pub fn plan_if_needed(&mut self) {
        self.wall.plan_if_needed();
    }

    pub fn coasting(&self) -> bool {
        self.wall.coasting()
    }

    pub fn hint_busy(&self, now: Instant) -> bool {
        self.hint_at.is_some_and(|at| now.saturating_duration_since(at).as_secs_f64() < 0.5)
    }

    /// Starts the empty-wall hint fade the first time the library is empty and idle.
    pub fn sync_hint(&mut self, now: Instant) {
        if self.show_hint() {
            if self.hint_at.is_none() {
                self.hint_at = Some(now);
            }
        } else {
            self.hint_at = None;
        }
    }

    fn show_hint(&self) -> bool {
        self.error.is_none() && !self.busy && self.tracks.is_empty()
    }

    /// Moves the wall so the track at `index` is in the middle.
    pub fn reveal(&mut self, index: usize) {
        self.wall.center_on(index);
    }

    /// Handles a page message; returns the index of a track to play when one was chosen.
    pub fn update(&mut self, message: Message) -> (Task<Message>, Option<usize>) {
        match message {
            Message::Import => {
                let pick =
                    async { rfd::AsyncFileDialog::new().pick_folder().await.map(|folder| folder.path().to_owned()) };
                (Task::perform(pick, Message::Picked), None)
            }
            Message::Picked(Some(folder)) => (self.start(Some(folder), true), None),
            Message::Picked(None) => (Task::none(), None),
            Message::Reload => (self.start(None, true), None),
            Message::Loaded(result) => {
                self.busy = false;
                match result {
                    Ok(tracks) => {
                        self.tracks = tracks;
                        self.error = None;
                        return (self.wall.relayout(&self.tracks).map(Message::Wall), None);
                    }
                    Err(err) => self.error = Some(err),
                }
                (Task::none(), None)
            }
            Message::Wall(message) => {
                let (task, play) = self.wall.update(message, &self.tracks);
                (task.map(Message::Wall), play)
            }
            Message::Frame => {
                self.wall.tick_coast(Instant::now());
                (Task::none(), None)
            }
        }
    }

    /// The seam's status line. The wall itself never says "no local music".
    pub fn status<'a>(&self, i18n: &'a I18n) -> Option<&'a str> {
        if self.error.is_some() {
            return None;
        }
        if self.busy {
            return Some(i18n.t("localMusic.importing"));
        }
        if self.tracks.is_empty() {
            return Some(i18n.t("localMusic.noLocalMusic"));
        }
        None
    }

    fn start(&mut self, folder: Option<PathBuf>, scan: bool) -> Task<Message> {
        if self.busy {
            return Task::none();
        }
        self.busy = true;
        load(self.db_path.clone(), folder, scan)
    }

    pub fn view<'a>(
        &'a self,
        i18n: &'a I18n,
        theme: &'a Theme,
        daylight: bool,
        _text_color: Color,
        muted: Color,
        playing: Option<i64>,
    ) -> Element<'a, Message> {
        let wall: Element<'a, wall::Message> = canvas(self.wall.view(&self.tracks, theme, daylight, playing))
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
        let mut layers = stack![ground(theme), wall.map(Message::Wall)];
        if self.error.is_none() && self.show_hint() && self.wall.viewport().width > 1.0 {
            layers = layers.push(hint(i18n, theme, self.wall.seam(), self.wall.viewport(), hint_fade(self.hint_at)));
        }
        layers = layers.push(veil(daylight));
        if let Some(error) = &self.error {
            layers = layers.push(container(text(error).color(muted)).center(Length::Fill));
        }
        layers.into()
    }
}

/// `wall.css`: the theme background lit by an accent glow top-left and a secondary glow bottom-right.
fn ground<'a, M: 'a>(theme: &Theme) -> Element<'a, M> {
    let srgb = |rgb: folia_core::Rgb| [rgb.r, rgb.g, rgb.b].map(|channel| f32::from(channel) / 255.0);
    let glow = |rgb: folia_core::Rgb, alpha: f32, center: [f32; 2], reach: f32| {
        let [r, g, b] = srgb(rgb);
        Glow { center, reach, rgba: [r, g, b, alpha] }
    };
    let wash = Wash {
        base: srgb(theme.background),
        glows: [glow(theme.accent, 0.16, [0.14, 0.12], 0.29), glow(theme.secondary, 0.20, [0.86, 0.82], 0.32)],
    };
    shader(wash).width(Length::Fill).height(Length::Fill).into()
}

/// The wall's edge vignette (52% black at the corners, 16% by day) and its grain, over the tiles.
fn veil<'a, M: 'a>(daylight: bool) -> Element<'a, M> {
    let veil = if daylight { Veil { vignette: 0.16, grain: 0.019 } } else { Veil { vignette: 0.52, grain: 0.035 } };
    shader(veil).width(Length::Fill).height(Length::Fill).into()
}

/// Empty-wall copy (`BravaisWallHint`): the wider side of the seam, fading in over 0.32s.
fn hint<'a>(
    i18n: &'a I18n,
    theme: &folia_core::Theme,
    seam: wall::Seam,
    view: iced::Size,
    fade: f32,
) -> Element<'a, Message> {
    let (origin, size, pad) = hint_frame(seam, view);
    let title_size = (view.width * 0.032).clamp(24.0, 48.0);
    let body_size = (view.width * 0.011).clamp(13.0, 16.0);
    let ink = folia_visualizer::color(theme.primary, 1.0);
    let rise = 6.0 * (1.0 - fade);
    let title = text(i18n.t("libraryBravaisHome.wallHint.localTitle"))
        .size(title_size)
        .line_height(iced::widget::text::LineHeight::Relative(1.18))
        .font(iced::Font { weight: iced::font::Weight::Bold, ..iced::Font::DEFAULT })
        .color(Color { a: 0.22 * fade, ..ink })
        .align_x(Alignment::Center)
        .width(Length::Fill);
    let action = i18n.t("localMusic.importFolder");
    let body = i18n.format("libraryBravaisHome.wallHint.localBody", &[("action", action)]);
    let body = text(body)
        .size(body_size)
        .line_height(iced::widget::text::LineHeight::Relative(1.65))
        .color(Color { a: 0.36 * fade, ..ink })
        .align_x(Alignment::Center)
        .width(Length::Fill);
    let block = column![title, body].spacing(title_size * 0.9).width(Length::Fixed(448.0)).align_x(Alignment::Center);
    let block = iced::widget::float(block).translate(move |_, _| iced::Vector::new(0.0, rise));
    let area = container(block)
        .width(Length::Fixed(size.0.max(1.0)))
        .height(Length::Fixed(size.1.max(1.0)))
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .padding(pad);
    container(area)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::Start)
        .align_y(Alignment::Start)
        .padding(iced::Padding { top: origin.1, right: 0.0, bottom: 0.0, left: origin.0 })
        .into()
}

fn hint_frame(seam: wall::Seam, view: iced::Size) -> ((f32, f32), (f32, f32), iced::Padding) {
    let side = (view.width * 0.04).clamp(20.0, 64.0);
    if seam.horizontal {
        let (top, height) = wall::wider_span(seam.center, seam.width, view.height);
        ((0.0, top), (view.width, height), iced::Padding { top: 32.0, right: side, bottom: 96.0, left: side })
    } else {
        let (left, width) = wall::wider_span(seam.center, seam.width, view.width);
        ((left, 0.0), (width, view.height), iced::Padding { top: 72.0, right: side, bottom: 112.0, left: side })
    }
}

fn hint_fade(started: Option<Instant>) -> f32 {
    let Some(at) = started else { return 1.0 };
    let t = (Instant::now().saturating_duration_since(at).as_secs_f64() / 0.32).clamp(0.0, 1.0);
    // CSS `ease`: cubic-bezier(0.25, 0.1, 0.25, 1).
    folia_visualizer::Ease::new(0.25, 0.1, 0.25, 1.0).at(t) as f32
}

fn load(db_path: PathBuf, folder: Option<PathBuf>, scan: bool) -> Task<Message> {
    let work = blocking(move || {
        let mut library = Library::open(&db_path)?;
        if let Some(folder) = &folder {
            library.add_folder(folder)?;
        }
        if scan {
            library.scan()?;
        }
        library.tracks()
    });
    Task::perform(work, |result| {
        Message::Loaded(match result {
            Some(Ok(tracks)) => Ok(tracks),
            Some(Err(err)) => {
                tracing::error!(error = &err as &dyn std::error::Error, "library load failed");
                Err(error_chain(&err))
            }
            None => Err("library worker stopped".to_owned()),
        })
    })
}

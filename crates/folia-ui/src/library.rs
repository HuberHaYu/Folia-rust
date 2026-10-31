// Home page: the cover wall of every library track, with import and rescan floating over it.

use std::path::PathBuf;

use folia_core::{Theme, Track};
use folia_i18n::I18n;
use folia_library::Library;
use folia_render::{Glow, Veil, Wash};
use iced::widget::{canvas, container, row, shader, stack, text};
use iced::{Alignment, Color, Element, Length, Task};
use lucide_static_svg::Icon;

use crate::background::{blocking, error_chain};
use crate::icons::icon_button;
use crate::wall::{self, Wall};

pub struct LibraryPage {
    db_path: PathBuf,
    pub tracks: Vec<Track>,
    busy: bool,
    error: Option<String>,
    wall: Wall,
}

#[derive(Debug, Clone)]
pub enum Message {
    Import,
    Picked(Option<PathBuf>),
    Reload,
    Loaded(Result<Vec<Track>, String>),
    Wall(wall::Message),
}

impl LibraryPage {
    /// Shows what the index already holds; scanning only happens on import or reload.
    pub fn new(db_path: PathBuf) -> (Self, Task<Message>) {
        let task = load(db_path.clone(), None, false);
        let page = Self { db_path, tracks: Vec::new(), busy: true, error: None, wall: Wall::default() };
        (page, task)
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
        }
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
        text_color: Color,
        muted: Color,
        playing: Option<i64>,
    ) -> Element<'a, Message> {
        let tools = row![
            icon_button(
                Icon::RefreshCw,
                18.0,
                text_color,
                i18n.t("localMusic.reload"),
                (!self.busy).then_some(Message::Reload)
            ),
            icon_button(
                Icon::FolderPlus,
                18.0,
                text_color,
                i18n.t("localMusic.importFolder"),
                (!self.busy).then_some(Message::Import)
            ),
        ]
        .spacing(4)
        .align_y(Alignment::Center);
        let tools = container(tools).width(Length::Fill).align_x(Alignment::End).padding([56, 16]);

        let body: Element<'a, Message> = if let Some(error) = &self.error {
            container(text(error).color(muted)).center(Length::Fill).into()
        } else if self.tracks.is_empty() {
            let note = if self.busy { i18n.t("localMusic.importing") } else { i18n.t("localMusic.noLocalMusic") };
            container(text(note).color(muted)).center(Length::Fill).into()
        } else {
            let wall: Element<'a, wall::Message> = canvas(self.wall.view(&self.tracks, theme, daylight, playing))
                .width(Length::Fill)
                .height(Length::Fill)
                .into();
            wall.map(Message::Wall)
        };
        stack![ground(theme), body, veil(daylight), tools].into()
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

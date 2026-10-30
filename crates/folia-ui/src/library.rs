// Library page: imports folders, rescans them in the background and lists every track.

use std::path::PathBuf;

use folia_core::Track;
use folia_i18n::I18n;
use folia_library::Library;
use iced::widget::scrollable::Viewport;
use iced::widget::{button, column, container, row, scrollable, space, text};
use iced::{Alignment, Color, Element, Length, Task};
use lucide_static_svg::Icon;

use crate::background::{blocking, error_chain, format_time};
use crate::icons::icon_button;

const ROW_HEIGHT: f32 = 52.0;

pub struct LibraryPage {
    db_path: PathBuf,
    pub tracks: Vec<Track>,
    busy: bool,
    error: Option<String>,
    offset: f32,
    viewport: f32,
}

#[derive(Debug, Clone)]
pub enum Message {
    Import,
    Picked(Option<PathBuf>),
    Reload,
    Loaded(Result<Vec<Track>, String>),
    Scrolled(Viewport),
    Play(usize),
}

impl LibraryPage {
    /// Shows what the index already holds; scanning only happens on import or reload.
    pub fn new(db_path: PathBuf) -> (Self, Task<Message>) {
        let task = load(db_path.clone(), None, false);
        let page = Self { db_path, tracks: Vec::new(), busy: true, error: None, offset: 0.0, viewport: 0.0 };
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
                    }
                    Err(err) => self.error = Some(err),
                }
                (Task::none(), None)
            }
            Message::Scrolled(viewport) => {
                self.offset = viewport.absolute_offset().y;
                self.viewport = viewport.bounds().height;
                (Task::none(), None)
            }
            Message::Play(index) => (Task::none(), Some(index)),
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
        text_color: Color,
        muted: Color,
        playing: Option<i64>,
    ) -> Element<'a, Message> {
        let title = if self.busy { i18n.t("localMusic.importing") } else { i18n.t("localMusic.allSongs") };
        let header = row![
            text(title).size(24),
            text(self.tracks.len().to_string()).size(14).color(muted),
            space().width(Length::Fill),
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
        .spacing(12)
        .align_y(Alignment::Center);

        let body: Element<'a, Message> = if let Some(error) = &self.error {
            text(error).color(muted).into()
        } else if self.tracks.is_empty() && !self.busy {
            container(text(i18n.t("localMusic.noLocalMusic")).color(muted)).center(Length::Fill).into()
        } else {
            self.track_list(i18n, muted, playing)
        };
        column![header, body].spacing(16).padding(24).into()
    }

    /// Only the rows inside the viewport are built, so large libraries stay cheap to draw.
    fn track_list<'a>(&'a self, i18n: &'a I18n, muted: Color, playing: Option<i64>) -> Element<'a, Message> {
        let total = self.tracks.len();
        let first = ((self.offset / ROW_HEIGHT).floor() as usize).min(total);
        let visible = ((self.viewport.max(900.0) / ROW_HEIGHT).ceil() as usize + 2).min(total - first);
        let rows = self.tracks[first..first + visible]
            .iter()
            .enumerate()
            .map(|(i, track)| track_row(first + i, track, i18n, muted, playing == Some(track.id)));

        let list = column![space().height(first as f32 * ROW_HEIGHT)]
            .extend(rows)
            .push(space().height((total - first - visible) as f32 * ROW_HEIGHT));
        scrollable(list).on_scroll(Message::Scrolled).height(Length::Fill).into()
    }
}

fn track_row<'a>(index: usize, track: &'a Track, i18n: &'a I18n, muted: Color, playing: bool) -> Element<'a, Message> {
    let artist = track.artist.as_deref().unwrap_or_else(|| i18n.t("player.unknownArtist"));
    let album = track.album.as_deref().unwrap_or_else(|| i18n.t("player.unknownAlbum"));
    let number = text((index + 1).to_string()).size(13).color(muted).width(Length::Fixed(40.0));
    let title = column![text(&track.title).size(15), text(artist).size(12).color(muted)].width(Length::FillPortion(3));
    let content = row![
        number,
        title,
        text(album).size(13).color(muted).width(Length::FillPortion(2)),
        text(format_time(track.duration)).size(13).color(muted),
    ]
    .spacing(12)
    .align_y(Alignment::Center);

    button(content)
        .height(Length::Fixed(ROW_HEIGHT))
        .width(Length::Fill)
        .style(if playing { button::secondary } else { button::text })
        .on_press(Message::Play(index))
        .into()
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

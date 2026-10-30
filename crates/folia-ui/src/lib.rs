// Top-level UI: pages, navigation and the iced update/view/subscription functions that drive them.

mod background;
mod icons;
mod library;
mod now_playing;
mod playback;
mod player_bar;
mod search;
mod settings;
mod track_list;

pub use background::{error_chain, format_time};

use std::path::PathBuf;

use folia_core::{Language, LanguagePreference, Lyrics, Theme, Track};
use folia_i18n::I18n;
use folia_omni::Omni;
use folia_store::{AppearanceSettings, FrameRate, GeneralSettings, PlaybackSettings, Store, ThemeMode};
use folia_visualizer::{Scene, Style};
use iced::widget::{button, column, container, row, rule, space};
use iced::{Alignment, Color, Element, Length, Subscription, Task};
use lucide_static_svg::Icon;

use crate::background::save;
use crate::library::LibraryPage;
use crate::playback::Playback;
use crate::search::SearchPage;

/// Everything the app reads before the window opens.
#[derive(Clone)]
pub struct Init {
    pub store: Store,
    pub general: GeneralSettings,
    pub appearance: AppearanceSettings,
    pub playback: PlaybackSettings,
    pub i18n: I18n,
    pub system_locale: Option<String>,
    pub library_db: PathBuf,
    /// Where downloaded online audio is cached.
    pub cache_dir: PathBuf,
    /// Audio files given on the command line, played as a queue once the window opens.
    pub open_files: Vec<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Library,
    Search,
    NowPlaying,
    Settings,
}

#[derive(Debug, Clone)]
pub enum Message {
    Navigate(Page),
    LanguagePicked(LanguagePreference),
    Theme(ThemeMode),
    FrameRate(FrameRate),
    Visualizer(Style),
    Library(library::Message),
    Search(search::Message),
    Playback(playback::Message),
    OpenFiles(Vec<Track>),
}

pub struct Shell {
    store: Store,
    general: GeneralSettings,
    appearance: AppearanceSettings,
    i18n: I18n,
    system_locale: Option<String>,
    theme: Theme,
    iced_theme: iced::Theme,
    page: Page,
    library: LibraryPage,
    search: SearchPage,
    omni: Option<Omni>,
    playback: Playback,
    empty_lyrics: Lyrics,
}

impl Shell {
    pub fn new(init: Init) -> (Self, Task<Message>) {
        let (library, load) = LibraryPage::new(init.library_db);
        let theme = init.appearance.theme.theme();
        let omni = Omni::new(init.cache_dir)
            .inspect_err(|err| tracing::error!(error = err as &dyn std::error::Error, "online music unavailable"))
            .ok();
        let shell = Self {
            store: init.store,
            general: init.general,
            iced_theme: iced_theme(&theme),
            theme,
            appearance: init.appearance,
            i18n: init.i18n,
            system_locale: init.system_locale,
            page: Page::Library,
            library,
            search: SearchPage::default(),
            playback: Playback::new(init.playback, omni.clone()),
            omni,
            empty_lyrics: Lyrics::default(),
        };
        let open = if init.open_files.is_empty() { Task::none() } else { open_files(init.open_files) };
        (shell, Task::batch([load.map(Message::Library), open]))
    }

    pub fn title(&self) -> String {
        match self.playback.current() {
            Some(track) => format!("{} - {}", track.display_name(), self.i18n.t("app.name")),
            None => self.i18n.t("app.name").to_owned(),
        }
    }

    pub fn theme(&self) -> iced::Theme {
        self.iced_theme.clone()
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Navigate(page) => {
                self.page = page;
                Task::none()
            }
            Message::LanguagePicked(preference) => self.set_language(preference),
            Message::Theme(mode) => {
                self.appearance.theme = mode;
                self.theme = mode.theme();
                self.iced_theme = iced_theme(&self.theme);
                save(&self.store, &self.appearance)
            }
            Message::FrameRate(rate) => {
                self.appearance.frame_rate = rate;
                save(&self.store, &self.appearance)
            }
            Message::Visualizer(style) => {
                self.appearance.visualizer = style.id().to_owned();
                save(&self.store, &self.appearance)
            }
            Message::Library(message) => {
                let (task, play) = self.library.update(message);
                let task = task.map(Message::Library);
                match play {
                    Some(index) => {
                        self.page = Page::NowPlaying;
                        let start = self.playback.play(self.library.tracks.clone(), index).map(Message::Playback);
                        Task::batch([task, start])
                    }
                    None => task,
                }
            }
            Message::Search(message) => {
                let (task, play) = self.search.update(message, self.omni.as_ref());
                let task = task.map(Message::Search);
                match play {
                    Some(index) => {
                        self.page = Page::NowPlaying;
                        let start = self.playback.play(self.search.results.clone(), index).map(Message::Playback);
                        Task::batch([task, start])
                    }
                    None => task,
                }
            }
            Message::Playback(message) => self.playback.update(message, &self.store).map(Message::Playback),
            Message::OpenFiles(tracks) if !tracks.is_empty() => {
                self.page = Page::NowPlaying;
                self.playback.play(tracks, 0).map(Message::Playback)
            }
            Message::OpenFiles(_) => Task::none(),
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        let (text_color, muted) =
            (folia_visualizer::color(self.theme.primary, 1.0), folia_visualizer::color(self.theme.secondary, 1.0));
        let page: Element<'_, Message> = match self.page {
            Page::Library => {
                let playing = self.playback.current().map(|track| track.id);
                self.library.view(&self.i18n, text_color, muted, playing).map(Message::Library)
            }
            Page::Search => {
                let playing = self.playback.current().map(|track| track.id);
                self.search.view(&self.i18n, muted, playing).map(Message::Search)
            }
            Page::NowPlaying => {
                let scene = Scene {
                    style: self.style(),
                    lyrics: self.playback.lyrics.as_ref().unwrap_or(&self.empty_lyrics),
                    time: self.playback.position(),
                    theme: &self.theme,
                    font_scale: self.appearance.lyrics_font_scale,
                };
                now_playing::view(scene, &self.i18n, muted)
            }
            Page::Settings => settings::view(&self.i18n, self.general.language, &self.appearance, self.style(), muted),
        };
        let bar = player_bar::view(&self.playback, &self.i18n, text_color, muted).map(Message::Playback);
        column![self.navigation(text_color, muted), container(page).height(Length::Fill), rule::horizontal(1), bar]
            .into()
    }

    /// Ticks only while music plays: every frame on the visualizer page, a few times a second elsewhere.
    pub fn subscription(&self) -> Subscription<Message> {
        if !self.playback.is_playing() {
            return Subscription::none();
        }
        let tick = |_| Message::Playback(playback::Message::Tick);
        if self.page != Page::NowPlaying {
            return iced::time::every(std::time::Duration::from_millis(250)).map(tick);
        }
        let fps = match (self.appearance.frame_rate.fps(), self.style().max_fps()) {
            (Some(setting), Some(cap)) => Some(setting.min(cap)),
            (setting, cap) => setting.or(cap),
        };
        match fps {
            Some(fps) => iced::time::every(std::time::Duration::from_secs_f64(1.0 / f64::from(fps))).map(tick),
            None => iced::window::frames().map(tick),
        }
    }

    fn style(&self) -> Style {
        Style::from_id(&self.appearance.visualizer).unwrap_or_default()
    }

    fn navigation(&self, text_color: Color, muted: Color) -> Element<'_, Message> {
        let tab = |page: Page, lucide: Icon, key: &str| {
            let color = if self.page == page { text_color } else { muted };
            let label =
                row![icons::icon(lucide, 16.0, color), iced::widget::text(self.i18n.t(key).to_owned()).color(color)]
                    .spacing(8)
                    .align_y(Alignment::Center);
            button(label).style(button::text).padding([6, 12]).on_press(Message::Navigate(page))
        };
        row![
            tab(Page::Library, Icon::ListMusic, "localMusic.allSongs"),
            tab(Page::Search, Icon::Search, "libraryBravaisHome.search"),
            tab(Page::NowPlaying, Icon::AudioLines, "ui.visualizer"),
            space().width(Length::Fill),
            tab(Page::Settings, Icon::Settings, "ui.options"),
        ]
        .padding([6, 12])
        .align_y(Alignment::Center)
        .into()
    }

    fn set_language(&mut self, preference: LanguagePreference) -> Task<Message> {
        let language: Language = preference.resolve(self.system_locale.as_deref());
        if let Err(err) = self.i18n.set_language(language) {
            tracing::error!(error = &err as &dyn std::error::Error, "cannot switch language");
            return Task::none();
        }
        self.general.language = preference;
        tracing::info!(language = language.code(), ?preference, "language switched");
        save(&self.store, &self.general)
    }
}

/// Reads the files' tags in the background and queues the playable ones.
fn open_files(paths: Vec<PathBuf>) -> Task<Message> {
    let work = background::blocking(move || {
        paths.iter().filter(|path| folia_library::is_audio(path)).map(|path| folia_library::read_track(path)).collect()
    });
    Task::perform(work, |tracks| Message::OpenFiles(tracks.unwrap_or_default()))
}

fn iced_theme(theme: &Theme) -> iced::Theme {
    let palette = iced::theme::Palette {
        background: folia_visualizer::color(theme.background, 1.0),
        text: folia_visualizer::color(theme.primary, 1.0),
        primary: folia_visualizer::color(theme.accent, 1.0),
        ..iced::theme::Palette::DARK
    };
    iced::Theme::custom(theme.name.clone(), palette)
}

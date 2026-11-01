// Top-level UI: pages, navigation and the iced update/view/subscription functions that drive them.

mod background;
mod chrome;
mod home;
mod icons;
mod library;
mod mode_picker;
mod motion;
mod now_playing;
mod overlay;
mod panel;
mod playback;
mod player_bar;
mod positioner;
mod search;
mod settings;
mod settings_kit;
mod splash;
mod track_list;
mod wall;

pub use background::{error_chain, format_time};
pub use overlay::{
    CAPSULE_COLLAPSE_MS, CAPSULE_EXPAND_MS, CAPSULE_FADE_MS, CAPSULE_SETTLE_MS, CHROME_HOLD_MS, CHROME_LEAVE_MS,
    CapsuleHover, CapsuleSpring, ChromeClock, GLOW_BLUR_PX, GlowEllipse, Subtitle, capsule_open, capsule_row,
    capsule_width, glow_coverage, glow_ellipse, row_gap, subtitle_at, subtitle_line_width, translation_px,
    upcoming_look, upcoming_px,
};
pub use wall::{SEAM_ANCHOR, Seam, camera_range_x, seam_geometry};

use std::path::PathBuf;

use folia_core::{Language, LanguagePreference, Lyrics, Theme, Track};
use folia_i18n::I18n;
use folia_omni::Omni;
use folia_store::{AppearanceSettings, FrameRate, GeneralSettings, PlaybackSettings, Store, ThemeMode};
use folia_visualizer::{Scene, Style};
use iced::widget::{container, mouse_area, space, stack};
use iced::{Alignment, Color, Element, Length, Size, Subscription, Task, mouse};
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
    pub player: folia_store::PlayerSettings,
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
}

#[derive(Debug, Clone)]
pub enum Message {
    Navigate(Page),
    OpenSettings,
    Settings(settings::Message),
    LanguagePicked(LanguagePreference),
    Theme(ThemeMode),
    FrameRate(FrameRate),
    /// A fixed lyric style, or `None` for a random style per song.
    Visualizer(Option<Style>),
    Library(library::Message),
    Search(search::Message),
    Playback(playback::Message),
    OpenFiles(Vec<Track>),
    Chrome(chrome::Message),
    WindowReady(Option<iced::window::Id>),
    WindowResized(Size),
    Maximized(bool),
    ChromeTick,
    PointerHeld(bool),
    RevealChrome,
    Bar(player_bar::Message),
    /// The bottom offset from settings, while its slider moves.
    BarOffset(f32),
    BarOffsetDone,
    BarOffsetReset,
    StartPositioning,
    Positioner(positioner::Message),
    PositionerKey(bool),
    ToggleFullscreen,
    SplashFrame,
    Home(home::Message),
    Panel(panel::Message),
    Art(i64, Option<folia_library::Cover>),
}

/// Settings id of the random lyric style choice, as in the original app.
const RANDOM_STYLE: &str = "random";

/// Pixel budget of the panel's decoded covers: eight 360×360 RGBA covers (about 0.5 MB each), so stepping
/// back and forth between neighbouring songs does not decode again.
const ART_BYTES: usize = 4 * 1024 * 1024;
/// The panel cover is decoded at 360px, the panel's width.
const ART_PX: u32 = 360;

pub struct Shell {
    store: Store,
    general: GeneralSettings,
    appearance: AppearanceSettings,
    i18n: I18n,
    system_locale: Option<String>,
    theme: Theme,
    iced_theme: iced::Theme,
    page: Page,
    settings: Option<settings::Settings>,
    library: LibraryPage,
    search: SearchPage,
    omni: Option<Omni>,
    playback: Playback,
    empty_lyrics: Lyrics,
    window: Option<iced::window::Id>,
    maximized: bool,
    fullscreen: bool,
    /// Origin for [`ChromeClock`] milliseconds.
    started: std::time::Instant,
    clock: ChromeClock,
    bar: player_bar::PlayerBar,
    window_size: Size,
    pointer_held: bool,
    /// Press on the drag strip, and the pointer position of the first move after it.
    drag_armed: bool,
    drag_from: Option<(f32, f32)>,
    splash: splash::Splash,
    home: home::HomeChrome,
    panel: panel::PlayerPanel,
    art: folia_library::CoverCache<i64, iced::widget::image::Handle>,
    art_pending: Option<i64>,
    titlebar: chrome::Titlebar,
    player: folia_store::PlayerSettings,
    positioning: Option<positioner::Positioning>,
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
            settings: None,
            library,
            search: SearchPage::default(),
            playback: Playback::new(init.playback, omni.clone()),
            omni,
            empty_lyrics: Lyrics::default(),
            window: None,
            maximized: false,
            fullscreen: false,
            started: std::time::Instant::now(),
            clock: ChromeClock::shown(0),
            bar: player_bar::PlayerBar::new(std::time::Instant::now()),
            window_size: Size::new(1200.0, 800.0),
            pointer_held: false,
            drag_armed: false,
            drag_from: None,
            splash: splash::Splash::new(),
            home: home::HomeChrome::new(),
            panel: panel::PlayerPanel::new(std::time::Instant::now()),
            art: folia_library::CoverCache::new(ART_BYTES),
            art_pending: None,
            titlebar: chrome::Titlebar::new(std::time::Instant::now()),
            player: init.player,
            positioning: None,
        };
        let open = if init.open_files.is_empty() { Task::none() } else { open_files(init.open_files) };
        let window = iced::window::oldest().map(Message::WindowReady);
        (shell, Task::batch([load.map(Message::Library), open, window]))
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
        let task = self.handle(message);
        let now = std::time::Instant::now();
        if self.page != Page::NowPlaying {
            self.positioning = None;
        }
        self.bar.set_home(self.page == Page::Library);
        self.library.set_seam(self.home.width(now));
        self.bar.set_forced(self.positioning.is_some());
        self.bar.sync(&self.playback, self.window_size.width, now);
        self.bar.set_shown(self.chrome_visible(), now);
        task
    }

    fn handle(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Navigate(page) => {
                self.page = page;
                if page == Page::NowPlaying {
                    self.reveal_chrome();
                }
                Task::none()
            }
            Message::OpenSettings => {
                self.settings = Some(settings::Settings::open());
                Task::none()
            }
            Message::Settings(message) => {
                let Some(window) = self.settings.as_mut() else { return Task::none() };
                let task = window.update(message).map(Message::Settings);
                if window.finished() {
                    self.settings = None;
                }
                task
            }
            Message::LanguagePicked(preference) => self.set_language(preference),
            Message::Theme(mode) => {
                let before = self.appearance.clone();
                self.appearance.theme = mode;
                self.theme = mode.theme();
                self.iced_theme = iced_theme(&self.theme);
                self.appearance_changed(&before)
            }
            Message::FrameRate(rate) => {
                let before = self.appearance.clone();
                self.appearance.frame_rate = rate;
                self.appearance_changed(&before)
            }
            Message::Visualizer(style) => {
                self.appearance.visualizer = style.map_or(RANDOM_STYLE, Style::id).to_owned();
                save(&self.store, &self.appearance)
            }
            Message::Library(message) => {
                if matches!(message, library::Message::Loaded(_)) {
                    self.splash.library_ready();
                }
                let (task, play) = self.library.update(message);
                let task = task.map(Message::Library);
                match play {
                    Some(index) => {
                        self.page = Page::NowPlaying;
                        self.reveal_chrome();
                        let origin = folia_core::Origin { name: self.i18n.t("localMusic.allSongs").to_owned() };
                        let start =
                            self.playback.play(self.library.tracks.clone(), index, Some(origin)).map(Message::Playback);
                        Task::batch([task, start, self.ensure_art()])
                    }
                    None => task,
                }
            }
            Message::Search(message) => {
                if matches!(message, search::Message::Close) {
                    self.search.close();
                }
                let now = std::time::Instant::now();
                if matches!(message, search::Message::Frame) && self.page == Page::Search && self.search.gone(now) {
                    self.page = Page::Library;
                }
                let (task, play) = self.search.update(message, self.omni.as_ref());
                let task = task.map(Message::Search);
                match play {
                    Some(index) => {
                        self.page = Page::NowPlaying;
                        self.reveal_chrome();
                        let name = self.search.query();
                        let name = if name.is_empty() { self.i18n.t("search.sourceNetease") } else { name };
                        let origin = folia_core::Origin { name: name.to_owned() };
                        let start =
                            self.playback.play(self.search.results.clone(), index, Some(origin)).map(Message::Playback);
                        Task::batch([task, start, self.ensure_art()])
                    }
                    None => task,
                }
            }
            Message::Playback(message) => {
                let task = self.playback.update(message, &self.store).map(Message::Playback);
                Task::batch([task, self.ensure_art()])
            }
            Message::OpenFiles(tracks) if !tracks.is_empty() => {
                self.page = Page::NowPlaying;
                self.reveal_chrome();
                let start = self.playback.play(tracks, 0, None).map(Message::Playback);
                Task::batch([start, self.ensure_art()])
            }
            Message::OpenFiles(_) => Task::none(),
            Message::WindowReady(id) => {
                self.window = id.or(self.window);
                Task::none()
            }
            Message::WindowResized(size) => {
                self.window_size = size;
                match self.window {
                    Some(id) => iced::window::is_maximized(id).map(Message::Maximized),
                    None => Task::none(),
                }
            }
            Message::Maximized(maximized) => {
                self.maximized = maximized;
                Task::none()
            }
            Message::ChromeTick => {
                self.clock = self.clock.tick(self.now_ms(), self.pointer_held);
                self.panel.set_chrome(self.chrome_visible(), std::time::Instant::now());
                Task::none()
            }
            Message::PointerHeld(held) => {
                self.pointer_held = held;
                if !held {
                    self.drag_armed = false;
                    if let Some(positioning) = self.positioning.as_mut() {
                        positioning.release();
                    }
                    self.drag_from = None;
                }
                if self.page == Page::NowPlaying {
                    self.reveal_chrome();
                }
                Task::none()
            }
            Message::RevealChrome => {
                if self.page == Page::NowPlaying {
                    self.reveal_chrome();
                }
                Task::none()
            }
            Message::BarOffset(offset) => {
                self.player.bottom_bar_offset = folia_store::clamp_bottom_bar_offset(offset, self.window_size.height);
                Task::none()
            }
            Message::BarOffsetDone => save(&self.store, &self.player),
            Message::BarOffsetReset => {
                self.player.bottom_bar_offset = folia_store::BOTTOM_BAR_BASE;
                save(&self.store, &self.player)
            }
            Message::StartPositioning => {
                if self.playback.current().is_none() {
                    return Task::none();
                }
                // `usePlayerBottomBarPositioningEntry`: close settings, go to the player, enter the mode.
                self.settings = None;
                self.page = Page::NowPlaying;
                self.reveal_chrome();
                self.positioning = Some(positioner::Positioning::new(self.bar_offset()));
                Task::none()
            }
            Message::Positioner(message) => self.positioner(message),
            Message::PositionerKey(commit) => {
                self.positioner(if commit { positioner::Message::Commit } else { positioner::Message::Cancel })
            }
            Message::Bar(player_bar::Message::Open) => {
                if self.page == Page::Library {
                    self.page = Page::NowPlaying;
                    self.reveal_chrome();
                }
                Task::none()
            }
            Message::Bar(message) => match self.bar.update(message, &self.playback, std::time::Instant::now()) {
                Some(message) => {
                    let task = self.playback.update(message, &self.store).map(Message::Playback);
                    Task::batch([task, self.ensure_art()])
                }
                None => Task::none(),
            },
            Message::ToggleFullscreen => self.toggle_fullscreen(),
            Message::SplashFrame => {
                self.splash.on_frame(std::time::Instant::now());
                Task::none()
            }
            Message::Home(message) => self.home(message),
            Message::Panel(message) => {
                let task = self.panel(message);
                Task::batch([task, self.ensure_art()])
            }
            Message::Art(id, cover) => {
                if self.art_pending == Some(id) {
                    self.art_pending = None;
                }
                let bytes = cover.as_ref().map_or(0, |cover| cover.rgba.len());
                tracing::debug!(bytes, held = self.art.used_bytes() + bytes, "panel cover decoded");
                let handle =
                    cover.map(|cover| iced::widget::image::Handle::from_rgba(cover.width, cover.height, cover.rgba));
                self.art.insert(id, handle, bytes);
                Task::none()
            }
            Message::Chrome(message) => self.chrome(message),
        }
    }

    /// Starts a window drag once the pointer has moved 4px from where the press was first seen.
    fn drag_move(&mut self, id: iced::window::Id, x: f32, y: f32) -> Task<Message> {
        if !self.drag_armed {
            return Task::none();
        }
        let Some((origin_x, origin_y)) = self.drag_from else {
            self.drag_from = Some((x, y));
            return Task::none();
        };
        let dx = x - origin_x;
        let dy = y - origin_y;
        if dx * dx + dy * dy < 16.0 {
            return Task::none();
        }
        self.drag_armed = false;
        self.drag_from = None;
        iced::window::drag(id)
    }

    fn now_ms(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    fn reveal_chrome(&mut self) {
        self.clock = self.clock.reveal(self.now_ms());
    }

    fn toggle_fullscreen(&mut self) -> Task<Message> {
        let Some(id) = self.window else {
            return iced::window::oldest().map(Message::WindowReady);
        };
        self.fullscreen = !self.fullscreen;
        let mode = if self.fullscreen { iced::window::Mode::Fullscreen } else { iced::window::Mode::Windowed };
        iced::window::set_mode(id, mode)
    }

    fn chrome(&mut self, message: chrome::Message) -> Task<Message> {
        let Some(id) = self.window else {
            return iced::window::oldest().map(Message::WindowReady);
        };
        match message {
            chrome::Message::Drag => iced::window::drag(id),
            chrome::Message::DragArm => {
                self.drag_armed = true;
                self.drag_from = None;
                Task::none()
            }
            chrome::Message::DragMove { x, y } => self.drag_move(id, x, y),
            chrome::Message::DragEnd => {
                self.drag_armed = false;
                self.drag_from = None;
                Task::none()
            }
            chrome::Message::Resize(direction) => iced::window::drag_resize(id, direction),
            chrome::Message::Minimize => iced::window::minimize(id, true),
            chrome::Message::ToggleMaximize => {
                self.maximized = !self.maximized;
                iced::window::toggle_maximize(id)
            }
            chrome::Message::ToggleFullscreen => self.toggle_fullscreen(),
            chrome::Message::Close => iced::window::close(id),
            chrome::Message::Activity { x, y } => {
                self.clock = self.clock.pointer(self.now_ms(), x, y);
                let height = self.window_size.height;
                if let Some(positioning) = self.positioning.as_mut() {
                    positioning.drag(y, height);
                }
                self.sync_home_back();
                let now = std::time::Instant::now();
                self.panel.set_chrome(self.chrome_visible(), now);
                self.titlebar.pointer(Some(y), now);
                Task::none()
            }
            chrome::Message::Left => {
                self.clock = self.clock.leave(self.now_ms(), self.pointer_held);
                let now = std::time::Instant::now();
                self.panel.set_chrome(self.chrome_visible(), now);
                self.titlebar.pointer(None, now);
                Task::none()
            }
            chrome::Message::Hover(button) => {
                self.titlebar.hover(button, std::time::Instant::now());
                Task::none()
            }
            chrome::Message::Frame => Task::none(),
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        let (text_color, muted) =
            (folia_visualizer::color(self.theme.primary, 1.0), folia_visualizer::color(self.theme.secondary, 1.0));
        let daylight = self.appearance.theme == ThemeMode::Light;
        let page: Element<'_, Message> = match self.page {
            Page::Library | Page::Search => {
                let playing = self.playback.current().map(|track| track.id);
                self.library.view(&self.i18n, &self.theme, daylight, text_color, muted, playing).map(Message::Library)
            }
            Page::NowPlaying => {
                let lyrics = self.playback.lyrics.as_ref().unwrap_or(&self.empty_lyrics);
                let scene = Scene {
                    style: self.style(),
                    lyrics,
                    time: self.playback.lyric_time(),
                    theme: &self.theme,
                    font_scale: self.appearance.lyrics_font_scale,
                    stage: None,
                };
                let subtitle = self.subtitle();
                let bottom =
                    if self.chrome_visible() { self.bar_offset() + SUBTITLE_CLEARANCE } else { SUBTITLE_BOTTOM_HIDDEN };
                let background = folia_visualizer::color(self.theme.background, 1.0);
                now_playing::view(scene, &self.i18n, muted, background, self.window_size, subtitle, bottom)
            }
        };
        let playing = self.page == Page::NowPlaying;
        let now = std::time::Instant::now();
        let mut frame: iced::widget::Stack<'_, Message, iced::Theme, iced::Renderer> = if playing {
            stack![page]
        } else if self.page == Page::Search {
            let search = self
                .search
                .view(
                    search::View {
                        i18n: &self.i18n,
                        text: text_color,
                        background: folia_visualizer::color(self.theme.background, 1.0),
                        accent: folia_visualizer::color(self.theme.accent, 1.0),
                        daylight,
                        width: self.window_size.width,
                        playing: self.playback.current().map(|track| track.id),
                    },
                    now,
                )
                .map(Message::Search);
            let search = chrome::drag_area(search, Message::Chrome);
            stack![page, search]
        } else {
            let shell = home::overlay(&self.home, self.home_host(), now).map(Message::Home);
            stack![page, shell]
        };
        if playing && self.back_hotspot() {
            let back = chrome::round_button(
                Icon::ChevronLeft,
                20.0,
                40.0,
                Color { a: 0.2, ..Color::BLACK },
                Color { r: 1.0, g: 1.0, b: 1.0, a: 0.6 },
                self.i18n.t("ui.back"),
                Message::Navigate(Page::Library),
            );
            frame = frame.push(pin(
                back,
                Alignment::Start,
                Alignment::Start,
                iced::Padding { top: 24.0, right: 0.0, bottom: 0.0, left: 24.0 },
            ));
        }
        // The original mounts the bar on home too, collapsed until hovered (`buildAppOverlaysModel.ts:255`).
        if (playing || self.page == Page::Library) && self.playback.current().is_some() {
            let look = player_bar::Look {
                i18n: &self.i18n,
                primary: text_color,
                secondary: muted,
                background: folia_visualizer::color(self.theme.background, 1.0),
                daylight,
                window: self.window_size,
                bottom: self.bar_offset(),
            };
            frame = frame.push(self.bar.view(&self.playback, look, now).map(Message::Bar));
        }
        if let (true, Some(_)) = (playing, self.positioning) {
            let row = overlay::capsule_row(self.window_size.width, false);
            let height = if self.window_size.width < 640.0 { 127.0 } else { 72.0 };
            let bar = iced::Rectangle {
                x: (self.window_size.width - row) / 2.0,
                y: self.window_size.height - self.bar_offset() - height,
                width: row,
                height,
            };
            let look = positioner::Look {
                i18n: &self.i18n,
                accent: folia_visualizer::color(self.theme.accent, 1.0),
                daylight,
                window: self.window_size,
                bar,
            };
            frame = frame.push(positioner::view(look).map(Message::Positioner));
        }
        if playing {
            let panel_view = panel::View {
                i18n: &self.i18n,
                theme: &self.theme,
                daylight,
                width: self.window_size.width,
                height: self.window_size.height,
                track: self.playback.current(),
                queue: self.playback.queue.items(),
                index: self.playback.queue.current_index(),
                volume: self.playback.settings.volume,
                muted: self.playback.muted,
                lyric_offset: self.playback.lyric_offset,
                bottom: self.bar_offset(),
                repeat: self.playback.settings.repeat,
                style_id: &self.appearance.visualizer,
                has_lyrics: self.playback.lyrics.as_ref().is_some_and(|lyrics| !lyrics.lines.is_empty()),
                font_scale: self.appearance.lyrics_font_scale,
                drawn_style: self.style(),
                origin: self.playback.queue.origin().map(|origin| origin.name.as_str()),
                art: self.playback.current().and_then(|track| self.art.peek(&track.id)).and_then(Option::as_ref),
                hover: self.panel.hover(),
                pill_hover: self.panel.pill_hover(),
            };
            frame = frame.push(self.panel.view(panel_view, std::time::Instant::now()).map(Message::Panel));
        }
        if let Some(window) = &self.settings {
            let values = settings::Values {
                language: self.general.language,
                resolved: self.i18n.language(),
                appearance: &self.appearance,
                style: self.chosen_style(),
                bar_offset: self.player.bottom_bar_offset,
                bar_max: folia_store::bottom_bar_max_offset(self.window_size.height),
                has_track: self.playback.current().is_some(),
            };
            frame = frame.push(window.view(&self.i18n, &self.theme, daylight, self.window_size, values));
        }
        // `z-[9999]`: the title bar sits above every page and dialog.
        let look = chrome::TitlebarLook {
            i18n: &self.i18n,
            player: playing,
            daylight,
            ink: text_color,
            maximized: self.maximized,
            fullscreen: self.fullscreen,
        };
        frame = frame.push(self.titlebar.view(look, now).map(Message::Chrome));
        frame = frame.push(chrome::resize_frame().map(Message::Chrome)).push(chrome::activity().map(Message::Chrome));
        let now = std::time::Instant::now();
        if self.splash.showing(now) {
            // A canvas for the plain cover keeps a GPU allocation after the splash is gone.
            if self.splash.word(now) > 0.004 {
                frame =
                    frame.push(iced::widget::canvas(self.splash.program(now)).width(Length::Fill).height(Length::Fill));
            } else {
                let color = self.splash.cover_color(now);
                let shade: Element<'_, Message> = container(space())
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .style(move |_| iced::widget::container::Style {
                        background: Some(color.into()),
                        ..iced::widget::container::Style::default()
                    })
                    .into();
                let layer = if self.splash.blocks_pointer() {
                    mouse_area(shade).interaction(mouse::Interaction::Idle).into()
                } else {
                    shade
                };
                frame = frame.push(layer);
            }
        }
        frame.width(Length::Fill).height(Length::Fill).into()
    }

    fn chrome_visible(&self) -> bool {
        self.page != Page::NowPlaying || self.clock.visible(self.now_ms(), self.pointer_held)
    }

    /// The original back button only appears inside the top-left 120px square.
    fn back_hotspot(&self) -> bool {
        self.clock.pointer.is_some_and(|(x, y)| x <= HOTSPOT && y <= HOTSPOT)
    }

    fn subtitle(&self) -> overlay::Subtitle<'_> {
        if self.style() == Style::Still {
            return overlay::Subtitle { translation: None, upcoming: None };
        }
        let Some(lyrics) = self.playback.lyrics.as_ref() else {
            return overlay::Subtitle { translation: None, upcoming: None };
        };
        overlay::subtitle_at(&lyrics.lines, self.playback.lyric_time())
    }

    /// Ticks only while music plays: every frame on the visualizer page, a few times a second elsewhere.
    pub fn subscription(&self) -> Subscription<Message> {
        let mut parts = Vec::new();
        parts.push(iced::window::resize_events().map(|(_id, size)| Message::WindowResized(size)));
        parts.push(iced::event::listen_with(fullscreen_key));
        if self.page == Page::NowPlaying {
            if self.clock.until_ms.is_some() {
                parts.push(iced::time::every(std::time::Duration::from_millis(200)).map(|_| Message::ChromeTick));
            }
            parts.push(iced::event::listen_with(chrome_pointer));
        }
        if let Some(window) = &self.settings {
            parts.push(iced::event::listen_with(settings_key));
            if window.animating() {
                parts.push(iced::window::frames().map(|_| Message::Settings(settings::Message::Frame)));
            }
        }
        if self.positioning.is_some() {
            parts.push(iced::event::listen_with(positioner_key));
        }
        if self.splash.showing(std::time::Instant::now()) {
            parts.push(iced::window::frames().map(|_| Message::SplashFrame));
        }
        let now = std::time::Instant::now();
        if self.page != Page::Search && self.bar.animating(now) {
            parts.push(iced::window::frames().map(|_| Message::Bar(player_bar::Message::Frame)));
        }
        if self.titlebar.animating(now) {
            parts.push(iced::window::frames().map(|_| Message::Chrome(chrome::Message::Frame)));
        }
        if self.page == Page::Library && self.home.animating(now) {
            parts.push(iced::window::frames().map(|_| Message::Home(home::Message::Frame)));
        }
        if self.page == Page::NowPlaying && self.panel.animating(now) {
            parts.push(iced::window::frames().map(|_| Message::Panel(panel::Message::Frame)));
        }
        if self.page == Page::Library && self.home_menu_open() {
            parts.push(iced::event::listen_with(home_dismiss));
        }
        if self.page == Page::Search {
            if self.settings.is_none() {
                parts.push(iced::event::listen_with(search_key));
            }
            if self.search.animating(now) {
                parts.push(iced::window::frames().map(|_| Message::Search(search::Message::Frame)));
            }
        }
        if self.playback.is_playing() {
            let tick = |_| Message::Playback(playback::Message::Tick);
            let playback = if self.page != Page::NowPlaying {
                iced::time::every(std::time::Duration::from_millis(250)).map(tick)
            } else {
                let fps = match (self.appearance.frame_rate.fps(), self.style().max_fps()) {
                    (Some(setting), Some(cap)) => Some(setting.min(cap)),
                    (setting, cap) => setting.or(cap),
                };
                match fps {
                    Some(fps) => iced::time::every(std::time::Duration::from_secs_f64(1.0 / f64::from(fps))).map(tick),
                    None => iced::window::frames().map(tick),
                }
            };
            parts.push(playback);
        }
        Subscription::batch(parts)
    }

    fn style(&self) -> Style {
        match self.chosen_style() {
            Some(style) => style,
            None => Style::random_for(self.playback.current().map_or(0, |track| track.id)),
        }
    }

    /// The style picked in settings; `None` means a random style per song.
    fn chosen_style(&self) -> Option<Style> {
        if self.appearance.visualizer == RANDOM_STYLE {
            return None;
        }
        Some(Style::from_id(&self.appearance.visualizer).unwrap_or_default())
    }

    fn home_host(&self) -> home::Host<'_> {
        home::Host {
            i18n: &self.i18n,
            text: folia_visualizer::color(self.theme.primary, 1.0),
            background: folia_visualizer::color(self.theme.background, 1.0),
            accent: folia_visualizer::color(self.theme.accent, 1.0),
            daylight: self.appearance.theme == ThemeMode::Light,
            has_track: self.playback.current().is_some(),
            busy: self.library.busy,
            volume: self.playback.settings.volume,
            muted: self.playback.muted,
            window_width: self.window_size.width,
            window_height: self.window_size.height,
            seam: self.library.seam(self.window_size.width),
        }
    }

    /// Decodes the playing song's cover for the panel in the background, unless it is cached or on its way.
    fn ensure_art(&mut self) -> Task<Message> {
        let Some(track) = self.playback.current() else { return Task::none() };
        let id = track.id;
        if self.art_pending == Some(id) {
            return Task::none();
        }
        if self.art.peek(&id).is_some() {
            self.art.touch(&id);
            return Task::none();
        }
        if let Some(url) = track.cover_url.clone() {
            let Some(omni) = self.omni.clone() else {
                self.art.insert(id, None, 0);
                return Task::none();
            };
            self.art_pending = Some(id);
            let load = async move {
                let path = omni.fetch_cover(&url).await.map_err(|err| err.to_string())?;
                let bytes = std::fs::read(&path).map_err(|err| err.to_string())?;
                folia_library::cover_rgba(&bytes, ART_PX, ART_PX).map_err(|err| err.to_string())
            };
            return Task::perform(load, move |cover: Result<folia_library::Cover, String>| {
                let cover = cover.inspect_err(|err| tracing::warn!(error = %err, "online cover unavailable")).ok();
                Message::Art(id, cover)
            });
        }
        if track.path.as_os_str().is_empty() {
            self.art.insert(id, None, 0);
            return Task::none();
        }
        self.art_pending = Some(id);
        let path = track.path.clone();
        Task::perform(
            crate::background::blocking(move || folia_library::cover_for(&path, ART_PX, ART_PX)),
            move |cover| Message::Art(id, cover.flatten()),
        )
    }

    fn positioner(&mut self, message: positioner::Message) -> Task<Message> {
        let Some(positioning) = self.positioning.as_mut() else { return Task::none() };
        match message {
            positioner::Message::Grab => positioning.grab(self.clock.pointer.map(|(_, y)| y)),
            positioner::Message::Reset => positioning.reset(),
            positioner::Message::Cancel => self.positioning = None,
            positioner::Message::Commit => {
                self.player.bottom_bar_offset = positioning.live;
                self.positioning = None;
                return save(&self.store, &self.player);
            }
        }
        Task::none()
    }

    /// `bottom` of the bar and everything on its baseline: the one being dragged, else the saved one.
    fn bar_offset(&self) -> f32 {
        let offset = self.positioning.map_or(self.player.bottom_bar_offset, |positioning| positioning.live);
        folia_store::clamp_bottom_bar_offset(offset, self.window_size.height)
    }

    fn panel(&mut self, message: panel::Message) -> Task<Message> {
        let style = self.appearance.visualizer.clone();
        match self.panel.update(&message, &style) {
            panel::Effect::None => Task::none(),
            panel::Effect::Home => {
                self.page = Page::Library;
                Task::none()
            }
            panel::Effect::Locate => {
                self.page = Page::Library;
                if let Some(index) = self.playback.queue.current_index() {
                    self.library.reveal(index);
                }
                Task::none()
            }
            panel::Effect::Settings => {
                self.settings = Some(settings::Settings::open());
                Task::none()
            }
            panel::Effect::Intensity => {
                self.theme.intensity = match self.theme.intensity {
                    folia_core::AnimationIntensity::Calm => folia_core::AnimationIntensity::Normal,
                    folia_core::AnimationIntensity::Normal => folia_core::AnimationIntensity::Chaotic,
                    folia_core::AnimationIntensity::Chaotic => folia_core::AnimationIntensity::Calm,
                };
                Task::none()
            }
            panel::Effect::Playback(message) => self.playback.update(message, &self.store).map(Message::Playback),
            panel::Effect::Theme(mode) => {
                let before = self.appearance.clone();
                let intensity = self.theme.intensity;
                self.appearance.theme = mode;
                self.theme = mode.theme();
                self.theme.intensity = intensity;
                self.iced_theme = iced_theme(&self.theme);
                self.appearance_changed(&before)
            }
            panel::Effect::Visualizer(style) => {
                self.appearance.visualizer = style.map_or(RANDOM_STYLE, Style::id).to_owned();
                save(&self.store, &self.appearance)
            }
        }
    }

    fn home(&mut self, message: home::Message) -> Task<Message> {
        self.home.update(&message);
        match message {
            home::Message::Search => {
                self.page = Page::Search;
                self.search.reveal();
                Task::none()
            }
            home::Message::Player => {
                if self.playback.current().is_some() {
                    self.page = Page::NowPlaying;
                    self.reveal_chrome();
                }
                Task::none()
            }
            home::Message::Settings => {
                self.settings = Some(settings::Settings::open());
                Task::none()
            }
            home::Message::Import => {
                let (task, _) = self.library.update(library::Message::Import);
                task.map(Message::Library)
            }
            home::Message::Reload => {
                let (task, _) = self.library.update(library::Message::Reload);
                task.map(Message::Library)
            }
            home::Message::Volume(volume) => {
                self.playback.update(playback::Message::Volume(volume), &self.store).map(Message::Playback)
            }
            home::Message::VolumeDone => {
                self.playback.update(playback::Message::VolumeDone, &self.store).map(Message::Playback)
            }
            home::Message::Mute => {
                self.playback.update(playback::Message::ToggleMute, &self.store).map(Message::Playback)
            }
            home::Message::Dismiss
            | home::Message::Fold
            | home::Message::More
            | home::Message::Tools
            | home::Message::BackHover(_)
            | home::Message::Frame => Task::none(),
        }
    }

    fn sync_home_back(&mut self) {
        let inside = self.clock.pointer.is_some_and(|(x, y)| x <= HOTSPOT && y <= HOTSPOT);
        let shown = self.page == Page::Library && self.playback.current().is_some() && inside;
        self.home.set_back(shown, std::time::Instant::now());
    }

    fn home_menu_open(&self) -> bool {
        self.home.menu_open()
    }

    /// Saves the appearance and lets an open settings window glide the marks the change moved.
    fn appearance_changed(&mut self, before: &AppearanceSettings) -> Task<Message> {
        if let Some(window) = self.settings.as_mut() {
            window.changed(before, &self.appearance);
        }
        save(&self.store, &self.appearance)
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

/// Top-left square that reveals the back button, from `PLAYER_CHROME_HOTSPOT_SIZE`.
const HOTSPOT: f32 = 120.0;
/// `PLAYER_BOTTOM_BAR_SUBTITLE_CLEARANCE_PX`: the subtitle sits this far above the bar's baseline while it is up.
const SUBTITLE_CLEARANCE: f32 = 80.0;
/// Subtitle `bottom` once the capsule has hidden. The baseline itself stays at 32.
const SUBTITLE_BOTTOM_HIDDEN: f32 = 32.0;

fn pin<'a>(button: Element<'a, Message>, x: Alignment, y: Alignment, pad: iced::Padding) -> Element<'a, Message> {
    container(button).width(Length::Fill).height(Length::Fill).align_x(x).align_y(y).padding(pad).into()
}

/// F11 toggles fullscreen, the shortcut listed in the original help.
fn fullscreen_key(event: iced::Event, _status: iced::event::Status, _id: iced::window::Id) -> Option<Message> {
    match event {
        iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
            key: iced::keyboard::Key::Named(iced::keyboard::key::Named::F11),
            repeat: false,
            ..
        }) => Some(Message::ToggleFullscreen),
        _ => None,
    }
}

/// Escape closes the search page. An open settings window takes the key first.
fn search_key(event: iced::Event, status: iced::event::Status, _id: iced::window::Id) -> Option<Message> {
    if status == iced::event::Status::Captured {
        return None;
    }
    match event {
        iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
            key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
            ..
        }) => Some(Message::Search(search::Message::Close)),
        _ => None,
    }
}

/// Escape clears the settings search, or closes the window when there is none; an open list takes it first.
fn settings_key(event: iced::Event, status: iced::event::Status, _id: iced::window::Id) -> Option<Message> {
    if status == iced::event::Status::Captured {
        return None;
    }
    match event {
        iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
            key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
            ..
        }) => Some(Message::Settings(settings::Message::Escape)),
        _ => None,
    }
}

/// Presses keep the chrome up; a scroll wheel counts as activity. Neither captures the event.
fn home_dismiss(event: iced::Event, status: iced::event::Status, _id: iced::window::Id) -> Option<Message> {
    if status == iced::event::Status::Captured {
        return None;
    }
    match event {
        iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
            key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
            ..
        }) => Some(Message::Home(home::Message::Dismiss)),
        iced::Event::Mouse(iced::mouse::Event::ButtonPressed(_)) => Some(Message::Home(home::Message::Dismiss)),
        _ => None,
    }
}

fn chrome_pointer(event: iced::Event, _status: iced::event::Status, _id: iced::window::Id) -> Option<Message> {
    match event {
        iced::Event::Mouse(iced::mouse::Event::ButtonPressed(_)) => Some(Message::PointerHeld(true)),
        iced::Event::Mouse(iced::mouse::Event::ButtonReleased(_)) => Some(Message::PointerHeld(false)),
        iced::Event::Mouse(iced::mouse::Event::WheelScrolled { .. }) => Some(Message::RevealChrome),
        _ => None,
    }
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

/// Positioning mode's keyboard exits: Escape rolls back, Enter keeps the new height.
fn positioner_key(event: iced::Event, _status: iced::event::Status, _id: iced::window::Id) -> Option<Message> {
    let iced::Event::Keyboard(iced::keyboard::Event::KeyPressed { key: iced::keyboard::Key::Named(named), .. }) = event
    else {
        return None;
    };
    match named {
        iced::keyboard::key::Named::Escape => Some(Message::PositionerKey(false)),
        iced::keyboard::key::Named::Enter => Some(Message::PositionerKey(true)),
        _ => None,
    }
}

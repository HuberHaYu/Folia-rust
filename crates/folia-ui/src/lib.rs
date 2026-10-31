// Top-level UI: pages, navigation and the iced update/view/subscription functions that drive them.

mod background;
mod chrome;
mod icons;
mod library;
mod now_playing;
mod overlay;
mod playback;
mod player_bar;
mod search;
mod settings;
mod settings_kit;
mod track_list;
mod wall;

pub use background::{error_chain, format_time};
pub use overlay::{
    CAPSULE_COLLAPSE_MS, CAPSULE_EXPAND_MS, CAPSULE_FADE_MS, CHROME_HOLD_MS, CHROME_LEAVE_MS, CapsuleFade,
    CapsuleHover, ChromeClock, GLOW_BLUR_PX, GlowEllipse, Subtitle, capsule_open, capsule_width, ease_out,
    glow_coverage, glow_ellipse, row_gap, subtitle_at, subtitle_line_width, translation_px, upcoming_look, upcoming_px,
};

use std::path::PathBuf;

use folia_core::{Language, LanguagePreference, Lyrics, Theme, Track};
use folia_i18n::I18n;
use folia_omni::Omni;
use folia_store::{AppearanceSettings, FrameRate, GeneralSettings, PlaybackSettings, Store, ThemeMode};
use folia_visualizer::{Scene, Style};
use iced::widget::{button, column, container, mouse_area, row, rule, space, stack};
use iced::{Alignment, Color, Element, Length, Size, Subscription, Task};
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
    CapsuleInside(bool),
    CapsuleFrame,
    ToggleFullscreen,
}

/// Settings id of the random lyric style choice, as in the original app.
const RANDOM_STYLE: &str = "random";

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
    capsule: CapsuleHover,
    /// Linear width of the capsule, 0 collapsed and 1 expanded, before easing.
    capsule_fade: CapsuleFade,
    window_size: Size,
    pointer_held: bool,
    /// Press on the drag strip, and the pointer position of the first move after it.
    drag_armed: bool,
    drag_from: Option<(f32, f32)>,
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
            capsule: CapsuleHover::collapsed(0),
            capsule_fade: CapsuleFade::settled(0.0, 0),
            window_size: Size::new(1200.0, 800.0),
            pointer_held: false,
            drag_armed: false,
            drag_from: None,
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
                let (task, play) = self.library.update(message);
                let task = task.map(Message::Library);
                match play {
                    Some(index) => {
                        self.page = Page::NowPlaying;
                        self.reveal_chrome();
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
                        self.reveal_chrome();
                        let start = self.playback.play(self.search.results.clone(), index).map(Message::Playback);
                        Task::batch([task, start])
                    }
                    None => task,
                }
            }
            Message::Playback(message) => self.playback.update(message, &self.store).map(Message::Playback),
            Message::OpenFiles(tracks) if !tracks.is_empty() => {
                self.page = Page::NowPlaying;
                self.reveal_chrome();
                self.playback.play(tracks, 0).map(Message::Playback)
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
                Task::none()
            }
            Message::PointerHeld(held) => {
                self.pointer_held = held;
                if !held {
                    self.drag_armed = false;
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
            Message::CapsuleInside(inside) => {
                self.capsule = self.capsule.set_inside(self.now_ms(), inside);
                Task::none()
            }
            Message::CapsuleFrame => {
                let now = self.now_ms();
                self.capsule = self.capsule.step(now);
                self.capsule_fade = self.capsule_fade.retarget(now, self.capsule_goal());
                Task::none()
            }
            Message::ToggleFullscreen => self.toggle_fullscreen(),
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
                Task::none()
            }
            chrome::Message::Left => {
                self.clock = self.clock.leave(self.now_ms(), self.pointer_held);
                Task::none()
            }
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        let (text_color, muted) =
            (folia_visualizer::color(self.theme.primary, 1.0), folia_visualizer::color(self.theme.secondary, 1.0));
        let daylight = self.appearance.theme == ThemeMode::Light;
        let page: Element<'_, Message> = match self.page {
            Page::Library => {
                let playing = self.playback.current().map(|track| track.id);
                self.library.view(&self.i18n, &self.theme, daylight, text_color, muted, playing).map(Message::Library)
            }
            Page::Search => {
                let playing = self.playback.current().map(|track| track.id);
                self.search.view(&self.i18n, muted, playing).map(Message::Search)
            }
            Page::NowPlaying => {
                let lyrics = self.playback.lyrics.as_ref().unwrap_or(&self.empty_lyrics);
                let scene = Scene {
                    style: self.style(),
                    lyrics,
                    time: self.playback.position(),
                    theme: &self.theme,
                    font_scale: self.appearance.lyrics_font_scale,
                };
                let subtitle = self.subtitle();
                let bottom = if self.chrome_visible() { SUBTITLE_BOTTOM_OPEN } else { SUBTITLE_BOTTOM_HIDDEN };
                let background = folia_visualizer::color(self.theme.background, 1.0);
                now_playing::view(scene, &self.i18n, muted, background, self.window_size, subtitle, bottom)
            }
        };
        let playing = self.page == Page::NowPlaying;
        let mut frame: iced::widget::Stack<'_, Message, iced::Theme, iced::Renderer> = if playing {
            stack![page, chrome::drag_strip().map(Message::Chrome)]
        } else {
            let bar = player_bar::view(&self.playback, &self.i18n, text_color, muted).map(Message::Playback);
            // The tab row is the title bar here: its empty space drags the window, its tabs still take clicks.
            let header = chrome::drag_area(self.navigation(text_color, muted), Message::Chrome);
            stack![column![header, container(page).height(Length::Fill), rule::horizontal(1), bar]]
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
        if playing && self.chrome_visible() {
            let openness = overlay::ease_out(self.capsule_fade.value(self.now_ms()));
            let width = overlay::capsule_width(self.window_size.width, openness);
            let bar = mouse_area(
                player_bar::capsule(&self.playback, &self.i18n, text_color, muted, daylight, openness)
                    .map(Message::Playback),
            )
            .on_enter(Message::CapsuleInside(true))
            .on_exit(Message::CapsuleInside(false));
            let bar = container(bar).width(Length::Fixed(width));
            let floating = container(bar)
                .width(Length::Fill)
                .height(Length::Fill)
                .padding(iced::Padding { top: 0.0, right: 16.0, bottom: CAPSULE_BOTTOM, left: 16.0 })
                .align_x(Alignment::Center)
                .align_y(Alignment::End);
            let settings_fill =
                if daylight { Color { r: 1.0, g: 1.0, b: 1.0, a: 0.7 } } else { Color { a: 0.4, ..Color::BLACK } };
            let settings_icon = if daylight { text_color } else { Color::WHITE };
            let settings = chrome::round_button(
                Icon::Settings2,
                20.0,
                48.0,
                settings_fill,
                settings_icon,
                self.i18n.t("ui.options"),
                Message::OpenSettings,
            );
            frame = frame.push(floating).push(pin(
                settings,
                Alignment::End,
                Alignment::End,
                iced::Padding { top: 0.0, right: 32.0, bottom: CAPSULE_BOTTOM, left: 0.0 },
            ));
            // Above the drag strip, so the tabs receive clicks.
            frame = frame.push(self.navigation(text_color, muted));
        }
        if !playing || self.chrome_visible() {
            let controls = container(
                chrome::controls(&self.i18n, text_color, self.maximized, self.fullscreen).map(Message::Chrome),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::End)
            .align_y(Alignment::Start);
            frame = frame.push(controls);
        }
        if let Some(window) = &self.settings {
            let values = settings::Values {
                language: self.general.language,
                resolved: self.i18n.language(),
                appearance: &self.appearance,
                style: self.chosen_style(),
            };
            frame = frame.push(window.view(&self.i18n, &self.theme, daylight, self.window_size, values));
        }
        frame = frame.push(chrome::resize_frame().map(Message::Chrome)).push(chrome::activity().map(Message::Chrome));
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
        overlay::subtitle_at(&lyrics.lines, self.playback.position())
    }

    fn capsule_goal(&self) -> f32 {
        let open =
            overlay::capsule_open(self.capsule.expanded, self.playback.is_playing(), self.playback.duration() > 0.0);
        if open { 1.0 } else { 0.0 }
    }

    fn capsule_live(&self) -> bool {
        let waiting = self.capsule.inside != self.capsule.expanded;
        waiting
            || (self.capsule_fade.to - self.capsule_goal()).abs() > 0.001
            || self.capsule_fade.running(self.now_ms())
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
            if self.chrome_visible() && self.capsule_live() {
                parts.push(iced::time::every(std::time::Duration::from_millis(16)).map(|_| Message::CapsuleFrame));
            }
            parts.push(iced::event::listen_with(chrome_pointer));
        }
        if let Some(window) = &self.settings {
            parts.push(iced::event::listen_with(settings_key));
            if window.animating() {
                parts.push(iced::window::frames().map(|_| Message::Settings(settings::Message::Frame)));
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

    fn navigation(&self, text_color: Color, muted: Color) -> Element<'_, Message> {
        let options_color = if self.settings.is_some() { text_color } else { muted };
        let options = button(
            row![
                icons::icon(Icon::Settings, 16.0, options_color),
                iced::widget::text(self.i18n.t("ui.options").to_owned()).color(options_color)
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .style(button::text)
        .padding([6, 12])
        .on_press(Message::OpenSettings);
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
            options,
        ]
        .padding(iced::Padding { top: 6.0, right: 12.0 + WINDOW_BUTTONS, bottom: 6.0, left: 12.0 })
        .align_y(Alignment::Center)
        .into()
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

/// Room the tab row leaves at its right end for the window buttons drawn over it.
const WINDOW_BUTTONS: f32 = 192.0;
/// Top-left square that reveals the back button, from `PLAYER_CHROME_HOTSPOT_SIZE`.
const HOTSPOT: f32 = 120.0;
/// Subtitle `bottom` while the capsule is on screen: baseline 32 plus 80px of clearance.
const SUBTITLE_BOTTOM_OPEN: f32 = 112.0;
/// Subtitle `bottom` once the capsule has hidden. The baseline itself stays at 32.
const SUBTITLE_BOTTOM_HIDDEN: f32 = 32.0;
/// Capsule `bottom`, the original `bottom-8`.
const CAPSULE_BOTTOM: f32 = 32.0;

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

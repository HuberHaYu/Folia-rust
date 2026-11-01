// Top-level UI: pages, navigation and the iced update/view/subscription functions that drive them.

mod album_wall;
mod background;
mod block_reflows;
mod chrome;
mod cover_flight;
mod drag_capture;
mod focus_card;
mod home;
mod icons;
mod library;
mod login;
mod lots;
mod mode_picker;
mod motion;
mod now_playing;
mod online;
mod overlay;
mod page_fade;
mod panel;
mod platform_icon;
mod playback;
mod player_bar;
mod positioner;
mod poster;
mod poster_motion;
mod queue_island;
mod search;
mod settings;
mod settings_kit;
mod splash;
mod track_list;
mod typeface;
mod wall;
mod wall_pan;
mod wall_shift;

pub use background::{error_chain, format_time};
pub use home::title::seam_title_activates;
pub use home::{default_level_is_spine, seam_safe_bottom};
pub use overlay::{
    CAPSULE_COLLAPSE_MS, CAPSULE_EXPAND_MS, CAPSULE_FADE_MS, CAPSULE_SETTLE_MS, CHROME_HOLD_MS, CHROME_LEAVE_MS,
    CapsuleHover, CapsuleSpring, ChromeClock, GLOW_BLUR_PX, GlowEllipse, Subtitle, capsule_open, capsule_row,
    capsule_width, glow_coverage, glow_ellipse, row_gap, subtitle_at, subtitle_line_width, translation_px,
    upcoming_look, upcoming_px,
};
pub use wall::{SEAM_ANCHOR, Seam, content_opacity, plan_opening, seam_geometry, seam_on_axis, wider_span};
pub use wall_pan::{Glide as WallGlide, Inertia as WallInertia, Release as WallRelease};
pub use wall_shift::{Kind as ShiftKind, Pose as ShiftPose, Shift as WallShift};

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
    /// The system keyring service that keeps platform sessions.
    pub keyring_service: String,
    /// Audio files given on the command line, played as a queue once the window opens.
    pub open_files: Vec<PathBuf>,
}

/// The home tab and the open collection, whose change turns the seam's content over.
type SeamKey = (folia_store::HomeTab, Option<String>, bool);

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
    /// The page being left, captured to fade out over the one replacing it.
    PageShot(iced::window::Screenshot),
    /// The capture is over; the page changes now if no picture of it came.
    PageShotDone,
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
    Login(login::Message),
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
    /// The page being left for or from the player fades out over the new one.
    page_fade: Option<page_fade::PageFade>,
    /// The page to show once the one being left has been captured.
    next_page: Option<Page>,
    seam_flip: Option<home::flip::SeamFlip>,
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
    login: login::Login,
}

impl Shell {
    pub fn new(init: Init) -> (Self, Task<Message>) {
        let theme = init.appearance.theme.theme();
        let omni = Omni::new(init.cache_dir, init.keyring_service)
            .inspect_err(|err| tracing::error!(error = err as &dyn std::error::Error, "online music unavailable"))
            .ok();
        let (library, load) = LibraryPage::new(init.library_db, init.general.home_tab, omni.clone());
        let shell = Self {
            store: init.store,
            general: init.general,
            iced_theme: iced_theme(&theme),
            theme,
            appearance: init.appearance,
            i18n: init.i18n,
            system_locale: init.system_locale,
            page: Page::Library,
            page_fade: None,
            next_page: None,
            seam_flip: None,
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
            login: login::Login::new(std::time::Instant::now()),
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
        let before = self.page;
        let settles_page = matches!(message, Message::PageShot(_) | Message::PageShotDone);
        let seam_before = self.seam_before(&message);
        let focus_title = matches!(message, Message::Home(home::Message::TitleToggle { keyboard: true, .. }));
        let task = self.handle(message);
        let now = std::time::Instant::now();
        if let Some((tab, layer, key)) = seam_before
            && key != self.seam_key()
        {
            self.seam_flip = Some(home::flip::SeamFlip::new(tab, layer, key.2, focus_title, now));
        }
        let mut focus = Task::none();
        if self.seam_flip.as_ref().is_some_and(|flip| !flip.running(now)) {
            if self.seam_flip.as_ref().is_some_and(|flip| flip.focus_title) {
                focus = iced::widget::operation::focus(home::title::id());
            }
            self.seam_flip = None;
        }
        if self.page_fade.as_ref().is_some_and(|fade| !fade.running(now)) {
            self.page_fade = None;
        }
        let fade = if settles_page { Task::none() } else { self.fade_page(before) };
        let task = Task::batch([task, fade, focus]);
        if self.page != Page::NowPlaying {
            self.positioning = None;
        }
        self.bar.set_home(self.page == Page::Library);
        self.home.sync(self.window_size.width, self.window_size.height, self.playback.current().is_some(), now);
        if let Some(horizontal) = self.home.take_flip(now) {
            self.library.set_horizontal(horizontal);
            self.library.replan(self.home.goal());
        }
        self.library.set_horizontal(self.home.is_horizontal());
        self.library.set_seam(self.home.width(now));
        self.library.plan_if_needed();
        let seam = self.library.seam();
        self.home.sync_tab(self.home.is_hidden() || seam.collapsed, now);
        self.login.sync(&self.library.online.account);
        self.library.set_tab_chrome(&mut self.home, now);
        self.library.sync_hint(&self.i18n, now);
        self.bar.set_forced(self.positioning.is_some());
        self.bar.sync(&self.playback, self.window_size.width, now);
        self.bar.set_shown(self.chrome_visible(), now);
        let room = panel::Room {
            width: self.window_size.width,
            height: self.window_size.height,
            bottom: self.bar_offset(),
            queue: self.playback.queue.items(),
            index: self.playback.queue.current_index(),
        };
        self.panel.follow(room);
        // The focus card stays above the player bar's safe area (`getBottomInset`) and knows what is playing.
        self.library.bottom_inset = if self.playback.current().is_some() {
            home::seam_safe_bottom(self.bar_offset(), self.home.presence(now))
        } else {
            focus_card::REVEAL_PAD
        };
        self.library.playing_id = self.playback.current().map(|track| track.id);
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
                let (task, effect) = self.library.update(message, &self.i18n);
                let task = task.map(Message::Library);
                Task::batch([task, self.library_effect(effect)])
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
            Message::PageShot(shot) => {
                if let Some(page) = self.next_page.take() {
                    self.page = page;
                    self.page_fade = Some(page_fade::PageFade::new(shot, std::time::Instant::now()));
                }
                Task::none()
            }
            Message::PageShotDone => {
                if let Some(page) = self.next_page.take() {
                    self.page = page;
                }
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
            Message::Login(message) => {
                let signed_in = self.library.online.signed_in().is_some();
                let (task, effect) = self.login.update(message, self.omni.as_ref(), signed_in);
                let task = task.map(Message::Login);
                let follow = match effect {
                    Some(login::Effect::Confirmed) => self.library.signed_in().map(Message::Library),
                    Some(login::Effect::SignOut) => {
                        // Signing out of the active platform empties the player, as a switch does.
                        self.playback.clear();
                        if self.page == Page::NowPlaying {
                            self.page = Page::Library;
                        }
                        self.library.sign_out(&self.i18n).map(Message::Library)
                    }
                    None => Task::none(),
                };
                Task::batch([task, follow])
            }
        }
    }

    /// Opens the shared sign-in window: on the platform in use when someone is signed in, unchosen for a guest.
    fn open_login(&mut self) -> Task<Message> {
        let signed_in = self.library.online.signed_in().is_some();
        let provider = signed_in.then_some(online::PROVIDER);
        self.login.open(provider, signed_in, self.omni.as_ref()).map(Message::Login)
    }

    /// Plays what the home wall chose, or keeps the tab it switched to.
    fn library_effect(&mut self, effect: Option<library::Effect>) -> Task<Message> {
        match effect {
            Some(library::Effect::Play { queue, index, origin }) => {
                self.page = Page::NowPlaying;
                self.reveal_chrome();
                let name = origin.unwrap_or_else(|| self.i18n.t("localMusic.allSongs").to_owned());
                let start = self.playback.play(queue, index, Some(folia_core::Origin { name })).map(Message::Playback);
                Task::batch([start, self.ensure_art()])
            }
            Some(library::Effect::PlayFm { queue, fm, origin }) => {
                self.page = Page::NowPlaying;
                self.reveal_chrome();
                let start =
                    self.playback.play_fm(queue, fm, folia_core::Origin { name: origin }).map(Message::Playback);
                Task::batch([start, self.ensure_art()])
            }
            Some(library::Effect::Enqueue(tracks)) => {
                let added = self.playback.enqueue(tracks);
                tracing::debug!(added, "songs added to the queue");
                Task::none()
            }
            Some(library::Effect::Toggle) => {
                self.playback.update(playback::Message::Toggle, &self.store).map(Message::Playback)
            }
            Some(library::Effect::EnterPlayer) => {
                self.page = Page::NowPlaying;
                self.reveal_chrome();
                Task::none()
            }
            Some(library::Effect::EnqueueOne(track)) => {
                // `applyQueueAddBehavior('append')`: a queued song moves to the end.
                match self.playback.queue.items().iter().position(|queued| queued.id == track.id) {
                    Some(index) => {
                        self.playback.update(playback::Message::MoveEnd(index), &self.store).map(Message::Playback)
                    }
                    None => {
                        self.playback.enqueue(vec![track]);
                        Task::none()
                    }
                }
            }
            Some(library::Effect::SaveTab(tab)) => {
                self.general.home_tab = tab;
                save(&self.store, &self.general)
            }
            None => Task::none(),
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
                let queued = self
                    .library
                    .expanded_track()
                    .is_some_and(|track| self.playback.queue.items().iter().any(|queued| queued.id == track.id));
                let look = library::CardLook {
                    queued,
                    playing: self.playback.is_playing(),
                    window_width: self.window_size.width,
                };
                self.library.view(&self.i18n, &self.theme, daylight, muted, playing, look).map(Message::Library)
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
            let mut layers = stack![page, shell];
            // The cover flying between the wall and the poster is drawn over both.
            if let (Page::Library, Some(flight)) = (self.page, self.library.flight_view(now)) {
                layers = layers.push(flight.map(Message::Library));
            }
            layers
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
        let look = login::Look {
            i18n: &self.i18n,
            tokens: settings_kit::Tokens::new(&self.theme, daylight),
            daylight,
            window: self.window_size,
            platforms: self.omni.as_ref().map(Omni::platforms).unwrap_or_default(),
            account: &self.library.online.account,
            avatar: self.library.online.avatar.as_ref(),
            has_track: self.playback.current().is_some(),
        };
        if let Some(window) = self.login.view(look, now) {
            frame = frame.push(window.map(Message::Login));
        }
        if let Some(fade) = &self.page_fade {
            frame = frame.push(fade.view(now));
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
        let quiet = self.settings.is_none() && !self.login.is_open() && !self.home_menu_open();
        if self.page == Page::Library && quiet {
            parts.push(iced::event::listen_with(wall_key));
            if self.library.filter_editing() {
                parts.push(iced::event::listen_with(filter_escape));
            }
        }
        if self.login.is_open() {
            parts.push(iced::event::listen_with(login_key));
        }
        if self.login.animating(std::time::Instant::now()) {
            parts.push(iced::window::frames().map(|_| Message::Login(login::Message::Frame)));
        }
        if self.splash.showing(std::time::Instant::now()) {
            parts.push(iced::window::frames().map(|_| Message::SplashFrame));
        }
        let now = std::time::Instant::now();
        if self.page != Page::Search && self.bar.animating(now) {
            parts.push(iced::window::frames().map(|_| Message::Bar(player_bar::Message::Frame)));
        }
        if self.page_fade.as_ref().is_some_and(|fade| fade.running(now)) {
            parts.push(iced::window::frames().map(|_| Message::Chrome(chrome::Message::Frame)));
        }
        if self.titlebar.animating(now) {
            parts.push(iced::window::frames().map(|_| Message::Chrome(chrome::Message::Frame)));
        }
        if self.page == Page::Library
            && (self.home.animating(now)
                || self.library.coasting()
                || self.library.hint_busy(now)
                || self.seam_flip.is_some())
        {
            parts.push(iced::window::frames().map(|_| Message::Library(library::Message::Frame)));
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
        let now = std::time::Instant::now();
        let leaving = self.seam_flip.as_ref().filter(|flip| flip.leaving(now));
        home::Host {
            i18n: &self.i18n,
            text: folia_visualizer::color(self.theme.primary, 1.0),
            background: folia_visualizer::color(self.theme.background, 1.0),
            accent: folia_visualizer::color(self.theme.accent, 1.0),
            daylight: self.appearance.theme == ThemeMode::Light,
            has_track: self.playback.current().is_some(),
            busy: self.library.busy,
            explore_refresh: self.library.explore_refresh(),
            volume: self.playback.settings.volume,
            muted: self.playback.muted,
            window_width: self.window_size.width,
            window_height: self.window_size.height,
            seam: self.library.seam(),
            spine: leaving.map_or(self.home.is_spine(), |flip| flip.spine),
            horizontal: self.home.is_horizontal(),
            opacity: content_opacity(self.library.seam().width, self.home.goal().max(1.0))
                * self.seam_flip.as_ref().map_or(1.0, |flip| flip.opacity(now)),
            bottom: if self.home.is_horizontal() {
                0.0
            } else {
                home::seam_safe_bottom(self.bar_offset(), self.home.presence(std::time::Instant::now()))
            },
            status: self.library.status(&self.i18n),
            show_tab: self.home.is_hidden() || self.library.seam().collapsed,
            tab_hidden: self.home.is_hidden(),
            tab: self.home.tab(std::time::Instant::now()),
            tabs: library::TABS.iter().map(|&tab| (tab, self.i18n.t(library::tab_label(tab)))).collect(),
            current: leaving.map_or(self.library.tab, |flip| flip.tab),
            account: self.account_slot(),
            layer: match leaving {
                Some(flip) => flip.layer.clone(),
                None => self.library.layer_seam(
                    &self.i18n,
                    self.playback.current().map(|track| track.id),
                    (self.window_size.height / home::LIST_ROW) as usize,
                    self.window_size,
                ),
            },
            poster_ground: self.library.poster_ground(),
            poster_tilted: self.library.poster_tilted(),
            list_hover: self.library.layer.as_ref().and_then(|layer| layer.list_hover),
        }
    }

    /// What the seam shows, to tell when its content changes layer or tab and turns over.
    fn seam_key(&self) -> SeamKey {
        (self.library.tab, self.library.layer.as_ref().map(|layer| layer.collection.id.clone()), self.home.is_spine())
    }

    /// The seam content before a message that can change the layer or the tab, for it to turn away.
    fn seam_before(&self, message: &Message) -> Option<(folia_store::HomeTab, Option<home::LayerSeam>, SeamKey)> {
        let turns = matches!(
            message,
            Message::Library(
                library::Message::Tab(_)
                    | library::Message::Back
                    | library::Message::ReloadCollection
                    | library::Message::Wall(wall::Message::Press(Some(_), _))
            ) | Message::Home(
                home::Message::SelectTab(_)
                    | home::Message::Back
                    | home::Message::ReloadCollection
                    | home::Message::TitleToggle { .. }
                    | home::Message::Expand
            )
        );
        if !turns || self.page != Page::Library {
            return None;
        }
        let rows = (self.window_size.height / home::LIST_ROW) as usize;
        let layer =
            self.library.layer_seam(&self.i18n, self.playback.current().map(|track| track.id), rows, self.window_size);
        Some((self.library.tab, layer, self.seam_key()))
    }

    /// A change of page to or from the player is held until the page being left is captured, then cross-faded.
    fn fade_page(&mut self, before: Page) -> Task<Message> {
        let to = self.page;
        let crosses = before != to && (before == Page::NowPlaying || to == Page::NowPlaying);
        let Some(window) = self.window.filter(|_| crosses) else { return Task::none() };
        self.page = before;
        let capturing = self.next_page.replace(to).is_some();
        if capturing {
            return Task::none();
        }
        iced::window::screenshot(window).map(Message::PageShot).chain(Task::done(Message::PageShotDone))
    }

    /// The seam's account entry on online tabs (`useBravaisHomeAccount`).
    fn account_slot(&self) -> Option<home::AccountSlot<'_>> {
        if !self.library.tab.is_online() || self.library.layer.is_some() {
            return None;
        }
        let platform = self.omni.as_ref()?.platforms().into_iter().find(|platform| platform.id == online::PROVIDER)?;
        let mark = folia_visualizer::color(folia_core::Rgb::hex(platform.color), 1.0);
        let label = platform.short_name;
        Some(match self.library.online.signed_in() {
            Some(account) => home::AccountSlot {
                guest: false,
                name: account.nickname.clone(),
                avatar: self.library.online.avatar.as_ref(),
                icon: platform_icon::platform_icon(platform.id),
                letter: platform.letter,
                mark,
                tip: format!("{} · {label} · {}", self.i18n.t("home.switchOnlineProvider"), account.nickname),
            },
            None => home::AccountSlot {
                guest: true,
                name: self.i18n.t("libraryBravaisHome.connect").to_owned(),
                avatar: None,
                icon: platform_icon::platform_icon(platform.id),
                letter: platform.letter,
                mark,
                tip: format!(
                    "{} · {label} · {}",
                    self.i18n.t("libraryBravaisHome.connect"),
                    self.i18n.t("libraryBravaisAccount.notSignedIn")
                ),
            },
        })
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
            return Task::perform(crate::background::online_cover(omni, url, ART_PX), move |cover| {
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
        let room = panel::Room {
            width: self.window_size.width,
            height: self.window_size.height,
            bottom: self.bar_offset(),
            queue: self.playback.queue.items(),
            index: self.playback.queue.current_index(),
        };
        let (effect, scroll) = self.panel.update(&message, &style, room);
        let scroll = scroll.map(Message::Panel);
        let task = match effect {
            panel::Effect::None => Task::none(),
            panel::Effect::Home => {
                self.page = Page::Library;
                Task::none()
            }
            panel::Effect::Locate => {
                self.page = Page::Library;
                match self.playback.current().map(|track| track.id) {
                    Some(track) => self.library_message(library::Message::Locate(track)),
                    None => Task::none(),
                }
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
        };
        Task::batch([task, scroll])
    }

    fn home(&mut self, message: home::Message) -> Task<Message> {
        if !matches!(message, home::Message::CollectionMenu) {
            self.library.dismiss_menu();
        }
        let reopen = matches!(message, home::Message::Tab) && !self.home.is_hidden();
        self.home.update(&message);
        if reopen {
            self.library.replan(self.home.goal());
        }
        match message {
            home::Message::TitleFocus { backwards } => {
                if backwards {
                    iced::widget::operation::focus_previous()
                } else {
                    iced::widget::operation::focus_next()
                }
            }
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
            home::Message::Import => self.library_message(library::Message::Import),
            home::Message::Reload => self.library_message(library::Message::Reload),
            home::Message::RefreshExplore => self.library_message(library::Message::RefreshExplore),
            home::Message::SelectTab(tab) => self.library_message(library::Message::Tab(tab)),
            home::Message::Back => self.library_message(library::Message::Back),
            home::Message::PlayAll => self.library_message(library::Message::PlayAll),
            home::Message::EnqueueAll => self.library_message(library::Message::EnqueueAll),
            home::Message::Query(query) => self.library_message(library::Message::Query(query)),
            home::Message::OpenList => self.library_message(library::Message::OpenList),
            home::Message::OpenFilter => self.library_message(library::Message::OpenFilter),
            home::Message::PosterCover(rect) => self.library_message(library::Message::PosterCoverAt(rect)),
            home::Message::FilterSubmit => self.library_message(library::Message::FilterSubmit),
            home::Message::ListScrolled(offset) => self.library_message(library::Message::ListScrolled(offset)),
            home::Message::LocateRow(row) => self.library_message(library::Message::LocateRow(row)),
            home::Message::PlayRow(row) => self.library_message(library::Message::PlayRow(row)),
            home::Message::ToggleSubscribe => self.library_message(library::Message::ToggleSubscribe),
            home::Message::CollectionMenu => self.library_message(library::Message::CollectionMenu),
            home::Message::ReloadCollection => self.library_message(library::Message::ReloadCollection),
            home::Message::RowHover(row) => self.library_message(library::Message::RowHover(row)),
            home::Message::Connect => self.open_login(),
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
            | home::Message::Expand
            | home::Message::TitleToggle { .. }
            | home::Message::Tab
            | home::Message::More
            | home::Message::Tools
            | home::Message::BackHover(_)
            | home::Message::Frame => Task::none(),
        }
    }

    fn library_message(&mut self, message: library::Message) -> Task<Message> {
        let (task, effect) = self.library.update(message, &self.i18n);
        Task::batch([task.map(Message::Library), self.library_effect(effect)])
    }

    fn sync_home_back(&mut self) {
        let inside = self.clock.pointer.is_some_and(|(x, y)| x <= HOTSPOT && y <= HOTSPOT);
        let shown = self.page == Page::Library && self.playback.current().is_some() && inside;
        self.home.set_back(shown, std::time::Instant::now());
    }

    fn home_menu_open(&self) -> bool {
        self.home.menu_open() || self.library.menu_open()
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

/// Escape closes the sign-in window (Sheet's own Escape).
/// The wall's keys (`resolveBravaisKey`), unless a control such as the filter took them.
fn wall_key(event: iced::Event, status: iced::event::Status, _id: iced::window::Id) -> Option<Message> {
    if status == iced::event::Status::Captured {
        return None;
    }
    let iced::Event::Keyboard(iced::keyboard::Event::KeyPressed { key, modifiers, repeat, .. }) = event else {
        return None;
    };
    if key == iced::keyboard::Key::Named(iced::keyboard::key::Named::Tab) {
        return Some(Message::Home(home::Message::TitleFocus { backwards: modifiers.shift() }));
    }
    focus_card::key_of(&key, modifiers, repeat).map(|key| Message::Library(library::Message::Key(key)))
}

/// Esc in the filter, which its input would otherwise keep for losing focus.
fn filter_escape(event: iced::Event, status: iced::event::Status, _id: iced::window::Id) -> Option<Message> {
    match event {
        iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
            key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
            repeat: false,
            ..
        }) if status == iced::event::Status::Captured => {
            Some(Message::Library(library::Message::Key(focus_card::Key::Escape)))
        }
        _ => None,
    }
}

fn login_key(event: iced::Event, _status: iced::event::Status, _id: iced::window::Id) -> Option<Message> {
    match event {
        iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
            key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
            ..
        }) => Some(Message::Login(login::Message::Close)),
        _ => None,
    }
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

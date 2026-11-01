// Home shell: the seam, the corner back button and the tools panel.

mod seam;
mod tools;

use std::time::Instant;

use folia_i18n::I18n;
use folia_visualizer::Ease;

use folia_store::HomeTab;

use crate::motion::Tween;
use crate::wall::Seam;
use iced::widget::{mouse_area, space};
use iced::{Color, Element, Length};

/// Seam width tween from `useBravaisSeam`: 0.36s, `cubic-bezier(0.65, 0, 0.35, 1)`.
const SEAM_EASE: Ease = Ease::new(0.65, 0.0, 0.35, 1.0);
const SEAM_MS: f64 = 360.0;
/// Panel open from `WallToolsButton`: 0.2s ease-out.
const PANEL_EASE: Ease = Ease::new(0.0, 0.0, 0.2, 1.0);
const PANEL_MS: f64 = 200.0;
/// Concealed back button: 200ms CSS `ease`.
const BACK_EASE: Ease = Ease::new(0.25, 0.1, 0.25, 1.0);
const BACK_MS: f64 = 200.0;
/// Edge tab slide: 240ms `cubic-bezier(0.2, 0.7, 0.25, 1)`.
const TAB_EASE: Ease = Ease::new(0.2, 0.7, 0.25, 1.0);
const TAB_MS: f64 = 240.0;
/// Menu pop from `bravaisPopMotion`: 0.18s in, 0.12s out.
const POP_IN: Ease = Ease::new(0.2, 0.7, 0.25, 1.0);
const POP_OUT: Ease = Ease::new(0.42, 0.0, 1.0, 1.0);
const POP_IN_MS: f64 = 180.0;
const POP_OUT_MS: f64 = 120.0;
/// `BRAVAIS_NARROW_VIEWPORT_PX`.
const NARROW_VW: f32 = 900.0;
const NARROW: f32 = 120.0;
const SPINE: f32 = 64.0;
const ROW_NARROW: f32 = 72.0;
const ROW_SPINE: f32 = 52.0;
/// `BRAVAIS_SEAM_FULL_WIDTH` and `BRAVAIS_SEAM_ROW_FULL_HEIGHT`: a collection's full info strip.
const FULL: f32 = 300.0;
/// `BRAVAIS_PANEL_MAX_WIDTH`.
pub const PANEL: f32 = 420.0;
/// `BRAVAIS_LIST_ROW_HEIGHT`.
pub const LIST_ROW: f32 = 44.0;
const ROW_FULL: f32 = 212.0;
/// Presence spring of the player safe area: stiffness 280, damping 28.
const PRESENCE_MS: f64 = 600.0;

/// `full` is the home narrow seam, `spine` the book spine, `hidden` the edge tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Level {
    Full,
    Spine,
    Hidden,
}

#[derive(Debug, Clone)]
pub enum Message {
    Fold,
    Expand,
    Tab,
    More,
    Tools,
    Search,
    Settings,
    Player,
    Import,
    Reload,
    /// 「换一批」 on the explore tab.
    RefreshExplore,
    Dismiss,
    Frame,
    Volume(f32),
    VolumeDone,
    Mute,
    BackHover(bool),
    SelectTab(HomeTab),
    /// The account entry: opens the shared sign-in window.
    Connect,
    /// The collection seam's back: returns to the home layer.
    Back,
    PlayAll,
    EnqueueAll,
    Query(String),
    OpenList,
    ListScrolled(f32),
    LocateRow(usize),
    PlayRow(usize),
    ToggleSubscribe,
    CollectionMenu,
    ReloadCollection,
    RowHover(Option<usize>),
}

pub struct HomeChrome {
    level: Level,
    /// The open width of the full level, vertical and sideways: the home seam, a collection's info strip,
    /// or its list panel.
    full: (f32, f32),
    restore: Level,
    horizontal: bool,
    /// The axis to open on once the current one has shut.
    pending_horizontal: Option<bool>,
    seen_width: f32,
    more: bool,
    tools_goal: f32,
    tools_from: f32,
    tools_at: Instant,
    seam_goal: f32,
    seam_from: f32,
    seam_at: Instant,
    back_goal: f32,
    back_from: f32,
    back_at: Instant,
    /// `transition: transform 200ms ease` on hover.
    back_hover: Tween,
    menu_goal: f32,
    menu_from: f32,
    menu_at: Instant,
    menu_ms: f64,
    presence_from: f32,
    presence_goal: f32,
    presence_at: Instant,
    tab_from: f32,
    tab_goal: f32,
    tab_at: Instant,
}

impl HomeChrome {
    pub fn new() -> Self {
        let now = Instant::now();
        Self {
            level: Level::Full,
            full: (NARROW, ROW_NARROW),
            restore: Level::Full,
            horizontal: false,
            pending_horizontal: None,
            seen_width: 0.0,
            more: false,
            tools_goal: 0.0,
            tools_from: 0.0,
            tools_at: now,
            seam_goal: NARROW,
            seam_from: NARROW,
            seam_at: now,
            back_goal: 0.0,
            back_from: 0.0,
            back_at: now,
            back_hover: Tween::new(0.0, BACK_MS, BACK_EASE, now),
            menu_goal: 0.0,
            menu_from: 0.0,
            menu_at: now,
            menu_ms: POP_IN_MS,
            presence_from: 0.0,
            presence_goal: 0.0,
            presence_at: now,
            tab_from: 0.0,
            tab_goal: 0.0,
            tab_at: now,
        }
    }

    pub fn update(&mut self, message: &Message) {
        let now = Instant::now();
        match message {
            Message::Fold => self.fold(now),
            Message::Expand => self.set_level(Level::Full, now),
            Message::Tab => self.restore_or_reopen(now),
            Message::More => {
                if self.level == Level::Spine {
                    self.set_level(Level::Full, now);
                    self.set_menu(now, true);
                } else {
                    self.set_menu(now, !self.more);
                }
            }
            Message::Tools => self.retarget_tools(now, if self.tools_goal > 0.5 { 0.0 } else { 1.0 }),
            Message::Dismiss => {
                self.set_menu(now, false);
                if self.tools_goal > 0.5 {
                    self.retarget_tools(now, 0.0);
                }
            }
            Message::Settings
            | Message::Player
            | Message::Search
            | Message::Import
            | Message::Reload
            | Message::RefreshExplore
            | Message::SelectTab(_)
            | Message::Connect
            | Message::Back
            | Message::PlayAll
            | Message::EnqueueAll
            | Message::OpenList
            | Message::ToggleSubscribe
            | Message::ReloadCollection => {
                self.set_menu(now, false);
                if self.tools_goal > 0.5 {
                    self.retarget_tools(now, 0.0);
                }
            }
            Message::BackHover(hovered) => self.back_hover.set(if *hovered { 1.0 } else { 0.0 }, now),
            Message::Frame
            | Message::Volume(_)
            | Message::VolumeDone
            | Message::Mute
            | Message::Query(_)
            | Message::ListScrolled(_)
            | Message::LocateRow(_)
            | Message::PlayRow(_)
            | Message::CollectionMenu
            | Message::RowHover(_) => {}
        }
    }

    /// Viewport width picks the default level; a portrait window (`height ≥ width × 1.1`) turns the seam sideways.
    pub fn sync(&mut self, width: f32, height: f32, has_track: bool, now: Instant) {
        if width > 0.0 {
            self.sync_width(width, now);
            self.sync_axis(height >= width * 1.1, now);
        }
        let goal = if has_track { 1.0 } else { 0.0 };
        if (goal - self.presence_goal).abs() > 0.001 {
            self.presence_from = self.presence(now);
            self.presence_goal = goal;
            self.presence_at = now;
        }
    }

    /// Opens the full info strip for a collection layer (`BRAVAIS_SEAM_FULL_WIDTH`), its list panel
    /// (`min(BRAVAIS_PANEL_MAX_WIDTH, viewport − 52)`), or narrows back to the home seam.
    pub fn set_collection(&mut self, collection: bool, list: bool, now: Instant) {
        let full = match (collection, list) {
            (false, _) => (NARROW, ROW_NARROW),
            (true, false) => (FULL, ROW_FULL),
            (true, true) => (PANEL.min(self.seen_width - 52.0).max(FULL.min(self.seen_width)), ROW_FULL),
        };
        if full == self.full {
            return;
        }
        self.full = full;
        self.set_menu(now, false);
        if self.level != Level::Hidden {
            self.retarget_seam(now);
        }
    }

    /// Slides the edge tab in when it becomes visible.
    pub fn sync_tab(&mut self, visible: bool, now: Instant) {
        let goal = if visible { 1.0 } else { 0.0 };
        if (goal - self.tab_goal).abs() < 0.001 {
            return;
        }
        self.tab_from = self.tab(now);
        self.tab_goal = goal;
        self.tab_at = now;
    }

    /// Once a direction change has shut the seam, opens it on the new axis. `Some` asks the wall to replan.
    pub fn take_flip(&mut self, now: Instant) -> Option<bool> {
        let next = self.pending_horizontal?;
        if self.width(now) > 0.5 {
            return None;
        }
        self.pending_horizontal = None;
        self.horizontal = next;
        self.seam_from = 0.0;
        self.seam_goal = open_width(self.level, next, self.full);
        self.seam_at = now;
        Some(next)
    }

    pub fn is_hidden(&self) -> bool {
        self.level == Level::Hidden
    }

    pub fn is_horizontal(&self) -> bool {
        self.horizontal
    }

    pub fn is_spine(&self) -> bool {
        self.level == Level::Spine
    }

    /// The width this level opens to, not the width it has animated to.
    pub fn goal(&self) -> f32 {
        open_width(self.level, self.horizontal, self.full)
    }

    pub fn set_back(&mut self, shown: bool, now: Instant) {
        let goal = if shown { 1.0 } else { 0.0 };
        if (goal - self.back_goal).abs() < 0.001 {
            return;
        }
        self.back_from = self.back(now);
        self.back_goal = goal;
        self.back_at = now;
    }

    pub fn menu_open(&self) -> bool {
        self.more || self.tools_goal > 0.5
    }

    pub fn animating(&self, now: Instant) -> bool {
        self.busy(self.tools_from, self.tools_goal, self.tools_at, now, PANEL_MS)
            || self.busy(self.seam_from, self.seam_goal, self.seam_at, now, SEAM_MS)
            || self.busy(self.back_from, self.back_goal, self.back_at, now, BACK_MS)
            || self.busy(self.menu_from, self.menu_goal, self.menu_at, now, self.menu_ms)
            || self.busy(self.tab_from, self.tab_goal, self.tab_at, now, TAB_MS)
            || self.back_hover.running(now)
            || self.presence_running(now)
            || self.pending_horizontal.is_some()
    }

    pub fn width(&self, now: Instant) -> f32 {
        mix(self.seam_from, self.seam_goal, self.seam_at, now, SEAM_MS, SEAM_EASE)
    }

    pub fn tools(&self, now: Instant) -> f32 {
        mix(self.tools_from, self.tools_goal, self.tools_at, now, PANEL_MS, PANEL_EASE)
    }

    pub fn back(&self, now: Instant) -> f32 {
        mix(self.back_from, self.back_goal, self.back_at, now, BACK_MS, BACK_EASE)
    }

    pub fn menu(&self, now: Instant) -> f32 {
        let ease = if self.menu_goal > self.menu_from { POP_IN } else { POP_OUT };
        mix(self.menu_from, self.menu_goal, self.menu_at, now, self.menu_ms, ease)
    }

    pub fn tab(&self, now: Instant) -> f32 {
        mix(self.tab_from, self.tab_goal, self.tab_at, now, TAB_MS, TAB_EASE)
    }

    pub fn presence(&self, now: Instant) -> f32 {
        let elapsed = now.saturating_duration_since(self.presence_at).as_secs_f64();
        if elapsed * 1000.0 >= PRESENCE_MS {
            return self.presence_goal;
        }
        folia_visualizer::spring(f64::from(self.presence_from), f64::from(self.presence_goal), elapsed, 280.0, 28.0)
            as f32
    }

    fn sync_width(&mut self, width: f32, now: Instant) {
        if self.seen_width == 0.0 {
            let level = default_level(width);
            self.level = level;
            self.restore = level;
            self.seen_width = width;
            let open = open_width(level, self.horizontal, self.full);
            self.seam_from = open;
            self.seam_goal = open;
            self.seam_at = now;
            return;
        }
        let crossed = (self.seen_width < NARROW_VW) != (width < NARROW_VW);
        self.seen_width = width;
        if !crossed {
            return;
        }
        let fallback = default_level(width);
        if self.level == Level::Hidden {
            self.restore = fallback;
        } else {
            self.set_level(fallback, now);
        }
    }

    fn sync_axis(&mut self, portrait: bool, now: Instant) {
        if portrait == self.horizontal || self.pending_horizontal == Some(portrait) {
            return;
        }
        self.pending_horizontal = Some(portrait);
        self.seam_from = self.width(now);
        self.seam_goal = 0.0;
        self.seam_at = now;
        self.set_menu(now, false);
    }

    fn fold(&mut self, now: Instant) {
        if self.level == Level::Hidden {
            return;
        }
        self.restore = self.level;
        self.set_level(Level::Hidden, now);
    }

    fn restore_or_reopen(&mut self, now: Instant) {
        if self.level == Level::Hidden {
            self.set_level(self.restore, now);
            return;
        }
        self.seam_from = 0.0;
        self.seam_goal = open_width(self.level, self.horizontal, self.full);
        self.seam_at = now;
    }

    fn set_level(&mut self, level: Level, now: Instant) {
        self.level = level;
        if level != Level::Hidden {
            self.restore = level;
        }
        if level != Level::Full {
            self.set_menu(now, false);
        }
        self.retarget_seam(now);
    }

    fn set_menu(&mut self, now: Instant, open: bool) {
        self.more = open;
        self.menu_from = self.menu(now);
        self.menu_goal = if open { 1.0 } else { 0.0 };
        self.menu_at = now;
        self.menu_ms = if open { POP_IN_MS } else { POP_OUT_MS };
    }

    fn retarget_seam(&mut self, now: Instant) {
        self.seam_from = self.width(now);
        self.seam_goal = open_width(self.level, self.horizontal, self.full);
        self.seam_at = now;
    }

    fn retarget_tools(&mut self, now: Instant, goal: f32) {
        self.tools_from = self.tools(now);
        self.tools_goal = goal;
        self.tools_at = now;
        if goal > 0.5 {
            self.set_menu(now, false);
        }
    }

    fn presence_running(&self, now: Instant) -> bool {
        (self.presence_from - self.presence_goal).abs() > 0.001
            && now.saturating_duration_since(self.presence_at).as_secs_f64() * 1000.0 < PRESENCE_MS
    }

    fn busy(&self, from: f32, goal: f32, at: Instant, now: Instant, ms: f64) -> bool {
        (from - goal).abs() > 0.001 && now.saturating_duration_since(at).as_secs_f64() * 1000.0 < ms
    }
}

/// `resolveSeamLevel` default: under 900px the seam starts as a spine.
pub fn default_level_is_spine(width: f32) -> bool {
    width < NARROW_VW
}

fn default_level(width: f32) -> Level {
    if default_level_is_spine(width) { Level::Spine } else { Level::Full }
}

fn open_width(level: Level, horizontal: bool, full: (f32, f32)) -> f32 {
    match (level, horizontal) {
        (Level::Hidden, _) => 0.0,
        (Level::Spine, false) => SPINE,
        (Level::Spine, true) => ROW_SPINE,
        (Level::Full, false) => full.0,
        (Level::Full, true) => full.1,
    }
}

/// Seam content bottom inset. No song keeps 20px; a song springs to `offset + 80 + 8`.
pub fn seam_safe_bottom(offset: f32, presence: f32) -> f32 {
    let safe = offset + 80.0 + 8.0;
    20.0 + presence * (safe - 20.0)
}

pub struct Host<'a> {
    pub i18n: &'a I18n,
    pub text: Color,
    pub background: Color,
    pub accent: Color,
    pub daylight: bool,
    pub has_track: bool,
    pub busy: bool,
    /// The explore tab's 「换一批」 (`refresh-explore`): `Some(enabled)` while the tab shows the page.
    pub explore_refresh: Option<bool>,
    pub volume: f32,
    pub muted: bool,
    pub window_width: f32,
    pub window_height: f32,
    pub seam: Seam,
    pub spine: bool,
    pub horizontal: bool,
    pub opacity: f32,
    pub bottom: f32,
    pub status: Option<String>,
    pub show_tab: bool,
    pub tab_hidden: bool,
    pub tab: f32,
    /// The first-level tabs with their labels, and the one open.
    pub tabs: Vec<(HomeTab, &'a str)>,
    pub current: HomeTab,
    /// The account entry above the tools, on online tabs only.
    pub account: Option<AccountSlot<'a>>,
    /// The open collection's title and count line; `None` on the home layer.
    pub layer: Option<LayerSeam>,
    /// The list row under the pointer.
    pub list_hover: Option<usize>,
}

/// The open collection as its seam shows it (`useBravaisCollectionSeam`).
#[derive(Debug, Clone)]
pub struct LayerSeam {
    pub title: String,
    /// The count, or `matches / total` while filtering.
    pub meta: String,
    pub query: String,
    pub filtering: bool,
    pub match_label: String,
    /// Saved, saving in flight, and the star's title; `None` where it cannot be saved.
    pub star: Option<(bool, bool, String)>,
    pub list: bool,
    /// The list rows in view start at row `first` of `count`.
    pub first: usize,
    pub count: usize,
    pub rows: Vec<ListRow>,
    pub menu: bool,
    pub playable: bool,
}

/// One row of the song list (`BravaisListPanel`): number, title over artist, length.
#[derive(Debug, Clone)]
pub struct ListRow {
    pub badge: String,
    pub title: String,
    pub subtitle: String,
    pub time: String,
    pub current: bool,
}

/// The seam's account entry (`BravaisSeamAccountSwitcher`).
pub struct AccountSlot<'a> {
    /// Nobody signed in: a plug and the connect label.
    pub guest: bool,
    pub name: String,
    pub avatar: Option<&'a iced::widget::image::Handle>,
    pub letter: char,
    pub mark: Color,
    pub tip: String,
}

/// Back button, then the seam, then a dismiss layer, then the tools. The seam paints over the back.
pub fn overlay<'a>(home: &'a HomeChrome, host: Host<'a>, now: Instant) -> Element<'a, Message> {
    let back = tools::back(&host, home.back(now), home.back_hover.value(now));
    let mut layers = iced::widget::stack![back, seam::bar(home, &host, now)];
    if home.menu_open() {
        let scrim = mouse_area(space().width(Length::Fill).height(Length::Fill)).on_press(Message::Dismiss);
        layers = layers.push(scrim);
    }
    if let Some(menu) = seam::popup(home, &host, now) {
        layers = layers.push(menu);
    }
    layers.push(tools::dock(home, &host, now)).width(Length::Fill).height(Length::Fill).into()
}

fn tr<'a>(host: &Host<'a>, key: &'static str) -> &'a str {
    host.i18n.t(key)
}

fn mix(from: f32, goal: f32, at: Instant, now: Instant, ms: f64, ease: Ease) -> f32 {
    let t = (now.saturating_duration_since(at).as_secs_f64() * 1000.0 / ms).clamp(0.0, 1.0);
    from + (goal - from) * ease.at(t) as f32
}

// Home shell: the left seam, the corner back button and the tools panel.

mod seam;
mod tools;

use std::time::Instant;

use folia_i18n::I18n;
use folia_visualizer::Ease;

use crate::motion::Tween;
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
/// Menu pop from `bravaisPopMotion`: 0.18s in, 0.12s out.
const POP_IN: Ease = Ease::new(0.2, 0.7, 0.25, 1.0);
const POP_OUT: Ease = Ease::new(0.42, 0.0, 1.0, 1.0);
const POP_IN_MS: f64 = 180.0;
const POP_OUT_MS: f64 = 120.0;
const NARROW: f32 = 120.0;
const SPINE: f32 = 64.0;

#[derive(Debug, Clone)]
pub enum Message {
    Fold,
    More,
    Tools,
    Search,
    Settings,
    Player,
    Import,
    Reload,
    Dismiss,
    Frame,
    Volume(f32),
    VolumeDone,
    Mute,
    BackHover(bool),
}

pub struct HomeChrome {
    spine: bool,
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
}

impl HomeChrome {
    pub fn new() -> Self {
        let now = Instant::now();
        Self {
            spine: false,
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
        }
    }

    pub fn update(&mut self, message: &Message) {
        let now = Instant::now();
        match message {
            Message::Fold => {
                self.spine = !self.spine;
                if self.spine {
                    self.set_menu(now, false);
                }
                self.retarget_seam(now);
            }
            Message::More => {
                if self.spine {
                    self.spine = false;
                    self.retarget_seam(now);
                }
                self.set_menu(now, !self.more);
            }
            Message::Tools => self.retarget_tools(now, if self.tools_goal > 0.5 { 0.0 } else { 1.0 }),
            Message::Dismiss => {
                self.set_menu(now, false);
                if self.tools_goal > 0.5 {
                    self.retarget_tools(now, 0.0);
                }
            }
            Message::Settings | Message::Player | Message::Search | Message::Import | Message::Reload => {
                self.set_menu(now, false);
                if self.tools_goal > 0.5 {
                    self.retarget_tools(now, 0.0);
                }
            }
            Message::BackHover(hovered) => self.back_hover.set(if *hovered { 1.0 } else { 0.0 }, now),
            Message::Frame | Message::Volume(_) | Message::VolumeDone | Message::Mute => {}
        }
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
            || self.back_hover.running(now)
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

    fn set_menu(&mut self, now: Instant, open: bool) {
        self.more = open;
        self.menu_from = self.menu(now);
        self.menu_goal = if open { 1.0 } else { 0.0 };
        self.menu_at = now;
        self.menu_ms = if open { POP_IN_MS } else { POP_OUT_MS };
    }

    fn retarget_seam(&mut self, now: Instant) {
        self.seam_from = self.width(now);
        self.seam_goal = if self.spine { SPINE } else { NARROW };
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

    fn busy(&self, from: f32, goal: f32, at: Instant, now: Instant, ms: f64) -> bool {
        (from - goal).abs() > 0.001 && now.saturating_duration_since(at).as_secs_f64() * 1000.0 < ms
    }
}

pub struct Host<'a> {
    pub i18n: &'a I18n,
    pub text: Color,
    pub background: Color,
    pub accent: Color,
    pub daylight: bool,
    pub has_track: bool,
    pub busy: bool,
    pub volume: f32,
    pub muted: bool,
    pub window_width: f32,
    pub window_height: f32,
    /// Where the wall has opened the seam on screen.
    pub seam: crate::wall::Seam,
}

/// Back button, then the seam, then a dismiss layer, then the tools. The seam paints over the back.
pub fn overlay<'a>(home: &'a HomeChrome, host: Host<'a>, now: Instant) -> Element<'a, Message> {
    // The seam is as wide as the wall opened it; near an edge that is less than the chrome asks for.
    let width = host.seam.width;
    let back = tools::back(&host, home.back(now), home.back_hover.value(now));
    let mut layers = iced::widget::stack![back, seam::bar(home, &host, width)];
    if home.menu_open() {
        let scrim = mouse_area(space().width(Length::Fill).height(Length::Fill)).on_press(Message::Dismiss);
        layers = layers.push(scrim);
    }
    if let Some(menu) = seam::popup(home, &host, width, now) {
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

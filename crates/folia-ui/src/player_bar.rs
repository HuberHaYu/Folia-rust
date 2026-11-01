// The now-playing control bar from `FloatingPlayerControls.tsx`: a pill with a thin progress bar that
// opens on hover into the play button, the switchable title, the progress row and two slot buttons.

use std::time::Instant;

use folia_core::Repeat;
use folia_i18n::I18n;
use folia_render::Plate;
use folia_visualizer::Ease;
use iced::widget::{button, column, container, mouse_area, row, shader, slider, space, stack, text, tooltip};
use iced::{Alignment, Background, Border, Color, Element, Font, Length, Size, Vector};
use lucide_static_svg::Icon;

use crate::icons::{Variant, icon, variant};
use crate::motion::{CSS_TRANSITION, Tween};
use crate::overlay::{CAPSULE_FADE_MS, CapsuleHover, CapsuleSpring, capsule_open, capsule_row, capsule_width};
use crate::playback::{self, Playback};

/// `HOVER_HITBOX_BOTTOM_BUFFER_PX`: the hover area reaches this far under the bar on the base line.
const HITBOX_BUFFER: f32 = 32.0;
/// `px-4 py-2` around a 32px row.
const COLLAPSED_H: f32 = 48.0;
/// `p-3` around the 48px play button.
const EXPANDED_H: f32 = 72.0;
/// Below `sm`: title 24, gap 8, play 48, gap 8, progress 15, inside `p-3`.
const EXPANDED_NARROW_H: f32 = 127.0;
const SM: f32 = 640.0;
const PLAY: f32 = 48.0;
const TITLE_H: f32 = 24.0;
/// `w-14` arrow zones on both sides of the title.
const ZONE: f32 = 56.0;
const SLIDE: f32 = 14.0;
const PREVIEW_OPACITY: f32 = 0.55;
/// `SWITCH_CONFIRM_MS`: no preview for this long after a switch.
const CONFIRM_MS: u128 = 1100;
/// Title enter and exit, `duration: 0.22, ease: 'easeOut'`.
const TITLE_MS: f64 = 220.0;
/// Arrow and preview `transition-opacity duration-200`.
const FADE_MS: f64 = 200.0;
/// Capsule `transition-colors duration-300`.
const TONE_MS: f64 = 300.0;
/// Play button `transition-transform`, Tailwind's 150ms.
const SCALE_MS: f64 = 150.0;
const TITLE_FONT: Font = Font { weight: iced::font::Weight::Bold, ..Font::DEFAULT };
const TIME_FONT: Font = Font { weight: iced::font::Weight::Medium, ..Font::MONOSPACE };

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Prev,
    Next,
}

#[derive(Debug, Clone)]
pub enum Message {
    Inside(bool),
    Frame,
    Playback(playback::Message),
    TitleHover(bool),
    Zone(Option<Side>),
    ArrowHover(Side, bool),
    Switch(Side),
    PlayHover(bool),
    SlotHover(usize, bool),
    /// A press on the bar outside its controls: `onNavigateToPlayer`.
    Open,
}

/// A new title sliding in while `old` slides out, from `AnimatePresence mode="sync"`.
struct Enter {
    at: Instant,
    slide: f32,
    old: String,
}

pub struct PlayerBar {
    epoch: Instant,
    hover: CapsuleHover,
    size: CapsuleSpring,
    shown: Tween,
    /// 0 collapsed colours, 0.5 a hovered collapsed bar, 1 expanded.
    tone: Tween,
    play: Tween,
    group_hover: bool,
    arrow_hover: [bool; 2],
    slot_hover: [bool; 2],
    arrows: [Tween; 2],
    zone: Option<Side>,
    current: Tween,
    preview_fade: Tween,
    preview: String,
    preview_of: Option<Side>,
    pending: Option<String>,
    confirm_at: Option<Instant>,
    last_side: Side,
    /// The playing entry: its place in the queue and its id (files opened outside the library share id 0).
    track: Option<(usize, i64)>,
    title: String,
    title_width: f32,
    enter: Option<Enter>,
    /// On home the bar only opens under the pointer and sits in a wider row (`currentView === 'home'`).
    home: bool,
    forced: bool,
}

/// What the bar needs from the shell.
pub struct Look<'a> {
    pub i18n: &'a I18n,
    /// `--text-primary`.
    pub primary: Color,
    /// `--text-secondary`.
    pub secondary: Color,
    /// `--bg-color`, the play glyph.
    pub background: Color,
    pub daylight: bool,
    pub window: Size,
    /// The bar's `bottom`: `PLAYER_BOTTOM_BAR_BASE_OFFSET_PX` lifted by the user's offset.
    pub bottom: f32,
}

impl PlayerBar {
    pub fn new(now: Instant) -> Self {
        let fade = Tween::new(0.0, FADE_MS, CSS_TRANSITION, now);
        Self {
            epoch: now,
            hover: CapsuleHover::collapsed(0),
            size: CapsuleSpring::settled(0.0, 0),
            shown: Tween::new(1.0, CAPSULE_FADE_MS as f64, Ease::OUT, now),
            tone: Tween::new(0.0, TONE_MS, CSS_TRANSITION, now),
            play: Tween::new(0.0, SCALE_MS, CSS_TRANSITION, now),
            group_hover: false,
            arrow_hover: [false; 2],
            slot_hover: [false; 2],
            arrows: [fade; 2],
            zone: None,
            current: Tween::new(1.0, TITLE_MS, Ease::OUT, now),
            preview_fade: fade,
            preview: String::new(),
            preview_of: None,
            pending: None,
            confirm_at: None,
            last_side: Side::Next,
            track: None,
            title: String::new(),
            title_width: 0.0,
            enter: None,
            home: false,
            forced: false,
        }
    }

    fn ms(&self, now: Instant) -> u64 {
        u64::try_from(now.saturating_duration_since(self.epoch).as_millis()).unwrap_or(u64::MAX)
    }

    /// Handles a bar message; a playback message is handed back for the shell to run.
    pub fn update(&mut self, message: Message, playback: &Playback, now: Instant) -> Option<playback::Message> {
        match message {
            Message::Inside(inside) => self.hover = self.hover.set_inside(self.ms(now), inside),
            Message::Frame | Message::Open => {}
            Message::Playback(message) => return Some(message),
            Message::TitleHover(hovered) => {
                self.group_hover = hovered;
                self.retarget_arrows(now);
            }
            Message::Zone(zone) => self.zone = zone,
            Message::ArrowHover(side, hovered) => {
                self.arrow_hover[side as usize] = hovered;
                self.retarget_arrows(now);
            }
            Message::Switch(side) => {
                self.last_side = side;
                self.pending = neighbor(playback, side).map(str::to_owned);
                if let Some(pending) = &self.pending {
                    self.title = folia_visualizer::clamp_lines(pending, 14.0, TITLE_FONT, self.title_width, 1);
                }
                self.confirm_at = Some(now);
                self.step(playback, now);
                return Some(match side {
                    Side::Prev => playback::Message::Previous,
                    Side::Next => playback::Message::Next,
                });
            }
            Message::PlayHover(hovered) => self.play.set(if hovered { 1.0 } else { 0.0 }, now),
            Message::SlotHover(index, hovered) => {
                if let Some(slot) = self.slot_hover.get_mut(index) {
                    *slot = hovered;
                }
            }
        }
        self.step(playback, now);
        None
    }

    /// Follows the playing track and the window; called after every shell update.
    pub fn sync(&mut self, playback: &Playback, window_width: f32, now: Instant) {
        let width = title_width(capsule_row(window_width, self.home), window_width);
        let id = playback.queue.current_index().zip(playback.current().map(|track| track.id));
        let changed = id != self.track;
        if changed || (width - self.title_width).abs() > 0.5 {
            // `currentSong.name`: the title alone.
            let raw = playback.current().map(|track| track.title.clone()).unwrap_or_default();
            let title = folia_visualizer::clamp_lines(&raw, 14.0, TITLE_FONT, width, 1);
            if changed && self.track.is_some() {
                if self.pending.as_deref() != Some(raw.as_str()) {
                    let slide = if self.last_side == Side::Next { SLIDE } else { -SLIDE };
                    self.enter = Some(Enter { at: now, slide, old: std::mem::take(&mut self.title) });
                }
                self.pending = None;
                self.confirm_at = Some(now);
            }
            self.track = id;
            self.title = title;
            self.title_width = width;
            self.preview_of = None;
        }
        self.step(playback, now);
    }

    /// Positioning mode shows the bar expanded (`isPositioning`).
    pub fn set_forced(&mut self, forced: bool) {
        self.forced = forced;
    }

    pub fn set_home(&mut self, home: bool) {
        self.home = home;
    }

    pub fn set_shown(&mut self, shown: bool, now: Instant) {
        self.shown.set(if shown { 1.0 } else { 0.0 }, now);
    }

    pub fn animating(&self, now: Instant) -> bool {
        let ms = self.ms(now);
        let waiting = self.hover.inside != self.hover.expanded;
        let tweens =
            [self.shown, self.tone, self.play, self.current, self.preview_fade, self.arrows[0], self.arrows[1]];
        let entering = self.enter.as_ref().is_some_and(|enter| elapsed_ms(enter.at, now) < TITLE_MS);
        waiting
            || self.size.running(ms)
            || tweens.iter().any(|tween| tween.running(now))
            || entering
            || (self.zone.is_some() && self.confirm_at.is_some())
    }

    fn step(&mut self, playback: &Playback, now: Instant) {
        let ms = self.ms(now);
        self.hover = self.hover.step(ms);
        let open = if self.forced {
            true
        } else if self.home {
            self.hover.expanded
        } else {
            capsule_open(self.hover.expanded, playback.is_playing(), playback.duration() > 0.0)
        };
        self.size = self.size.retarget(ms, if open { 1.0 } else { 0.0 });
        let tone = if open {
            1.0
        } else if self.hover.inside {
            0.5
        } else {
            0.0
        };
        self.tone.set(tone, now);
        if self.confirm_at.is_some_and(|at| now.saturating_duration_since(at).as_millis() >= CONFIRM_MS) {
            self.confirm_at = None;
        }
        if self.enter.as_ref().is_some_and(|enter| elapsed_ms(enter.at, now) >= TITLE_MS) {
            self.enter = None;
        }
        self.refresh_preview(playback, now);
    }

    /// `useTrackTitlePreview`: near an arrow the title fades out and the neighbour's shows at 55%.
    fn refresh_preview(&mut self, playback: &Playback, now: Instant) {
        let side = if self.confirm_at.is_some() { None } else { self.zone };
        let target = side.and_then(|side| neighbor(playback, side).map(|title| (side, title)));
        if let Some((side, title)) = target
            && self.preview_of != Some(side)
        {
            self.preview = folia_visualizer::clamp_lines(title, 14.0, TITLE_FONT, self.title_width, 1);
            self.preview_of = Some(side);
        }
        let previewing = target.is_some();
        self.current.set(if previewing { 0.0 } else { 1.0 }, now);
        self.preview_fade.set(if previewing { PREVIEW_OPACITY } else { 0.0 }, now);
    }

    fn retarget_arrows(&mut self, now: Instant) {
        for (index, tween) in self.arrows.iter_mut().enumerate() {
            let goal = if self.arrow_hover[index] {
                1.0
            } else if self.group_hover {
                0.7
            } else {
                0.0
            };
            tween.set(goal, now);
        }
    }

    pub fn view<'a>(&'a self, playback: &'a Playback, look: Look<'a>, now: Instant) -> Element<'a, Message> {
        let shown = self.shown.value(now);
        if shown <= 0.0 {
            return space().into();
        }
        let t = self.size.value(self.ms(now));
        let open = self.size.to > 0.5;
        let window = look.window;
        let narrow = window.width < SM;
        let row_w = capsule_row(window.width, self.home);
        let width = capsule_width(row_w, window.width, t).max(0.0);
        let open_h = if narrow { EXPANDED_NARROW_H } else { EXPANDED_H };
        let height = COLLAPSED_H + (open_h - COLLAPSED_H) * t;
        let tone = self.tone.value(now);
        let (fill, border) = if look.daylight {
            (Color { a: 0.4 + 0.2 * tone, ..Color::WHITE }, Color { a: 0.2, ..Color::WHITE })
        } else {
            (Color { a: 0.2 + 0.2 * tone, ..Color::BLACK }, Color { a: 0.05, ..Color::WHITE })
        };
        let body: Element<'a, Message> = if open {
            container(self.expanded(playback, &look, narrow, shown, now))
                .width(Length::Fixed(row_w))
                .height(Length::Fixed(open_h))
                .padding(12)
                .into()
        } else {
            let collapsed_w = capsule_width(row_w, window.width, 0.0);
            container(container(progress(playback, &look, shown)).padding([0, 16]).center_y(Length::Fixed(32.0)))
                .width(Length::Fixed(collapsed_w))
                .height(Length::Fixed(COLLAPSED_H))
                .padding([8, 16])
                .into()
        };
        let radius = height.min(width) / 2.0;
        let border = Color { a: border.a * shown, ..border };
        let capsule = container(body)
            .width(Length::Fixed(width))
            .height(Length::Fixed(height))
            .center_x(Length::Fixed(width))
            .center_y(Length::Fixed(height))
            .clip(true)
            .style(move |_| container::Style {
                border: Border { radius: radius.into(), width: 1.0, color: border },
                ..container::Style::default()
            });
        let lift = 24.0 * (1.0 - shown);
        let scale = 0.97 + 0.03 * shown;
        let plate = shadow_plate(width, height, radius, fill, look.daylight, open, shown);
        let plate_layer = float_in(plate.element, lift, scale);
        let plate_layer =
            container(plate_layer).width(Length::Fill).height(Length::Fill).align_x(Alignment::Center).padding(
                iced::Padding {
                    top: (window.height - look.bottom - height - plate.top).max(0.0),
                    ..iced::Padding::ZERO
                },
            );
        let capsule = mouse_area(capsule).on_press(Message::Open);
        let row = container(capsule).width(Length::Fixed(row_w)).align_x(Alignment::Center);
        // The hover area spans the whole row and the buffer below it, even under a collapsed bar.
        // `HOVER_HITBOX_BOTTOM_BUFFER_PX` shrinks as the bar is lifted; below it nothing takes the pointer.
        let hitbox = (HITBOX_BUFFER - (look.bottom - folia_store::BOTTOM_BAR_BASE)).max(0.0);
        let row = mouse_area(column![float_in(row.into(), lift, scale), space().height(Length::Fixed(hitbox))])
            .on_enter(Message::Inside(true))
            .on_exit(Message::Inside(false));
        let row = column![row, space().height(Length::Fixed((look.bottom - hitbox).max(0.0)))];
        let row_layer =
            container(row).width(Length::Fill).height(Length::Fill).align_x(Alignment::Center).align_y(Alignment::End);
        stack![plate_layer, row_layer].width(Length::Fill).height(Length::Fill).into()
    }

    fn expanded<'a>(
        &'a self,
        playback: &'a Playback,
        look: &Look<'a>,
        narrow: bool,
        shown: f32,
        now: Instant,
    ) -> Element<'a, Message> {
        let title = self.title_navigator(playback, look, shown, now);
        let play = self.play_button(playback, look, shown, now);
        let icon_px = if narrow { 20.0 } else { 18.0 };
        let [loop_slot, timeline] = slots(self, playback, look, icon_px, shown);
        let progress = container(progress(playback, look, shown)).padding([0, 8]);
        if narrow {
            let buttons = row![
                container(loop_slot).width(Length::Fill).align_x(Alignment::End),
                play,
                container(timeline).width(Length::Fill).align_x(Alignment::Start),
            ]
            .spacing(16)
            .align_y(Alignment::Center);
            return column![container(title).padding([0, 8]), buttons, progress].spacing(8).into();
        }
        let middle = column![container(title).padding([0, 8]), progress].spacing(8).width(Length::Fill);
        let slots = row![loop_slot, timeline].spacing(4).align_y(Alignment::Center);
        row![play, middle, slots].spacing(16).align_y(Alignment::Center).height(Length::Fill).into()
    }

    /// `TrackTitleNavigator`: the title with previous / next arrows that appear on hover.
    fn title_navigator<'a>(
        &'a self,
        playback: &'a Playback,
        look: &Look<'a>,
        shown: f32,
        now: Instant,
    ) -> Element<'a, Message> {
        let ink = look.primary;
        let mut layers = stack![].width(Length::Fill).height(Length::Fixed(TITLE_H));
        let current = self.current.value(now);
        // The layers stay the same in number, so a press on an arrow is not lost when a title starts sliding.
        let (old, old_alpha, old_x, enter_alpha, enter_x) = match &self.enter {
            Some(enter) => {
                let p = Ease::OUT.at(elapsed_ms(enter.at, now) / TITLE_MS) as f32;
                (enter.old.as_str(), 1.0 - p, -enter.slide * p, p, enter.slide * (1.0 - p))
            }
            None => ("", 0.0, 0.0, 1.0, 0.0),
        };
        layers = layers.push(title_text(old, ink, old_alpha * shown, old_x));
        layers = layers.push(title_text(&self.title, ink, enter_alpha * current * shown, enter_x));
        layers = layers.push(title_text(&self.preview, ink, self.preview_fade.value(now) * shown, 0.0));
        let zones = row![
            self.zone_view(playback, look, Side::Prev, shown, now),
            space().width(Length::Fill),
            self.zone_view(playback, look, Side::Next, shown, now),
        ];
        layers = layers.push(zones);
        mouse_area(container(layers).padding([0, 4]))
            .on_enter(Message::TitleHover(true))
            .on_exit(Message::TitleHover(false))
            .into()
    }

    fn zone_view<'a>(
        &'a self,
        playback: &'a Playback,
        look: &Look<'a>,
        side: Side,
        shown: f32,
        now: Instant,
    ) -> Element<'a, Message> {
        let index = side as usize;
        let (glyph, label, align, pad) = match side {
            Side::Prev => (Variant::BoldChevronLeft, "ui.previousTrack", Alignment::Start, [0.0, 0.0, 0.0, 4.0]),
            Side::Next => (Variant::BoldChevronRight, "ui.nextTrack", Alignment::End, [0.0, 4.0, 0.0, 0.0]),
        };
        let mut zone = container(space()).width(Length::Fixed(ZONE)).height(Length::Fixed(TITLE_H));
        if let Some(target) = neighbor(playback, side) {
            let opacity = self.arrows[index].value(now) * shown;
            let hover_fill =
                if look.daylight { Color { a: 0.10, ..Color::BLACK } } else { Color { a: 0.15, ..Color::WHITE } };
            let hovered = self.arrow_hover[index];
            let face = container(variant(glyph, 16.0, Color { a: opacity, ..look.primary }))
                .center(Length::Fixed(24.0))
                .style(move |_| container::Style {
                    background: hovered.then_some(Background::Color(Color { a: hover_fill.a * shown, ..hover_fill })),
                    border: Border { radius: 12.0.into(), ..Border::default() },
                    ..container::Style::default()
                });
            let arrow = button(face).padding(0).on_press(Message::Switch(side)).style(|_, _| button::Style::default());
            let arrow =
                mouse_area(arrow).on_enter(Message::ArrowHover(side, true)).on_exit(Message::ArrowHover(side, false));
            let tip = if target.is_empty() { look.i18n.t(label) } else { target };
            let arrow =
                tooltip(arrow, text(tip.to_owned()).size(12), tooltip::Position::Top).style(container::rounded_box);
            zone = container(arrow)
                .width(Length::Fixed(ZONE))
                .height(Length::Fixed(TITLE_H))
                .align_x(align)
                .align_y(Alignment::Center)
                .padding(iced::Padding { top: pad[0], right: pad[1], bottom: pad[2], left: pad[3] });
        }
        mouse_area(zone).on_enter(Message::Zone(Some(side))).on_exit(Message::Zone(None)).into()
    }

    /// 48px round play / pause in the theme's text colour; hover grows it to 105%.
    fn play_button<'a>(
        &'a self,
        playback: &'a Playback,
        look: &Look<'a>,
        shown: f32,
        now: Instant,
    ) -> Element<'a, Message> {
        let scale = 1.0 + 0.05 * self.play.value(now);
        let size = PLAY * scale;
        let fill = Color { a: look.primary.a * shown, ..look.primary };
        let glyph = Color { a: shown, ..look.background };
        let face: Element<'a, Message> = if playback.is_playing() {
            variant(Variant::FilledPause, 20.0 * scale, glyph)
        } else {
            // `ml-1` nudges the triangle right.
            container(variant(Variant::FilledPlay, 20.0 * scale, glyph))
                .padding(iced::Padding { left: 4.0 * scale, ..iced::Padding::ZERO })
                .into()
        };
        let face = container(face).center(Length::Fixed(size)).style(move |_| container::Style {
            background: Some(Background::Color(fill)),
            border: Border { radius: (size / 2.0).into(), ..Border::default() },
            shadow: iced::Shadow {
                color: Color { a: 0.1 * shown, ..Color::BLACK },
                offset: Vector::new(0.0, 10.0),
                blur_radius: 15.0,
            },
            ..container::Style::default()
        });
        let can_toggle = playback.current().is_some();
        let press = can_toggle.then_some(Message::Playback(playback::Message::Toggle));
        let button = button(face).padding(0).on_press_maybe(press).style(|_, _| button::Style::default());
        let button = mouse_area(button).on_enter(Message::PlayHover(true)).on_exit(Message::PlayHover(false));
        container(button).center(Length::Fixed(PLAY)).into()
    }
}

/// Room the title gets between the two 56px arrow zones, matching `expanded`'s layout.
fn title_width(row_w: f32, window_width: f32) -> f32 {
    let inner = row_w - 24.0;
    let column = if window_width < SM { inner } else { inner - PLAY - 16.0 - 72.0 - 16.0 };
    (column - 16.0 - 8.0 - 2.0 * ZONE).max(24.0)
}

fn neighbor(playback: &Playback, side: Side) -> Option<&str> {
    let queue = &playback.queue;
    let index = match side {
        Side::Prev => queue.previous_index(),
        Side::Next => queue.next_index(),
    }?;
    queue.items().get(index).map(|track| track.title.as_str())
}

fn elapsed_ms(at: Instant, now: Instant) -> f64 {
    now.saturating_duration_since(at).as_secs_f64() * 1000.0
}

/// One centred title line between the arrow zones, shifted `x` pixels.
fn title_text<'a>(content: &'a str, ink: Color, opacity: f32, x: f32) -> Element<'a, Message> {
    let line = text(content)
        .size(14)
        .font(TITLE_FONT)
        .color(Color { a: ink.a * opacity.clamp(0.0, 1.0), ..ink })
        .wrapping(iced::widget::text::Wrapping::None);
    let shift =
        iced::Padding { left: ZONE + (2.0 * x).max(0.0), right: ZONE + (-2.0 * x).max(0.0), ..iced::Padding::ZERO };
    container(line)
        .width(Length::Fill)
        .height(Length::Fixed(TITLE_H))
        .center_x(Length::Fill)
        .center_y(Length::Fixed(TITLE_H))
        .padding(shift)
        .clip(true)
        .into()
}

/// `ProgressBar`: `mm:ss`, a 6px track with the played part in the text colour, and the duration.
fn progress<'a>(playback: &Playback, look: &Look<'a>, shown: f32) -> Element<'a, Message> {
    let duration = playback.duration();
    let position = playback.position().clamp(0.0, duration.max(0.0));
    let time_color = Color { a: look.secondary.a * 0.6 * shown, ..look.secondary };
    let time = |seconds: f64, align: Alignment| {
        container(
            text(crate::background::format_time(seconds))
                .size(10)
                .font(TIME_FONT)
                .line_height(iced::widget::text::LineHeight::Relative(1.5))
                .color(time_color),
        )
        .width(Length::Fixed(32.0))
        .align_x(align)
    };
    let track = if look.daylight { Color { a: 0.05, ..Color::BLACK } } else { Color { a: 0.1, ..Color::WHITE } };
    let track = Color { a: track.a * shown, ..track };
    let fill = if duration > 0.0 { Color { a: look.primary.a * shown, ..look.primary } } else { track };
    let radius = if look.window.width >= 768.0 { 3.0 } else { 2.0 };
    let bar = slider(0.0..=duration.max(0.001), position, |value| Message::Playback(playback::Message::Seeking(value)))
        .on_release(Message::Playback(playback::Message::SeekDone))
        .step(0.1)
        .height(6)
        .style(move |_, _| slider::Style {
            rail: slider::Rail {
                backgrounds: (Background::Color(fill), Background::Color(track)),
                width: 6.0,
                border: Border { radius: radius.into(), ..Border::default() },
            },
            handle: slider::Handle {
                shape: slider::HandleShape::Circle { radius: 0.0 },
                background: Background::Color(Color::TRANSPARENT),
                border_width: 0.0,
                border_color: Color::TRANSPARENT,
            },
        });
    row![time(position, Alignment::End), bar, time(duration, Alignment::Start)]
        .spacing(12)
        .align_y(Alignment::Center)
        .into()
}

/// How a slot button looks: `active` is highlighted, `disabled` dimmed to 20%.
#[derive(Clone, Copy)]
struct Slot {
    index: usize,
    active: bool,
    disabled: bool,
    hovered: bool,
}

/// The two slots with their default actions: repeat, then the lyrics timeline.
fn slots<'a>(
    bar: &PlayerBar,
    playback: &Playback,
    look: &Look<'a>,
    icon_px: f32,
    shown: f32,
) -> [Element<'a, Message>; 2] {
    let (lucide, key) = match playback.settings.repeat {
        Repeat::Off => (Icon::RepeatOff, "player.loopOff"),
        Repeat::All => (Icon::Repeat, "player.loopAll"),
        Repeat::One => (Icon::Repeat1, "player.loopOne"),
    };
    let repeat =
        Slot { index: 0, active: playback.settings.repeat != Repeat::Off, disabled: false, hovered: bar.slot_hover[0] };
    let cycle = Some(Message::Playback(playback::Message::CycleRepeat));
    // The lyrics timeline window is not ported, so its slot stays disabled.
    let timeline = Slot { index: 1, active: false, disabled: true, hovered: bar.slot_hover[1] };
    let timeline_label = look.i18n.t("options.playerControlSlotAction_lyricsTimeline");
    [
        slot_button(lucide, look.i18n.t(key), repeat, look, icon_px, shown, cycle),
        slot_button(Icon::ChartBar, timeline_label, timeline, look, icon_px, shown, None),
    ]
}

/// `PlayerControlSlotButton`: `p-2 rounded-full` in the text colour. Opacity changes at once; only the
/// colours have `transition-colors`.
fn slot_button<'a>(
    lucide: Icon,
    label: &'a str,
    state: Slot,
    look: &Look<'a>,
    icon_px: f32,
    shown: f32,
    on_press: Option<Message>,
) -> Element<'a, Message> {
    let daylight = look.daylight;
    let opacity = if state.disabled {
        0.2
    } else if state.active || state.hovered {
        1.0
    } else {
        0.4
    };
    let background = if state.disabled {
        None
    } else if state.active {
        Some(if daylight { Color { a: 0.10, ..Color::BLACK } } else { Color { a: 0.20, ..Color::WHITE } })
    } else if state.hovered {
        Some(if daylight { Color { a: 0.05, ..Color::BLACK } } else { Color { a: 0.10, ..Color::WHITE } })
    } else {
        None
    };
    let background = background.map(|color| Background::Color(Color { a: color.a * opacity * shown, ..color }));
    let size = icon_px + 16.0;
    let ink = Color { a: look.primary.a * opacity * shown, ..look.primary };
    let face = container(icon(lucide, icon_px, ink)).center(Length::Fixed(size)).style(move |_| container::Style {
        background,
        border: Border { radius: (size / 2.0).into(), ..Border::default() },
        ..container::Style::default()
    });
    let button = button(face)
        .padding(0)
        .on_press_maybe(on_press.filter(|_| !state.disabled))
        .style(|_, _| button::Style::default());
    let button = mouse_area(button)
        .on_enter(Message::SlotHover(state.index, true))
        .on_exit(Message::SlotHover(state.index, false))
        .interaction(if state.disabled {
            iced::mouse::Interaction::NotAllowed
        } else {
            iced::mouse::Interaction::Pointer
        });
    tooltip(button, text(label.to_owned()).size(12), tooltip::Position::Top).style(container::rounded_box).into()
}

/// The capsule's fill and shadow, and how far the shadow reaches above the capsule.
struct ShadowPlate<'a> {
    element: Element<'a, Message>,
    top: f32,
}

/// `shadow-2xl` in the dark; daylight lists `shadow-xl` open and `shadow-lg` collapsed after it, so
/// their first layer wins there. `bg-*` is the fill.
fn shadow_plate<'a>(
    width: f32,
    height: f32,
    radius: f32,
    fill: Color,
    daylight: bool,
    open: bool,
    shown: f32,
) -> ShadowPlate<'a> {
    let (y, blur, spread, alpha): (f32, f32, f32, f32) = match (daylight, open) {
        (false, _) => (25.0, 50.0, -12.0, 0.25),
        (true, true) => (20.0, 25.0, -5.0, 0.1),
        (true, false) => (10.0, 15.0, -3.0, 0.1),
    };
    let reach = (blur * 1.5 + spread).max(0.0);
    let top = (reach - y).max(0.0);
    let bottom = reach + y;
    let plate = Plate {
        radius,
        fill_origin: [reach, top],
        fill_size: [width, height],
        fill: [fill.r, fill.g, fill.b, fill.a * shown],
        shadow: [0.0, 0.0, 0.0, alpha * shown],
        shadow_blur: blur,
        shadow_y: y,
        shadow_spread: spread,
        ring: [0.0; 4],
    };
    let element =
        shader(plate).width(Length::Fixed(width + 2.0 * reach)).height(Length::Fixed(height + top + bottom)).into();
    ShadowPlate { element, top }
}

/// The hiding transition: `y: 24, scale: 0.97` as the bar fades. Always a `float`, so the tree keeps its shape
/// when the transition ends and a press in flight is not dropped.
fn float_in<'a>(element: Element<'a, Message>, lift: f32, scale: f32) -> Element<'a, Message> {
    iced::widget::float(element).translate(move |_, _| Vector::new(0.0, lift)).scale(scale).into()
}

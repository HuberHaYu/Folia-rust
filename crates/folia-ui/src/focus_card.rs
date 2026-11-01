// The focus card (`useBravaisFocus`, `BravaisFocusCardBody`): a song grown to 6x6 in its block, the block making room.

use std::sync::LazyLock;
use std::time::Instant;

use folia_core::Track;
use folia_i18n::I18n;
use folia_visualizer::Ease;
use iced::widget::{button, column, container, pin, row, text, tooltip};
use iced::{Alignment, Background, Border, Color, Element, Length, Rectangle, Size, Vector};
use lucide_static_svg::Icon;

use crate::icons::icon;

/// `WALL_REFLOW_SPRING`: stiffness 300, damping 34, mass 1.
const STIFFNESS: f64 = 300.0;
const DAMPING: f64 = 34.0;
/// `CURVE_STEP_MS`.
const CURVE_STEP_MS: f64 = 4.0;
/// framer's `calcGeneratorDuration` steps and cap.
const GENERATOR_STEP_MS: f64 = 50.0;
const GENERATOR_MAX_MS: f64 = 20_000.0;
/// `WALL_REFLOW_CONTROLS_REVEAL`: 140ms in, 240ms, from 12px below, framer's default `easeInOut`.
const ACTIONS_DELAY_MS: f64 = 140.0;
const ACTIONS_MS: f64 = 240.0;
const ACTIONS_RISE: f32 = 12.0;
const EASE_IN_OUT: Ease = Ease::new(0.42, 0.0, 0.58, 1.0);
/// `BRAVAIS_CAMERA_TWEEN_S` with its `[0.22, 1, 0.36, 1]`.
const CAMERA_MS: f64 = 420.0;
const CAMERA_EASE: Ease = Ease::new(0.22, 1.0, 0.36, 1.0);
/// `BRAVAIS_REVEAL_PAD_PX`.
pub const REVEAL_PAD: f32 = 24.0;
/// `.bravais-tile-face { transition: transform 300ms ease-out }`: the focus pop.
const POP_MS: f64 = 300.0;
const EASE_OUT: Ease = Ease::new(0.0, 0.0, 0.58, 1.0);

/// A slot on the wall: a block's template slot, or an explore lot's seat (the heading is the seat past the last).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SlotId {
    pub column: i64,
    pub row: i64,
    pub index: usize,
    pub lot: bool,
}

impl SlotId {
    fn block(&self) -> (i64, i64, bool) {
        (self.column, self.row, self.lot)
    }
}

/// `createWallReflowCurve` for a whole block's width: the spring's progress sampled every 4ms, and when it rests.
struct Curve {
    duration_ms: f64,
    points: Vec<f32>,
}

/// The spring from rest at 0 to `distance`, `t` in milliseconds (framer's underdamped `spring`).
fn spring_at(distance: f64, t_ms: f64) -> f64 {
    let omega = STIFFNESS.sqrt();
    let zeta = DAMPING / (2.0 * STIFFNESS.sqrt());
    let damped = omega * (1.0 - zeta * zeta).sqrt();
    let t = t_ms / 1000.0;
    let envelope = (-zeta * omega * t).exp();
    distance - envelope * ((zeta * omega * distance) / damped * (damped * t).sin() + distance * (damped * t).cos())
}

static CURVE: LazyLock<Curve> = LazyLock::new(|| {
    let span = f64::from(crate::wall::BLOCK_WIDTH).max(1.0);
    // `restDelta` 0.5 and `restSpeed` 2/s for a move of 5 units or more; the speed is read over the last 5ms.
    let done = |t: f64| {
        let current = spring_at(span, t);
        let speed = if t == 0.0 {
            0.0
        } else {
            (current - spring_at(span, (t - 5.0).max(0.0))) * 1000.0 / (t - (t - 5.0).max(0.0))
        };
        speed.abs() <= 2.0 && (span - current).abs() <= 0.5
    };
    let mut duration_ms = 0.0;
    while !done(duration_ms) && duration_ms < GENERATOR_MAX_MS {
        duration_ms += GENERATOR_STEP_MS;
    }
    let count = ((duration_ms / CURVE_STEP_MS).round() as usize).max(2) + 1;
    let points = (0..count)
        .map(|index| {
            if index == count - 1 {
                1.0
            } else {
                ((spring_at(span, duration_ms * index as f64 / (count - 1) as f64) / span) * 1e5).round() as f32 / 1e5
            }
        })
        .collect();
    Curve { duration_ms, points }
});

/// `bravaisReflowEase`: the sampled spring, linearly between samples.
fn reflow_ease(progress: f32) -> f32 {
    let points = &CURVE.points;
    if progress <= 0.0 {
        return points[0];
    }
    if progress >= 1.0 {
        return 1.0;
    }
    let position = progress * (points.len() - 1) as f32;
    let index = position.floor() as usize;
    let from = points[index];
    from + (points[index + 1] - from) * (position - index as f32)
}

/// `BRAVAIS_REFLOW_MS`.
fn reflow_ms() -> f64 {
    CURVE.duration_ms
}

fn lerp(from: Rectangle, to: Rectangle, t: f32) -> Rectangle {
    Rectangle {
        x: from.x + (to.x - from.x) * t,
        y: from.y + (to.y - from.y) * t,
        width: from.width + (to.width - from.width) * t,
        height: from.height + (to.height - from.height) * t,
    }
}

/// One block on its way between two arrangements; each slot index has a rect in both.
struct Motion {
    block: (i64, i64, bool),
    from: Vec<Rectangle>,
    to: Vec<Rectangle>,
    at: Instant,
}

impl Motion {
    fn rect(&self, index: usize, now: Instant) -> Option<Rectangle> {
        let (from, to) = (self.from.get(index)?, self.to.get(index)?);
        let t = (now.saturating_duration_since(self.at).as_secs_f64() * 1000.0 / reflow_ms()) as f32;
        Some(lerp(*from, *to, reflow_ease(t)))
    }

    fn running(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.at).as_secs_f64() * 1000.0 < reflow_ms()
    }
}

/// The expanded card, the keyboard focus, the blocks moving, and the camera tween that reveals them.
#[derive(Default)]
pub struct Focus {
    /// The grown slot and the tile it shows.
    expanded: Option<(SlotId, usize)>,
    keyboard: Option<SlotId>,
    /// Where the keyboard focus was before, popping back in.
    left: Option<SlotId>,
    pop_at: Option<Instant>,
    motions: Vec<Motion>,
    camera: Option<(Vector, Vector, Instant)>,
    expanded_at: Option<Instant>,
}

/// How the wall lays out one block: its slots at rest and with `index` grown.
pub struct BlockShape {
    pub base: Vec<Rectangle>,
    pub grown: Vec<Rectangle>,
}

impl Focus {
    pub fn expanded(&self) -> Option<(SlotId, usize)> {
        self.expanded
    }

    pub fn keyboard(&self) -> Option<SlotId> {
        self.keyboard
    }

    /// Where a slot is drawn now: moving with its block, or grown with it, else where it rests.
    pub fn rect(&self, slot: SlotId, rest: Rectangle, now: Instant) -> Rectangle {
        self.motions
            .iter()
            .rev()
            .find(|motion| motion.block == slot.block())
            .and_then(|motion| motion.rect(slot.index, now))
            .unwrap_or(rest)
    }

    /// Where a slot is headed while its block moves or stays grown.
    pub fn target(&self, slot: SlotId) -> Option<Rectangle> {
        let motion = self.motions.iter().rev().find(|motion| motion.block == slot.block())?;
        motion.to.get(slot.index).copied()
    }

    /// Whether a slot's block is moving or grown: its tiles stay drawn though their rest is off screen.
    pub fn involves(&self, slot: SlotId) -> bool {
        self.motions.iter().any(|motion| motion.block == slot.block())
    }

    /// `expand`: the slot takes the 6x6, the other block (if any) folds back, and the camera reveals it.
    pub fn expand(&mut self, slot: SlotId, tile: usize, shape: BlockShape, previous: Option<BlockShape>, now: Instant) {
        if let (Some((old, _)), Some(previous)) = (self.expanded, previous)
            && old.block() != slot.block()
        {
            self.fold(old.block(), previous.base, now);
        }
        let from = (0..shape.grown.len())
            .map(|index| {
                let rest = shape.base.get(index).copied().unwrap_or(shape.grown[index]);
                self.rect(SlotId { index, ..slot }, rest, now)
            })
            .collect();
        self.motions.retain(|motion| motion.block != slot.block());
        self.motions.push(Motion { block: slot.block(), from, to: shape.grown, at: now });
        self.expanded = Some((slot, tile));
        self.expanded_at = Some(now);
        self.set_keyboard(Some(slot), now);
    }

    /// `collapse`: the block goes back to its template.
    pub fn collapse(&mut self, base: Vec<Rectangle>, now: Instant) {
        if let Some((slot, _)) = self.expanded.take() {
            self.fold(slot.block(), base, now);
        }
        self.expanded_at = None;
    }

    fn fold(&mut self, block: (i64, i64, bool), base: Vec<Rectangle>, now: Instant) {
        let from = (0..base.len())
            .map(|index| self.rect(SlotId { column: block.0, row: block.1, index, lot: block.2 }, base[index], now))
            .collect();
        self.motions.retain(|motion| motion.block != block);
        self.motions.push(Motion { block, from, to: base, at: now });
    }

    /// A new layout: nothing is grown or focused any more.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn set_keyboard(&mut self, slot: Option<SlotId>, now: Instant) {
        if self.keyboard != slot {
            self.left = self.keyboard;
            self.keyboard = slot;
            self.pop_at = Some(now);
        }
    }

    /// The keyboard focus pop (each side out by a gap) of a slot: 1 for the focused one once it has popped.
    pub fn pop(&self, slot: SlotId, now: Instant) -> f32 {
        let t = self.pop_at.map_or(1.0, |at| ease_at(at, now, POP_MS, EASE_OUT));
        if self.keyboard == Some(slot) {
            t
        } else if self.left == Some(slot) {
            1.0 - t
        } else {
            0.0
        }
    }

    /// The card's buttons rising in (`bravais-focus-actions-in`): opacity and how far below they still are.
    pub fn actions_reveal(&self, now: Instant) -> (f32, f32) {
        let t = self.expanded_at.map_or(1.0, |at| {
            let ms = now.saturating_duration_since(at).as_secs_f64() * 1000.0 - ACTIONS_DELAY_MS;
            if ms <= 0.0 { 0.0 } else { EASE_IN_OUT.at((ms / ACTIONS_MS).min(1.0)) as f32 }
        });
        (t, ACTIONS_RISE * (1.0 - t))
    }

    /// Starts the camera's 420ms tween to `target` (`tweenTo`); nothing when it is already there.
    pub fn tween_to(&mut self, from: Vector, target: Vector, now: Instant) {
        if (target.x - from.x).abs() < 0.5 && (target.y - from.y).abs() < 0.5 {
            return;
        }
        self.camera = Some((from, target, now));
    }

    /// Where the camera is this frame while its tween runs.
    pub fn camera(&mut self, now: Instant) -> Option<Vector> {
        let (from, to, at) = self.camera?;
        let t = ease_at(at, now, CAMERA_MS, CAMERA_EASE);
        if t >= 1.0 {
            self.camera = None;
        }
        Some(from + (to - from) * t)
    }

    /// A drag or a scroll takes the camera back from the tween (`stopCamera`).
    pub fn stop_camera(&mut self) {
        self.camera = None;
    }

    pub fn animating(&self, now: Instant) -> bool {
        self.camera.is_some()
            || self.motions.iter().any(|motion| motion.running(now))
            || self.pop_at.is_some_and(|at| ease_at(at, now, POP_MS, EASE_OUT) < 1.0)
            || self.expanded_at.is_some_and(|at| self.actions_reveal(now).0 < 1.0 && now >= at)
    }

    /// Drops the finished folds back to rest so a block at rest has no motion left.
    pub fn settle(&mut self, now: Instant) {
        let expanded = self.expanded.map(|(slot, _)| slot.block());
        self.motions.retain(|motion| motion.running(now) || Some(motion.block) == expanded);
    }
}

fn ease_at(at: Instant, now: Instant, ms: f64, ease: Ease) -> f32 {
    let t = (now.saturating_duration_since(at).as_secs_f64() * 1000.0 / ms).min(1.0);
    ease.at(t) as f32
}

/// The camera's place on the wall, for revealing a rect (`resolveRevealCenter`).
#[derive(Debug, Clone, Copy)]
pub struct View {
    pub center: Vector,
    pub size: Size,
    pub scale: f32,
    /// The seam's world line on its axis, its open width on screen and its push of either half.
    pub anchor: f32,
    pub seam_width: f32,
    pub push: f32,
    pub horizontal: bool,
    pub seam_edge: f32,
    pub bottom_inset: f32,
}

/// `resolveRevealCenter`: the smallest camera move that shows `rect` whole, keeping the seam on screen when both fit.
pub fn reveal(rect: Rectangle, view: View) -> Vector {
    let pad = REVEAL_PAD;
    let clamp = |value: f32, low: f32, high: f32| value.max(low).min(high);
    let has_seam = view.seam_width > 1.0;
    // Along the seam's axis: the half the rect is in is pushed away from the seam.
    let along = |start: f32, extent: f32, center: f32, length: f32, low_pad: f32, high_pad: f32, split: bool| {
        let middle = length / 2.0;
        let offset = if split && has_seam {
            if start + extent / 2.0 < view.anchor { -view.push } else { view.push }
        } else {
            0.0
        };
        let fit_low = start + extent - (length - high_pad - middle - offset) / view.scale;
        let fit_high = start - (low_pad - middle - offset) / view.scale;
        if split && has_seam {
            let need = view.seam_width / 2.0 + view.seam_edge;
            let seam_low = view.anchor - (length - need - middle) / view.scale;
            let seam_high = view.anchor + (middle - need) / view.scale;
            let (low, high) = (fit_low.max(seam_low), fit_high.min(seam_high));
            if low <= high {
                return clamp(center, low, high);
            }
        }
        if fit_low <= fit_high {
            clamp(center, fit_low, fit_high)
        } else if split {
            clamp(center, fit_high, fit_low)
        } else {
            start + extent / 2.0
        }
    };
    if view.horizontal {
        let x = along(rect.x, rect.width, view.center.x, view.size.width, pad, pad, false);
        let y = along(rect.y, rect.height, view.center.y, view.size.height, pad, view.bottom_inset, true);
        Vector::new(x, y)
    } else {
        let x = along(rect.x, rect.width, view.center.x, view.size.width, pad, pad, true);
        let y = along(rect.y, rect.height, view.center.y, view.size.height, pad, view.bottom_inset, false);
        Vector::new(x, y)
    }
}

/// `resolveBravaisKey`: the wall's keys (no printable characters, no Ctrl or Meta).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Move(Direction),
    Enter,
    Enqueue,
    Escape,
    First,
    Last,
    Page(i8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

pub fn key_of(key: &iced::keyboard::Key, modifiers: iced::keyboard::Modifiers, repeat: bool) -> Option<Key> {
    use iced::keyboard::key::Named;
    if modifiers.control() || modifiers.logo() {
        return None;
    }
    let iced::keyboard::Key::Named(named) = key else { return None };
    match named {
        // Alt+Enter opens the album or artist, which have no pages here.
        Named::Enter if !repeat && !modifiers.alt() => Some(if modifiers.shift() { Key::Enqueue } else { Key::Enter }),
        _ if modifiers.alt() || modifiers.shift() => None,
        Named::Escape if !repeat => Some(Key::Escape),
        Named::ArrowUp => Some(Key::Move(Direction::Up)),
        Named::ArrowDown => Some(Key::Move(Direction::Down)),
        Named::ArrowLeft => Some(Key::Move(Direction::Left)),
        Named::ArrowRight => Some(Key::Move(Direction::Right)),
        Named::Home => Some(Key::First),
        Named::End => Some(Key::Last),
        Named::PageDown => Some(Key::Page(1)),
        Named::PageUp => Some(Key::Page(-1)),
        _ => None,
    }
}

/// `findAdjacentSlot`: among `slots`, the nearest one whose middle lies on the `direction` side of `from`'s middle,
/// weighing the sideways distance double so the move stays in its row or column.
pub fn adjacent(
    from: Rectangle,
    direction: Direction,
    slots: impl Iterator<Item = (SlotId, Rectangle)>,
) -> Option<SlotId> {
    let origin = from.center();
    slots
        .filter_map(|(slot, rect)| {
            let center = rect.center();
            let (ahead, aside) = match direction {
                Direction::Up => (origin.y - center.y, center.x - origin.x),
                Direction::Down => (center.y - origin.y, center.x - origin.x),
                Direction::Left => (origin.x - center.x, center.y - origin.y),
                Direction::Right => (center.x - origin.x, center.y - origin.y),
            };
            (ahead > 1.0).then_some((slot, ahead + aside.abs() * 2.0))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(slot, _)| slot)
}

/// What the card's buttons do.
#[derive(Debug, Clone)]
pub enum Action {
    /// 「立即播放」, or pause / resume on the playing song.
    Play,
    /// 「进入」: the playing page.
    Enter,
    /// 「加入队列」, or moving it to the end when it is queued already.
    Enqueue,
}

/// What the card shows of its song and the player.
pub struct Card<'a> {
    pub track: &'a Track,
    pub queued: bool,
    pub current: bool,
    pub playing: bool,
    /// The card's rect on screen, and the wall's scale.
    pub rect: Rectangle,
    pub scale: f32,
    pub ink: Color,
    pub window_width: f32,
}

/// `BravaisFocusCardBody`: the big title, the artists · album · length, and the play, enter and queue buttons,
/// placed over the grown tile (`right: 32px; bottom: 28px; left: 32px`).
pub fn body<'a>(i18n: &'a I18n, card: Card<'a>, reveal: (f32, f32)) -> Element<'a, Action> {
    let s = card.scale;
    let ink = card.ink;
    let fade = move |alpha: f32| Color { a: ink.a * alpha, ..ink };
    // `.is-expanded .lattice-poster-copy strong`: clamp(48px, 6vw, 82px), line-height 0.95, at most 76% wide.
    let title_size = (card.window_width * 0.06).clamp(48.0, 82.0) * s;
    let title = container(
        text(card.track.title.clone())
            .size(title_size)
            .line_height(iced::widget::text::LineHeight::Relative(0.95))
            .font(crate::typeface::font(iced::font::Weight::Bold))
            .color(ink),
    )
    .max_width((card.rect.width - 64.0 * s) * 0.76)
    .max_height(title_size * 0.95 * 3.0)
    .clip(true);
    // `.bravais-focus-links`: 17px, 650, at 85%; the separators at half that.
    let link = |value: String| {
        text(value).size(17.0 * s).font(crate::typeface::font(iced::font::Weight::Semibold)).color(fade(0.85))
    };
    let mark = |value: &'static str| text(value).size(17.0 * s).color(fade(0.85 * 0.5));
    let mut links: Vec<Element<'a, Action>> = Vec::new();
    if let Some(artist) = card.track.artist.as_ref().filter(|artist| !artist.is_empty()) {
        for (index, name) in artist.split(" / ").enumerate() {
            if index > 0 {
                links.push(mark(",").into());
            }
            links.push(link(name.to_owned()).into());
        }
    }
    if let Some(album) = card.track.album.as_ref().filter(|album| !album.is_empty()) {
        if !links.is_empty() {
            links.push(mark("·").into());
        }
        links.push(link(album.clone()).into());
    }
    if card.track.duration > 0.0 {
        links.push(mark("·").into());
        links.push(link(crate::background::format_time(card.track.duration)).into());
    }
    let links = row(links).spacing(10.0 * s).align_y(Alignment::End).wrap();
    // `.bravais-chrome-button`: 44px, radius 12, 15px 700, at 80% until hovered.
    let chrome =
        |face: Element<'a, Action>, square: bool, action: Option<Action>, tip: Option<String>| -> Element<'a, Action> {
            let press = button(container(face).height(Length::Fixed(44.0 * s)).align_y(Alignment::Center))
                .width(if square { Length::Fixed(44.0 * s) } else { Length::Shrink })
                .padding(if square { iced::Padding::ZERO } else { iced::Padding::from([0.0, 16.0 * s]) })
                .on_press_maybe(action)
                .style(move |_theme, status| {
                    let hot = matches!(status, button::Status::Hovered | button::Status::Pressed);
                    button::Style {
                        background: hot.then_some(Background::Color(fade(0.16))),
                        border: Border {
                            radius: (12.0 * s).into(),
                            width: 1.0,
                            color: if hot { fade(0.26) } else { Color::TRANSPARENT },
                        },
                        ..button::Style::default()
                    }
                });
            match tip {
                Some(tip) => {
                    tooltip(press, text(tip).size(12), tooltip::Position::Top).style(container::rounded_box).into()
                }
                None => press.into(),
            }
        };
    let play_label = if card.current {
        i18n.t(if card.playing { "libraryBravais.pausePlayback" } else { "libraryBravais.resumePlayback" })
    } else {
        i18n.t("libraryBravais.playNow")
    };
    let play_icon = if card.current && card.playing { Icon::Pause } else { Icon::Play };
    let lit = move |alpha: f32| Color { a: ink.a * alpha * reveal.0, ..ink };
    let mut actions: Vec<Element<'a, Action>> = vec![chrome(
        container(icon(play_icon, 20.0 * s, lit(1.0))).center(Length::Fixed(44.0 * s)).into(),
        true,
        Some(Action::Play),
        Some(play_label.to_owned()),
    )];
    if card.current {
        actions.push(chrome(
            container(icon(Icon::Maximize2, 20.0 * s, lit(1.0))).center(Length::Fixed(44.0 * s)).into(),
            true,
            Some(Action::Enter),
            Some(i18n.t("libraryBravais.enterPlayer").to_owned()),
        ));
    }
    // 「已在队列」 shows 「插入队列」 on hover (`has-hover-label`); the playing song only ever says it is queued.
    let (glyph, label) =
        if card.queued { (Icon::Check, "libraryBravais.inQueue") } else { (Icon::Plus, "libraryBravais.addToQueue") };
    let queue_face = row![
        icon(glyph, 18.0 * s, lit(0.8)),
        text(i18n.t(label).to_owned())
            .size(15.0 * s)
            .font(crate::typeface::font(iced::font::Weight::Bold))
            .color(lit(0.8))
    ]
    .spacing(8.0 * s)
    .align_y(Alignment::Center);
    let hover_label = (card.queued && !card.current).then(|| i18n.t("libraryBravais.insertIntoQueue").to_owned());
    actions.push(chrome(queue_face.into(), false, Some(Action::Enqueue), hover_label));
    let actions = container(row(actions).spacing(8.0 * s).align_y(Alignment::Center).wrap()).padding(iced::Padding {
        top: 26.0 * s + reveal.1 * s,
        right: 0.0,
        bottom: 0.0,
        left: 0.0,
    });
    let copy = column![title, container(links).padding(iced::Padding::ZERO.top(7.0 * s)), actions];
    let placed = container(copy)
        .width(Length::Fixed(card.rect.width))
        .height(Length::Fixed(card.rect.height))
        .align_y(Alignment::End)
        .padding(iced::Padding { top: 0.0, right: 32.0 * s, bottom: 28.0 * s - reveal.1 * s, left: 32.0 * s });
    pin(placed).x(card.rect.x).y(card.rect.y).into()
}

/// Where the shade bands of a grown card are drawn (`.is-expanded .lattice-poster-shade`).
pub const EXPANDED_SHADE: [(f32, f32); 4] = [(0.0, 0.86), (0.14, 0.62), (0.30, 0.26), (0.46, 0.0)];
pub const EXPANDED_SIDE: [(f32, f32); 3] = [(0.0, 0.88), (0.52, 0.50), (1.0, 0.08)];

/// The keyboard focus ring (`.lattice-poster.is-focused:not(.is-current)`): a 1px hairline, a 3px ring in the ink,
/// then another hairline, outside the tile.
pub fn ring(daylight: bool, ink: Color) -> [(f32, Color); 3] {
    if daylight {
        [(1.0, Color::from_rgba(1.0, 1.0, 1.0, 0.78)), (4.0, ink), (5.0, Color::from_rgba(1.0, 1.0, 1.0, 0.45))]
    } else {
        [(1.0, Color::from_rgba(0.0, 0.0, 0.0, 0.62)), (4.0, ink), (5.0, Color::from_rgba(0.0, 0.0, 0.0, 0.38))]
    }
}

/// A tile popped by focus: each side out by a gap, scaled about its middle.
pub fn popped(rect: Rectangle, gap: f32, amount: f32) -> Rectangle {
    let grow = gap * amount;
    Rectangle { x: rect.x - grow, y: rect.y - grow, width: rect.width + 2.0 * grow, height: rect.height + 2.0 * grow }
}

// The player panel's queue list (`WallToolsQueueIsland`, `WallToolsQueueRow`): virtual rows, scrolling and drag reorder.

use std::collections::HashMap;
use std::time::Instant;

use folia_core::Track;
use folia_i18n::I18n;
use folia_visualizer::Ease;
use iced::widget::operation::{self, AbsoluteOffset};
use iced::widget::scrollable::Viewport;
use iced::widget::{Id, button, column, container, mouse_area, pin, row, scrollable, space, stack, text, tooltip};
use iced::{Alignment, Background, Border, Color, Element, Length, Point, Rectangle, Shadow, Task, Vector};
use lucide_static_svg::Icon;

use crate::drag_capture::Capture;
use crate::icons::icon;
use crate::motion::Tween;
use crate::settings_kit::{self, Tokens};

/// `QUEUE_ISLAND_ROW_HEIGHT`.
pub const ROW: f32 = 40.0;
/// `ISLAND_MAX_VISIBLE_ROWS` and what is left of `ISLAND_MIN_HEIGHT` under the header and borders.
const MAX_ROWS: usize = 8;
const MIN_LIST: f32 = 112.0 - 2.0 - 36.0;
/// react-window's `overscanCount`.
const OVERSCAN: usize = 4;
const HANDLE_W: f32 = 18.0;
/// `.lattice-tools-island-list { padding: 0 4px }`.
const SIDE: f32 = 4.0;
/// `QUEUE_DRAG_THRESHOLD_PX`, `QUEUE_AUTOSCROLL_EDGE_PX`, `QUEUE_AUTOSCROLL_MAX_SPEED`.
const DRAG_THRESHOLD: f32 = 4.0;
const EDGE: f32 = 32.0;
const MAX_SPEED: f32 = 14.0;
/// `.is-dragging { transition: transform 140ms ease }`.
const SHIFT_MS: f64 = 140.0;
const CSS_EASE: Ease = Ease::new(0.25, 0.1, 0.25, 1.0);
/// `behavior: 'smooth'`: the browser picks the length; this one is Chromium's usual 300ms ease-in-out.
const SMOOTH_MS: f64 = 300.0;
const SMOOTH_EASE: Ease = Ease::new(0.42, 0.0, 0.58, 1.0);

/// `queueIslandHeight` without the header and the borders: up to eight rows, at least 74.
pub fn list_height(rows: usize) -> f32 {
    (rows.min(MAX_ROWS) as f32 * ROW).max(MIN_LIST)
}

#[derive(Debug, Clone)]
pub enum Message {
    Scrolled(Viewport),
    Press(Point),
    Move(Point),
    Release,
    Cancel,
    Hover(Option<usize>),
    HandleHover(Option<usize>),
    /// The row action under the pointer: the row and which of its three.
    ActionHover(Option<(usize, u8)>),
    Jump(usize),
    MoveNext(usize),
    MoveEnd(usize),
    Remove(usize),
}

/// What a list message asks of the queue.
pub enum Change {
    Jump(usize),
    MoveNext(usize),
    MoveEnd(usize),
    Remove(usize),
    /// `moveQueueSong(from, to)`.
    Reorder(usize, usize),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Edge {
    Top,
    Bottom,
}

/// `DragSession`: the press on a handle, then the drag once it moved past the threshold.
struct Drag {
    from: usize,
    to: usize,
    track: i64,
    start: Point,
    pointer: f32,
    /// From the row's top to the press, so the ghost keeps the grab point.
    grab: f32,
    suppressed: Option<Edge>,
    active: bool,
}

pub struct QueueList {
    id: Id,
    offset: f32,
    /// Waiting for the list to be on screen to centre the playing row (`centeredRef`).
    center_pending: bool,
    following: Option<i64>,
    scroll: Option<Tween>,
    drag: Option<Drag>,
    shifts: HashMap<usize, Tween>,
    hover: Option<usize>,
    handle_hover: Option<usize>,
    action_hover: Option<(usize, u8)>,
}

impl Default for QueueList {
    fn default() -> Self {
        Self {
            id: Id::unique(),
            offset: 0.0,
            center_pending: false,
            following: None,
            scroll: None,
            drag: None,
            shifts: HashMap::new(),
            hover: None,
            handle_hover: None,
            action_hover: None,
        }
    }
}

impl QueueList {
    /// The list came on screen: centre the playing row once it is laid out (`scrollToRow align: 'center'`).
    pub fn opened(&mut self) {
        self.offset = 0.0;
        self.scroll = None;
        self.center_pending = true;
        self.finish(None);
    }

    pub fn animating(&self, now: Instant) -> bool {
        self.center_pending
            || self.scroll.is_some_and(|tween| tween.running(now))
            || self.shifts.values().any(|tween| tween.running(now))
            || self.drag.as_ref().is_some_and(|drag| drag.active)
    }

    /// One frame: the pending centring, the smooth scroll, the drag's edge scrolling.
    pub fn frame(&mut self, queue: &[Track], current: Option<usize>, list_h: f32, now: Instant) -> Task<Message> {
        let max = max_offset(queue.len(), list_h);
        if self.center_pending {
            self.center_pending = false;
            self.following = current.and_then(|index| queue.get(index)).map(|track| track.id);
            let Some(index) = current else { return Task::none() };
            return self.jump_to((index as f32 * ROW + ROW / 2.0 - list_h / 2.0).clamp(0.0, max));
        }
        if let Some(tween) = self.scroll {
            let value = tween.value(now).clamp(0.0, max);
            if !tween.running(now) {
                self.scroll = None;
            }
            return self.jump_to(value);
        }
        let Some(drag) = self.drag.as_mut().filter(|drag| drag.active) else { return Task::none() };
        let (velocity, suppressed) = auto_scroll(drag.pointer, list_h, drag.suppressed);
        drag.suppressed = suppressed;
        let next = (self.offset + velocity).clamp(0.0, max);
        if (next - self.offset).abs() < 0.01 {
            return Task::none();
        }
        let task = self.jump_to(next);
        self.retarget(queue.len(), now);
        task
    }

    /// The playing song changed: a smooth scroll to its row unless it is in view, never during a drag (`align: 'smart'`).
    pub fn follow(&mut self, queue: &[Track], current: Option<usize>, list_h: f32, now: Instant) {
        let id = current.and_then(|index| queue.get(index)).map(|track| track.id);
        if id == self.following {
            return;
        }
        self.following = id;
        let Some(index) = current.filter(|_| self.drag.is_none() && !self.center_pending) else { return };
        let (top, bottom) = (index as f32 * ROW, index as f32 * ROW + ROW);
        let (view_top, view_bottom) = (self.offset, self.offset + list_h);
        let goal = if top >= view_top && bottom <= view_bottom {
            return;
        } else if bottom < view_top - list_h || top > view_bottom + list_h {
            top + ROW / 2.0 - list_h / 2.0
        } else if top < view_top {
            top
        } else {
            bottom - list_h
        };
        let goal = goal.clamp(0.0, max_offset(queue.len(), list_h));
        let mut tween = Tween::new(self.offset, SMOOTH_MS, SMOOTH_EASE, now);
        tween.set(goal, now);
        self.scroll = Some(tween);
    }

    pub fn update(&mut self, message: Message, queue: &[Track], list_h: f32) -> (Task<Message>, Option<Change>) {
        let now = Instant::now();
        match message {
            Message::Scrolled(viewport) => {
                self.offset = viewport.absolute_offset().y;
                if self.drag.as_ref().is_some_and(|drag| drag.active) {
                    self.retarget(queue.len(), now);
                }
                (Task::none(), None)
            }
            Message::Press(at) => {
                let index = ((at.y + self.offset) / ROW).floor().max(0.0) as usize;
                if let Some(track) = queue.get(index) {
                    self.scroll = None;
                    self.drag = Some(Drag {
                        from: index,
                        to: index,
                        track: track.id,
                        start: at,
                        pointer: at.y,
                        grab: at.y + self.offset - index as f32 * ROW,
                        suppressed: edge_at(at.y, list_h),
                        active: false,
                    });
                }
                (Task::none(), None)
            }
            Message::Move(at) => {
                let Some(drag) = self.drag.as_mut() else { return (Task::none(), None) };
                drag.pointer = at.y;
                if !drag.active {
                    if (at.x - drag.start.x).hypot(at.y - drag.start.y) <= DRAG_THRESHOLD {
                        return (Task::none(), None);
                    }
                    drag.active = true;
                    self.hover = None;
                }
                self.retarget(queue.len(), now);
                (Task::none(), None)
            }
            Message::Release => (Task::none(), self.finish(Some(queue))),
            Message::Cancel => (Task::none(), self.finish(None)),
            Message::Hover(index) => {
                self.hover = index;
                (Task::none(), None)
            }
            Message::HandleHover(index) => {
                self.handle_hover = index;
                (Task::none(), None)
            }
            Message::ActionHover(action) => {
                self.action_hover = action;
                (Task::none(), None)
            }
            Message::Jump(index) => (Task::none(), Some(Change::Jump(index))),
            Message::MoveNext(index) => (Task::none(), Some(Change::MoveNext(index))),
            Message::MoveEnd(index) => (Task::none(), Some(Change::MoveEnd(index))),
            Message::Remove(index) => (Task::none(), Some(Change::Remove(index))),
        }
    }

    fn jump_to(&mut self, offset: f32) -> Task<Message> {
        self.offset = offset;
        operation::scroll_to(self.id.clone(), AbsoluteOffset { x: None, y: Some(offset) })
    }

    /// `resolveDropIndex` for the pointer, and the rows between the old and new place start moving aside.
    fn retarget(&mut self, count: usize, now: Instant) {
        let Some(drag) = self.drag.as_mut() else { return };
        let center = drag.pointer + self.offset - drag.grab + ROW / 2.0;
        let to = ((center / ROW).floor().max(0.0) as usize).min(count.saturating_sub(1));
        if to == drag.to {
            return;
        }
        let (low, high) = (drag.from.min(drag.to).min(to), drag.from.max(drag.to).max(to));
        drag.to = to;
        let from = drag.from;
        for index in low..=high {
            let goal = row_shift(index, from, to);
            let tween = self.shifts.entry(index).or_insert_with(|| Tween::new(0.0, SHIFT_MS, CSS_EASE, now));
            tween.set(goal, now);
        }
    }

    /// Ends the drag; a release on another place moves the song if it is still where the drag began.
    fn finish(&mut self, queue: Option<&[Track]>) -> Option<Change> {
        let drag = self.drag.take()?;
        self.shifts.clear();
        let queue = queue?;
        let same = queue.get(drag.from).is_some_and(|track| track.id == drag.track);
        (drag.active && drag.from != drag.to && drag.to < queue.len() && same)
            .then_some(Change::Reorder(drag.from, drag.to))
    }

    pub fn view<'a>(
        &'a self,
        look: Look<'a>,
        queue: &'a [Track],
        current: Option<usize>,
        list_h: f32,
        now: Instant,
    ) -> Element<'a, Message> {
        let count = queue.len();
        let total = count as f32 * ROW;
        let dragging = self.drag.as_ref().filter(|drag| drag.active);
        let first = ((self.offset / ROW).floor().max(0.0) as usize).saturating_sub(OVERSCAN);
        let last = (((self.offset + list_h) / ROW).ceil().max(0.0) as usize + OVERSCAN).min(count);
        let mut layers = vec![space().width(Length::Fill).height(Length::Fixed(total)).into()];
        for (index, track) in queue.iter().enumerate().take(last).skip(first) {
            if dragging.is_some_and(|drag| drag.from == index) {
                continue;
            }
            let shift = self.shifts.get(&index).map_or(0.0, |tween| tween.value(now));
            let face = queue_row(self, look, track, index, current == Some(index), dragging.is_some());
            layers.push(pin(face).y(index as f32 * ROW + shift).width(Length::Fill).into());
        }
        let rows = stack(layers).width(Length::Fill).height(Length::Fixed(total));
        let list = scrollable(container(rows).padding([0.0, SIDE]))
            .id(self.id.clone())
            .direction(settings_kit::scrollbar())
            .style(settings_kit::scroll_style(look.tokens))
            .on_scroll(Message::Scrolled)
            .width(Length::Fill)
            .height(Length::Fixed(list_h));
        let mut face = stack![list].width(Length::Fill).height(Length::Fixed(list_h));
        if let Some(drag) = dragging {
            let y = (drag.pointer - drag.grab).clamp(-ROW / 2.0, list_h - ROW / 2.0);
            let ghost = container(ghost(look, &queue[drag.from])).padding([0.0, SIDE]);
            face = face.push(pin(ghost).y(y).width(Length::Fill));
        }
        let handle = Rectangle { x: SIDE, y: 0.0, width: HANDLE_W, height: (total - self.offset).clamp(0.0, list_h) };
        let held = self.drag.is_some();
        Capture::new(face, handle, Message::Press, Message::Move, Message::Release, Message::Cancel)
            .held(held, dragging.is_some())
            .into()
    }
}

#[derive(Clone, Copy)]
pub struct Look<'a> {
    pub i18n: &'a I18n,
    pub tokens: Tokens,
    pub ink: Color,
    pub daylight: bool,
}

/// `WallToolsQueueRow`: handle, the song (marker, title over artist), and on hover the row's actions.
fn queue_row<'a>(
    list: &QueueList,
    look: Look<'a>,
    track: &'a Track,
    index: usize,
    active: bool,
    dragging: bool,
) -> Element<'a, Message> {
    let ink = look.ink;
    let hot = !dragging && list.hover == Some(index);
    let grip = if dragging {
        0.0
    } else if list.handle_hover == Some(index) {
        0.9
    } else if hot {
        0.5
    } else {
        0.0
    };
    let handle = mouse_area(
        container(icon(Icon::GripVertical, 13.0, Color { a: ink.a * grip, ..ink }))
            .center_x(Length::Fixed(HANDLE_W))
            .center_y(Length::Fixed(32.0)),
    )
    .on_enter(Message::HandleHover(Some(index)))
    .on_exit(Message::HandleHover(None));
    let marker =
        container(space()).width(Length::Fixed(3.0)).height(Length::Fixed(18.0)).style(move |_| container::Style {
            background: active.then_some(Background::Color(look.tokens.accent)),
            border: Border { radius: 2.0.into(), ..Border::default() },
            ..container::Style::default()
        });
    let weight = if active { iced::font::Weight::Bold } else { iced::font::Weight::Semibold };
    let title = text(track.title.as_str())
        .size(12.5)
        .line_height(iced::widget::text::LineHeight::Relative(1.25))
        .font(crate::typeface::font(weight))
        .color(ink)
        .wrapping(iced::widget::text::Wrapping::None);
    let artist = text(track.artist.as_deref().unwrap_or(""))
        .size(10.5)
        .line_height(iced::widget::text::LineHeight::Relative(1.25))
        .color(Color { a: ink.a * 0.52, ..ink })
        .wrapping(iced::widget::text::Wrapping::None);
    let words = container(column![title, artist]).width(Length::Fill).clip(true);
    let song = button(row![marker, words].spacing(8).align_y(Alignment::Center))
        .width(Length::Fill)
        .height(Length::Fixed(36.0))
        .padding(iced::Padding { top: 0.0, right: 6.0, bottom: 0.0, left: 2.0 })
        .on_press(Message::Jump(index))
        .style(|_, _| button::Style {
            border: Border { radius: 8.0.into(), ..Border::default() },
            ..button::Style::default()
        });
    let mut face = row![handle, song].align_y(Alignment::Center);
    if hot {
        // The playing row has no 「下一首播放」 and no 「移除」.
        let mut actions = row![].align_y(Alignment::Center);
        let lit = |which: u8| list.action_hover == Some((index, which));
        if !active {
            actions = actions.push(action(
                look,
                Icon::ListPlus,
                "queue.playNext",
                Message::MoveNext(index),
                (index, 0),
                lit(0),
            ));
        }
        actions =
            actions.push(action(look, Icon::ListEnd, "queue.moveToEnd", Message::MoveEnd(index), (index, 1), lit(1)));
        if !active {
            actions =
                actions.push(action(look, Icon::Trash, "queue.remove", Message::Remove(index), (index, 2), lit(2)));
        }
        face = face.push(actions);
    }
    let fill = if active {
        Some(if look.daylight { Color::from_rgba(0.0, 0.0, 0.0, 0.07) } else { Color::from_rgba(1.0, 1.0, 1.0, 0.10) })
    } else if hot {
        Some(if look.daylight { Color::from_rgba(0.0, 0.0, 0.0, 0.05) } else { Color::from_rgba(1.0, 1.0, 1.0, 0.07) })
    } else {
        None
    };
    let alpha = ink.a;
    let body = container(face)
        .width(Length::Fill)
        .height(Length::Fixed(ROW))
        .padding(iced::Padding { top: 2.0, right: 4.0, bottom: 2.0, left: 0.0 })
        .align_y(Alignment::Center)
        .style(move |_| container::Style {
            background: fill.map(|fill| Background::Color(Color { a: fill.a * alpha, ..fill })),
            border: Border { radius: 10.0.into(), ..Border::default() },
            ..container::Style::default()
        });
    mouse_area(body).on_enter(Message::Hover(Some(index))).on_exit(Message::Hover(None)).into()
}

/// `.lattice-tools-island-row-action`: 26px, radius 7, the glyph at 70% that comes up to 100% on hover.
fn action<'a>(
    look: Look<'a>,
    glyph: Icon,
    label: &'static str,
    message: Message,
    key: (usize, u8),
    hot: bool,
) -> Element<'a, Message> {
    let ink = look.ink;
    let daylight = look.daylight;
    let glyph = icon(glyph, 14.0, Color { a: ink.a * if hot { 1.0 } else { 0.7 }, ..ink });
    let press = button(container(glyph).center(Length::Fixed(26.0))).padding(0).on_press(message).style(move |_, _| {
        let fill = if daylight { Color::from_rgba(0.0, 0.0, 0.0, 0.08) } else { Color::from_rgba(1.0, 1.0, 1.0, 0.12) };
        button::Style {
            background: hot.then_some(Background::Color(Color { a: fill.a * ink.a, ..fill })),
            border: Border { radius: 7.0.into(), ..Border::default() },
            ..button::Style::default()
        }
    });
    let press = mouse_area(press).on_enter(Message::ActionHover(Some(key))).on_exit(Message::ActionHover(None));
    tooltip(press, text(look.i18n.t(label).to_owned()).size(12), tooltip::Position::Top)
        .style(container::rounded_box)
        .into()
}

/// `.lattice-tools-island-ghost`: the dragged song under the pointer, raised with a shadow and a ring.
fn ghost<'a>(look: Look<'a>, track: &'a Track) -> Element<'a, Message> {
    let ink = look.ink;
    let words = column![
        text(track.title.as_str())
            .size(12.5)
            .line_height(iced::widget::text::LineHeight::Relative(1.25))
            .font(crate::typeface::font(iced::font::Weight::Semibold))
            .color(ink)
            .wrapping(iced::widget::text::Wrapping::None),
        text(track.artist.as_deref().unwrap_or(""))
            .size(10.5)
            .line_height(iced::widget::text::LineHeight::Relative(1.25))
            .color(Color { a: ink.a * 0.52, ..ink })
            .wrapping(iced::widget::text::Wrapping::None),
    ];
    let face = row![
        icon(Icon::GripVertical, 13.0, Color { a: ink.a * 0.6, ..ink }),
        container(words).width(Length::Fill).clip(true)
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    let (fill, shadow, ring) = if look.daylight {
        (
            Color::from_rgba(1.0, 1.0, 1.0, 0.92),
            Color::from_rgba8(42, 35, 27, 0.20),
            Color::from_rgba(0.0, 0.0, 0.0, 0.08),
        )
    } else {
        (
            Color::from_rgba(1.0, 1.0, 1.0, 0.18),
            Color::from_rgba(0.0, 0.0, 0.0, 0.38),
            Color::from_rgba(1.0, 1.0, 1.0, 0.12),
        )
    };
    container(face)
        .width(Length::Fill)
        .height(Length::Fixed(ROW))
        .padding(iced::Padding { top: 0.0, right: 12.0, bottom: 0.0, left: 3.0 })
        .align_y(Alignment::Center)
        .style(move |_| container::Style {
            background: Some(Background::Color(fill)),
            border: Border { radius: 10.0.into(), width: 1.0, color: ring },
            shadow: Shadow { color: shadow, offset: Vector::new(0.0, 10.0), blur_radius: 28.0 },
            ..container::Style::default()
        })
        .into()
}

fn max_offset(count: usize, list_h: f32) -> f32 {
    (count as f32 * ROW - list_h).max(0.0)
}

/// `queueRowShift`: rows between the two places move one row towards where the dragged song left.
fn row_shift(index: usize, from: usize, to: usize) -> f32 {
    if index == from || from == to {
        0.0
    } else if from < to && index > from && index <= to {
        -ROW
    } else if to < from && index >= to && index < from {
        ROW
    } else {
        0.0
    }
}

/// `autoScrollZone`: 32px, narrowed to half the list when it is shorter than twice that.
fn zone(list_h: f32) -> f32 {
    EDGE.min((list_h / 2.0).max(0.0))
}

/// `autoScrollEdgeAt`, in the list's own coordinates.
fn edge_at(pointer: f32, list_h: f32) -> Option<Edge> {
    let zone = zone(list_h);
    if zone <= 0.0 {
        None
    } else if pointer < zone {
        Some(Edge::Top)
    } else if pointer > list_h - zone {
        Some(Edge::Bottom)
    } else {
        None
    }
}

/// `resolveAutoScroll`: faster the deeper into an edge, except the edge the drag began in until it is left.
fn auto_scroll(pointer: f32, list_h: f32, suppressed: Option<Edge>) -> (f32, Option<Edge>) {
    let at = edge_at(pointer, list_h);
    if suppressed.is_some() && at == suppressed {
        return (0.0, suppressed);
    }
    let zone = zone(list_h);
    let velocity = match at {
        Some(Edge::Top) => -MAX_SPEED * ((zone - pointer) / zone).clamp(0.0, 1.0),
        Some(Edge::Bottom) => MAX_SPEED * ((pointer - (list_h - zone)) / zone).clamp(0.0, 1.0),
        None => 0.0,
    };
    (velocity, None)
}

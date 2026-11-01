// The now-playing player panel: frame from `PanelShell`, handle from `PanelHandle`, tabs from `UnifiedPanel`.

use std::time::Instant;

use folia_core::{Repeat, Theme, Track};
use folia_i18n::I18n;
use folia_render::Plate;
use folia_store::ThemeMode;
use folia_visualizer::{Ease, Style};
use iced::widget::text::LineHeight;
use iced::widget::{button, column, container, image, mouse_area, row, shader, slider, space, stack, text, tooltip};
use iced::{Alignment, Background, Border, Color, Element, Length, Size, Task, Theme as IcedTheme};
use lucide_static_svg::Icon;

use crate::icons::icon;
use crate::mode_picker;
use crate::motion::{CSS_TRANSITION, Tween};
use crate::playback;
use crate::queue_island::{self, QueueList};
use crate::settings_kit::{self, Tokens};

const OPEN_MS: f64 = 300.0;
const CLOSE_MS: f64 = 220.0;
/// The kit's `fast`.
const FAST_MS: f64 = 120.0;
/// `FOLDED_PX`: the folded header and its cover.
const FOLDED: f32 = 48.0;
const HANDLE_MS: f64 = 240.0;
const OPEN_EASE: Ease = Ease::new(0.22, 1.0, 0.36, 1.0);
/// Handle colours and hover, `transition-all duration-300`.
const HANDLE_TONE_MS: f64 = 300.0;
/// `HANDLE_ROOM_PX`: the 48px handle and the 12px gap above it.
const HANDLE_ROOM: f32 = 60.0;
/// `BARE_GAP_PX`: under the surface when no handle is drawn.
const BARE_GAP: f32 = 8.0;
const TOP_MARGIN: f32 = 56.0;
const SHADOW_BLUR: f32 = 80.0;
const SHADOW_Y: f32 = 30.0;
/// Three sigmas of the `80px` blur.
const SHADOW_REACH: f32 = 120.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Cover,
    Source,
    Controls,
    Queue,
    Quality,
}

#[derive(Debug, Clone)]
pub enum Message {
    Toggle,
    Choose(Tab),
    Frame,
    HandleHover(bool),
    PillHover(bool),
    /// Sets the lyric timeline offset, in milliseconds.
    Offset(i64),
    Queue(queue_island::Message),
    Volume(f32),
    VolumeDone,
    Mute,
    Repeat,
    Home,
    Locate,
    Settings,
    Theme(ThemeMode),
    Picker(mode_picker::Message),
    Intensity,
}

pub enum Effect {
    None,
    Home,
    Locate,
    Settings,
    Playback(playback::Message),
    Intensity,
    Theme(ThemeMode),
    Visualizer(Option<Style>),
}

pub struct PlayerPanel {
    open: bool,
    tab: Tab,
    /// Playing-page chrome is up. The handle stays with it (`hideToggleButton`).
    chrome: bool,
    pill_hover: bool,
    queue: QueueList,
    /// Surface opacity and scale: 0 is `{ opacity: 0, scale: 0.92 }`, 1 is open.
    surface: Tween,
    /// Handle presence: `{ opacity: 0, x: 20, y: 12, scale: 0.92 }` at 0.
    handle: Tween,
    /// Handle colours, closed 0 and open 1 (`transition-all duration-300`).
    handle_tone: Tween,
    handle_hover: Tween,
    /// The song header folding from the cover tab (0) to the others (1): shape, folded row, big block.
    fold: [Tween; 3],
    /// The last tab other than the cover, whose body the panel grows to.
    body_tab: Tab,
    pickers: mode_picker::Pickers,
}

impl PlayerPanel {
    pub fn new(now: Instant) -> Self {
        Self {
            open: false,
            tab: Tab::Cover,
            chrome: true,
            pill_hover: false,
            queue: QueueList::default(),
            surface: Tween::new(0.0, OPEN_MS, OPEN_EASE, now),
            handle: Tween::new(1.0, HANDLE_MS, Ease::OUT, now),
            handle_tone: Tween::new(0.0, HANDLE_TONE_MS, CSS_TRANSITION, now),
            handle_hover: Tween::new(0.0, HANDLE_TONE_MS, CSS_TRANSITION, now),
            fold: [
                Tween::new(0.0, OPEN_MS, OPEN_EASE, now),
                Tween::new(0.0, CLOSE_MS, OPEN_EASE, now),
                Tween::new(1.0, CLOSE_MS, OPEN_EASE, now),
            ],
            body_tab: Tab::Controls,
            pickers: mode_picker::Pickers::new(now),
        }
    }

    pub fn pill_hover(&self) -> bool {
        self.pill_hover
    }

    /// Shows or hides the handle with the playing-page chrome.
    pub fn set_chrome(&mut self, visible: bool, now: Instant) {
        self.chrome = visible;
        self.handle.set(if visible { 1.0 } else { 0.0 }, now);
    }

    pub fn animating(&self, now: Instant) -> bool {
        [self.surface, self.handle, self.handle_tone, self.handle_hover]
            .iter()
            .chain(&self.fold)
            .any(|tween| tween.running(now))
            || (self.open && self.pickers.animating(now))
            || (self.open && self.tab == Tab::Queue && self.queue.animating(now))
    }

    /// The playing song changed: the open queue list scrolls to it (`align: 'smart'`).
    pub fn follow(&mut self, room: Room<'_>) {
        let list_h = queue_list_h(room.width, room.height, room.bottom, self.chrome, room.queue.len());
        self.queue.follow(room.queue, room.index, list_h, Instant::now());
    }

    pub fn update(&mut self, message: &Message, style_id: &str, room: Room<'_>) -> (Effect, Task<Message>) {
        let now = Instant::now();
        let list_h = queue_list_h(room.width, room.height, room.bottom, self.chrome, room.queue.len());
        let effect = match message {
            Message::Toggle => {
                self.set_open(!self.open, now);
                if self.open && self.tab == Tab::Queue {
                    self.queue.opened();
                }
                Effect::None
            }
            Message::Choose(tab) => {
                if *tab == Tab::Queue && self.tab != Tab::Queue {
                    self.queue.opened();
                }
                self.tab = *tab;
                self.fold_to(*tab, now);
                self.pickers.set_open(None, now);
                Effect::None
            }
            Message::Queue(message) => {
                let (task, change) = self.queue.update(message.clone(), room.queue, list_h);
                let effect = match change {
                    Some(queue_island::Change::Jump(index)) => Effect::Playback(playback::Message::Jump(index)),
                    Some(queue_island::Change::MoveNext(index)) => Effect::Playback(playback::Message::MoveNext(index)),
                    Some(queue_island::Change::MoveEnd(index)) => Effect::Playback(playback::Message::MoveEnd(index)),
                    Some(queue_island::Change::Remove(index)) => Effect::Playback(playback::Message::Remove(index)),
                    Some(queue_island::Change::Reorder(from, to)) => {
                        Effect::Playback(playback::Message::Reorder(from, to))
                    }
                    None => Effect::None,
                };
                return (effect, task.map(Message::Queue));
            }
            Message::PillHover(hovered) => {
                self.pill_hover = *hovered;
                Effect::None
            }
            Message::HandleHover(hovered) => {
                self.handle_hover.set(if *hovered { 1.0 } else { 0.0 }, now);
                Effect::None
            }
            Message::Volume(value) => Effect::Playback(playback::Message::Volume(*value)),
            Message::VolumeDone => Effect::Playback(playback::Message::VolumeDone),
            Message::Mute => Effect::Playback(playback::Message::ToggleMute),
            Message::Offset(ms) => Effect::Playback(playback::Message::LyricOffset(*ms as f64 / 1000.0)),
            Message::Repeat => Effect::Playback(playback::Message::CycleRepeat),
            Message::Home => {
                self.set_open(false, now);
                Effect::Home
            }
            Message::Locate => {
                self.set_open(false, now);
                Effect::Locate
            }
            Message::Settings => {
                self.set_open(false, now);
                Effect::Settings
            }
            Message::Theme(mode) => Effect::Theme(*mode),
            Message::Picker(message) => match self.pickers.update(message.clone(), style_id, now) {
                mode_picker::Outcome::None => Effect::None,
                mode_picker::Outcome::Style(style) => Effect::Visualizer(style),
                mode_picker::Outcome::Settings => {
                    self.set_open(false, now);
                    Effect::Settings
                }
            },
            Message::Intensity => Effect::Intensity,
            Message::Frame => {
                if self.open && self.tab == Tab::Queue {
                    let task = self.queue.frame(room.queue, room.index, list_h, now);
                    return (Effect::None, task.map(Message::Queue));
                }
                Effect::None
            }
        };
        (effect, Task::none())
    }

    pub fn view<'a>(&'a self, view: View<'a>, now: Instant) -> Element<'a, Message> {
        let open = self.surface.value(now);
        let shown = self.handle.value(now);
        let surface = (open > 0.0).then(|| surface(self, view, open, now));
        let look = HandleLook {
            open: self.open,
            shown,
            tone: self.handle_tone.value(now),
            hover: self.handle_hover.value(now),
        };
        let handle = (shown > 0.0).then(|| handle_button(view, look));
        // Always the same two layers: the handle coming and going must not rebuild the surface's widget state
        // (the queue list's scroll position).
        match surface {
            Some(surface) => stack![surface, handle.unwrap_or_else(|| space().into())].into(),
            None => handle.unwrap_or_else(|| space().into()),
        }
    }

    /// `st-t-unfold` for the shape, `st-t-move` for the folded row; the big block leaves with `st-t-fast`.
    fn fold_to(&mut self, tab: Tab, now: Instant) {
        let folded = if tab == Tab::Cover { 0.0 } else { 1.0 };
        if tab != Tab::Cover {
            self.body_tab = tab;
        }
        self.fold[0].set(folded, now);
        self.fold[1].set(folded, now);
        if folded > 0.5 {
            self.fold[2].set_with(0.0, FAST_MS, CSS_TRANSITION, now);
        } else {
            self.fold[2].set_with(1.0, CLOSE_MS, OPEN_EASE, now);
        }
    }

    /// Opens with the kit's `unfold` (300ms) and closes with `move` (220ms), both `KIT_EASE.out`.
    fn set_open(&mut self, open: bool, now: Instant) {
        self.open = open;
        let (goal, ms) = if open { (1.0, OPEN_MS) } else { (0.0, CLOSE_MS) };
        self.surface.set_with(goal, ms, OPEN_EASE, now);
        self.handle_tone.set(goal, now);
        if !open {
            self.pickers.dismiss(now);
        }
    }
}

/// What the panel's update needs of the window and the queue.
#[derive(Clone, Copy)]
pub struct Room<'a> {
    pub width: f32,
    pub height: f32,
    pub bottom: f32,
    pub queue: &'a [Track],
    pub index: Option<usize>,
}

#[derive(Clone, Copy)]
pub struct View<'a> {
    pub i18n: &'a I18n,
    pub theme: &'a Theme,
    pub daylight: bool,
    pub width: f32,
    pub height: f32,
    pub track: Option<&'a Track>,
    pub queue: &'a [Track],
    pub index: Option<usize>,
    pub volume: f32,
    pub muted: bool,
    /// Lyric timeline offset of the playing song, seconds.
    pub lyric_offset: f64,
    /// The bottom baseline the panel and handle stand on, with the user's offset.
    pub bottom: f32,
    pub repeat: Repeat,
    pub style_id: &'a str,
    pub has_lyrics: bool,
    /// `lyricsFontScale`, for the style pictures.
    pub font_scale: f32,
    /// The style on the page; with random chosen, the one this song drew.
    pub drawn_style: Style,
    pub origin: Option<&'a str>,
    pub art: Option<&'a iced::widget::image::Handle>,
    pub pill_hover: bool,
}

fn surface<'a>(panel: &'a PlayerPanel, view: View<'a>, open: f32, now: Instant) -> Element<'a, Message> {
    // iced cannot fade a subtree, so every paint of the surface takes the open state's opacity.
    let tokens = tokens_of(&view).faded(open);
    let online = view.track.is_some_and(|track| track.online.is_some());
    let local = view.track.is_some_and(|track| track.online.is_none());
    let tab = shown_tab(panel.tab, local, online);
    let edge = if view.width >= 768.0 { 32.0 } else { 16.0 };
    // `HANDLE_ROOM_PX` while the handle is drawn under the surface, `BARE_GAP_PX` once it hides.
    let under = if panel.chrome { HANDLE_ROOM } else { BARE_GAP };
    let panel_w = (view.width - 32.0).clamp(200.0, 360.0);
    let inner = (panel_w - 24.0).max(96.0);
    let max_h = (view.height - under - view.bottom - TOP_MARGIN).max(220.0);
    // The header and body heights move with the fold, between the cover tab and the last other tab.
    let (big_h, _, cover_px) = measure(Tab::Cover, view.queue.len(), inner, max_h);
    let (_, full_body, _) = measure(panel.body_tab, view.queue.len(), inner, max_h);
    let fold =
        Fold { shape: panel.fold[0].value(now), folded: panel.fold[1].value(now), under: panel.fold[2].value(now) };
    let header_h = big_h + (FOLDED - big_h) * fold.shape;
    let body_h = full_body * fold.shape;
    let visual_h = (24.0 + header_h + body_h + 12.0 + 36.0).min(max_h);
    let visual_right = view.width - edge;
    let visual_bottom = view.height - view.bottom - under;
    let visual_left = visual_right - panel_w;
    let visual_top = visual_bottom - visual_h;
    let glass = if view.daylight {
        [244.0 / 255.0, 244.0 / 255.0, 245.0 / 255.0, 0.8 * open]
    } else {
        [28.0 / 255.0, 28.0 / 255.0, 31.0 / 255.0, 0.8 * open]
    };
    // `--pn-shadow`: `0 30px 80px` and a 1px ring.
    let (shadow, ring) = if view.daylight {
        ([0.0, 0.0, 0.0, 0.24 * open], [0.0, 0.0, 0.0, 0.06 * open])
    } else {
        ([0.0, 0.0, 0.0, 0.55 * open], [1.0, 1.0, 1.0, 0.09 * open])
    };
    let x0 = (visual_left - SHADOW_REACH).max(0.0);
    let y0 = (visual_top - (SHADOW_REACH - SHADOW_Y)).max(0.0);
    let x1 = (visual_right + SHADOW_REACH).min(view.width).max(x0 + 1.0);
    let y1 = (visual_bottom + SHADOW_REACH + SHADOW_Y).min(view.height).max(y0 + 1.0);
    let plate = shader(Plate {
        radius: 20.0,
        fill_origin: [visual_left - x0, visual_top - y0],
        fill_size: [panel_w, visual_h],
        fill: glass,
        shadow,
        shadow_blur: SHADOW_BLUR,
        shadow_y: SHADOW_Y,
        shadow_spread: 0.0,
        ring,
    })
    .width(Length::Fixed(x1 - x0))
    .height(Length::Fixed(y1 - y0));
    let column = Column { tab, width: panel_w, header_h, body_h, cover_px, open, fold };
    let face = mouse_area(face_column(view, tokens, column, (panel, now)))
        .on_press(Message::Frame)
        .interaction(iced::mouse::Interaction::Idle);
    let face = container(face).width(Length::Fixed(panel_w)).height(Length::Fixed(visual_h));
    let mut picker = None;
    if tab == Tab::Controls {
        let look = mode_picker::Look {
            i18n: view.i18n,
            tokens,
            theme: view.theme,
            font_scale: view.font_scale,
            style_id: view.style_id,
            open,
        };
        let mut place = picker_place(&view, panel.pickers.drawn(), Size::new(panel_w, visual_h), header_h);
        place.origin = (visual_left - x0, visual_top - y0);
        picker = panel.pickers.picker(look, place, now);
    }
    let face = container(face).padding(pad(visual_top - y0, 0.0, 0.0, visual_left - x0));
    let patch: Element<'a, Message> = stack![plate, face]
        .push(picker.unwrap_or_else(|| space().into()))
        .width(Length::Fixed(x1 - x0))
        .height(Length::Fixed(y1 - y0))
        .into();
    // `transformOrigin`: the handle's centre, 24px in from the surface's right and `under - 24` below it.
    let scale = 0.92 + 0.08 * open;
    let origin = iced::Vector::new(visual_right - 24.0, visual_bottom + under - 24.0);
    let centre = iced::Vector::new((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let shift = (origin - centre) * (1.0 - scale);
    let patch: Element<'a, Message> = iced::widget::float(patch).translate(move |_, _| shift).scale(scale).into();
    // The patch stops at the window edge, which clips the shadow there as the window would.
    container(patch).width(Length::Fill).height(Length::Fill).padding(pad(y0, 0.0, 0.0, x0)).into()
}

/// The mode rows' pickers and the frame time, for the controls tab.
type Rows<'a> = (&'a PlayerPanel, Instant);

struct Column {
    tab: Tab,
    width: f32,
    header_h: f32,
    body_h: f32,
    cover_px: f32,
    open: f32,
    fold: Fold,
}

fn face_column<'a>(view: View<'a>, tokens: Tokens, column: Column, rows: Rows<'a>) -> Element<'a, Message> {
    let Column { tab, width, header_h, body_h, cover_px, open, fold } = column;
    let ink = Color { a: open, ..tokens.t1 };
    let header = header(view, tokens, cover_px, header_h, fold);
    // The body keeps its hairline while it closes back into the cover tab; it is cut to its moving height.
    let content: Element<'a, Message> =
        if tab == Tab::Cover { space().into() } else { tab_body(view, tokens, tab, ink, rows) };
    let body: Element<'a, Message> = if body_h < 0.5 {
        space().into()
    } else {
        column![
            container(space()).height(1.0).width(Length::Fill).style(move |_theme: &IcedTheme| {
                container::Style { background: Some(tokens.line.into()), ..container::Style::default() }
            }),
            space().height(8.0),
            content,
        ]
        .padding(pad(12.0, 0.0, 0.0, 0.0))
        .height(Length::Fixed(body_h))
        .clip(true)
        .into()
    };
    column![header, body, space().height(12.0), dock(view, tokens, tab, ink)]
        .spacing(0)
        .padding(12)
        .width(Length::Fixed(width))
        .into()
}

/// Where the song header is between the cover tab's big cover (0) and the other tabs' folded row (1).
#[derive(Clone, Copy)]
struct Fold {
    /// Cover size and header height, `st-t-unfold`.
    shape: f32,
    /// The folded title and heart, `st-t-move`.
    folded: f32,
    /// The big title and the actions under the cover.
    under: f32,
}

/// `SongHeader`: one cover that shrinks from the panel's width to 48px (radius 10 to 8) while the header folds
/// to 48px; the folded title and heart come in from 4px above, the big title and actions leave 8px down.
fn header<'a>(view: View<'a>, tokens: Tokens, cover_px: f32, header_h: f32, fold: Fold) -> Element<'a, Message> {
    let (title, album) = title_album(view);
    let open = tokens.t1.a;
    let size = cover_px + (FOLDED - cover_px) * fold.shape;
    let cover = cover_box(tokens, view.art, size, 10.0 - 2.0 * fold.shape, 40.0 - 20.0 * fold.shape);
    let cover = button(cover)
        .padding(0)
        .on_press_maybe((fold.shape > 0.5).then_some(Message::Choose(Tab::Cover)))
        .style(|_theme: &IcedTheme, _status| button::Style::default());

    let small = tokens.faded(fold.folded);
    let small_ink = Color { a: open * fold.folded, ..tokens.t1 };
    let lift = -4.0 * (1.0 - fold.folded);
    let lines = column![
        text(title)
            .size(14)
            .line_height(LineHeight::Absolute(iced::Pixels(20.0)))
            .font(settings_kit::SEMIBOLD)
            .color(small_ink)
            .wrapping(iced::widget::text::Wrapping::None),
        artists(view, small, small_ink.a, 12.0, 18.0),
    ];
    let lines = container(lines).width(Length::Fill).clip(true).padding(pad(5.0 + lift, 44.0, 0.0, 60.0));
    let heart = container(heart(view, small, small_ink, false))
        .width(Length::Fill)
        .align_x(Alignment::End)
        .padding(pad(8.0 + lift, 0.0, 0.0, 0.0));
    // Hidden parts are left out, so their buttons take no clicks.
    let folded_row: Element<'a, Message> = if fold.folded > 0.0 { stack![lines, heart].into() } else { space().into() };

    let big = tokens.faded(fold.under);
    let big_ink = Color { a: open * fold.under, ..tokens.t1 };
    let drop = 8.0 * (1.0 - fold.under);
    let credits = column![
        text(title)
            .size(18)
            .line_height(LineHeight::Absolute(iced::Pixels(26.0)))
            .font(settings_kit::SEMIBOLD)
            .color(big_ink),
        artists(view, big, big_ink.a, 13.0, 20.0),
        text(album).size(12).line_height(LineHeight::Absolute(iced::Pixels(18.0))).color(big.t3),
    ];
    let under = column![credits, actions(view, big, big_ink)].padding(pad(14.0, 4.0, 0.0, 4.0));

    // A box `drop` taller than the clipped header, bottom-aligned, moves the block down by `drop`.
    let under: Element<'a, Message> = if fold.under > 0.0 {
        container(under).width(Length::Fill).height(Length::Fixed(header_h + drop)).align_y(Alignment::End).into()
    } else {
        space().into()
    };

    stack![under, folded_row, cover].width(Length::Fill).height(Length::Fixed(header_h)).clip(true).into()
}

fn artists<'a>(view: View<'a>, tokens: Tokens, open: f32, size: f32, leading: f32) -> Element<'a, Message> {
    let raw = view.track.and_then(|track| track.artist.as_deref()).unwrap_or("");
    let mut row = iced::widget::Row::new().spacing(0);
    for (index, name) in raw.split(" / ").filter(|name| !name.is_empty()).enumerate() {
        if index > 0 {
            row = row.push(
                text(" / ")
                    .size(size)
                    .line_height(LineHeight::Absolute(iced::Pixels(leading)))
                    .color(fade(tokens.t3, open)),
            );
        }
        row = row.push(
            text(name).size(size).line_height(LineHeight::Absolute(iced::Pixels(leading))).color(fade(tokens.t2, open)),
        );
    }
    row.into()
}

fn actions<'a>(view: View<'a>, tokens: Tokens, ink: Color) -> Element<'a, Message> {
    let dim = Color { a: 0.4 * ink.a, ..tokens.t2 };
    let mut items = vec![heart(view, tokens, ink, true)];
    if view.track.is_some() {
        items.push(icon_btn(
            Icon::Star,
            17.0,
            32.0,
            dim,
            Some(tokens.fill),
            view.i18n.t("localMusic.addToPlaylist").to_owned(),
            None,
        ));
    }
    if let Some(name) = view.origin {
        items.push(source_pill(view, tokens, ink, name, view.pill_hover));
    }
    items.push(space().width(Length::Fill).into());
    items.push(icon_btn(
        Icon::House,
        17.0,
        32.0,
        ink,
        None,
        view.i18n.t("ui.backToHome").to_owned(),
        Some(Message::Home),
    ));
    items.push(icon_btn(
        Icon::Settings,
        17.0,
        32.0,
        ink,
        None,
        view.i18n.t("ui.options").to_owned(),
        Some(Message::Settings),
    ));
    row(items).spacing(6).padding(pad(14.0, 0.0, 0.0, 0.0)).into()
}

/// `SongActions` playing-from pill: 32px high, fully round, `pl-2.5 pr-3 gap-1.5`, Crosshair 14 and 12px text,
/// both centred on the row; hover lifts the fill to `--st-fill2` and the text to `--st-t1`.
fn source_pill<'a>(view: View<'a>, tokens: Tokens, ink: Color, name: &'a str, hovered: bool) -> Element<'a, Message> {
    let open = ink.a;
    let tip = view.i18n.format("ui.playingFrom", &[("name", name)]);
    let tint = fade(if hovered { tokens.t1 } else { tokens.t2 }, open);
    let fill = fade(if hovered { tokens.fill2 } else { tokens.fill }, open);
    let mark = container(icon(Icon::Crosshair, 14.0, tint)).center(Length::Fixed(14.0));
    let label = text(name).size(12).color(tint).wrapping(iced::widget::text::Wrapping::None);
    let body = row![mark, label].spacing(6).align_y(Alignment::Center);
    let body = container(body).center_y(Length::Fixed(32.0)).padding(pad(0.0, 12.0, 0.0, 10.0)).clip(true);
    let button =
        button(body).padding(0).on_press(Message::Locate).style(move |_theme: &IcedTheme, _status| button::Style {
            background: Some(Background::Color(fill)),
            border: Border { radius: 16.0.into(), ..Border::default() },
            ..button::Style::default()
        });
    let button = mouse_area(button).on_enter(Message::PillHover(true)).on_exit(Message::PillHover(false));
    tooltip(button, text(tip).size(12), tooltip::Position::Top).style(container::rounded_box).into()
}

fn heart<'a>(view: View<'a>, tokens: Tokens, ink: Color, filled: bool) -> Element<'a, Message> {
    icon_btn(
        Icon::Heart,
        17.0,
        32.0,
        Color { a: 0.4 * ink.a, ..tokens.t2 },
        filled.then_some(tokens.fill),
        view.i18n.t("player.like").to_owned(),
        None,
    )
}

fn dock<'a>(view: View<'a>, tokens: Tokens, tab: Tab, ink: Color) -> Element<'a, Message> {
    let local = view.track.is_some_and(|track| track.online.is_none());
    let online = view.track.is_some_and(|track| track.online.is_some());
    let mut tabs = vec![
        (Tab::Cover, view.i18n.t("panel.cover")),
        (Tab::Controls, view.i18n.t("panel.controls")),
        (Tab::Queue, view.i18n.t("panel.playlist")),
        (Tab::Quality, view.i18n.t("panel.account")),
    ];
    if local {
        tabs.insert(1, (Tab::Source, view.i18n.t("localMusic.folder")));
    } else if online {
        tabs.insert(1, (Tab::Source, view.i18n.t("localMusic.lyrics")));
    }
    let buttons = tabs.into_iter().map(|(id, label)| {
        let on = id == tab;
        let bg = if on { tokens.raise } else { Color::TRANSPARENT };
        button(
            container(text(label).size(13).color(if on { ink } else { Color { a: ink.a, ..tokens.t2 } }))
                .center_x(Length::Fill)
                .center_y(Length::Fill),
        )
        .padding(0)
        .width(Length::Fill)
        .height(Length::Fill)
        .on_press_maybe((!on).then_some(Message::Choose(id)))
        .style(move |_theme: &IcedTheme, _status| button::Style {
            background: Some(Background::Color(bg)),
            text_color: ink,
            border: Border { radius: 8.0.into(), ..Border::default() },
            ..button::Style::default()
        })
        .into()
    });
    container(row(buttons).spacing(0))
        .padding(2)
        .width(Length::Fill)
        .height(Length::Fixed(36.0))
        .style(move |_theme: &IcedTheme| container::Style {
            background: Some(tokens.fill.into()),
            border: Border { radius: 10.0.into(), ..Border::default() },
            ..container::Style::default()
        })
        .into()
}

fn tab_body<'a>(view: View<'a>, tokens: Tokens, tab: Tab, ink: Color, rows: Rows<'a>) -> Element<'a, Message> {
    match tab {
        Tab::Cover => space().into(),
        Tab::Controls => controls(view, tokens, ink, rows),
        Tab::Queue => queue_tab(rows.0, view, tokens, ink, rows.1),
        Tab::Source => source_tab(view, tokens),
        Tab::Quality => quality_tab(view, tokens, ink),
    }
}

fn controls<'a>(view: View<'a>, tokens: Tokens, ink: Color, rows: Rows<'a>) -> Element<'a, Message> {
    // `VolumeRow`: a mute shows 0 and keeps the level underneath.
    let shown = if view.muted { 0.0 } else { view.volume };
    let volume_icon = if shown <= 0.0 {
        Icon::VolumeX
    } else if shown < 0.5 {
        Icon::Volume1
    } else {
        Icon::Volume2
    };
    let volume = row![
        icon_btn(volume_icon, 17.0, 32.0, ink, None, view.i18n.t("ui.volume").to_owned(), Some(Message::Mute)),
        slider(0.0..=1.0, shown, Message::Volume)
            .step(0.01_f32)
            .on_release(Message::VolumeDone)
            .width(Length::Fill)
            .style(move |_, _| settings_kit::slider_style(tokens)),
        container(text(format!("{}%", (shown * 100.0).round() as i32)).size(12).color(Color { a: ink.a, ..tokens.t2 }))
            .width(Length::Fixed(38.0))
            .align_x(Alignment::End),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .height(Length::Fixed(40.0));
    let loop_label = match view.repeat {
        Repeat::Off => view.i18n.t("player.loopOff"),
        Repeat::All => view.i18n.t("player.loopAll"),
        Repeat::One => view.i18n.t("player.loopOne"),
    };
    let tiles = row![
        tile(tokens, Icon::Repeat, loop_label, view.repeat != Repeat::Off, Some(Message::Repeat)),
        tile(tokens, Icon::Blend, view.i18n.t("ui.automix"), false, None),
        tile(tokens, Icon::SlidersHorizontal, view.i18n.t("ui.equalizerTitle"), false, None),
    ]
    .spacing(8);
    let (panel, now) = rows;
    let pickers = &panel.pickers;
    let look = mode_picker::Look {
        i18n: view.i18n,
        tokens,
        theme: view.theme,
        font_scale: view.font_scale,
        style_id: view.style_id,
        open: ink.a,
    };
    let intensity = view.i18n.t(intensity_key(view.theme.intensity));
    let intensity_tip = format!("{}: {}", view.i18n.t("ui.animationIntensity"), intensity);
    let aside = chip(tokens, ink.a, &intensity_tip, intensity, Some(Message::Intensity));
    let style_row = pickers.row(look, mode_picker::Row::Style, Some(aside), now);
    // Random: the style this song drew (reroll) and the pool, both without a backend here.
    let random = (view.style_id == "random").then(|| {
        let drawn = view.i18n.t(view.drawn_style.label_key());
        let reroll = format!("{}: {}", view.i18n.t("options.randomVisualizerReroll"), drawn);
        let pool = view.i18n.t("options.randomPoolChoose");
        container(row![chip(tokens, ink.a, &reroll, drawn, None), chip(tokens, ink.a, pool, pool, None)].spacing(6))
            .width(Length::Fill)
            .align_x(Alignment::End)
            .padding(pad(4.0, 0.0, 0.0, 0.0))
    });
    let background = pickers.row(look, mode_picker::Row::Background, None, now);
    let transparent = row![
        text(view.i18n.t("options.transparentPlayerBackground"))
            .size(14)
            .color(fade(tokens.t2, ink.a * 0.4))
            .width(Length::Fill),
        off_switch(tokens),
    ]
    .align_y(Alignment::Center)
    .height(Length::Fixed(44.0));
    let sources = row![
        source_seg(tokens, ink, view.i18n.t("ui.default"), true, None),
        source_seg(tokens, ink, view.i18n.t("ui.aiTheme"), false, None),
        source_seg(
            tokens,
            ink,
            view.i18n.t("options.customTheme"),
            false,
            Some(view.i18n.t("options.customThemeUnavailable"))
        ),
        icon_btn(
            Icon::Palette,
            15.0,
            28.0,
            fade(tokens.t2, ink.a * 0.4),
            Some(tokens.fill),
            view.i18n.t("options.themeQuickEditUnavailable").to_owned(),
            None
        ),
    ]
    .spacing(6)
    .align_y(Alignment::Center);
    let theme_name =
        if view.daylight { view.i18n.t("theme.daylightDefault") } else { view.i18n.t("theme.midnightDefault") };
    let name_row = row![
        text(theme_name).size(13).color(fade(tokens.t2, ink.a)).width(Length::Fill),
        day_night(tokens, ink, view.daylight),
        text_btn(view.i18n.t("ui.generateAITheme"), fade(tokens.t2, ink.a * 0.4), None),
    ]
    .spacing(6)
    .align_y(Alignment::Center);
    let mut column = column![volume, tiles, hairline(tokens), style_row].spacing(8);
    if let Some(random) = random {
        column = column.push(random);
    }
    column.push(background).push(transparent).push(hairline(tokens)).push(sources).push(name_row).into()
}

fn queue_tab<'a>(
    panel: &'a PlayerPanel,
    view: View<'a>,
    tokens: Tokens,
    ink: Color,
    now: Instant,
) -> Element<'a, Message> {
    if view.queue.is_empty() {
        return text(view.i18n.t("queue.empty")).size(13).color(Color { a: ink.a, ..tokens.t2 }).into();
    }
    let head = row![
        text(view.i18n.t("queue.title")).size(13).font(settings_kit::SEMIBOLD).color(Color { a: ink.a, ..tokens.t2 }),
        text(view.queue.len().to_string()).size(13).color(Color { a: ink.a, ..tokens.t3 }),
        space().width(Length::Fill),
        icon_btn(
            Icon::Shuffle,
            15.0,
            28.0,
            Color { a: 0.4 * ink.a, ..tokens.t2 },
            None,
            if view.queue.len() < 2 {
                view.i18n.t("wallTools.shuffleUnavailable").to_owned()
            } else {
                view.i18n.t("queue.shuffle").to_owned()
            },
            None,
        ),
    ]
    .spacing(6)
    .align_y(Alignment::Center)
    .height(Length::Fixed(36.0));
    let list_h = queue_list_h(view.width, view.height, view.bottom, panel.chrome, view.queue.len());
    let look = queue_island::Look { i18n: view.i18n, tokens, ink, daylight: view.daylight };
    let list = panel.queue.view(look, view.queue, view.index, list_h, now).map(Message::Queue);
    column![head, list].spacing(0).into()
}

/// `SourceTab`: where the lyrics come from, the timeline offset, online matching and the lyric file, and the gain.
fn source_tab<'a>(view: View<'a>, tokens: Tokens) -> Element<'a, Message> {
    let i18n = view.i18n;
    let online = view.track.is_some_and(|track| track.online.is_some());
    let source = if !view.has_lyrics {
        i18n.t("localMusic.statusNone")
    } else if online {
        i18n.t("lyricProvider.netease")
    } else {
        i18n.t("localMusic.statusLocal")
    };
    let source = text(source).size(13).color(tokens.t2).wrapping(iced::widget::text::Wrapping::None);
    // The reset keeps its place at 0 so the number beside it does not move.
    let offset_ms = (view.lyric_offset * 1000.0).round() as i64;
    let reset = icon_btn(
        Icon::RotateCcw,
        15.0,
        28.0,
        fade(tokens.t2, if offset_ms == 0 { 0.0 } else { 1.0 }),
        None,
        i18n.t("localMusic.resetLyricTimelineOffset").to_owned(),
        (offset_ms != 0).then_some(Message::Offset(0)),
    );
    let offset = row![reset, stepper(tokens, offset_ms, i18n.t("localMusic.lyricTimelineOffset"))]
        .spacing(6)
        .align_y(Alignment::Center);
    // Matching and the lyric file window have no backend yet; they stay as disabled buttons.
    let buttons = row![
        panel_button(tokens, Icon::Search, i18n.t("localMusic.matchOnline"), true),
        panel_button(tokens, Icon::ArrowUpDown, i18n.t("lyricFile.button"), false),
    ]
    .spacing(8);
    let gains = [
        "localMusic.replayGainOff",
        "localMusic.replayGainAuto",
        "localMusic.replayGainTrack",
        "localMusic.replayGainAlbum",
    ];
    let mut segments = row![].spacing(0);
    for (index, key) in gains.into_iter().enumerate() {
        segments = segments.push(segment(tokens, i18n.t(key), index == 0, index != 0));
    }
    let segments =
        container(segments.height(Length::Fill)).width(Length::Fill).height(Length::Fixed(32.0)).padding(2).style(
            move |_theme: &IcedTheme| container::Style {
                background: Some(tokens.fill.into()),
                border: Border { radius: 8.0.into(), ..Border::default() },
                ..container::Style::default()
            },
        );
    column![
        panel_row(tokens, i18n.t("localMusic.lyrics"), source.into()),
        panel_row(tokens, i18n.t("localMusic.lyricTimelineOffset"), offset.into()),
        container(buttons).padding(pad(8.0, 4.0, 8.0, 4.0)),
        container(hairline(tokens)).padding(pad(8.0, 4.0, 8.0, 4.0)),
        panel_row(tokens, i18n.t("localMusic.replayGainTitle"), segments.into()),
    ]
    .into()
}

/// `PanelRow`: at least 44px, `px-1 py-1.5`, a 14px name on the left and the controls to the right.
fn panel_row<'a>(tokens: Tokens, label: &'a str, control: Element<'a, Message>) -> Element<'a, Message> {
    let name = text(label).size(14).line_height(iced::Pixels(20.0)).color(tokens.t1);
    let name = container(name).width(Length::Fixed(72.0));
    let control = container(control).width(Length::Fill).align_x(Alignment::End);
    row![name, control]
        .spacing(12)
        .align_y(Alignment::Center)
        .padding(pad(6.0, 4.0, 6.0, 4.0))
        .height(Length::Fixed(44.0))
        .into()
}

/// `Stepper`: a 32px `--st-fill` pill with − and + of 250ms, the value in 13px mono and `ms` in `--st-t3`.
fn stepper<'a>(tokens: Tokens, value_ms: i64, label: &'a str) -> Element<'a, Message> {
    let step = move |lucide: Icon, to: i64| -> Element<'a, Message> {
        button(container(icon(lucide, 14.0, tokens.t2)).center(Length::Fixed(28.0)))
            .padding(0)
            .on_press(Message::Offset(to))
            .style(move |_theme: &IcedTheme, status| button::Style {
                background: matches!(status, button::Status::Hovered | button::Status::Pressed)
                    .then_some(Background::Color(tokens.fill2)),
                border: Border { radius: 8.0.into(), ..Border::default() },
                ..button::Style::default()
            })
            .into()
    };
    let number = if value_ms > 0 { format!("+{value_ms}") } else { value_ms.to_string() };
    let mono = iced::Font::MONOSPACE;
    let value = text(number).size(13).font(mono).color(tokens.t1);
    let value = container(value).width(Length::Fixed(48.0)).align_x(Alignment::End);
    let unit = container(text("ms").size(12).font(mono).color(tokens.t3)).padding(pad(0.0, 4.0, 0.0, 3.0));
    let body = row![step(Icon::Minus, value_ms - 250), value, unit, step(Icon::Plus, value_ms + 250)]
        .align_y(Alignment::Center);
    let pill = container(body).center_y(Length::Fixed(32.0)).style(move |_theme: &IcedTheme| container::Style {
        background: Some(tokens.fill.into()),
        border: Border { radius: 8.0.into(), ..Border::default() },
        ..container::Style::default()
    });
    tooltip(pill, text(label).size(12), tooltip::Position::Top).style(container::rounded_box).into()
}

/// `PanelButton`: 32px, `rounded-lg`, `--st-fill`, a 14px icon and 13px text; disabled it is at 40%.
fn panel_button<'a>(tokens: Tokens, lucide: Icon, label: &'a str, grow: bool) -> Element<'a, Message> {
    let ink = fade(tokens.t1, 0.4);
    let label = text(label).size(13).color(ink).wrapping(iced::widget::text::Wrapping::None);
    let body = row![icon(lucide, 14.0, ink), label].spacing(6).align_y(Alignment::Center);
    let button = button(container(body).center_y(Length::Fixed(32.0)).center_x(Length::Fill))
        .height(Length::Fixed(32.0))
        .padding(pad(0.0, 12.0, 0.0, 12.0))
        .style(move |_theme: &IcedTheme, _status| button::Style {
            background: Some(fade(tokens.fill, 0.4).into()),
            border: Border { radius: 8.0.into(), ..Border::default() },
            ..button::Style::default()
        });
    if grow { button.width(Length::Fill).into() } else { button.into() }
}

/// One option of `Segments` (`m`): 28px, `rounded-md`, 13px; the chosen one sits on `--st-raise`.
fn segment<'a>(tokens: Tokens, label: &'a str, chosen: bool, disabled: bool) -> Element<'a, Message> {
    let ink = if chosen { tokens.t1 } else { tokens.t2 };
    let ink = if disabled { fade(ink, 0.4) } else { ink };
    container(text(label).size(13).color(ink).wrapping(iced::widget::text::Wrapping::None))
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .style(move |_theme: &IcedTheme| container::Style {
            background: chosen.then_some(Background::Color(tokens.raise)),
            border: Border { radius: 6.0.into(), ..Border::default() },
            ..container::Style::default()
        })
        .into()
}

fn quality_tab<'a>(view: View<'a>, tokens: Tokens, ink: Color) -> Element<'a, Message> {
    column![
        labeled(
            ink,
            view.i18n.t("account.guestMode"),
            text_btn(view.i18n.t("account.loginOnHome"), Color { a: 0.4 * ink.a, ..ink }, None),
        ),
        hairline(tokens),
        labeled(
            ink,
            view.i18n.t("account.audioQuality"),
            text(view.i18n.t("account.qualityStandard")).size(13).color(Color { a: 0.4 * ink.a, ..tokens.t2 }).into(),
        ),
    ]
    .spacing(4)
    .into()
}

/// Where the handle is in its transitions, each 0..1.
#[derive(Clone, Copy)]
struct HandleLook {
    open: bool,
    shown: f32,
    tone: f32,
    hover: f32,
}

/// `PanelHandle`: a 48px round button on the bottom baseline. Closed it shows Settings2 on black 40%
/// (daylight white 70%), open an X on white; the colours cross over in 300ms. Closed it grows to 105% under
/// the pointer from 768px up, and below that it sits half tucked away at 60% until hovered.
fn handle_button<'a>(view: View<'a>, look: HandleLook) -> Element<'a, Message> {
    let edge = if view.width >= 768.0 { 32.0 } else { 16.0 };
    let closed = 1.0 - look.tone;
    let (rest_fill, rest_ink) = if view.daylight {
        (Color { a: 0.7, ..Color::WHITE }, Color::from_rgb8(0x18, 0x18, 0x1b))
    } else {
        (Color { a: 0.4, ..Color::BLACK }, Color::WHITE)
    };
    let (hover_scale, tuck, opacity) = if view.width >= 768.0 {
        (1.0 + 0.05 * look.hover * closed, 0.0, 1.0)
    } else {
        (1.0, 24.0 * (1.0 - look.hover) * closed, 1.0 - 0.4 * (1.0 - look.hover) * closed)
    };
    let alpha = look.shown * opacity;
    let fill = mix_color(rest_fill, Color::WHITE, look.tone);
    let fill = Color { a: fill.a * alpha, ..fill };
    let ink = mix_color(rest_ink, Color::BLACK, look.tone);
    let mark = container(icon(if look.open { Icon::X } else { Icon::Settings2 }, 20.0, Color { a: alpha, ..ink }))
        .center(Length::Fixed(48.0))
        .style(move |_theme: &IcedTheme| container::Style {
            background: Some(Background::Color(fill)),
            border: Border { radius: 24.0.into(), ..Border::default() },
            shadow: iced::Shadow {
                color: Color { a: 0.1 * alpha, ..Color::BLACK },
                offset: iced::Vector::new(0.0, 10.0),
                blur_radius: 15.0,
            },
            ..container::Style::default()
        });
    let button = button(mark).padding(0).on_press(Message::Toggle).style(|_, _| button::Style::default());
    // `group w-20`: the hover reaches 32px left of the button.
    let group = container(button).width(Length::Fixed(80.0)).align_x(Alignment::End);
    let group = mouse_area(group).on_enter(Message::HandleHover(true)).on_exit(Message::HandleHover(false));
    let scale = (0.92 + 0.08 * look.shown) * hover_scale;
    let shift = iced::Vector::new(20.0 * (1.0 - look.shown) + tuck, 12.0 * (1.0 - look.shown));
    let group: Element<'a, Message> = iced::widget::float(group).translate(move |_, _| shift).scale(scale).into();
    container(group)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::End)
        .align_y(Alignment::End)
        .padding(pad(0.0, edge, view.bottom, 0.0))
        .into()
}

fn mix_color(from: Color, to: Color, t: f32) -> Color {
    Color {
        r: from.r + (to.r - from.r) * t,
        g: from.g + (to.g - from.g) * t,
        b: from.b + (to.b - from.b) * t,
        a: from.a + (to.a - from.a) * t,
    }
}

fn cover_box<'a>(
    tokens: Tokens,
    art: Option<&'a iced::widget::image::Handle>,
    px: f32,
    radius: f32,
    glyph: f32,
) -> Element<'a, Message> {
    let face: Element<'a, Message> = if let Some(art) = art {
        image(art.clone())
            .width(Length::Fixed(px))
            .height(Length::Fixed(px))
            .content_fit(iced::ContentFit::Cover)
            .border_radius(radius)
            // The tokens carry the surface's fade; `--st-t1` is opaque at rest.
            .opacity(tokens.t1.a)
            .into()
    } else {
        container(icon(Icon::Disc, glyph, tokens.t3))
            .width(Length::Fixed(px))
            .height(Length::Fixed(px))
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .into()
    };
    container(face)
        .width(Length::Fixed(px))
        .height(Length::Fixed(px))
        .style(move |_theme: &IcedTheme| container::Style {
            background: Some(tokens.fill.into()),
            border: Border { radius: radius.into(), width: 1.0, color: tokens.line },
            ..container::Style::default()
        })
        .into()
}

fn tile<'a>(
    tokens: Tokens,
    lucide: Icon,
    label: &'a str,
    lit: bool,
    on_press: Option<Message>,
) -> Element<'a, Message> {
    let color = if lit { tokens.accent } else { fade(tokens.t3, if on_press.is_some() { 1.0 } else { 0.4 }) };
    let bg = if lit { tokens.raise } else { tokens.fill };
    button(
        column![
            container(icon(lucide, 18.0, color)).width(Length::Fill).align_x(Alignment::Start),
            text(label).size(12).color(if lit { tokens.t1 } else { Color { a: color.a, ..tokens.t2 } }),
        ]
        .spacing(8)
        .padding(pad(10.0, 10.0, 9.0, 10.0)),
    )
    .width(Length::Fill)
    .height(Length::Fixed(64.0))
    .padding(0)
    .on_press_maybe(on_press)
    .style(move |_theme: &IcedTheme, _status| button::Style {
        background: Some(Background::Color(bg)),
        border: Border { radius: 10.0.into(), ..Border::default() },
        ..button::Style::default()
    })
    .into()
}

fn labeled<'a>(ink: Color, label: &'a str, control: Element<'a, Message>) -> Element<'a, Message> {
    row![text(label).size(14).color(ink).width(Length::Fill), control]
        .spacing(12)
        .align_y(Alignment::Center)
        .padding(pad(6.0, 4.0, 6.0, 4.0))
        .height(Length::Fixed(44.0))
        .into()
}

fn text_btn<'a>(label: &'a str, color: Color, on_press: Option<Message>) -> Element<'a, Message> {
    button(text(label).size(13).color(color))
        .padding(pad(8.0, 12.0, 8.0, 12.0))
        .on_press_maybe(on_press)
        .style(button::text)
        .into()
}

/// `ModeStepperRow`: arrows at the two ends, the mode in the middle, an optional chip before the right arrow.
/// 「更多设置」lives in the picture picker (`QuickEffectPicker`), which is not drawn here.
fn hairline<'a>(tokens: Tokens) -> Element<'a, Message> {
    container(space())
        .width(Length::Fill)
        .height(1.0)
        .style(move |_theme: &IcedTheme| container::Style {
            background: Some(tokens.line.into()),
            ..container::Style::default()
        })
        .into()
}

fn icon_btn<'a>(
    lucide: Icon,
    glyph: f32,
    box_px: f32,
    color: Color,
    fill: Option<Color>,
    tip: String,
    on_press: Option<Message>,
) -> Element<'a, Message> {
    let mark = container(icon(lucide, glyph, color))
        .width(Length::Fixed(box_px))
        .height(Length::Fixed(box_px))
        .align_x(Alignment::Center)
        .align_y(Alignment::Center);
    let button = button(mark).padding(0).on_press_maybe(on_press).style(move |_theme: &IcedTheme, status| {
        let hot = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: fill.or(hot.then_some(Color { a: 0.08, ..color })).map(Background::Color),
            border: Border { radius: 8.0.into(), ..Border::default() },
            ..button::Style::default()
        }
    });
    tooltip(button, text(tip).size(12), tooltip::Position::Top).style(container::rounded_box).into()
}

fn intensity_key(intensity: folia_core::AnimationIntensity) -> &'static str {
    match intensity {
        folia_core::AnimationIntensity::Calm => "animation.calm",
        folia_core::AnimationIntensity::Normal => "animation.normal",
        folia_core::AnimationIntensity::Chaotic => "animation.chaotic",
    }
}

/// `QuickControlChip`: 28px high, `rounded-lg`, `px-2.5`, 12px; `--st-fill` with `--st-t2`, hover `--st-fill2`
/// with `--st-t1`, disabled at 40%.
fn chip<'a>(tokens: Tokens, alpha: f32, tip: &str, label: &'a str, on_press: Option<Message>) -> Element<'a, Message> {
    let alpha = if on_press.is_some() { alpha } else { alpha * 0.4 };
    let enabled = on_press.is_some();
    let button = button(
        container(text(label).size(12).wrapping(iced::widget::text::Wrapping::None)).center_y(Length::Fixed(28.0)),
    )
    .height(Length::Fixed(28.0))
    .padding(pad(0.0, 10.0, 0.0, 10.0))
    .on_press_maybe(on_press)
    .style(move |_theme: &IcedTheme, status| {
        let hot = enabled && matches!(status, button::Status::Hovered | button::Status::Pressed);
        let (fill, ink) = if hot { (tokens.fill2, tokens.t1) } else { (tokens.fill, tokens.t2) };
        button::Style {
            background: Some(Background::Color(fade(fill, alpha))),
            text_color: fade(ink, alpha),
            border: Border { radius: 8.0.into(), ..Border::default() },
            ..button::Style::default()
        }
    });
    tooltip(button, text(tip.to_owned()).size(12), tooltip::Position::Top).style(container::rounded_box).into()
}

fn source_seg<'a>(tokens: Tokens, ink: Color, label: &'a str, on: bool, tip: Option<&'a str>) -> Element<'a, Message> {
    let button = button(text(label).size(13).color(if on { ink } else { fade(tokens.t2, ink.a * 0.4) }))
        .padding(pad(6.0, 10.0, 6.0, 10.0))
        .on_press_maybe(None)
        .style(move |_theme: &IcedTheme, _status| button::Style {
            background: on.then_some(Background::Color(tokens.raise)),
            border: Border { radius: 8.0.into(), ..Border::default() },
            ..button::Style::default()
        });
    match tip {
        Some(tip) => tooltip(button, text(tip).size(12), tooltip::Position::Top).style(container::rounded_box).into(),
        None => button.into(),
    }
}

fn day_night<'a>(tokens: Tokens, ink: Color, daylight: bool) -> Element<'a, Message> {
    let one = |lucide: Icon, on: bool, message: Message| {
        button(container(icon(lucide, 15.0, if on { ink } else { fade(tokens.t2, ink.a) })).center(Length::Fill))
            .width(Length::Fixed(28.0))
            .height(Length::Fixed(28.0))
            .padding(0)
            .on_press(message)
            .style(move |_theme: &IcedTheme, _status| button::Style {
                background: on.then_some(Background::Color(tokens.raise)),
                border: Border { radius: 8.0.into(), ..Border::default() },
                ..button::Style::default()
            })
    };
    row![
        one(Icon::Sun, daylight, Message::Theme(ThemeMode::Light)),
        one(Icon::Moon, !daylight, Message::Theme(ThemeMode::Dark))
    ]
    .spacing(4)
    .into()
}

fn off_switch<'a>(tokens: Tokens) -> Element<'a, Message> {
    let knob = container(space()).width(18).height(18).style(move |_theme: &IcedTheme| container::Style {
        background: Some(tokens.knob.into()),
        border: Border { radius: 9.0.into(), ..Border::default() },
        ..container::Style::default()
    });
    container(knob)
        .width(40)
        .height(24)
        .padding(pad(3.0, 0.0, 0.0, 3.0))
        .style(move |_theme: &IcedTheme| container::Style {
            background: Some(fade(tokens.track, 0.4).into()),
            border: Border { radius: 12.0.into(), ..Border::default() },
            ..container::Style::default()
        })
        .into()
}

fn fade(color: Color, open: f32) -> Color {
    Color { a: color.a * open, ..color }
}

fn title_album<'a>(view: View<'a>) -> (&'a str, &'a str) {
    match view.track {
        Some(track) => (track.title.as_str(), track.album.as_deref().unwrap_or("")),
        None => (view.i18n.t("ui.noTrack"), ""),
    }
}

fn shown_tab(tab: Tab, local: bool, online: bool) -> Tab {
    if tab == Tab::Source && !local && !online { Tab::Cover } else { tab }
}

fn measure(tab: Tab, queue_len: usize, inner: f32, max_h: f32) -> (f32, f32, f32) {
    let under = 14.0 + 26.0 + 20.0 + 18.0 + 14.0 + 32.0;
    let room = (max_h - 24.0 - 12.0 - 36.0).max(under + 96.0);
    let cover_px = (room - under).clamp(96.0, inner);
    if tab == Tab::Cover {
        return (cover_px + under, 0.0, cover_px);
    }
    let body = match tab {
        Tab::Controls => 460.0,
        Tab::Queue => 36.0 + queue_island::list_height(queue_len),
        Tab::Source => 210.0,
        Tab::Quality => 110.0,
        Tab::Cover => 0.0,
    };
    let chrome = 24.0 + 48.0 + body + 12.0 + 36.0;
    let body = if chrome > max_h { (body - (chrome - max_h)).max(48.0) } else { body };
    (48.0, 12.0 + 1.0 + 8.0 + body, cover_px)
}

/// The queue list's height: what `measure` leaves the queue tab under its 36px head.
fn queue_list_h(width: f32, height: f32, bottom: f32, chrome: bool, len: usize) -> f32 {
    let under = if chrome { HANDLE_ROOM } else { BARE_GAP };
    let panel_w = (width - 32.0).clamp(200.0, 360.0);
    let inner = (panel_w - 24.0).max(96.0);
    let max_h = (height - under - bottom - TOP_MARGIN).max(220.0);
    let (_, body, _) = measure(Tab::Queue, len, inner, max_h);
    (body - 21.0 - 36.0).max(ROW_MIN)
}

/// The list never folds under one row.
const ROW_MIN: f32 = queue_island::ROW;

fn tokens_of(view: &View<'_>) -> Tokens {
    let mut tokens = Tokens::new(view.theme, view.daylight);
    if !view.daylight {
        tokens.raise = Color { r: 1.0, g: 1.0, b: 1.0, a: 0.18 };
    }
    tokens
}

fn pad(top: f32, right: f32, bottom: f32, left: f32) -> iced::Padding {
    iced::Padding { top, right, bottom, left }
}

/// Where a mode row's trigger is in the face: padding 12, the folded header, the body's gap, hairline and
/// gap, then volume 40, tiles 64 and the hairline with their 8px gaps, and the row's own `py-1`.
fn picker_place(view: &View<'_>, row: Option<mode_picker::Row>, size: Size, header_h: f32) -> mode_picker::Place {
    let style_top = 12.0 + header_h + 21.0 + 40.0 + 8.0 + 64.0 + 8.0 + 1.0 + 8.0 + 4.0;
    let params = if view.style_id == "random" { mode_picker::PARAMS_H + 8.0 } else { 0.0 };
    let top = match row {
        Some(mode_picker::Row::Background) => style_top + mode_picker::ROW_H + 8.0 + params,
        _ => style_top,
    };
    mode_picker::Place {
        width: size.width,
        height: size.height,
        trigger_top: top,
        trigger_bottom: top + 48.0,
        origin: (0.0, 0.0),
    }
}

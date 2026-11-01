// The player panel's mode rows from `ModeStepperRow.tsx` and the picture picker they open (`QuickEffectPicker.tsx`).

use std::time::Instant;

use folia_core::{Lyrics, Theme};
use folia_i18n::I18n;
use folia_render::Plate;
use folia_visualizer::{Ease, Scene, Style};
use iced::widget::{button, canvas, column, container, mouse_area, row, scrollable, shader, space, stack, svg, text};
use iced::{Alignment, Background, Border, Color, Element, Length, Size};
use lucide_static_svg::Icon;

use crate::icons::icon;
use crate::motion::{CSS_TRANSITION, Tween};
use crate::settings_kit::Tokens;

/// The kit's `move` and `fast` durations and its `out` curve.
const MOVE_MS: f64 = 220.0;
const FAST_MS: f64 = 120.0;
const KIT_OUT: Ease = Ease::new(0.22, 1.0, 0.36, 1.0);
/// `STEP_ENTER_OFFSET_PX`: a stepped mode slides in from this far.
const STEP_OFFSET: f32 = 10.0;
/// `py-1` around the 48px trigger.
pub const ROW_H: f32 = 56.0;
/// The params line under a row: `pt-1` and 28px chips.
pub const PARAMS_H: f32 = 32.0;
/// `POPOVER_GAP` between the trigger and the picker, `POPOVER_GUTTER` inside the panel.
const GAP: f32 = 6.0;
const GUTTER: f32 = 8.0;
const COLUMNS: usize = 3;
const GRID_GAP: f32 = 12.0;
const GRID_MAX: f32 = 300.0;
/// The kit scrollbar's gutter (`st-scroll`).
const GUTTER_SCROLL: f32 = 12.0;
/// `mt-2 h-9` of the more button.
const MORE_H: f32 = 44.0;
/// `pt-2 pb-1` of the picker.
const PICKER_PAD: (f32, f32) = (8.0, 4.0);
/// Random is a picker choice after the styles, as `RANDOM_VISUALIZER_CHOICE`.
pub const RANDOM: usize = Style::ALL.len();
/// The common background's glyph from `backgrounds/common/entry.tsx`.
const COMMON_GLYPH: &str = concat!(
    r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">"#,
    r#"<path d="M4 17l5-8 5 8z"/><circle cx="17.5" cy="8.5" r="3" opacity="0.55"/></svg>"#
);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    Style,
    Background,
}

#[derive(Debug, Clone)]
pub enum Message {
    Toggle(Row),
    Close,
    Step(Row, i32),
    Pick(Row, usize),
    Hover(Option<usize>),
    More,
}

/// What a picker message asks of the panel.
pub enum Outcome {
    None,
    /// A lyric style, or `None` for random.
    Style(Option<Style>),
    Settings,
}

pub struct Pickers {
    open: Option<Row>,
    /// The row whose picker is drawn, kept while it leaves.
    drawn: Option<Row>,
    pop: Tween,
    clock: Instant,
    hovered: Option<usize>,
    step: [f32; 2],
    enter: [Tween; 2],
    chevron: [Tween; 2],
    preview: Lyrics,
    pictures: folia_visualizer::Pictures,
}

/// What the rows need from the panel.
#[derive(Clone, Copy)]
pub struct Look<'a> {
    pub i18n: &'a I18n,
    pub tokens: Tokens,
    pub theme: &'a Theme,
    pub font_scale: f32,
    /// The chosen style id, or `random`.
    pub style_id: &'a str,
    /// Panel opacity while it opens and closes.
    pub open: f32,
}

impl Pickers {
    pub fn new(now: Instant) -> Self {
        let enter = Tween::new(1.0, MOVE_MS, KIT_OUT, now);
        let chevron = Tween::new(0.0, MOVE_MS, CSS_TRANSITION, now);
        Self {
            open: None,
            drawn: None,
            pop: Tween::new(0.0, MOVE_MS, KIT_OUT, now),
            clock: now,
            hovered: None,
            step: [0.0; 2],
            enter: [enter; 2],
            chevron: [chevron; 2],
            preview: folia_visualizer::preview_lyrics(),
            pictures: folia_visualizer::Pictures::default(),
        }
    }

    pub fn update(&mut self, message: Message, style_id: &str, now: Instant) -> Outcome {
        match message {
            Message::Toggle(row) => {
                let next = if self.open == Some(row) { None } else { Some(row) };
                self.set_open(next, now);
                Outcome::None
            }
            Message::Close => {
                self.set_open(None, now);
                Outcome::None
            }
            Message::Step(row, direction) => {
                self.entered(row, direction as f32, now);
                match row {
                    Row::Style => Outcome::Style(step_style(style_id, direction)),
                    Row::Background => Outcome::None,
                }
            }
            Message::Pick(row, index) => {
                self.entered(row, 0.0, now);
                match row {
                    Row::Style if index == RANDOM => Outcome::Style(None),
                    Row::Style => Style::ALL.get(index).map_or(Outcome::None, |style| Outcome::Style(Some(*style))),
                    Row::Background => Outcome::None,
                }
            }
            Message::Hover(index) => {
                self.hovered = index;
                Outcome::None
            }
            Message::More => {
                self.set_open(None, now);
                Outcome::Settings
            }
        }
    }

    pub fn drawn(&self) -> Option<Row> {
        self.drawn
    }

    /// Gone at once as the panel closes: its live pictures cannot fade with the panel.
    pub fn dismiss(&mut self, now: Instant) {
        self.set_open(None, now);
        self.drawn = None;
        self.pop = Tween::new(0.0, MOVE_MS, KIT_OUT, now);
    }

    pub fn set_open(&mut self, open: Option<Row>, now: Instant) {
        self.open = open;
        if open.is_some() {
            self.drawn = open;
            self.clock = now;
            self.hovered = None;
            self.pop.set_with(1.0, MOVE_MS, KIT_OUT, now);
        } else {
            self.pop.set_with(0.0, FAST_MS, CSS_TRANSITION, now);
        }
        for (index, chevron) in self.chevron.iter_mut().enumerate() {
            chevron.set(if open.map(|row| row as usize) == Some(index) { 1.0 } else { 0.0 }, now);
        }
    }

    fn entered(&mut self, row: Row, direction: f32, now: Instant) {
        self.step[row as usize] = direction;
        self.enter[row as usize].restart(0.0, now);
    }

    pub fn animating(&self, now: Instant) -> bool {
        let tweens = self.enter.iter().chain(&self.chevron).chain(std::iter::once(&self.pop));
        let live_picture = self.open == Some(Row::Style) && self.hovered.is_some_and(|index| index < RANDOM);
        tweens.into_iter().any(|tween| tween.running(now)) || live_picture
    }

    /// `‹ glyph caption/mode⌄ aside ›`, the trigger opening this row's picker.
    pub fn row<'a>(
        &'a self,
        look: Look<'a>,
        which: Row,
        aside: Option<Element<'a, crate::panel::Message>>,
        now: Instant,
    ) -> Element<'a, crate::panel::Message> {
        let tokens = look.tokens;
        let alpha = look.open;
        let options = options(look, which);
        let value = chosen(look, which);
        let can_step = options.len() > 1;
        let enter = self.enter[which as usize].value(now);
        let open = self.open == Some(which);
        let slide = self.step[which as usize] * STEP_OFFSET * (1.0 - enter);
        let caption = match which {
            Row::Style => look.i18n.t("ui.animationMode"),
            Row::Background => look.i18n.t("options.visualizerBackgroundMode"),
        };
        let fade = |color: Color, amount: f32| Color { a: color.a * amount, ..color };
        let glyph = container(glyph(which, value, 18.0, fade(tokens.t2, alpha * enter))).center(Length::Fixed(30.0));
        let glyph_box = container(glyph)
            .width(Length::Fixed(48.0))
            .height(Length::Fixed(30.0))
            .center_x(Length::Fixed(48.0))
            .style(move |_| container::Style {
                background: Some(Background::Color(fade(tokens.fill, alpha))),
                border: Border { radius: 6.0.into(), width: 1.0, color: fade(tokens.line, alpha) },
                ..container::Style::default()
            });
        let label = options.get(value).copied().unwrap_or_default();
        let chevron = svg(svg::Handle::from_memory(Icon::ChevronDown.svg_bytes()))
            .width(Length::Fixed(13.0))
            .height(Length::Fixed(13.0))
            .rotation(iced::Radians(std::f32::consts::PI * self.chevron[which as usize].value(now)))
            .style(move |_, _| svg::Style { color: Some(Color { a: 1.0, ..tokens.t3 }) })
            .opacity(tokens.t3.a * alpha);
        let name =
            row![text(label).size(14).line_height(iced::Pixels(19.0)).color(fade(tokens.t1, alpha * enter)), chevron]
                .spacing(4)
                .align_y(Alignment::Center);
        let words = column![
            text(caption).size(11.5).line_height(iced::Pixels(15.0)).color(fade(tokens.t3, alpha * enter)),
            name
        ];
        // The mode slides in from the step's side; `pl-1.5` shifts with it.
        let shift = iced::Padding { left: 6.0 + slide.max(0.0), right: 8.0 + (-slide).max(0.0), ..iced::Padding::ZERO };
        let inside = row![glyph_box, words].spacing(10).align_y(Alignment::Center);
        let trigger = button(container(inside).center_y(Length::Fixed(48.0)).padding(shift))
            .width(Length::Fill)
            .height(Length::Fixed(48.0))
            .padding(0)
            .on_press(crate::panel::Message::Picker(Message::Toggle(which)))
            .style(move |_, status| {
                let hot = open || matches!(status, button::Status::Hovered | button::Status::Pressed);
                button::Style {
                    background: hot.then_some(Background::Color(fade(tokens.fill, alpha))),
                    border: Border { radius: 10.0.into(), ..Border::default() },
                    ..button::Style::default()
                }
            });
        let mut line = row![step_button(tokens, alpha, Icon::ChevronLeft, can_step.then_some((which, -1)))].spacing(4);
        line = line.push(trigger);
        if let Some(aside) = aside {
            line = line.push(aside);
        }
        line = line.push(step_button(tokens, alpha, Icon::ChevronRight, can_step.then_some((which, 1))));
        container(line.align_y(Alignment::Center)).padding([4, 0]).into()
    }

    /// The open picker, placed over the panel under (or over) its row. `top` is the trigger's top and
    /// `bottom` its bottom inside a face `height` tall.
    pub fn picker<'a>(
        &'a self,
        look: Look<'a>,
        place: Place,
        now: Instant,
    ) -> Option<Element<'a, crate::panel::Message>> {
        let which = self.drawn?;
        let pop = self.pop.value(now);
        if pop <= 0.0 {
            return None;
        }
        let tokens = look.tokens;
        let width = place.width - 2.0 * GUTTER;
        let options = options(look, which);
        let value = chosen(look, which);
        // `p-1` and `scrollbar-gutter: stable both-edges`: 4px of padding and a 12px gutter on each side.
        let cell_w = ((width - 8.0 - 2.0 * GUTTER_SCROLL - GRID_GAP * (COLUMNS - 1) as f32) / COLUMNS as f32).floor();
        let picture_h = (cell_w * 10.0 / 16.0).round();
        let rows = options.len().div_ceil(COLUMNS);
        let grid_h = rows as f32 * (picture_h + 24.0) + (rows.saturating_sub(1)) as f32 * GRID_GAP + 8.0;
        let natural = PICKER_PAD.0 + grid_h.min(GRID_MAX) + MORE_H + PICKER_PAD.1;
        let room_under = place.height - GUTTER - (place.trigger_bottom + GAP);
        let room_over = place.trigger_top - GAP - GUTTER;
        let under = room_under >= natural || room_under >= room_over;
        let height = natural.min(if under { room_under } else { room_over }).max(MORE_H + 40.0);
        let top = if under { place.trigger_bottom + GAP } else { place.trigger_top - GAP - height };
        let alpha = pop * look.open;
        let mut grid =
            column![].spacing(GRID_GAP).padding(iced::Padding { left: 4.0 + GUTTER_SCROLL, ..iced::Padding::new(4.0) });
        for start in (0..options.len()).step_by(COLUMNS) {
            let mut line = row![].spacing(GRID_GAP);
            for (index, label) in options.iter().enumerate().skip(start).take(COLUMNS) {
                let cell = Cell { which, index, label, chosen: index == value, width: cell_w, picture_h };
                line = line.push(self.cell(look, cell, alpha, now));
            }
            grid = grid.push(line);
        }
        let grid_room = height - PICKER_PAD.0 - MORE_H - PICKER_PAD.1;
        let grid = scrollable(grid)
            .height(Length::Fixed(grid_room.min(grid_h).max(0.0)))
            .direction(crate::settings_kit::scrollbar())
            .style(crate::settings_kit::scroll_style(tokens));
        let more = more_button(look, alpha);
        let body = column![grid, more].padding(iced::Padding {
            top: PICKER_PAD.0,
            bottom: PICKER_PAD.1,
            ..iced::Padding::ZERO
        });
        let body = container(body).width(Length::Fixed(width)).height(Length::Fixed(height));
        // `st-pop`: in with `translateY(∓4px) scale(0.98)` and opacity; the panel's own plate draws the glass.
        let lift = (if under { -4.0 } else { 4.0 }) * (1.0 - pop);
        let plate = popover_plate(tokens, width, height, alpha);
        let layer: Element<'a, crate::panel::Message> = stack![
            plate.element,
            container(body).padding(iced::Padding { top: plate.inset.1, left: plate.inset.0, ..iced::Padding::ZERO })
        ]
        .into();
        // Laid out in the panel's shadow patch, which is wide enough for the plate's own shadow margin, so
        // nothing is squeezed; it pops in from 98% and scales with the panel as one piece.
        let left = (place.origin.0 + GUTTER - plate.inset.0).max(0.0);
        let top = (place.origin.1 + top - plate.inset.1).max(0.0);
        let layer: Element<'a, crate::panel::Message> = iced::widget::float(layer)
            .translate(move |_, _| iced::Vector::new(0.0, lift))
            .scale(0.98 + 0.02 * pop)
            .into();
        let catcher = mouse_area(container(space()).width(Length::Fill).height(Length::Fill))
            .on_press(crate::panel::Message::Picker(Message::Close));
        let placed = container(layer).padding(iced::Padding { top, left, ..iced::Padding::ZERO });
        let mut layers = stack![].width(Length::Fill).height(Length::Fill);
        if self.open.is_some() {
            layers = layers.push(catcher);
        }
        Some(layers.push(placed).into())
    }

    fn cell<'a>(
        &'a self,
        look: Look<'a>,
        cell: Cell<'a>,
        alpha: f32,
        now: Instant,
    ) -> Element<'a, crate::panel::Message> {
        let tokens = look.tokens;
        let fade = move |color: Color| Color { a: color.a * alpha, ..color };
        let Cell { which, index, label, chosen, width, picture_h } = cell;
        let hovered = self.hovered == Some(index);
        let picture: Element<'a, crate::panel::Message> = match (which, Style::ALL.get(index)) {
            (Row::Style, Some(style)) => {
                // `posterTime` at rest; the picture under the pointer plays the clock.
                let time = if hovered {
                    now.saturating_duration_since(self.clock).as_secs_f64() % folia_visualizer::PREVIEW_LOOP
                } else {
                    folia_visualizer::POSTER_TIME
                };
                let scene = Scene {
                    style: *style,
                    lyrics: &self.preview,
                    time,
                    theme: look.theme,
                    font_scale: look.font_scale,
                    stage: Some(Size::new(folia_visualizer::STAGE_WIDTH, folia_visualizer::STAGE_HEIGHT)),
                };
                if hovered {
                    canvas(scene).width(Length::Fill).height(Length::Fill).into()
                } else {
                    canvas(self.pictures.frozen(scene)).width(Length::Fill).height(Length::Fill).into()
                }
            }
            (Row::Style, None) => container(icon(Icon::Shuffle, 28.0, fade(tokens.accent))).center(Length::Fill).into(),
            (Row::Background, _) => container(glyph(which, index, 22.0, fade(tokens.t2))).center(Length::Fill).into(),
        };
        let frame = container(picture).width(Length::Fixed(width)).height(Length::Fixed(picture_h)).clip(true).style(
            move |_| container::Style {
                background: Some(Background::Color(fade(tokens.pv))),
                ..container::Style::default()
            },
        );
        let ring = if chosen { (2.0, tokens.accent) } else { (1.0, tokens.line) };
        let edge = container(space()).width(Length::Fixed(width)).height(Length::Fixed(picture_h)).style(move |_| {
            container::Style {
                border: Border { radius: 8.0.into(), width: ring.0, color: fade(ring.1) },
                ..container::Style::default()
            }
        });
        let ink = if chosen || hovered { tokens.t1 } else { tokens.t2 };
        let weight = if chosen { iced::font::Weight::Semibold } else { iced::font::Weight::Normal };
        let caption = text(label)
            .size(12)
            .line_height(iced::Pixels(18.0))
            .font(crate::typeface::font(weight))
            .color(fade(ink))
            .wrapping(iced::widget::text::Wrapping::None);
        let body =
            column![stack![frame, edge], container(caption).center_x(Length::Fixed(width)).clip(true)].spacing(6);
        let press = button(body)
            .padding(0)
            .on_press(crate::panel::Message::Picker(Message::Pick(which, index)))
            .style(|_, _| button::Style::default());
        mouse_area(press)
            .on_enter(crate::panel::Message::Picker(Message::Hover(Some(index))))
            .on_exit(crate::panel::Message::Picker(Message::Hover(None)))
            .into()
    }
}

/// One picture of the grid, its caption and whether it is the chosen one.
struct Cell<'a> {
    which: Row,
    index: usize,
    label: &'a str,
    chosen: bool,
    width: f32,
    picture_h: f32,
}

/// Where the picker may go inside the panel's face.
#[derive(Clone, Copy)]
pub struct Place {
    pub width: f32,
    pub height: f32,
    pub trigger_top: f32,
    pub trigger_bottom: f32,
    /// Where the panel's face starts inside the layer the picker is drawn in.
    pub origin: (f32, f32),
}

fn options<'a>(look: Look<'a>, which: Row) -> Vec<&'a str> {
    match which {
        Row::Style => Style::ALL
            .iter()
            .map(|style| look.i18n.t(style.label_key()))
            .chain(std::iter::once(look.i18n.t("ui.visualizerRandom")))
            .collect(),
        Row::Background => vec![look.i18n.t("options.visualizerBackgroundModeCommon")],
    }
}

fn chosen(look: Look<'_>, which: Row) -> usize {
    match which {
        Row::Style if look.style_id == "random" => RANDOM,
        Row::Style => Style::ALL.iter().position(|style| style.id() == look.style_id).unwrap_or(0),
        Row::Background => 0,
    }
}

/// The step through styles the arrows take, wrapping and passing random as the last choice.
pub fn step_style(current: &str, step: i32) -> Option<Style> {
    let count = RANDOM as i32 + 1;
    let index = if current == "random" {
        RANDOM as i32
    } else {
        Style::ALL.iter().position(|style| style.id() == current).unwrap_or(0) as i32
    };
    let next = (index + step).rem_euclid(count) as usize;
    Style::ALL.get(next).copied()
}

fn glyph<'a, M: 'a>(which: Row, index: usize, size: f32, color: Color) -> Element<'a, M> {
    let source: &'static str = match (which, Style::ALL.get(index)) {
        (Row::Style, Some(style)) => style.glyph_svg(),
        (Row::Style, None) => return icon(Icon::Shuffle, size, color),
        (Row::Background, _) => COMMON_GLYPH,
    };
    svg(svg::Handle::from_memory(source.as_bytes()))
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .opacity(color.a)
        .style(move |_, _| svg::Style { color: Some(Color { a: 1.0, ..color }) })
        .into()
}

/// 28px step arrow, `rounded-lg`, `--st-t3`; hover lifts it to `--st-t1` on `--st-fill`, a lone mode dims it to 40%.
fn step_button<'a>(
    tokens: Tokens,
    alpha: f32,
    lucide: Icon,
    step: Option<(Row, i32)>,
) -> Element<'a, crate::panel::Message> {
    let dim = if step.is_some() { 1.0 } else { 0.4 };
    let message = step.map(|(row, direction)| crate::panel::Message::Picker(Message::Step(row, direction)));
    button(
        container(icon(lucide, 14.0, Color { a: tokens.t3.a * alpha * dim, ..tokens.t3 })).center(Length::Fixed(28.0)),
    )
    .padding(0)
    .on_press_maybe(message)
    .style(move |_, status| {
        let hot = dim >= 1.0 && matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: hot.then_some(Background::Color(Color { a: tokens.fill.a * alpha, ..tokens.fill })),
            border: Border { radius: 8.0.into(), ..Border::default() },
            ..button::Style::default()
        }
    })
    .into()
}

/// `PickerMoreButton`: a hairline, then the label and a chevron at 13px in `--st-t2`.
fn more_button<'a>(look: Look<'a>, alpha: f32) -> Element<'a, crate::panel::Message> {
    let tokens = look.tokens;
    let fade = move |color: Color| Color { a: color.a * alpha, ..color };
    let line = container(space()).width(Length::Fill).height(Length::Fixed(1.0)).style(move |_| container::Style {
        background: Some(Background::Color(fade(tokens.line))),
        ..container::Style::default()
    });
    let label = row![
        text(look.i18n.t("ui.moreSettings")).size(13).color(fade(tokens.t2)).width(Length::Fill),
        icon(Icon::ChevronRight, 14.0, fade(tokens.t2)),
    ]
    .align_y(Alignment::Center);
    let press = button(column![line, container(label).center_y(Length::Fixed(35.0)).padding([0, 4])])
        .padding(0)
        .on_press(crate::panel::Message::Picker(Message::More))
        .style(|_, _| button::Style::default());
    container(press).padding(iced::Padding { top: 8.0, left: 12.0, right: 12.0, bottom: 0.0 }).into()
}

struct PopoverPlate<'a> {
    element: Element<'a, crate::panel::Message>,
    /// How far the box sits inside the plate's widget, left and top.
    inset: (f32, f32),
}

/// `--st-menu` with `0 14px 36px rgba(0,0,0,0.34), 0 0 0 1px --st-line2`, `rounded-[14px]`.
fn popover_plate<'a>(tokens: Tokens, width: f32, height: f32, alpha: f32) -> PopoverPlate<'a> {
    let reach = 54.0;
    let top = reach - 14.0;
    let menu = tokens.menu;
    let ring = tokens.line2;
    let plate = Plate {
        radius: 14.0,
        fill_origin: [reach, top],
        fill_size: [width, height],
        fill: [menu.r, menu.g, menu.b, menu.a * alpha],
        shadow: [0.0, 0.0, 0.0, 0.34 * alpha],
        shadow_blur: 36.0,
        shadow_y: 14.0,
        shadow_spread: 0.0,
        ring: [ring.r, ring.g, ring.b, ring.a * alpha],
    };
    let element = shader(plate)
        .width(Length::Fixed(width + 2.0 * reach))
        .height(Length::Fixed(height + top + reach + 14.0))
        .into();
    PopoverPlate { element, inset: (reach, top) }
}

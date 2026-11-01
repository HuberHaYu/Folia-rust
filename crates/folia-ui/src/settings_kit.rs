// The settings window's parts, after the original `modal/settings/kit`: colours, rows in cards, switch, choice, list, tabs.

use folia_core::{Rgb, Theme};
use iced::widget::{button, column, container, pick_list, row, scrollable, space, stack, text};
use iced::{Alignment, Background, Border, Color, Element, Font, Length, Padding, Shadow, Vector};

use crate::icons::icon;

const ON_ACCENT_DARK: Rgb = Rgb::hex(0x17171a);
const ON_ACCENT_LIGHT: Rgb = Rgb::hex(0xffffff);
pub const SEMIBOLD: Font = crate::typeface::font(iced::font::Weight::Semibold);
/// Height of a row with a hint line: `min-h-14 py-2.5`, a 20px label and an 18px hint 2px under it.
pub const ROW_HINT_HEIGHT: f32 = 60.0;
/// Space above a group (`mt-7`, `mt-5` for the first) and its heading line with the gap under it.
pub const GROUP_GAP: f32 = 28.0;
pub const FIRST_GROUP_GAP: f32 = 20.0;
pub const GROUP_HEADING: f32 = 32.0;

/// `settingsTokens`: fixed surfaces per light and dark, text and fills from the theme's text colour.
#[derive(Debug, Clone, Copy)]
pub struct Tokens {
    pub win: Color,
    pub side: Color,
    pub card: Color,
    pub menu: Color,
    /// `--st-pv`, behind a picture.
    pub pv: Color,
    pub raise: Color,
    pub knob: Color,
    pub t1: Color,
    pub t2: Color,
    pub t3: Color,
    pub line: Color,
    pub line2: Color,
    pub fill: Color,
    pub fill2: Color,
    pub track: Color,
    pub accent: Color,
    pub on_accent: Color,
    pub muted: Color,
    pub hover: Color,
    pub shadow: Color,
}

impl Tokens {
    pub fn new(theme: &Theme, daylight: bool) -> Self {
        let pick = |light: u32, dark: u32| rgb(Rgb::hex(if daylight { light } else { dark }), 1.0);
        let ink = |alpha: f32| rgb(theme.primary, alpha);
        let on_accent = if contrast(ON_ACCENT_DARK, theme.accent) >= contrast(ON_ACCENT_LIGHT, theme.accent) {
            ON_ACCENT_DARK
        } else {
            ON_ACCENT_LIGHT
        };
        Self {
            win: pick(0xf3f3f4, 0x1a1a1d),
            side: pick(0xe9e9eb, 0x141416),
            card: pick(0xffffff, 0x232327),
            menu: pick(0xffffff, 0x2b2b30),
            pv: pick(0xececef, 0x151517),
            raise: pick(0xffffff, 0x45454c),
            knob: pick(0xffffff, 0xececee),
            t1: ink(1.0),
            t2: ink(0.62),
            t3: ink(0.42),
            line: ink(0.08),
            line2: ink(0.16),
            fill: ink(0.07),
            fill2: ink(0.12),
            track: ink(0.17),
            accent: rgb(theme.accent, 1.0),
            on_accent: rgb(on_accent, 1.0),
            muted: rgb(theme.secondary, 0.7),
            hover: if daylight { Color { a: 0.04, ..Color::BLACK } } else { Color { a: 0.06, ..Color::WHITE } },
            shadow: Color::BLACK,
        }
    }

    /// Every colour at `alpha` of its opacity: iced cannot fade a whole subtree, so the window fades its paints.
    pub fn faded(self, alpha: f32) -> Self {
        let fade = |color: Color| Color { a: color.a * alpha, ..color };
        Self {
            win: fade(self.win),
            side: fade(self.side),
            card: fade(self.card),
            menu: fade(self.menu),
            pv: fade(self.pv),
            raise: fade(self.raise),
            knob: fade(self.knob),
            t1: fade(self.t1),
            t2: fade(self.t2),
            t3: fade(self.t3),
            line: fade(self.line),
            line2: fade(self.line2),
            fill: fade(self.fill),
            fill2: fade(self.fill2),
            track: fade(self.track),
            accent: fade(self.accent),
            on_accent: fade(self.on_accent),
            muted: fade(self.muted),
            hover: fade(self.hover),
            shadow: fade(self.shadow),
        }
    }
}

/// A list entry whose label is already translated.
#[derive(Debug, Clone, PartialEq)]
pub struct Choice<T> {
    pub value: T,
    pub label: String,
}

impl<T> std::fmt::Display for Choice<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

/// One option: label at the left, an optional hint line under it, the control at the right.
pub struct Row<'a, M> {
    pub label: String,
    pub hint: Option<String>,
    /// Belongs under the row above it: indented 40px instead of 16px.
    pub sub: bool,
    pub control: Element<'a, M>,
}

/// Rows in one card under a small heading; a `bare` group has no heading.
pub struct Group<'a, M> {
    pub title: String,
    pub bare: bool,
    pub rows: Vec<Row<'a, M>>,
}

pub fn group<'a, M: 'a>(tokens: Tokens, group: Group<'a, M>, first: bool) -> Element<'a, M> {
    let mut card = column![];
    for (index, entry) in group.rows.into_iter().enumerate() {
        if index > 0 {
            let inset = if entry.sub { 40.0 } else { 16.0 };
            card = card.push(container(fill_box(tokens.line).height(1)).padding(Padding::ZERO.left(inset)));
        }
        card = card.push(row_view(tokens, entry));
    }
    let card = container(card).width(Length::Fill).style(move |_| container::Style {
        background: Some(tokens.card.into()),
        border: Border { color: tokens.line, width: 1.0, radius: 12.0.into() },
        ..container::Style::default()
    });
    let mut body = column![space().height(if first { FIRST_GROUP_GAP } else { GROUP_GAP })];
    if !group.bare {
        let heading = text(group.title).size(13).line_height(px(20.0)).font(SEMIBOLD).color(tokens.t2);
        body = body.push(container(heading).height(24).align_y(Alignment::Center).padding([0, 4]));
        body = body.push(space().height(8));
    }
    body.push(card).into()
}

fn row_view<'a, M: 'a>(tokens: Tokens, entry: Row<'a, M>) -> Element<'a, M> {
    let mut label = column![text(entry.label).size(14).line_height(px(20.0)).color(tokens.t1)];
    if let Some(hint) = entry.hint {
        label = label.push(space().height(2)).push(text(hint).size(12).line_height(px(18.0)).color(tokens.t2));
    }
    let left = if entry.sub { 40.0 } else { 16.0 };
    let content = row![container(label).width(Length::Fill), entry.control].spacing(24).align_y(Alignment::Center);
    // The zero-width strut keeps the row at its 56px minimum, `min-h-14`.
    row![space().height(36), content]
        .align_y(Alignment::Center)
        .padding(Padding { top: 10.0, right: 16.0, bottom: 10.0, left })
        .into()
}

/// The on/off switch; `position` runs from 0 (off) to 1 (on) and is between the two while the thumb glides.
pub fn toggle<'a, M: Clone + 'a>(tokens: Tokens, position: f32, on_press: M) -> Element<'a, M> {
    let knob_color = mix(tokens.knob, tokens.on_accent, position);
    let knob = container(space()).width(18).height(18).style(move |_| container::Style {
        background: Some(knob_color.into()),
        border: Border { radius: 9.0.into(), ..Border::default() },
        shadow: Shadow {
            color: Color { a: 0.3 * tokens.shadow.a, ..Color::BLACK },
            offset: Vector::new(0.0, 1.0),
            blur_radius: 2.0,
        },
        ..container::Style::default()
    });
    let track_color = mix(tokens.track, tokens.accent, position);
    let track = container(knob)
        .width(40)
        .height(24)
        .padding(Padding { top: 3.0, left: 3.0 + 16.0 * position, ..Padding::ZERO })
        .style(move |_| container::Style {
            background: Some(track_color.into()),
            border: Border { radius: 12.0.into(), ..Border::default() },
            ..container::Style::default()
        });
    button(track).padding(0).style(|_, _| button::Style::default()).on_press(on_press).into()
}

/// Two to five short options side by side; the raised mark sits at `mark`, a fractional index while it slides.
pub fn segmented<'a, T: Copy + PartialEq + 'a, M: Clone + 'a>(
    tokens: Tokens,
    options: Vec<(T, String)>,
    value: T,
    mark: Option<f32>,
    on_change: impl Fn(T) -> M,
) -> Element<'a, M> {
    let widths: Vec<f32> = options.iter().map(|(_, label)| label_width(label, 13.0) + 24.0).collect();
    let total: f32 = widths.iter().sum();
    let ground = fill_box(tokens.fill).width(total + 4.0).height(32).style(move |_| rounded(tokens.fill, 8.0));
    let mut layers = stack![ground];
    if let Some(mark) = mark {
        let (x, width) = slide(&widths, mark);
        let raised = container(space()).width(width).height(28).style(move |_| container::Style {
            background: Some(tokens.raise.into()),
            border: Border { radius: 6.0.into(), ..Border::default() },
            shadow: Shadow {
                color: Color { a: 0.2 * tokens.shadow.a, ..Color::BLACK },
                offset: Vector::new(0.0, 1.0),
                blur_radius: 2.0,
            },
            ..container::Style::default()
        });
        layers = layers.push(container(raised).padding(Padding { top: 2.0, left: 2.0 + x, ..Padding::ZERO }));
    }
    let mut choices = row![].padding(2);
    for ((option, label), width) in options.into_iter().zip(widths) {
        let selected = option == value;
        let label = container(text(label).size(13)).center_x(width).center_y(28);
        let choice = button(label).padding(0).style(move |_, status| button::Style {
            text_color: if selected || status == button::Status::Hovered { tokens.t1 } else { tokens.t2 },
            ..button::Style::default()
        });
        choices = choices.push(if selected { choice } else { choice.on_press(on_change(option)) });
    }
    layers.push(choices).into()
}

/// The compact pick list: a 32px filled trigger, its menu on the opaque menu surface.
pub fn select<'a, T: Clone + PartialEq + 'a, M: Clone + 'a>(
    tokens: Tokens,
    options: Vec<Choice<T>>,
    value: Choice<T>,
    on_change: impl Fn(Choice<T>) -> M + 'a,
) -> Element<'a, M> {
    let widest = options.iter().map(|choice| label_width(&choice.label, 13.0)).fold(0.0, f32::max);
    let width = (widest + 12.0 + 8.0 + 8.0 + 16.0).clamp(136.0, 240.0);
    pick_list(options, Some(value), on_change)
        .width(width)
        .padding(Padding { top: 7.0, right: 8.0, bottom: 7.0, left: 12.0 })
        .text_size(13)
        .text_line_height(px(18.0))
        .handle(pick_list::Handle::Arrow { size: Some(16.into()) })
        .style(move |_, status| pick_list::Style {
            text_color: tokens.t1,
            placeholder_color: tokens.t3,
            handle_color: tokens.muted,
            background: if status == pick_list::Status::Active { tokens.fill } else { tokens.fill2 }.into(),
            border: Border { radius: 8.0.into(), ..Border::default() },
        })
        .menu_style(move |_| iced::overlay::menu::Style {
            background: tokens.menu.into(),
            border: Border { color: tokens.line2, width: 1.0, radius: 10.0.into() },
            text_color: tokens.t1,
            selected_text_color: tokens.t1,
            selected_background: tokens.hover.into(),
            shadow: Shadow {
                color: Color { a: 0.36 * tokens.shadow.a, ..Color::BLACK },
                offset: Vector::new(0.0, 14.0),
                blur_radius: 36.0,
            },
        })
        .into()
}

/// Text tabs over a hairline; the accent line under the chosen one sits at `mark`, a fractional index while it slides.
pub fn tabs<'a, M: Clone + 'a>(
    tokens: Tokens,
    tabs: Vec<(String, M)>,
    chosen: Option<usize>,
    mark: Option<f32>,
    top: f32,
    side: f32,
) -> Element<'a, M> {
    let widths: Vec<f32> = tabs.iter().map(|(label, _)| label_width(label, 13.0)).collect();
    let mut strip = row![].spacing(22);
    for (index, ((label, message), width)) in tabs.into_iter().zip(widths.iter().copied()).enumerate() {
        let current = chosen == Some(index);
        let label = container(text(label).size(13)).center_x(width).center_y(36);
        strip = strip.push(button(label).padding(0).on_press(message).style(move |_, status| button::Style {
            text_color: if current || status == button::Status::Hovered { tokens.t1 } else { tokens.t2 },
            ..button::Style::default()
        }));
    }
    let mut layers = stack![strip];
    if let Some(mark) = mark {
        let spaced: Vec<f32> = widths.iter().map(|width| width + 22.0).collect();
        let (x, width) = slide(&spaced, mark);
        let line = container(space()).width(width - 22.0).height(2).style(move |_| container::Style {
            background: Some(tokens.accent.into()),
            border: Border { radius: iced::border::Radius::default().top(2.0), ..Border::default() },
            ..container::Style::default()
        });
        layers = layers.push(container(line).padding(Padding { top: 34.0, left: x, ..Padding::ZERO }));
    }
    column![container(layers).padding(Padding { top, left: side, right: side, bottom: 0.0 }), hairline(tokens)].into()
}

/// A plain value in a row's control slot.
pub fn value<'a, M: 'a>(tokens: Tokens, content: String) -> Element<'a, M> {
    text(content).size(13).color(tokens.t2).into()
}

/// A full-width one-pixel line in the window's line colour.
pub fn hairline<'a, M: 'a>(tokens: Tokens) -> Element<'a, M> {
    fill_box(tokens.line).width(Length::Fill).height(1).into()
}

/// A square icon button: 32px, 8px corners, filled while hovered.
pub fn icon_button<'a, M: Clone + 'a>(
    tokens: Tokens,
    lucide: lucide_static_svg::Icon,
    size: f32,
    on_press: M,
) -> Element<'a, M> {
    let mark = |color: Color| container(icon(lucide, size, color)).center(32);
    let idle =
        button(mark(tokens.t2)).padding(0).on_press(on_press.clone()).style(move |_, _| button::Style::default());
    let hot = button(mark(tokens.t1)).padding(0).on_press(on_press).style(move |_, _| rounded_button(tokens.fill, 8.0));
    hover(idle.into(), hot.into())
}

/// `idle` until the pointer is over it, then `hot`: for parts whose icon colour changes on hover.
pub fn hover<'a, M: 'a>(idle: Element<'a, M>, hot: Element<'a, M>) -> Element<'a, M> {
    iced::widget::hover(idle, hot)
}

pub fn rounded(color: Color, radius: f32) -> container::Style {
    container::Style {
        background: Some(color.into()),
        border: Border { radius: radius.into(), ..Border::default() },
        ..container::Style::default()
    }
}

pub fn rounded_button(color: Color, radius: f32) -> button::Style {
    button::Style {
        background: Some(Background::Color(color)),
        border: Border { radius: radius.into(), ..Border::default() },
        ..button::Style::default()
    }
}

fn fill_box<'a, M: 'a>(color: Color) -> container::Container<'a, M> {
    container(space())
        .style(move |_| container::Style { background: Some(color.into()), ..container::Style::default() })
}

/// Where a mark at fractional index `at` starts and how wide it is, across items of `widths`.
fn slide(widths: &[f32], at: f32) -> (f32, f32) {
    let last = widths.len().saturating_sub(1);
    let at = at.clamp(0.0, last as f32);
    let index = (at.floor() as usize).min(last);
    let next = (index + 1).min(last);
    let t = at - index as f32;
    let start: f32 = widths[..index].iter().sum();
    (start + widths[index] * t, widths[index] + (widths[next] - widths[index]) * t)
}

fn label_width(label: &str, size: f32) -> f32 {
    folia_visualizer::measure(label, size, Font::DEFAULT, f32::INFINITY).width.ceil()
}

fn px(value: f32) -> iced::widget::text::LineHeight {
    iced::widget::text::LineHeight::Absolute(value.into())
}

fn mix(from: Color, to: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    let lerp = |a: f32, b: f32| a + (b - a) * t;
    Color { r: lerp(from.r, to.r), g: lerp(from.g, to.g), b: lerp(from.b, to.b), a: lerp(from.a, to.a) }
}

fn rgb(color: Rgb, alpha: f32) -> Color {
    folia_visualizer::color(color, alpha)
}

/// WCAG contrast ratio, the original `getContrastRatio`.
fn contrast(a: Rgb, b: Rgb) -> f32 {
    let luminance = |c: Rgb| {
        let channel = |v: u8| {
            let v = f32::from(v) / 255.0;
            if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * channel(c.r) + 0.7152 * channel(c.g) + 0.0722 * channel(c.b)
    };
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// `st-scroll`: a 12px gutter holding a 4px rounded `--st-fill2` thumb on no track.
pub fn scrollbar() -> scrollable::Direction {
    scrollable::Direction::Vertical(scrollable::Scrollbar::new().width(4).scroller_width(4).margin(4))
}

pub fn scroll_style(tokens: Tokens) -> impl Fn(&iced::Theme, scrollable::Status) -> scrollable::Style {
    move |_, _| {
        let rail = scrollable::Rail {
            background: None,
            border: Border::default(),
            scroller: scrollable::Scroller {
                background: tokens.fill2.into(),
                border: Border { radius: 2.0.into(), ..Border::default() },
            },
        };
        scrollable::Style {
            container: container::Style::default(),
            vertical_rail: rail,
            horizontal_rail: rail,
            gap: None,
            auto_scroll: scrollable::AutoScroll {
                background: Color::TRANSPARENT.into(),
                border: Border::default(),
                shadow: Shadow::default(),
                icon: Color::TRANSPARENT,
            },
        }
    }
}

/// The kit slider (`PanelSlider`): a 4px `--st-track` rail filled with `--st-accent`, a 16px white thumb with a 14% black ring.
pub fn slider_style(tokens: Tokens) -> iced::widget::slider::Style {
    let alpha = tokens.t1.a;
    iced::widget::slider::Style {
        rail: iced::widget::slider::Rail {
            backgrounds: (Background::Color(tokens.accent), Background::Color(tokens.track)),
            width: 4.0,
            border: Border { radius: 2.0.into(), ..Border::default() },
        },
        handle: iced::widget::slider::Handle {
            shape: iced::widget::slider::HandleShape::Circle { radius: 8.0 },
            background: Background::Color(Color { a: alpha, ..Color::WHITE }),
            border_width: 1.0,
            border_color: Color { a: 0.14 * alpha, ..Color::BLACK },
        },
    }
}

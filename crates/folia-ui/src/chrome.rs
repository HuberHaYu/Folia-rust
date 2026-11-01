// Frameless window: edge resizing, the drag zone, and the hidden title bar with the window buttons.

use std::time::Instant;

use folia_i18n::I18n;
use folia_visualizer::Ease;
use iced::widget::{button, container, mouse_area, row, space, stack};
use iced::{Alignment, Background, Border, Color, Element, Length, Shadow, Theme, Vector};
use lucide_static_svg::Icon;

use crate::icons::icon;
use crate::motion::{CSS_TRANSITION, Tween};

const EDGE: f32 = 6.0;
const CORNER: f32 = 14.0;
const BAR: f32 = 32.0;
const BUTTON: f32 = 44.0;

#[derive(Debug, Clone, Copy)]
pub enum Message {
    Drag,
    /// Press on the drag strip. The window starts moving only after the pointer travels a few pixels,
    /// so a double click is not swallowed by the system move loop.
    DragArm,
    DragMove {
        x: f32,
        y: f32,
    },
    DragEnd,
    Resize(iced::window::Direction),
    Minimize,
    ToggleMaximize,
    ToggleFullscreen,
    Close,
    /// Pointer moved inside the window. `x` and `y` are logical pixels from the top left.
    Activity {
        x: f32,
        y: f32,
    },
    Left,
    /// Pointer over title-bar button `index`, or over none.
    Hover(Option<usize>),
    Frame,
}

fn resize_cursor(direction: iced::window::Direction) -> iced::mouse::Interaction {
    use iced::mouse::Interaction;
    use iced::window::Direction;
    match direction {
        Direction::North | Direction::South => Interaction::ResizingVertically,
        Direction::East | Direction::West => Interaction::ResizingHorizontally,
        Direction::NorthEast | Direction::SouthWest => Interaction::ResizingDiagonallyUp,
        Direction::NorthWest | Direction::SouthEast => Interaction::ResizingDiagonallyDown,
    }
}

/// Invisible resize strips over the page. They stay up when the rest of the chrome hides.
pub fn resize_frame<'a>() -> Element<'a, Message> {
    let edge = |direction| {
        mouse_area(space().width(Length::Fill).height(Length::Fill))
            .on_press(Message::Resize(direction))
            .interaction(resize_cursor(direction))
    };
    let horizontal = |direction| container(edge(direction)).width(Length::Fill).height(Length::Fixed(EDGE));
    let vertical = |direction| container(edge(direction)).width(Length::Fixed(EDGE)).height(Length::Fill);
    let corner = |direction| container(edge(direction)).width(Length::Fixed(CORNER)).height(Length::Fixed(CORNER));
    let place = |child, x, y| container(child).width(Length::Fill).height(Length::Fill).align_x(x).align_y(y);
    stack![
        place(horizontal(iced::window::Direction::North), Alignment::Start, Alignment::Start),
        place(horizontal(iced::window::Direction::South), Alignment::Start, Alignment::End),
        place(vertical(iced::window::Direction::West), Alignment::Start, Alignment::Start),
        place(vertical(iced::window::Direction::East), Alignment::End, Alignment::Start),
        place(corner(iced::window::Direction::NorthWest), Alignment::Start, Alignment::Start),
        place(corner(iced::window::Direction::NorthEast), Alignment::End, Alignment::Start),
        place(corner(iced::window::Direction::SouthWest), Alignment::Start, Alignment::End),
        place(corner(iced::window::Direction::SouthEast), Alignment::End, Alignment::End),
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// `content` that drags the window wherever none of its own controls takes the press.
pub fn drag_area<'a, M: Clone + 'a>(content: Element<'a, M>, wrap: fn(Message) -> M) -> Element<'a, M> {
    mouse_area(content)
        .on_press(wrap(Message::DragArm))
        .on_move(move |point| wrap(Message::DragMove { x: point.x, y: point.y }))
        .on_release(wrap(Message::DragEnd))
        .on_double_click(wrap(Message::ToggleMaximize))
        .into()
}

/// Pointer band at the top of the window that reveals the title bar, `TITLEBAR_REVEAL_HEIGHT`.
pub const REVEAL_HEIGHT: f32 = 56.0;
/// Backdrop fade from `AppShell.tsx`: 0.18s framer-motion `easeOut`.
const BACKDROP_MS: f64 = 180.0;
/// Button reveal and hover from `WindowControls.tsx`: Tailwind `transition-all duration-200`.
const BUTTON_MS: f64 = 200.0;
const BUTTONS: usize = 4;
/// Hidden buttons sit 4px higher, `-translate-y-1`.
const BUTTON_LIFT: f32 = 4.0;

/// The hidden title bar: fullscreen, minimize, maximize and close fade in when the pointer nears the top.
pub struct Titlebar {
    backdrop: Tween,
    reveal: Tween,
    hover: [Tween; BUTTONS],
}

/// What the title bar needs from the shell.
#[derive(Clone, Copy)]
pub struct TitlebarLook<'a> {
    pub i18n: &'a I18n,
    pub player: bool,
    pub daylight: bool,
    /// `--text-primary`; the daylight bar uses zinc-800 instead.
    pub ink: Color,
    pub maximized: bool,
    pub fullscreen: bool,
}

impl Titlebar {
    pub fn new(now: Instant) -> Self {
        let button = Tween::new(0.0, BUTTON_MS, CSS_TRANSITION, now);
        Self { backdrop: Tween::new(0.0, BACKDROP_MS, Ease::OUT, now), reveal: button, hover: [button; BUTTONS] }
    }

    /// `y` is the pointer's distance from the top, or `None` once it has left the window.
    pub fn pointer(&mut self, y: Option<f32>, now: Instant) {
        let revealed = if y.is_some_and(|y| y <= REVEAL_HEIGHT) { 1.0 } else { 0.0 };
        self.backdrop.set(revealed, now);
        self.reveal.set(revealed, now);
    }

    pub fn hover(&mut self, button: Option<usize>, now: Instant) {
        for (index, tween) in self.hover.iter_mut().enumerate() {
            tween.set(if Some(index) == button { 1.0 } else { 0.0 }, now);
        }
    }

    pub fn animating(&self, now: Instant) -> bool {
        self.backdrop.running(now) || self.reveal.running(now) || self.hover.iter().any(|tween| tween.running(now))
    }

    /// The 32px bar: a backdrop off the player page, the drag zone, and the buttons on the right.
    pub fn view<'a>(&self, look: TitlebarLook<'a>, now: Instant) -> Element<'a, Message> {
        let mut layers = stack![].width(Length::Fill).height(Length::Fixed(BAR));
        let backdrop = self.backdrop.value(now);
        // `titlebarBackdropClassName` with default settings: none on the player page, only a blur on dark pages.
        if !look.player && look.daylight && backdrop > 0.0 {
            layers = layers.push(daylight_backdrop(backdrop));
        }
        layers = layers.push(drag_area(space().width(Length::Fill).height(Length::Fill).into(), |message| message));
        let reveal = self.reveal.value(now);
        if reveal > 0.0 {
            let buttons = self.buttons(look, reveal, now);
            layers = layers.push(container(buttons).width(Length::Fill).align_x(Alignment::End));
        }
        container(layers).width(Length::Fill).height(Length::Fill).align_y(Alignment::Start).into()
    }

    fn buttons<'a>(&self, look: TitlebarLook<'a>, reveal: f32, now: Instant) -> Element<'a, Message> {
        let i18n = look.i18n;
        let (maximize_icon, maximize_key) =
            if look.maximized { (Icon::Copy, "ui.restoreWindow") } else { (Icon::Square, "ui.maximizeWindow") };
        let (fullscreen_icon, fullscreen_key) = if look.fullscreen {
            (Icon::Minimize, "ui.exitFullscreen")
        } else {
            (Icon::Maximize, "ui.enterFullscreen")
        };
        // Only fullscreen and maximize carry a `title` in the original.
        let specs = [
            (fullscreen_icon, 13.0, Some(i18n.t(fullscreen_key)), Message::ToggleFullscreen),
            (Icon::Minus, 16.0, None, Message::Minimize),
            (maximize_icon, 13.0, Some(i18n.t(maximize_key)), Message::ToggleMaximize),
            (Icon::X, 16.0, None, Message::Close),
        ];
        let mut buttons = row![];
        for (index, (lucide, size, title, message)) in specs.into_iter().enumerate() {
            let face = ButtonLook {
                reveal,
                hover: self.hover[index].value(now),
                daylight: look.daylight,
                ink: look.ink,
                close: index == BUTTONS - 1,
            };
            buttons = buttons.push(bar_button(index, lucide, size, title, message, face));
        }
        buttons.into()
    }
}

#[derive(Clone, Copy)]
struct ButtonLook {
    reveal: f32,
    hover: f32,
    daylight: bool,
    ink: Color,
    close: bool,
}

/// `bg-white/[0.12] border-b border-white/15`, faded by the backdrop's opacity.
fn daylight_backdrop<'a>(opacity: f32) -> Element<'a, Message> {
    let fill = Color { a: 0.12 * opacity, ..Color::WHITE };
    let line = Color { a: 0.15 * opacity, ..Color::WHITE };
    let body = container(space()).width(Length::Fill).height(Length::Fixed(BAR - 1.0));
    let edge = container(space())
        .width(Length::Fill)
        .height(Length::Fixed(1.0))
        .style(move |_| container::Style { background: Some(Background::Color(line)), ..container::Style::default() });
    container(iced::widget::column![body, edge])
        .width(Length::Fill)
        .height(Length::Fixed(BAR))
        .style(move |_| container::Style { background: Some(Background::Color(fill)), ..container::Style::default() })
        .into()
}

/// A 44×32 title-bar button. Shown it rests at 0.75 opacity (daylight 0.85) and hover lifts it to 1.
fn bar_button<'a>(
    index: usize,
    lucide: Icon,
    size: f32,
    title: Option<&'a str>,
    message: Message,
    look: ButtonLook,
) -> Element<'a, Message> {
    let rest = if look.daylight { 0.85 } else { 0.75 };
    let opacity = look.reveal * (rest + (1.0 - rest) * look.hover);
    let ink = if look.daylight { Color::from_rgb8(0x27, 0x27, 0x2a) } else { look.ink };
    let (fill, ink) = if look.close {
        (Color { a: look.hover, ..Color::from_rgb8(0xef, 0x44, 0x44) }, mix(ink, Color::WHITE, look.hover))
    } else if look.daylight {
        (Color { a: 0.05 * look.hover, ..Color::BLACK }, ink)
    } else {
        (Color { a: 0.10 * look.hover, ..Color::WHITE }, ink)
    };
    let fill = Color { a: fill.a * look.reveal, ..fill };
    // `-translate-y-1` while hidden: the glyph rides 4px higher, centred in a box with that much less below.
    let lift = BUTTON_LIFT * (1.0 - look.reveal);
    let glyph = container(icon(lucide, size, Color { a: opacity, ..ink }))
        .center(Length::Fill)
        .padding(iced::Padding { bottom: 2.0 * lift, ..iced::Padding::ZERO });
    let face = container(glyph)
        .center(Length::Fill)
        .style(move |_| container::Style { background: Some(Background::Color(fill)), ..container::Style::default() });
    // `pointer-events-none` while hidden.
    let shown = look.reveal > 0.5;
    let button = button(face)
        .width(Length::Fixed(BUTTON))
        .height(Length::Fixed(BAR))
        .padding(0)
        .on_press_maybe(shown.then_some(message))
        .style(|_, _| button::Style::default());
    let hovered = mouse_area(button).on_enter(Message::Hover(Some(index))).on_exit(Message::Hover(None));
    match title {
        Some(title) if shown => iced::widget::tooltip(
            hovered,
            iced::widget::text(title.to_owned()).size(12),
            iced::widget::tooltip::Position::Bottom,
        )
        .style(container::rounded_box)
        .into(),
        _ => hovered.into(),
    }
}

fn mix(from: Color, to: Color, t: f32) -> Color {
    Color {
        r: from.r + (to.r - from.r) * t,
        g: from.g + (to.g - from.g) * t,
        b: from.b + (to.b - from.b) * t,
        a: from.a + (to.a - from.a) * t,
    }
}

/// Catches pointer motion anywhere so the now-playing chrome can show itself. Presses fall through.
pub fn activity<'a>() -> Element<'a, Message> {
    mouse_area(space().width(Length::Fill).height(Length::Fill))
        .on_move(|point| Message::Activity { x: point.x, y: point.y })
        .on_exit(Message::Left)
        .into()
}

/// A round overlay button. `box_px` is the diameter, matching the original's fixed pixel sizes.
pub fn round_button<'a, M: Clone + 'a>(
    lucide: Icon,
    icon_px: f32,
    box_px: f32,
    fill: Color,
    icon_color: Color,
    label: &'a str,
    message: M,
) -> Element<'a, M> {
    let pad = ((box_px - icon_px) * 0.5).max(0.0);
    let radius = box_px * 0.5;
    let button =
        button(icon(lucide, icon_px, icon_color)).padding(pad).on_press(message).style(move |theme: &Theme, status| {
            let mut style = button::text(theme, status);
            let hot = status == button::Status::Hovered || status == button::Status::Pressed;
            style.background =
                Some(Background::Color(Color { a: if hot { (fill.a + 0.12).min(1.0) } else { fill.a }, ..fill }));
            style.border = Border { radius: radius.into(), width: 0.0, color: Color::TRANSPARENT };
            style.shadow =
                Shadow { color: Color { a: 0.28, ..Color::BLACK }, offset: Vector::new(0.0, 4.0), blur_radius: 12.0 };
            style
        });
    iced::widget::tooltip(button, iced::widget::text(label.to_owned()).size(12), iced::widget::tooltip::Position::Top)
        .style(container::rounded_box)
        .into()
}

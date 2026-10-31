// Frameless window: edge resizing, a drag strip, and the minimize / maximize / close buttons.

use folia_i18n::I18n;
use iced::widget::{button, container, mouse_area, row, space, stack};
use iced::{Alignment, Background, Border, Color, Element, Length, Shadow, Theme, Vector};
use lucide_static_svg::Icon;

use crate::icons::icon;

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

/// The top strip that drags the window. A double click toggles maximize, like a title bar.
pub fn drag_strip<'a>() -> Element<'a, Message> {
    let strip = mouse_area(space().width(Length::Fill).height(Length::Fill))
        .on_press(Message::DragArm)
        .on_move(|point| Message::DragMove { x: point.x, y: point.y })
        .on_release(Message::DragEnd)
        .on_double_click(Message::ToggleMaximize)
        .interaction(iced::mouse::Interaction::Grab);
    container(strip).width(Length::Fill).height(Length::Fixed(BAR)).align_y(Alignment::Start).into()
}

/// Fullscreen, minimize, maximize or restore, and close. Each hit target is 44×32.
pub fn controls<'a>(i18n: &'a I18n, color: Color, maximized: bool, fullscreen: bool) -> Element<'a, Message> {
    let (maximize_icon, maximize_key) =
        if maximized { (Icon::Copy, "ui.restoreWindow") } else { (Icon::Square, "ui.maximizeWindow") };
    let (fullscreen_icon, fullscreen_key) =
        if fullscreen { (Icon::Minimize, "ui.exitFullscreen") } else { (Icon::Maximize, "ui.enterFullscreen") };
    row![
        bar_button(fullscreen_icon, 13.0, color, i18n.t(fullscreen_key), Message::ToggleFullscreen, false),
        bar_button(Icon::Minus, 16.0, color, i18n.t("ui.minimizeWindow"), Message::Minimize, false),
        bar_button(maximize_icon, 13.0, color, i18n.t(maximize_key), Message::ToggleMaximize, false),
        bar_button(Icon::X, 16.0, color, i18n.t("ui.close"), Message::Close, true),
    ]
    .into()
}

/// A title-bar button whose whole 44×32 cell takes the click, not just the icon glyph.
fn bar_button<'a>(
    lucide: Icon,
    size: f32,
    color: Color,
    label: &'a str,
    message: Message,
    close: bool,
) -> Element<'a, Message> {
    let button = button(container(icon(lucide, size, color)).center(Length::Fill))
        .width(Length::Fixed(BUTTON))
        .height(Length::Fixed(BAR))
        .padding(0)
        .on_press(message)
        .style(move |theme: &Theme, status| {
            let mut style = button::text(theme, status);
            if close && (status == button::Status::Hovered || status == button::Status::Pressed) {
                style.background = Some(Background::Color(Color::from_rgb8(0xef, 0x44, 0x44)));
                style.text_color = Color::WHITE;
            }
            style
        });
    iced::widget::tooltip(
        button,
        iced::widget::text(label.to_owned()).size(12),
        iced::widget::tooltip::Position::Bottom,
    )
    .style(container::rounded_box)
    .into()
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

// The bottom-right wall tools button and its panel, from `WallToolsButton`.

use folia_render::Plate;
use iced::widget::slider::{Handle, HandleShape, Rail};
use iced::widget::{button, column, container, mouse_area, row, shader, space, stack, text};
use iced::{Alignment, Background, Border, Color, Element, Length, Theme};
use lucide_static_svg::Icon;

use super::{HomeChrome, Host, Message};
use crate::icons::icon;

const PANEL_H: f32 = 104.0;
const BUTTON: f32 = 48.0;
const GAP: f32 = 16.0;
const BLUR: f32 = 64.0;
const SHADOW_Y: f32 = 24.0;
/// A CSS blur radius is two sigmas; the Gaussian is spent three sigmas out.
const REACH: f32 = BLUR * 1.5;

pub fn dock<'a>(home: &'a HomeChrome, host: &Host<'a>, now: std::time::Instant) -> Element<'a, Message> {
    let open = home.tools(now);
    let edge = if host.window_width < 640.0 { 16.0 } else { 24.0 };
    let button = tool_button(host, open > 0.5);
    if open <= 0.02 {
        return pin_corner(button, edge);
    }
    let panel_w = panel_width(host);
    // Presses on the panel padding stay here. The dismiss layer is underneath.
    let column = mouse_area(
        column![panel_face(host, open, panel_w), space().height(GAP), button]
            .width(Length::Fixed(panel_w))
            .align_x(Alignment::End),
    )
    .on_press(Message::Frame)
    .interaction(iced::mouse::Interaction::Idle);
    stack![plate_layer(host, open, edge, panel_w), pin_corner(column, edge)].into()
}

fn pin_corner<'a>(body: impl Into<Element<'a, Message>>, edge: f32) -> Element<'a, Message> {
    container(body)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::End)
        .align_y(Alignment::End)
        .padding(iced::Padding { top: 0.0, right: edge, bottom: edge, left: 0.0 })
        .into()
}

/// Shadow room around the rounded fill: blur on every side, plus the 24px downward shift.
fn plate_layer<'a>(host: &Host<'a>, open: f32, edge: f32, panel_w: f32) -> Element<'a, Message> {
    let panel_right = host.window_width - edge;
    let panel_bottom = host.window_height - edge - BUTTON - GAP;
    let panel_left = panel_right - panel_w;
    let panel_top = panel_bottom - PANEL_H;
    let x0 = (panel_left - REACH).max(0.0);
    let y0 = (panel_top - (REACH - SHADOW_Y).max(0.0)).max(0.0);
    let x1 = (panel_right + REACH).min(host.window_width.max(1.0));
    let y1 = (panel_bottom + REACH + SHADOW_Y).min(host.window_height.max(1.0));
    let fill = if host.daylight { [1.0, 1.0, 1.0, 0.62 * open] } else { [0.0, 0.0, 0.0, 0.40 * open] };
    let shadow = if host.daylight {
        [42.0 / 255.0, 35.0 / 255.0, 27.0 / 255.0, 0.18 * open]
    } else {
        [0.0, 0.0, 0.0, 0.42 * open]
    };
    let plate = Plate {
        radius: 24.0,
        fill_origin: [panel_left - x0, panel_top - y0],
        fill_size: [panel_w, PANEL_H],
        fill,
        shadow,
        shadow_blur: BLUR,
        shadow_y: SHADOW_Y,
        shadow_spread: 0.0,
        ring: [0.0; 4],
    };
    let patch = shader(plate).width(Length::Fixed((x1 - x0).max(1.0))).height(Length::Fixed((y1 - y0).max(1.0)));
    container(patch)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(iced::Padding { top: y0, right: 0.0, bottom: 0.0, left: x0 })
        .into()
}

/// `WallBackButton` concealed: 40px round at `top: 20px; left: 24px` of the wall (`left: 16px` below 640px).
/// Hidden it is `opacity: 0; translateX(-6px) scale(0.92)` and takes no pointer; hover grows it to 105%.
pub fn back<'a>(host: &Host<'a>, shown: f32, hover: f32) -> Element<'a, Message> {
    if shown <= 0.0 {
        return space().into();
    }
    let fill = if host.daylight { Color { a: 0.05, ..Color::BLACK } } else { Color { a: 0.08, ..Color::WHITE } };
    let fill = Color { a: fill.a * shown, ..fill };
    let face = container(icon(Icon::ChevronLeft, 20.0, Color { a: host.text.a * shown, ..host.text }))
        .center(Length::Fixed(40.0))
        .style(move |_theme: &Theme| container::Style {
            background: Some(Background::Color(fill)),
            border: Border { radius: 20.0.into(), ..Border::default() },
            shadow: iced::Shadow {
                color: Color { a: 0.24 * shown, ..Color::BLACK },
                offset: iced::Vector::new(0.0, 10.0),
                blur_radius: 30.0,
            },
            ..container::Style::default()
        });
    let revealed = host.has_track && shown > 0.5;
    let button = button(face)
        .padding(0)
        .on_press_maybe(revealed.then_some(Message::Player))
        .style(|_theme: &Theme, _status| button::Style::default());
    let button = mouse_area(button).on_enter(Message::BackHover(true)).on_exit(Message::BackHover(false));
    let scale = (0.92 + 0.08 * shown) * (1.0 + 0.05 * hover);
    let shift = iced::Vector::new(-6.0 * (1.0 - shown), 0.0);
    let button: Element<'a, Message> = iced::widget::float(button).translate(move |_, _| shift).scale(scale).into();
    let (top, left) = if host.window_width < 640.0 { (14.0, 16.0) } else { (20.0, 24.0) };
    container(button)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::Start)
        .align_y(Alignment::Start)
        .padding(iced::Padding { top, right: 0.0, bottom: 0.0, left })
        .into()
}

fn panel_face<'a>(host: &Host<'a>, open: f32, panel_w: f32) -> Element<'a, Message> {
    let border = if host.daylight {
        Color { a: 0.07 * open, ..Color::BLACK }
    } else {
        Color { r: 1.0, g: 1.0, b: 1.0, a: 0.08 * open }
    };
    let ink = Color { a: host.text.a * open, ..host.text };
    let inner = column![quick_row(host, ink, panel_w), volume_row(host, ink)]
        .spacing(6)
        .padding(10)
        .width(Length::Fixed(panel_w));
    container(inner)
        .width(Length::Fixed(panel_w))
        .height(Length::Fixed(PANEL_H))
        .style(move |_theme: &Theme| container::Style {
            border: Border { radius: 24.0.into(), width: 1.0, color: border },
            ..container::Style::default()
        })
        .into()
}

fn panel_width(host: &Host<'_>) -> f32 {
    (host.window_width - 32.0).clamp(0.0, 296.0)
}

fn quick_row<'a>(host: &Host<'a>, ink: Color, panel_w: f32) -> Element<'a, Message> {
    let cell = ((panel_w - 20.0) - 4.0 * 5.0) / 6.0;
    // Column 4 is the Lattice destination. It stays empty. Shuffle, theme and queue have no action here.
    row![
        quick(Icon::Shuffle, cell, ink, host.daylight, None),
        quick(Icon::Sparkles, cell, ink, host.daylight, None),
        quick(Icon::ListMusic, cell, ink, host.daylight, None),
        space().width(Length::Fixed(cell)).height(Length::Fixed(40.0)),
        quick(Icon::Disc3, cell, ink, host.daylight, host.has_track.then_some(Message::Player)),
        quick(Icon::Settings, cell, ink, host.daylight, Some(Message::Settings)),
    ]
    .spacing(4)
    .into()
}

fn volume_row<'a>(host: &Host<'a>, ink: Color) -> Element<'a, Message> {
    let shown = if host.muted { 0.0 } else { host.volume };
    // `volumeIcon`: by the shown value, 0 / under half / the rest.
    let icon_kind = if shown <= 0.0 {
        Icon::VolumeX
    } else if shown < 0.5 {
        Icon::Volume1
    } else {
        Icon::Volume2
    };
    let icon_color = if host.muted { host.accent } else { ink };
    let label = if host.muted {
        super::tr(host, "wallTools.muted").to_owned()
    } else {
        format!("{}%", (host.volume * 100.0).round() as i32)
    };
    let track = Color { a: 0.18 * ink.a, ..ink };
    let accent = Color { a: ink.a, ..host.accent };
    let value_color = Color { a: 0.66 * ink.a, ..ink };
    let row = row![
        button(container(icon(icon_kind, 17.0, icon_color)).center(Length::Fill))
            .width(Length::Fixed(30.0))
            .height(Length::Fixed(30.0))
            .padding(0)
            .on_press(Message::Mute)
            .style(move |_theme: &Theme, status| {
                let hot = matches!(status, button::Status::Hovered | button::Status::Pressed);
                button::Style {
                    background: Some(Background::Color(if hot {
                        Color { a: 0.10, ..ink }
                    } else {
                        Color::TRANSPARENT
                    })),
                    border: Border { radius: 9.0.into(), ..Border::default() },
                    ..button::Style::default()
                }
            }),
        iced::widget::slider(0.0..=1.0, shown, Message::Volume).step(0.01_f32).on_release(Message::VolumeDone).style(
            move |_theme: &Theme, _status| iced::widget::slider::Style {
                rail: Rail {
                    backgrounds: (Background::Color(accent), Background::Color(track)),
                    width: 4.0,
                    border: Border { radius: 2.0.into(), width: 0.0, color: Color::TRANSPARENT },
                },
                handle: Handle {
                    shape: HandleShape::Circle { radius: 7.0 },
                    background: Background::Color(ink),
                    border_width: 1.0,
                    border_color: Color { a: 0.22, ..Color::BLACK },
                },
            }
        ),
        text(label).size(12).color(value_color).font(semibold()).width(Length::Fixed(38.0)).align_x(Alignment::End),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .padding(iced::Padding { top: 0.0, right: 12.0, bottom: 0.0, left: 4.0 })
    .height(Length::Fixed(38.0));
    let fill = if host.daylight {
        Color { a: 0.05 * ink.a, ..Color::BLACK }
    } else {
        Color { r: 1.0, g: 1.0, b: 1.0, a: 0.06 * ink.a }
    };
    container(row)
        .style(move |_theme: &Theme| container::Style {
            background: Some(Background::Color(fill)),
            border: Border { radius: 12.0.into(), ..Border::default() },
            ..container::Style::default()
        })
        .into()
}

fn tool_button<'a>(host: &Host<'a>, open: bool) -> Element<'a, Message> {
    let (fill, icon_color) = if host.daylight {
        (Color { r: 1.0, g: 1.0, b: 1.0, a: 0.7 }, host.text)
    } else {
        (Color { a: 0.4, ..Color::BLACK }, Color::WHITE)
    };
    let icon = if open { Icon::X } else { Icon::Settings2 };
    round_button(icon, 20.0, BUTTON, fill, icon_color, Some(Message::Tools))
}

fn quick<'a>(lucide: Icon, cell: f32, ink: Color, daylight: bool, on_press: Option<Message>) -> Element<'a, Message> {
    let enabled = on_press.is_some();
    let color = if enabled { ink } else { Color { a: ink.a * 0.35, ..ink } };
    button(container(icon(lucide, 18.0, color)).center(Length::Fill))
        .width(Length::Fixed(cell))
        .height(Length::Fixed(40.0))
        .padding(0)
        .on_press_maybe(on_press)
        .style(move |_theme: &Theme, status| {
            let hot = enabled && matches!(status, button::Status::Hovered | button::Status::Pressed);
            let idle = if daylight { 0.05 } else { 0.06 };
            let hover = if daylight { 0.10 } else { 0.13 };
            button::Style {
                background: Some(Background::Color(Color {
                    a: if hot { hover } else { idle },
                    ..if daylight { Color::BLACK } else { Color::WHITE }
                })),
                border: Border { radius: 12.0.into(), ..Border::default() },
                ..button::Style::default()
            }
        })
        .into()
}

fn round_button<'a>(
    lucide: Icon,
    icon_px: f32,
    box_px: f32,
    fill: Color,
    icon_color: Color,
    on_press: Option<Message>,
) -> Element<'a, Message> {
    let pad = ((box_px - icon_px) * 0.5).max(0.0);
    button(icon(lucide, icon_px, icon_color))
        .padding(pad)
        .on_press_maybe(on_press)
        .style(move |_theme: &Theme, status| {
            let hot = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: Some(Background::Color(Color {
                    a: if hot { (fill.a + 0.08).min(1.0) } else { fill.a },
                    ..fill
                })),
                border: Border { radius: (box_px * 0.5).into(), ..Border::default() },
                shadow: iced::Shadow {
                    color: Color { a: 0.10, ..Color::BLACK },
                    offset: iced::Vector::new(0.0, 10.0),
                    blur_radius: 15.0,
                },
                ..button::Style::default()
            }
        })
        .into()
}

fn semibold() -> iced::Font {
    crate::typeface::font(iced::font::Weight::Semibold)
}

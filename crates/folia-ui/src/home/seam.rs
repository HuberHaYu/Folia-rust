// The home seam: 120px, or a 64px spine, from `bravais.css` and `bravaisHome.css`.

use std::time::Instant;

use folia_render::Plate;
use iced::widget::{button, column, container, row, shader, space, stack, text};
use iced::{Alignment, Background, Border, Color, Element, Length, Theme};
use lucide_static_svg::Icon;

use super::{HomeChrome, Host, Message};
use crate::icons::icon;

const SHADOW: f32 = 64.0;

/// Narrow seam, or the spine while `width` is near 64, standing where the wall opened it.
pub fn bar<'a>(home: &'a HomeChrome, host: &Host<'a>, width: f32) -> Element<'a, Message> {
    let spine = width < 90.0;
    let left = host.seam.center - width / 2.0;
    let ink = host.text;
    let paper = mix_paper(host);
    let shadow = seam_shadow(host);
    // `--bravais-seam-shadow`: a 1px ring of 6% (daylight 8%) text, then `0 24px 64px`.
    let ring = Color { a: if host.daylight { 0.08 } else { 0.06 }, ..ink };
    let plate = Plate {
        radius: 0.0,
        fill_origin: [left, 0.0],
        fill_size: [width, 0.0],
        fill: [paper.r, paper.g, paper.b, 1.0],
        shadow,
        shadow_blur: SHADOW,
        shadow_y: 24.0,
        shadow_spread: 0.0,
        ring: [ring.r, ring.g, ring.b, ring.a],
    };
    let body = seam_body(home, host, spine, ink);
    let content = container(container(body).width(Length::Fixed(width)).height(Length::Fill))
        .padding(iced::Padding { left: left.max(0.0), ..iced::Padding::ZERO });
    // The plate spans the window so the shadow can fall on both sides of the seam.
    stack![shader(plate).width(Length::Fill).height(Length::Fill), content].into()
}

fn seam_body<'a>(home: &'a HomeChrome, host: &Host<'a>, spine: bool, ink: Color) -> Element<'a, Message> {
    let (pad_y, pad_x, gap) = if spine { (16.0, 8.0, 12.0) } else { (22.0, 10.0, 18.0) };
    let title_size = if spine { 12.0 } else { 15.0 };
    let title_track = if spine { 0.08 } else { 0.3 };
    let title = tracked_row(super::tr(host, "libraryBravais.homeTitle"), title_size, title_track, title_track, ink);
    let fold_icon = if home.spine { Icon::ChevronsRight } else { Icon::ChevronsLeft };
    let fold_key = super::tr(host, if home.spine { "libraryBravais.seamExpand" } else { "libraryBravais.seamFold" });
    let tab_size = if spine { 12.0 } else { 14.0 };
    let tab_track = if spine { 0.1 } else { 0.2 };
    let tabs = tab(super::tr(host, "search.sourceLocal"), tab_size, tab_track, ink);
    let head = column![title, ghost(fold_icon, 16.0, 36.0, ink, Some(Message::Fold), fold_key), tabs]
        .spacing(8)
        .align_x(Alignment::Center);
    let tools = if spine { tool_column(ink) } else { tool_grid(ink) };
    column![head, space().height(Length::Fill), tools]
        .spacing(gap)
        .padding(iced::Padding { top: pad_y, right: pad_x, bottom: 0.0, left: pad_x })
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::Center)
        .into()
}

fn tool_grid<'a>(ink: Color) -> Element<'a, Message> {
    column![
        row![
            tool(Icon::Search, 40.0, ink, Some(Message::Search), false),
            tool(Icon::Settings, 40.0, ink, Some(Message::Settings), false),
        ]
        .spacing(8),
        row![
            tool(Icon::PanelsTopLeft, 40.0, ink, None, true),
            tool(Icon::Ellipsis, 40.0, ink, Some(Message::More), false),
        ]
        .spacing(8),
    ]
    .spacing(8)
    .align_x(Alignment::Center)
    .into()
}

/// The ⋯ menu, above the dismiss layer so its rows can be pressed.
pub fn popup<'a>(home: &'a HomeChrome, host: &Host<'a>, width: f32, now: Instant) -> Option<Element<'a, Message>> {
    let shown = home.menu(now);
    if shown <= 0.02 || width < 90.0 {
        return None;
    }
    let pad_x = 10.0;
    let grid_left = pad_x + (width - pad_x * 2.0 - 88.0) / 2.0;
    let menu_left = host.seam.center - width / 2.0 + grid_left - 6.0;
    let bottom = 88.0 + 8.0 * shown;
    let menu = menu(host, host.text, shown);
    Some(
        container(menu)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::Start)
            .align_y(Alignment::End)
            .padding(iced::Padding { top: 0.0, right: 0.0, bottom, left: menu_left })
            .into(),
    )
}

fn tool_column<'a>(ink: Color) -> Element<'a, Message> {
    column![
        tool(Icon::Search, 36.0, ink, Some(Message::Search), false),
        tool(Icon::Settings, 36.0, ink, Some(Message::Settings), false),
        tool(Icon::PanelsTopLeft, 36.0, ink, None, true),
        tool(Icon::Ellipsis, 36.0, ink, Some(Message::More), false),
    ]
    .spacing(6)
    .align_x(Alignment::Center)
    .into()
}

fn menu<'a>(host: &Host<'a>, ink: Color, shown: f32) -> Element<'a, Message> {
    let paper = mix_paper(host);
    let import = (!host.busy).then_some(Message::Import);
    let reload = (!host.busy).then_some(Message::Reload);
    let player = host.has_track.then_some(Message::Player);
    let mut rows = Vec::new();
    if host.has_track {
        rows.push(menu_row(super::tr(host, "libraryBravaisHome.backToPlayer"), player, ink, shown));
    }
    rows.push(menu_row(super::tr(host, "localMusic.importFolder"), import, ink, shown));
    rows.push(menu_row(super::tr(host, "localMusic.reload"), reload, ink, shown));
    let list = column(rows).spacing(0).width(Length::Fill);
    // 6px wider than the 88px tool grid on each side, 8px above it (the column spacing).
    container(list)
        .padding(4)
        .width(Length::Fixed(100.0))
        .style(move |_theme: &Theme| container::Style {
            background: Some(Background::Color(Color { a: 0.92 * shown, ..paper })),
            border: Border { radius: 12.0.into(), width: 1.0, color: Color { a: 0.14 * shown, ..ink } },
            ..container::Style::default()
        })
        .into()
}

fn menu_row<'a>(label: &'a str, on_press: Option<Message>, ink: Color, shown: f32) -> Element<'a, Message> {
    let color = Color { a: ink.a * shown * if on_press.is_some() { 1.0 } else { 0.35 }, ..ink };
    button(text(label).size(12).color(color))
        .width(Length::Fill)
        .padding([7, 6])
        .style(button::text)
        .on_press_maybe(on_press)
        .into()
}

fn tab<'a>(label: &'a str, size: f32, tracking: f32, ink: Color) -> Element<'a, Message> {
    // The only source Folia-rust can show. Pressing it does not leave the wall.
    container(tracked_col(label, size, tracking, ink))
        .padding([10, 4])
        .style(move |_theme: &Theme| container::Style {
            background: Some(Background::Color(Color { a: 0.14, ..ink })),
            border: Border { radius: 12.0.into(), ..Border::default() },
            ..container::Style::default()
        })
        .into()
}

fn tool<'a>(lucide: Icon, box_px: f32, ink: Color, on_press: Option<Message>, disabled: bool) -> Element<'a, Message> {
    let color = if disabled { Color { a: 0.3, ..ink } } else { ink };
    ghost(lucide, 18.0, box_px, color, on_press.filter(|_| !disabled), "")
}

fn ghost<'a>(
    lucide: Icon,
    icon_px: f32,
    box_px: f32,
    ink: Color,
    on_press: Option<Message>,
    _label: &'a str,
) -> Element<'a, Message> {
    button(container(icon(lucide, icon_px, ink)).center(Length::Fill))
        .width(Length::Fixed(box_px))
        .height(Length::Fixed(box_px))
        .padding(0)
        .on_press_maybe(on_press)
        .style(move |_theme: &Theme, status| {
            let hot = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: Some(Background::Color(if hot { Color { a: 0.16, ..ink } } else { Color::TRANSPARENT })),
                border: Border {
                    radius: 12.0.into(),
                    width: 1.0,
                    color: if hot { Color { a: 0.26, ..ink } } else { Color::TRANSPARENT },
                },
                ..button::Style::default()
            }
        })
        .into()
}

fn tracked_row<'a>(label: &'a str, size: f32, tracking: f32, indent: f32, color: Color) -> Element<'a, Message> {
    let chars = label
        .chars()
        .map(|ch| text(ch.to_string()).size(size).color(color).font(extra_bold()).into())
        .collect::<Vec<_>>();
    row(chars)
        .spacing(size * tracking)
        .padding(iced::Padding { top: 0.0, right: 0.0, bottom: 0.0, left: size * indent })
        .into()
}

fn tracked_col<'a>(label: &'a str, size: f32, tracking: f32, color: Color) -> Element<'a, Message> {
    let chars =
        label.chars().map(|ch| text(ch.to_string()).size(size).color(color).font(bold()).into()).collect::<Vec<_>>();
    column(chars).spacing(size * tracking).align_x(Alignment::Center).into()
}

fn extra_bold() -> iced::Font {
    iced::Font { weight: iced::font::Weight::ExtraBold, ..iced::Font::DEFAULT }
}

fn bold() -> iced::Font {
    iced::Font { weight: iced::font::Weight::Bold, ..iced::Font::DEFAULT }
}

fn mix_paper(host: &Host<'_>) -> Color {
    let bg = host.background;
    let ink = host.text;
    Color { r: bg.r * 0.9 + ink.r * 0.1, g: bg.g * 0.9 + ink.g * 0.1, b: bg.b * 0.9 + ink.b * 0.1, a: 1.0 }
}

fn seam_shadow(host: &Host<'_>) -> [f32; 4] {
    if host.daylight { [42.0 / 255.0, 35.0 / 255.0, 27.0 / 255.0, 0.18] } else { [0.0, 0.0, 0.0, 0.42] }
}

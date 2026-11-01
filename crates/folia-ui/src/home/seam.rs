// The home seam: a 120px strip, a 64px spine, or an edge tab, from `bravaisHome.css`.

use std::time::Instant;

use folia_render::Plate;
use iced::widget::{button, column, container, mouse_area, row, shader, space, stack, text};
use iced::{Alignment, Background, Border, Color, Element, Length, Theme, Vector};
use lucide_static_svg::Icon;

use super::{HomeChrome, Host, Message};
use crate::icons::icon;

const SHADOW: f32 = 64.0;
const LAYOUT_NARROW: f32 = 120.0;
const LAYOUT_SPINE: f32 = 64.0;

/// The paper strip where the wall opened it, plus the edge tab when the strip is shut.
pub fn bar<'a>(_home: &'a HomeChrome, host: &Host<'a>, _now: Instant) -> Element<'a, Message> {
    let mut layers = iced::widget::stack![];
    if host.seam.width >= 1.0 {
        layers = layers.push(strip(host));
    }
    if host.show_tab {
        layers = layers.push(tab_button(host));
    }
    layers.into()
}

fn strip<'a>(host: &Host<'a>) -> Element<'a, Message> {
    let horizontal = host.horizontal;
    let ink = fade(host.text, host.opacity);
    let paper = mix_paper(host);
    let ring = Color { a: if host.daylight { 0.08 } else { 0.06 }, ..ink };
    let (origin, size) = plate_box(host);
    let plate = Plate {
        radius: 0.0,
        fill_origin: origin,
        fill_size: size,
        fill: [paper.r, paper.g, paper.b, 1.0],
        shadow: seam_shadow(host),
        shadow_blur: SHADOW,
        shadow_y: 24.0,
        shadow_spread: 0.0,
        ring: [ring.r, ring.g, ring.b, ring.a],
    };
    let body = if horizontal { row_body(host, ink) } else { column_body(host, ink) };
    let clipped = clip_body(host, body);
    stack![shader(plate).width(Length::Fill).height(Length::Fill), clipped].into()
}

fn plate_box(host: &Host<'_>) -> ([f32; 2], [f32; 2]) {
    if host.horizontal {
        let top = host.seam.center - host.seam.width / 2.0;
        ([0.0, top], [host.window_width, host.seam.width])
    } else {
        let left = host.seam.center - host.seam.width / 2.0;
        ([left, 0.0], [host.seam.width, 0.0])
    }
}

/// Lays the content out at its own width and clips it to the opening, centred on the seam.
fn clip_body<'a>(host: &Host<'a>, body: Element<'a, Message>) -> Element<'a, Message> {
    let (clip_origin, clip_size, shift) = clip_frame(host);
    let shifted = iced::widget::float(body).translate(move |_, _| shift);
    let clipped = container(shifted).width(Length::Fixed(clip_size.0)).height(Length::Fixed(clip_size.1)).clip(true);
    let seam = mouse_area(clipped).on_press(Message::Frame).interaction(iced::mouse::Interaction::Idle);
    container(seam)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::Start)
        .align_y(Alignment::Start)
        .padding(iced::Padding { top: clip_origin.1.max(0.0), right: 0.0, bottom: 0.0, left: clip_origin.0.max(0.0) })
        .into()
}

fn clip_frame(host: &Host<'_>) -> ((f32, f32), (f32, f32), Vector) {
    if host.horizontal {
        let top = host.seam.center - host.seam.width / 2.0;
        let clip_top = top.max(0.0);
        let height = (host.seam.width - (clip_top - top)).max(1.0);
        let layout = if host.spine { 52.0 } else { 72.0 };
        let shift = Vector::new(0.0, host.seam.center - layout / 2.0 - clip_top);
        ((0.0, clip_top), (host.window_width.max(1.0), height), shift)
    } else {
        let left = host.seam.center - host.seam.width / 2.0;
        let clip_left = left.max(0.0);
        let width = (host.seam.width - (clip_left - left)).max(1.0);
        let layout = if host.spine { LAYOUT_SPINE } else { LAYOUT_NARROW };
        let shift = Vector::new(host.seam.center - layout / 2.0 - clip_left, 0.0);
        ((clip_left, 0.0), (width, host.window_height.max(1.0)), shift)
    }
}

fn column_body<'a>(host: &Host<'a>, ink: Color) -> Element<'a, Message> {
    let spine = host.spine;
    let (pad_y, pad_x, gap) = if spine { (14.0, 8.0, 12.0) } else { (16.0, 10.0, 18.0) };
    let title_size = if spine { 12.0 } else { 15.0 };
    let title_track = if spine { 0.08 } else { 0.3 };
    let title = tracked_row(super::tr(host, "libraryBravais.homeTitle"), title_size, title_track, title_track, ink);
    let fold = ghost(Icon::FoldHorizontal, 16.0, 36.0, ink, Some(Message::Fold));
    let tab_size = if spine { 12.0 } else { 14.0 };
    let tab_track = if spine { 0.1 } else { 0.18 };
    let tab_pad = if spine { (7.0, 3.0) } else { (9.0, 4.0) };
    let tabs = source_tab(super::tr(host, "search.sourceLocal"), tab_size, tab_track, tab_pad, true, ink);
    let head = column![title, fold, tabs].spacing(8).align_x(Alignment::Center);
    let tools = if spine { tool_column(ink) } else { tool_grid(ink) };
    let middle = container(status_col(host, ink)).width(Length::Fill).height(Length::Fill).align_y(Alignment::Center);
    let mut body = column![head, middle, tools].spacing(gap).align_x(Alignment::Center);
    if spine {
        body = body.push(ghost(Icon::Maximize2, 16.0, 36.0, ink, Some(Message::Expand)));
    }
    body.padding(iced::Padding { top: pad_y, right: pad_x, bottom: host.bottom, left: pad_x })
        .width(Length::Fixed(if spine { LAYOUT_SPINE } else { LAYOUT_NARROW }))
        .height(Length::Fill)
        .into()
}

fn row_body<'a>(host: &Host<'a>, ink: Color) -> Element<'a, Message> {
    let spine = host.spine;
    let gap = if spine { 10.0 } else { 14.0 };
    let pad_x = (host.window_width * 0.024).max(20.0);
    let fold = ghost(Icon::FoldHorizontal, 16.0, if spine { 34.0 } else { 36.0 }, ink, Some(Message::Fold));
    let tab_pad = if spine { (5.0, 9.0) } else { (8.0, 12.0) };
    let tab_size = if spine { 12.0 } else { 14.0 };
    let tab_track = if spine { 0.06 } else { 0.12 };
    let tabs = source_tab(super::tr(host, "search.sourceLocal"), tab_size, tab_track, tab_pad, false, ink);
    let tools = tool_row(ink, spine);
    let mut items: Vec<Element<'a, Message>> = Vec::new();
    items.push(fold);
    if !spine {
        items.push(tracked_row(super::tr(host, "libraryBravais.homeTitle"), 15.0, 0.3, 0.3, ink));
    }
    items.push(tabs);
    if let Some(status) = host.status {
        let soft = Color { a: ink.a * 0.55, ..ink };
        items.push(text(status).size(12).color(soft).into());
    }
    items.push(space().width(Length::Fill).into());
    items.push(tools);
    if spine {
        items.push(ghost(Icon::Maximize2, 16.0, 34.0, ink, Some(Message::Expand)));
    }
    row(items)
        .spacing(gap)
        .align_y(Alignment::Center)
        .padding(iced::Padding { top: 0.0, right: pad_x, bottom: 0.0, left: pad_x })
        .width(Length::Fill)
        .height(Length::Fixed(if spine { 52.0 } else { 72.0 }))
        .into()
}

fn status_col<'a>(host: &Host<'a>, ink: Color) -> Element<'a, Message> {
    let Some(status) = host.status else { return space().into() };
    let soft = Color { a: ink.a * 0.55, ..ink };
    container(tracked_col(status, 11.0, 0.1, soft)).width(Length::Fill).align_x(Alignment::Center).into()
}

/// The ⋯ menu, above the dismiss layer so its rows can be pressed.
pub fn popup<'a>(home: &'a HomeChrome, host: &Host<'a>, now: Instant) -> Option<Element<'a, Message>> {
    let shown = home.menu(now);
    if shown <= 0.02 || host.spine || host.seam.width < 90.0 {
        return None;
    }
    let menu = menu(host, fade(host.text, shown), shown);
    if host.horizontal {
        return Some(row_popup(host, menu));
    }
    let layout = LAYOUT_NARROW;
    let grid_left = (layout - 88.0) / 2.0;
    let menu_left = host.seam.center - layout / 2.0 + grid_left - 6.0;
    let bottom = host.bottom + 88.0 + 8.0 * shown;
    Some(
        container(menu)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::Start)
            .align_y(Alignment::End)
            .padding(iced::Padding { top: 0.0, right: 0.0, bottom, left: menu_left.max(0.0) })
            .into(),
    )
}

fn row_popup<'a>(host: &Host<'a>, menu: Element<'a, Message>) -> Element<'a, Message> {
    let top = host.seam.center - host.seam.width / 2.0;
    let menu_below = host.seam.before;
    let pad = if menu_below {
        iced::Padding { top: (top + host.seam.width + 8.0).max(0.0), right: 20.0, bottom: 0.0, left: 0.0 }
    } else {
        iced::Padding { top: 0.0, right: 20.0, bottom: (host.window_height - top + 8.0).max(0.0), left: 0.0 }
    };
    container(menu)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::End)
        .align_y(if menu_below { Alignment::Start } else { Alignment::End })
        .padding(pad)
        .into()
}

fn tab_button<'a>(host: &Host<'a>) -> Element<'a, Message> {
    let ink = host.text;
    let paper = mix_paper(host);
    let shadow = seam_shadow(host);
    let radius = if host.horizontal || host.seam.before {
        iced::border::Radius::default().top_right(6.0).bottom_right(6.0)
    } else {
        iced::border::Radius::default().top_left(6.0).bottom_left(6.0)
    };
    let label = tracked_col(super::tr(host, "libraryBravais.homeTitle"), 13.0, 0.12, ink);
    let face = container(label).padding([14.0, 8.0]).style(move |_theme: &Theme| container::Style {
        background: Some(Background::Color(paper)),
        border: Border { radius, ..Border::default() },
        shadow: iced::Shadow {
            color: Color { a: shadow[3], r: shadow[0], g: shadow[1], b: shadow[2] },
            offset: Vector::new(0.0, 24.0),
            blur_radius: 64.0,
        },
        ..container::Style::default()
    });
    let tip =
        super::tr(host, if host.tab_hidden { "libraryBravais.seamRestore" } else { "libraryBravais.seamReopenHere" });
    let button = iced::widget::Tooltip::new(
        button(face).padding(0).on_press(Message::Tab).style(button::text),
        text(tip).size(12),
        iced::widget::tooltip::Position::Right,
    );
    let travel = 28.0 * (1.0 - host.tab);
    let shift = Vector::new(if host.seam.before { -travel } else { travel }, 0.0);
    let button = iced::widget::float(button).translate(move |_, _| shift);
    let side = if host.horizontal || host.seam.before { Alignment::Start } else { Alignment::End };
    container(button).width(Length::Fill).height(Length::Fill).align_x(side).align_y(Alignment::Center).into()
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

fn tool_row<'a>(ink: Color, spine: bool) -> Element<'a, Message> {
    let box_px = if spine { 34.0 } else { 38.0 };
    row![
        tool(Icon::Search, box_px, ink, Some(Message::Search), false),
        tool(Icon::Settings, box_px, ink, Some(Message::Settings), false),
        tool(Icon::PanelsTopLeft, box_px, ink, None, true),
        tool(Icon::Ellipsis, box_px, ink, Some(Message::More), false),
    ]
    .spacing(6)
    .into()
}

fn menu<'a>(host: &Host<'a>, ink: Color, shown: f32) -> Element<'a, Message> {
    let paper = mix_paper(host);
    let import = (!host.busy).then_some(Message::Import);
    let reload = (!host.busy).then_some(Message::Reload);
    let player = host.has_track.then_some(Message::Player);
    let mut rows = vec![
        menu_row(super::tr(host, "localMusic.importFolder"), import, ink, shown),
        menu_row(super::tr(host, "options.refresh"), reload, ink, shown),
    ];
    if host.has_track {
        rows.push(menu_row(super::tr(host, "libraryBravaisHome.backToPlayer"), player, ink, shown));
    }
    container(column(rows).spacing(0).width(Length::Fill))
        .padding(4)
        .width(Length::Fixed(160.0))
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

fn source_tab<'a>(
    label: &'a str,
    size: f32,
    tracking: f32,
    pad: (f32, f32),
    vertical: bool,
    ink: Color,
) -> Element<'a, Message> {
    let face: Element<'a, Message> =
        if vertical { tracked_col(label, size, tracking, ink) } else { tracked_row(label, size, tracking, 0.0, ink) };
    container(face)
        .padding([pad.0, pad.1])
        .style(move |_theme: &Theme| container::Style {
            background: Some(Background::Color(Color { a: 0.14 * ink.a, ..ink })),
            border: Border { radius: 10.0.into(), ..Border::default() },
            ..container::Style::default()
        })
        .into()
}

fn tool<'a>(lucide: Icon, box_px: f32, ink: Color, on_press: Option<Message>, disabled: bool) -> Element<'a, Message> {
    let color = if disabled { Color { a: ink.a * 0.3, ..ink } } else { ink };
    ghost(lucide, 18.0, box_px, color, on_press.filter(|_| !disabled))
}

fn ghost<'a>(lucide: Icon, icon_px: f32, box_px: f32, ink: Color, on_press: Option<Message>) -> Element<'a, Message> {
    button(container(icon(lucide, icon_px, ink)).center(Length::Fill))
        .width(Length::Fixed(box_px))
        .height(Length::Fixed(box_px))
        .padding(0)
        .on_press_maybe(on_press)
        .style(move |_theme: &Theme, status| {
            let hot = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: Some(Background::Color(if hot {
                    Color { a: 0.16 * ink.a, ..ink }
                } else {
                    Color::TRANSPARENT
                })),
                border: Border {
                    radius: 12.0.into(),
                    width: 1.0,
                    color: if hot { Color { a: 0.26 * ink.a, ..ink } } else { Color::TRANSPARENT },
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

fn fade(color: Color, opacity: f32) -> Color {
    Color { a: color.a * opacity, ..color }
}

fn mix_paper(host: &Host<'_>) -> Color {
    let bg = host.background;
    let ink = host.text;
    Color { r: bg.r * 0.9 + ink.r * 0.1, g: bg.g * 0.9 + ink.g * 0.1, b: bg.b * 0.9 + ink.b * 0.1, a: 1.0 }
}

fn seam_shadow(host: &Host<'_>) -> [f32; 4] {
    if host.daylight { [42.0 / 255.0, 35.0 / 255.0, 27.0 / 255.0, 0.18] } else { [0.0, 0.0, 0.0, 0.42] }
}

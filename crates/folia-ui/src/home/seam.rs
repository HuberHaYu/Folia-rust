// The home seam: a 120px strip, a 64px spine, or an edge tab, from `bravaisHome.css`.

use std::time::Instant;

use folia_render::Plate;
use iced::widget::{button, column, container, mouse_area, row, shader, space, stack, text};
use iced::{Alignment, Background, Border, Color, Element, Length, Theme, Vector};
use lucide_static_svg::Icon;

use super::{AccountSlot, HomeChrome, Host, Message};
use crate::icons::icon;

const SHADOW: f32 = 64.0;
const LAYOUT_NARROW: f32 = 120.0;
const LAYOUT_SPINE: f32 = 64.0;
/// `BRAVAIS_SEAM_FULL_WIDTH`.
const LAYOUT_FULL: f32 = 300.0;

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
        let layout = row_layout(host);
        let shift = Vector::new(0.0, host.seam.center - layout / 2.0 - clip_top);
        ((0.0, clip_top), (host.window_width.max(1.0), height), shift)
    } else {
        let left = host.seam.center - host.seam.width / 2.0;
        let clip_left = left.max(0.0);
        let width = (host.seam.width - (clip_left - left)).max(1.0);
        let layout = column_layout(host);
        let shift = Vector::new(host.seam.center - layout / 2.0 - clip_left, 0.0);
        ((clip_left, 0.0), (width, host.window_height.max(1.0)), shift)
    }
}

fn column_layout(host: &Host<'_>) -> f32 {
    match (host.spine, host.layer.is_some()) {
        (true, _) => LAYOUT_SPINE,
        (false, true) => LAYOUT_FULL,
        (false, false) => LAYOUT_NARROW,
    }
}

fn row_layout(host: &Host<'_>) -> f32 {
    match (host.spine, host.layer.is_some()) {
        (true, _) => 52.0,
        (false, true) => 212.0,
        (false, false) => 72.0,
    }
}

/// How far the home head gives way when the column is short (`useBravaisSeamTabsFit`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fit {
    Titled,
    Bare,
    Short,
}

fn column_body<'a>(host: &Host<'a>, ink: Color) -> Element<'a, Message> {
    if host.layer.is_some() {
        return if host.spine { collection_spine(host, ink) } else { collection_full(host, ink) };
    }
    let spine = host.spine;
    let (pad_y, pad_x, gap) = if spine { (14.0, 8.0, 12.0) } else { (16.0, 10.0, 18.0) };
    let fit = fit(host);
    let title_size = if spine { 12.0 } else { 15.0 };
    let title_track = if spine { 0.08 } else { 0.3 };
    let fold = ghost(Icon::FoldHorizontal, 16.0, 36.0, ink, Some(Message::Fold));
    let mut head = column![].spacing(8).align_x(Alignment::Center);
    if fit == Fit::Titled {
        head = head.push(tracked_row(
            super::tr(host, "libraryBravais.homeTitle"),
            title_size,
            title_track,
            title_track,
            ink,
        ));
    }
    head = head.push(fold).push(tab_column(host, ink, fit));
    let middle = container(status_col(host, ink)).width(Length::Fill).height(Length::Fill).align_y(Alignment::Center);
    let tools = if spine { tool_column(ink) } else { tool_grid(ink) };
    let mut dock = column![].spacing(if spine { 6 } else { 8 }).align_x(Alignment::Center);
    if let Some(account) = &host.account {
        dock = dock.push(account_entry(account, ink, spine));
    }
    dock = dock.push(tools);
    let mut body = column![head, middle, dock].spacing(gap).align_x(Alignment::Center);
    if spine {
        body = body.push(ghost(Icon::Maximize2, 16.0, 36.0, ink, Some(Message::Expand)));
    }
    body.padding(iced::Padding { top: pad_y, right: pad_x, bottom: host.bottom, left: pad_x })
        .width(Length::Fixed(column_layout(host)))
        .height(Length::Fill)
        .into()
}

/// The tab column's height at a fit level, laid out as `tab_column` draws it.
fn tabs_height(host: &Host<'_>, fit: Fit) -> f32 {
    let (size, track, pad) = tab_metrics(host.spine, fit);
    let gap = if fit == Fit::Short { 2.0 } else { 4.0 };
    let tabs: f32 = host
        .tabs
        .iter()
        .map(|(_, label)| {
            let count = if fit == Fit::Short { 1.0 } else { label.chars().count().max(1) as f32 };
            count * size * 1.3 + (count - 1.0) * size * track + pad.0 * 2.0 + 2.0
        })
        .sum();
    tabs + gap * host.tabs.len().saturating_sub(1) as f32
}

/// Hides the title first, then cuts the tabs to one character, as the original gives way.
fn fit(host: &Host<'_>) -> Fit {
    let spine = host.spine;
    let (pad_y, gap) = if spine { (14.0, 12.0) } else { (16.0, 18.0) };
    let title = if spine { 12.0 * 1.3 } else { 15.0 * 1.3 } + 8.0;
    let account = match &host.account {
        Some(_) if spine => 36.0 + 6.0,
        Some(slot) => 8.0 + 32.0 + 5.0 + if slot.guest { 32.0 } else { 16.0 } + 6.0 + 8.0,
        None => 0.0,
    };
    let tools = if spine { 4.0 * 36.0 + 3.0 * 6.0 + gap + 36.0 } else { 88.0 };
    let room = host.window_height - pad_y - host.bottom - (36.0 + 8.0) - gap * 2.0 - account - tools;
    if tabs_height(host, Fit::Titled) + title <= room {
        Fit::Titled
    } else if tabs_height(host, Fit::Bare) <= room {
        Fit::Bare
    } else {
        Fit::Short
    }
}

/// Size, letter spacing and padding of a home tab (`.bravais-seam-home-tab`, `.is-short`, `.is-compact`).
fn tab_metrics(spine: bool, fit: Fit) -> (f32, f32, (f32, f32)) {
    let size = if spine { 12.0 } else { 14.0 };
    match (fit, spine) {
        (Fit::Short, _) => (size, 0.0, (5.0, 4.0)),
        (_, true) => (size, 0.1, (7.0, 3.0)),
        (_, false) => (size, 0.18, (9.0, 4.0)),
    }
}

/// `abbreviateSeamTabLabels`: the first character, two where they would clash.
fn short_labels(labels: &[&str]) -> Vec<String> {
    let first = |label: &str, count: usize| -> String {
        let head: String = label.trim().chars().take(count).collect();
        let mut chars = head.chars();
        chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
    };
    let shorts: Vec<String> = labels.iter().map(|label| first(label, 1)).collect();
    labels
        .iter()
        .zip(&shorts)
        .map(
            |(label, short)| {
                if shorts.iter().filter(|other| *other == short).count() > 1 { first(label, 2) } else { short.clone() }
            },
        )
        .collect()
}

fn tab_column<'a>(host: &Host<'a>, ink: Color, fit: Fit) -> Element<'a, Message> {
    let (size, track, pad) = tab_metrics(host.spine, fit);
    let labels: Vec<&str> = host.tabs.iter().map(|(_, label)| *label).collect();
    let shorts = short_labels(&labels);
    let tabs = host.tabs.iter().zip(shorts).map(|(&(tab, label), short)| {
        let shown = if fit == Fit::Short { short } else { label.to_owned() };
        let active = tab == host.current;
        let face = tracked_col(&shown, size, track, if active { ink } else { soft(ink) });
        home_tab(face, pad, active, ink, Message::SelectTab(tab), label)
    });
    column(tabs).spacing(if fit == Fit::Short { 2 } else { 4 }).align_x(Alignment::Center).into()
}

fn home_tab<'a>(
    face: Element<'a, Message>,
    pad: (f32, f32),
    active: bool,
    ink: Color,
    on_press: Message,
    label: &str,
) -> Element<'a, Message> {
    let tab = button(face).padding([pad.0, pad.1]).on_press(on_press).style(move |_theme: &Theme, status| {
        let hot = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let fill = if active {
            0.14
        } else if hot {
            0.16
        } else {
            0.0
        };
        button::Style {
            background: Some(Background::Color(Color { a: fill * ink.a, ..ink })),
            border: Border {
                radius: 10.0.into(),
                width: 1.0,
                color: if hot { Color { a: 0.26 * ink.a, ..ink } } else { Color::TRANSPARENT },
            },
            ..button::Style::default()
        }
    });
    iced::widget::Tooltip::new(tab, text(label.to_owned()).size(12), iced::widget::tooltip::Position::Right).into()
}

/// The account entry: a 32px picture over the name, or a 36px icon button on the spine.
fn account_entry<'a>(slot: &AccountSlot<'a>, ink: Color, spine: bool) -> Element<'a, Message> {
    let picture = avatar(slot, if spine { 28.0 } else { 32.0 }, ink);
    let face: Element<'a, Message> = if spine {
        container(picture).center(Length::Fixed(36.0)).into()
    } else {
        let name = text(slot.name.clone())
            .size(12)
            .font(extra_bold())
            .line_height(iced::widget::text::LineHeight::Relative(1.3))
            .align_x(Alignment::Center)
            .color(ink)
            .wrapping(if slot.guest {
                iced::widget::text::Wrapping::WordOrGlyph
            } else {
                iced::widget::text::Wrapping::None
            });
        column![picture, name].spacing(5).align_x(Alignment::Center).width(Length::Fill).into()
    };
    let entry = button(face)
        .padding(if spine {
            iced::Padding::ZERO
        } else {
            iced::Padding { top: 8.0, right: 4.0, bottom: 6.0, left: 4.0 }
        })
        .width(if spine { Length::Fixed(36.0) } else { Length::Fill })
        .on_press(Message::Connect)
        .style(move |_theme: &Theme, status| {
            let hot = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: Some(Background::Color(if hot {
                    Color { a: 0.12 * ink.a, ..ink }
                } else {
                    Color::TRANSPARENT
                })),
                border: Border { radius: 12.0.into(), ..Border::default() },
                ..button::Style::default()
            }
        });
    iced::widget::Tooltip::new(entry, text(slot.tip.clone()).size(12), iced::widget::tooltip::Position::Right).into()
}

/// `BravaisProviderAvatar`: the picture, the platform's letter on its colour, or a plug on a pale disc for a guest.
fn avatar<'a>(slot: &AccountSlot<'a>, size: f32, ink: Color) -> Element<'a, Message> {
    if slot.guest {
        return container(icon(Icon::Plug, 16.0, ink))
            .center(Length::Fixed(size))
            .style(move |_theme: &Theme| container::Style {
                background: Some(Background::Color(Color { a: 0.12 * ink.a, ..ink })),
                border: Border { radius: (size / 2.0).into(), ..Border::default() },
                ..container::Style::default()
            })
            .into();
    }
    if let Some(handle) = slot.avatar {
        return iced::widget::image(handle.clone())
            .width(Length::Fixed(size))
            .height(Length::Fixed(size))
            .content_fit(iced::ContentFit::Cover)
            .border_radius(size / 2.0)
            .into();
    }
    let mark = slot.mark;
    let letter = text(slot.letter.to_string())
        .size(if size < 30.0 { 12.0 } else { 14.0 })
        .font(iced::Font { weight: iced::font::Weight::Black, ..iced::Font::DEFAULT })
        .color(Color::WHITE);
    container(letter)
        .center(Length::Fixed(size))
        .style(move |_theme: &Theme| container::Style {
            background: Some(Background::Color(mark)),
            border: Border { radius: (size / 2.0).into(), ..Border::default() },
            ..container::Style::default()
        })
        .into()
}

/// `verticalTitleSize`: shrink long titles so they stay inside the strip.
fn vertical_title_size(title: &str, base: f32, min: f32) -> f32 {
    (320.0 / title.chars().count().max(1) as f32).round().clamp(min, base)
}

/// The full info strip of a collection layer (`FullSeam`): crumbs, the vertical title, the count, play all.
fn collection_full<'a>(host: &Host<'a>, ink: Color) -> Element<'a, Message> {
    let (title, meta) = host.layer.clone().unwrap_or_default();
    let back = back_button(host, ink);
    let trail = format!("{} › {}", super::tr(host, "libraryBravais.homeTitle"), title);
    let crumbs = row![back, text(trail).size(11).color(soft(ink)).wrapping(iced::widget::text::Wrapping::None)]
        .spacing(6)
        .align_y(Alignment::Center)
        .height(Length::Fixed(32.0));
    let quote = |mark: &'static str, side: Alignment| {
        container(
            text(mark)
                .size(28)
                .line_height(iced::widget::text::LineHeight::Relative(0.6))
                .color(Color { a: ink.a * 0.35, ..ink }),
        )
        .width(Length::Fill)
        .align_x(side)
    };
    let size = vertical_title_size(&title, 52.0, 26.0);
    let vertical = container(tracked_col_weight(&title, size, 0.04, ink, iced::font::Weight::ExtraBold))
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .clip(true);
    let area = column![quote("\u{201c}", Alignment::End), vertical, quote("\u{201d}", Alignment::Start)]
        .spacing(14)
        .height(Length::Fill);
    let rule = container(space()).width(Length::Fill).height(Length::Fixed(1.0)).style(move |_theme: &Theme| {
        container::Style {
            background: Some(Background::Color(Color { a: 0.12 * ink.a, ..ink })),
            ..container::Style::default()
        }
    });
    let mut body = column![crumbs, area, rule, text(meta).size(12).color(soft(ink))].spacing(14);
    if let Some(status) = &host.status {
        body = body.push(text(status.clone()).size(12).color(ink));
    }
    body = body.push(play_all(host, ink));
    body.padding(iced::Padding { top: 22.0, right: 22.0, bottom: host.bottom, left: 22.0 })
        .width(Length::Fixed(LAYOUT_FULL))
        .height(Length::Fill)
        .into()
}

/// The spine of a collection layer (`SpineSeam`): back, fold, the vertical title, the count, play, expand.
fn collection_spine<'a>(host: &Host<'a>, ink: Color) -> Element<'a, Message> {
    let (title, meta) = host.layer.clone().unwrap_or_default();
    let size = vertical_title_size(&title, 30.0, 18.0);
    let title = button(tracked_col_weight(&title, size, 0.04, ink, iced::font::Weight::ExtraBold))
        .padding(0)
        .on_press(Message::Expand)
        .style(button::text);
    let count = container(tracked_col(&meta, 11.0, 0.1, soft(ink))).clip(true);
    column![
        ghost(Icon::ChevronLeft, 16.0, 36.0, ink, Some(Message::Back)),
        ghost(Icon::FoldHorizontal, 16.0, 36.0, ink, Some(Message::Fold)),
        container(title).height(Length::Shrink).clip(true),
        count,
        space().height(Length::Fill),
        ghost(Icon::Play, 16.0, 36.0, ink, Some(Message::PlayAll)),
        ghost(Icon::Maximize2, 16.0, 36.0, ink, Some(Message::Expand)),
    ]
    .spacing(12)
    .align_x(Alignment::Center)
    .padding(iced::Padding { top: 16.0, right: 8.0, bottom: host.bottom, left: 8.0 })
    .width(Length::Fixed(LAYOUT_SPINE))
    .height(Length::Fill)
    .into()
}

/// `.bravais-seam-back`: a 32px circle on white 8% (black 5% by day) with a 16px chevron.
fn back_button<'a>(host: &Host<'a>, ink: Color) -> Element<'a, Message> {
    let fill = if host.daylight { Color { a: 0.05, ..Color::BLACK } } else { Color { a: 0.08, ..Color::WHITE } };
    let face = container(icon(Icon::ChevronLeft, 16.0, ink)).center(Length::Fill);
    let back =
        button(face).width(Length::Fixed(32.0)).height(Length::Fixed(32.0)).padding(0).on_press(Message::Back).style(
            move |_theme: &Theme, _status| button::Style {
                background: Some(Background::Color(fill)),
                border: Border { radius: 16.0.into(), ..Border::default() },
                ..button::Style::default()
            },
        );
    iced::widget::Tooltip::new(
        back,
        text(super::tr(host, "libraryBravais.seamBack")).size(12),
        iced::widget::tooltip::Position::Bottom,
    )
    .into()
}

/// `.bravais-chrome-button.is-primary`: 44px high, 15px bold, a 18px play icon on a 16% fill.
fn play_all<'a>(host: &Host<'a>, ink: Color) -> Element<'a, Message> {
    let face = row![
        icon(Icon::Play, 18.0, ink),
        text(super::tr(host, "libraryBravais.playAll")).size(15).font(bold()).color(ink)
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    button(container(face).height(Length::Fill).align_y(Alignment::Center))
        .height(Length::Fixed(44.0))
        .padding([0.0, 16.0])
        .on_press(Message::PlayAll)
        .style(move |_theme: &Theme, status| {
            let hot = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: Some(Background::Color(Color { a: 0.16 * ink.a, ..ink })),
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

fn row_body<'a>(host: &Host<'a>, ink: Color) -> Element<'a, Message> {
    let spine = host.spine;
    let gap = if spine { 10.0 } else { 14.0 };
    let pad_x = (host.window_width * 0.024).max(20.0);
    let fold = ghost(Icon::FoldHorizontal, 16.0, if spine { 34.0 } else { 36.0 }, ink, Some(Message::Fold));
    let mut items: Vec<Element<'a, Message>> = Vec::new();
    if let Some((title, meta)) = host.layer.clone() {
        items.push(ghost(Icon::ChevronLeft, 16.0, if spine { 34.0 } else { 36.0 }, ink, Some(Message::Back)));
        items.push(fold);
        items.push(tracked_row(&title, if spine { 15.0 } else { 22.0 }, 0.04, 0.0, ink));
        items.push(text(meta).size(12).color(soft(ink)).into());
        if let Some(status) = &host.status {
            items.push(text(status.clone()).size(12).color(ink).into());
        }
        items.push(space().width(Length::Fill).into());
        items.push(if spine {
            ghost(Icon::Play, 16.0, 34.0, ink, Some(Message::PlayAll))
        } else {
            play_all(host, ink)
        });
    } else {
        items.push(fold);
        if !spine {
            items.push(tracked_row(super::tr(host, "libraryBravais.homeTitle"), 15.0, 0.3, 0.3, ink));
        }
        let tab_pad = if spine { (5.0, 9.0) } else { (8.0, 12.0) };
        let tab_size = if spine { 12.0 } else { 14.0 };
        let tab_track = if spine { 0.06 } else { 0.12 };
        let tabs = host.tabs.iter().map(|&(tab, label)| {
            let active = tab == host.current;
            let face = tracked_row(label, tab_size, tab_track, 0.0, if active { ink } else { soft(ink) });
            home_tab(face, tab_pad, active, ink, Message::SelectTab(tab), label)
        });
        items.push(row(tabs).spacing(4).align_y(Alignment::Center).into());
        if let Some(status) = &host.status {
            items.push(text(status.clone()).size(12).color(soft(ink)).into());
        }
        items.push(space().width(Length::Fill).into());
        if let Some(account) = &host.account {
            items.push(account_entry(account, ink, true));
        }
        items.push(tool_row(ink, spine));
    }
    if spine {
        items.push(ghost(Icon::Maximize2, 16.0, 34.0, ink, Some(Message::Expand)));
    }
    row(items)
        .spacing(gap)
        .align_y(Alignment::Center)
        .padding(iced::Padding { top: 0.0, right: pad_x, bottom: 0.0, left: pad_x })
        .width(Length::Fill)
        .height(Length::Fixed(row_layout(host)))
        .into()
}

fn status_col<'a>(host: &Host<'a>, ink: Color) -> Element<'a, Message> {
    let Some(status) = &host.status else { return space().into() };
    container(tracked_col(status, 11.0, 0.1, soft(ink))).width(Length::Fill).align_x(Alignment::Center).into()
}

fn soft(ink: Color) -> Color {
    Color { a: ink.a * 0.55, ..ink }
}

/// The ⋯ menu, above the dismiss layer so its rows can be pressed.
pub fn popup<'a>(home: &'a HomeChrome, host: &Host<'a>, now: Instant) -> Option<Element<'a, Message>> {
    let shown = home.menu(now);
    let empty = host.current != folia_store::HomeTab::Local && !host.has_track;
    if shown <= 0.02 || host.spine || host.seam.width < 90.0 || host.layer.is_some() || empty {
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
    let mut rows = Vec::new();
    // The tab's own items first (the local tab's import and refresh), then the app's, with a rule between.
    if host.current == folia_store::HomeTab::Local {
        rows.push(menu_row(super::tr(host, "localMusic.importFolder"), import, ink, shown));
        rows.push(menu_row(super::tr(host, "options.refresh"), reload, ink, shown));
    }
    if host.has_track {
        if !rows.is_empty() {
            let rule = Color { a: 0.14 * shown * ink.a, ..ink };
            let line =
                container(space()).width(Length::Fill).height(Length::Fixed(1.0)).style(move |_theme: &Theme| {
                    container::Style { background: Some(Background::Color(rule)), ..container::Style::default() }
                });
            rows.push(container(line).padding([3, 6]).into());
        }
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

fn tracked_row<'a>(label: &str, size: f32, tracking: f32, indent: f32, color: Color) -> Element<'a, Message> {
    let chars = label
        .chars()
        .map(|ch| text(ch.to_string()).size(size).color(color).font(extra_bold()).into())
        .collect::<Vec<_>>();
    row(chars)
        .spacing(size * tracking)
        .padding(iced::Padding { top: 0.0, right: 0.0, bottom: 0.0, left: size * indent })
        .into()
}

fn tracked_col<'a>(label: &str, size: f32, tracking: f32, color: Color) -> Element<'a, Message> {
    tracked_col_weight(label, size, tracking, color, iced::font::Weight::Bold)
}

fn tracked_col_weight<'a>(
    label: &str,
    size: f32,
    tracking: f32,
    color: Color,
    weight: iced::font::Weight,
) -> Element<'a, Message> {
    let font = iced::Font { weight, ..iced::Font::DEFAULT };
    let chars =
        label.chars().map(|ch| text(ch.to_string()).size(size).color(color).font(font).into()).collect::<Vec<_>>();
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

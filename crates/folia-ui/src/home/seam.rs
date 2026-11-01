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
    // While the layer turns with the poster, the wall draws the seam's ground turned; the plate would cover it.
    if host.poster_tilted {
        return clipped;
    }
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
    match (host.spine, host.layer.as_ref().map(|layer| layer.list)) {
        (true, _) => LAYOUT_SPINE,
        (false, Some(true)) => super::PANEL.min(host.window_width - 52.0).max(LAYOUT_FULL.min(host.window_width)),
        (false, Some(false)) if host.layer.as_ref().is_some_and(|layer| layer.album.is_some()) => {
            crate::poster::width(host.window_width)
        }
        (false, Some(false)) => LAYOUT_FULL,
        (false, None) => LAYOUT_NARROW,
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
        .font(crate::typeface::font(iced::font::Weight::Black))
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

/// The full info strip of a collection layer (`FullSeam`): crumbs, the page filter, the vertical title, the count,
/// play all, enqueue and the star, then the list and ⋯ more. The open list panel replaces it.
fn collection_full<'a>(host: &Host<'a>, ink: Color) -> Element<'a, Message> {
    let Some(seam) = host.layer.as_ref() else { return space().into() };
    if seam.list {
        return list_panel(host, seam, ink);
    }
    if let Some(album) = &seam.album {
        return album_poster(host, seam, album);
    }
    let crumbs = crumbs(host, seam, ink, false);
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
    let size = vertical_title_size(&seam.title, 52.0, 26.0);
    let vertical = container(tracked_col_weight(&seam.title, size, 0.04, ink, iced::font::Weight::ExtraBold))
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .clip(true);
    let area = column![quote("\u{201c}", Alignment::End), vertical, quote("\u{201d}", Alignment::Start)]
        .spacing(14)
        .height(Length::Fill);
    let mut body = column![
        crumbs,
        filter_field(host, seam, ink),
        area,
        hairline(ink, 0.12),
        text(seam.meta.clone()).size(12).color(soft(ink))
    ]
    .spacing(14);
    if let Some(status) = &host.status {
        body = body.push(text(status.clone()).size(12).color(ink));
    }
    let mut actions = vec![
        chrome_button(
            Icon::Play,
            super::tr(host, "libraryBravais.playAll"),
            true,
            ink,
            seam.playable.then_some(Message::PlayAll),
        ),
        chrome_button(
            Icon::ListPlus,
            super::tr(host, "libraryBravais.enqueueAll"),
            false,
            ink,
            seam.playable.then_some(Message::EnqueueAll),
        ),
    ];
    if let Some((saved, pending, title)) = &seam.star {
        actions.push(star(*saved, *pending, title.clone(), host.accent, ink));
    }
    // `.bravais-seam-actions { flex-wrap: wrap }`.
    let actions = row(actions).spacing(6).wrap();
    let more = row![
        chrome_button(
            Icon::List,
            super::tr(host, "libraryBravaisCollection.list"),
            false,
            ink,
            Some(Message::OpenList)
        ),
        chrome_button(
            Icon::Ellipsis,
            super::tr(host, "libraryBravaisCollection.more"),
            false,
            ink,
            Some(Message::CollectionMenu)
        ),
    ]
    .spacing(6)
    .wrap();
    body = body.push(actions).push(more);
    let body = body
        .padding(iced::Padding { top: 22.0, right: 22.0, bottom: host.bottom, left: 22.0 })
        .width(Length::Fixed(column_layout(host)))
        .height(Length::Fill);
    if !seam.menu {
        return body.into();
    }
    // `.bravais-seam-more-anchor > .bravais-seam-menu`: floats 6px above the button row, over the strip.
    let menu = collection_menu(host, ink);
    let lifted = container(menu)
        .width(Length::Fixed(column_layout(host)))
        .height(Length::Fill)
        .align_y(Alignment::End)
        .padding(iced::Padding { top: 0.0, right: 22.0, bottom: host.bottom + 36.0 + 6.0, left: 22.0 });
    stack![body, lifted].into()
}

/// `BravaisSeamCrumbs`: the back button and `书库 › 名字` (`› 列表` while the panel is open).
fn crumbs<'a>(host: &Host<'a>, seam: &super::LayerSeam, ink: Color, panel: bool) -> Element<'a, Message> {
    let mut trail = format!("{} › {}", super::tr(host, "libraryBravais.homeTitle"), seam.title);
    if panel {
        trail.push_str(" › ");
        trail.push_str(super::tr(host, "libraryBravaisCollection.listCrumb"));
    }
    let mut line = row![
        back_button(host, ink),
        container(text(trail).size(11).color(soft(ink)).wrapping(iced::widget::text::Wrapping::None))
            .width(Length::Fill)
            .clip(true)
    ]
    .spacing(6)
    .align_y(Alignment::Center)
    .height(Length::Fixed(32.0));
    if panel {
        // `.bravais-seam-level`: the fold as a word.
        let label = text(super::tr(host, "libraryBravais.seamFold")).size(11).color(soft(ink));
        line = line.push(button(label).padding([3.0, 9.0]).on_press(Message::Fold).style(
            move |_theme: &Theme, status| {
                let hot = matches!(status, button::Status::Hovered | button::Status::Pressed);
                button::Style {
                    background: hot.then_some(Background::Color(Color { a: 0.16 * ink.a, ..ink })),
                    border: Border {
                        radius: 12.0.into(),
                        width: 1.0,
                        color: if hot { Color { a: 0.26 * ink.a, ..ink } } else { Color::TRANSPARENT },
                    },
                    ..button::Style::default()
                }
            },
        ));
    }
    line.into()
}

/// `BravaisSeamFilterField`: a funnel, the input over an underline, then the match count and a clear button.
fn filter_field<'a>(host: &Host<'a>, seam: &super::LayerSeam, ink: Color) -> Element<'a, Message> {
    let has_query = !seam.query.is_empty();
    let tone = if has_query { ink } else { soft(ink) };
    let accent = host.accent;
    let input = iced::widget::text_input(super::tr(host, "libraryBravais.filterPlaceholder"), &seam.query)
        .id(super::filter_id())
        .on_input(Message::Query)
        .on_submit(Message::FilterSubmit)
        .size(13)
        .padding(0)
        .font(crate::typeface::font(iced::font::Weight::Semibold))
        .style(move |_theme: &Theme, _status| iced::widget::text_input::Style {
            background: Background::Color(Color::TRANSPARENT),
            border: Border::default(),
            icon: tone,
            placeholder: soft(ink),
            value: ink,
            selection: Color { a: 0.3, ..accent },
        });
    let mut line =
        row![icon(Icon::ListFilter, 14.0, if has_query { accent } else { Color { a: tone.a * 0.75, ..tone } }), input]
            .spacing(8)
            .align_y(Alignment::Center)
            .height(Length::Fixed(34.0));
    if has_query {
        line = line.push(text(seam.match_label.clone()).size(11).color(soft(ink)));
        let clear = button(container(icon(Icon::X, 14.0, soft(ink))).center(Length::Fill))
            .width(Length::Fixed(24.0))
            .height(Length::Fixed(24.0))
            .padding(0)
            .on_press(Message::Query(String::new()))
            .style(move |_theme: &Theme, status| button::Style {
                background: matches!(status, button::Status::Hovered | button::Status::Pressed)
                    .then_some(Background::Color(Color { a: 0.1 * ink.a, ..ink })),
                border: Border { radius: 8.0.into(), ..Border::default() },
                ..button::Style::default()
            });
        line = line.push(iced::widget::Tooltip::new(
            clear,
            text(super::tr(host, "libraryBravaisCollection.clearFilter")).size(12),
            iced::widget::tooltip::Position::Bottom,
        ));
    }
    let underline = hairline(ink, if has_query { 0.36 } else { 0.18 });
    container(column![line, underline]).height(Length::Fixed(44.0)).align_y(Alignment::Center).into()
}

/// The song list panel (`BravaisListPanel`): crumbs, filter, a horizontal title, the count, the rows in view, play
/// all and enqueue. Rows are 44px; click flies the wall to the song, double-click plays it.
fn list_panel<'a>(host: &Host<'a>, seam: &super::LayerSeam, ink: Color) -> Element<'a, Message> {
    let title = text(seam.title.clone())
        .size(24)
        .font(crate::typeface::font(iced::font::Weight::ExtraBold))
        .line_height(iced::widget::text::LineHeight::Relative(1.15))
        .color(ink)
        .wrapping(iced::widget::text::Wrapping::None);
    let row_px = super::LIST_ROW;
    let before = seam.first as f32 * row_px;
    let after = seam.count.saturating_sub(seam.first + seam.rows.len()) as f32 * row_px;
    let mut list = column![space().height(Length::Fixed(before))];
    for (offset, entry) in seam.rows.iter().enumerate() {
        list = list.push(list_row(host, entry, seam.first + offset, ink));
    }
    list = list.push(space().height(Length::Fixed(after)));
    let list = iced::widget::scrollable(list.width(Length::Fill))
        .on_scroll(|viewport| Message::ListScrolled(viewport.absolute_offset().y))
        .height(Length::Fill);
    let actions = row![
        chrome_button(
            Icon::Play,
            super::tr(host, "libraryBravais.playAll"),
            true,
            ink,
            seam.playable.then_some(Message::PlayAll)
        ),
        chrome_button(
            Icon::ListPlus,
            super::tr(host, "libraryBravais.enqueueAll"),
            false,
            ink,
            seam.playable.then_some(Message::EnqueueAll)
        ),
    ]
    .spacing(6);
    let mut body = column![
        crumbs(host, seam, ink, true),
        filter_field(host, seam, ink),
        container(title).width(Length::Fill).clip(true),
        text(seam.meta.clone()).size(12).color(soft(ink)),
    ]
    .spacing(10);
    if let (true, Some(status)) = (seam.rows.is_empty(), &host.status) {
        body = body.push(text(status.clone()).size(12).color(ink));
    }
    body.push(container(list).height(Length::Fill))
        .push(actions)
        .padding(iced::Padding { top: 22.0, right: 12.0, bottom: host.bottom, left: 12.0 })
        .width(Length::Fixed(column_layout(host)))
        .height(Length::Fill)
        .into()
}

/// `.bravais-list-row`: `4ch` number (accent when playing), title over artist, length; hover 12%.
fn list_row<'a>(host: &Host<'a>, entry: &super::ListRow, index: usize, ink: Color) -> Element<'a, Message> {
    let badge_color = if entry.current { host.accent } else { soft(ink) };
    let badge = container(
        text(entry.badge.clone())
            .size(11)
            .font(crate::typeface::font(iced::font::Weight::ExtraBold))
            .color(badge_color),
    )
    .width(Length::Fixed(32.0));
    let words = column![
        text(entry.title.clone()).size(13).font(bold()).color(ink).wrapping(iced::widget::text::Wrapping::None),
        text(entry.subtitle.clone()).size(11).color(soft(ink)).wrapping(iced::widget::text::Wrapping::None),
    ];
    let face = row![
        badge,
        container(words).width(Length::Fill).clip(true),
        text(entry.time.clone()).size(11).color(soft(ink))
    ]
    .spacing(10)
    .align_y(Alignment::Center);
    let hovered = host.list_hover == Some(index);
    let cell =
        container(face).height(Length::Fixed(super::LIST_ROW)).padding([0.0, 10.0]).align_y(Alignment::Center).style(
            move |_theme: &Theme| container::Style {
                background: hovered.then_some(Background::Color(Color { a: 0.12 * ink.a, ..ink })),
                border: Border { radius: 10.0.into(), ..Border::default() },
                ..container::Style::default()
            },
        );
    mouse_area(cell)
        .on_press(Message::LocateRow(index))
        .on_double_click(Message::PlayRow(index))
        .on_enter(Message::RowHover(Some(index)))
        .on_exit(Message::RowHover(None))
        .interaction(iced::mouse::Interaction::Pointer)
        .into()
}

/// The ⋯ more menu of a collection: reload, a rule, then folding the strip (`BravaisSeamCollectionMenu`).
fn collection_menu<'a>(host: &Host<'a>, ink: Color) -> Element<'a, Message> {
    let (bg, text_ink) = (host.background, host.text);
    let paper = Color {
        r: bg.r * 0.92 + text_ink.r * 0.08,
        g: bg.g * 0.92 + text_ink.g * 0.08,
        b: bg.b * 0.92 + text_ink.b * 0.08,
        a: ink.a,
    };
    let item = |label: &'a str, message: Message| -> Element<'a, Message> {
        button(text(label).size(13).font(crate::typeface::font(iced::font::Weight::Semibold)).color(ink))
            .width(Length::Fill)
            .padding([8.0, 10.0])
            .on_press(message)
            .style(move |_theme: &Theme, status| button::Style {
                background: matches!(status, button::Status::Hovered | button::Status::Pressed)
                    .then_some(Background::Color(Color { a: 0.14 * ink.a, ..ink })),
                border: Border { radius: 10.0.into(), ..Border::default() },
                ..button::Style::default()
            })
            .into()
    };
    let rule = container(hairline(ink, 0.14)).padding([3.0, 6.0]);
    container(
        column![
            item(super::tr(host, "playlist.reload"), Message::ReloadCollection),
            rule,
            item(super::tr(host, "libraryBravais.seamFoldStrip"), Message::Fold),
        ]
        .width(Length::Fill),
    )
    .padding(4)
    .width(Length::Fill)
    .style(move |_theme: &Theme| container::Style {
        background: Some(Background::Color(paper)),
        border: Border { radius: 12.0.into(), width: 1.0, color: Color { a: 0.14 * ink.a, ..ink } },
        ..container::Style::default()
    })
    .into()
}

/// `.bravais-seam-star`: 36px, the outline star in soft ink, filled with the accent once saved; a spinner while saving.
fn star<'a>(saved: bool, pending: bool, title: String, accent: Color, ink: Color) -> Element<'a, Message> {
    let glyph: Element<'a, Message> = if pending {
        icon(Icon::LoaderCircle, 18.0, soft(ink))
    } else if saved {
        crate::icons::variant(crate::icons::Variant::FilledStar, 18.0, accent)
    } else {
        icon(Icon::Star, 18.0, soft(ink))
    };
    let face = button(container(glyph).center(Length::Fill))
        .width(Length::Fixed(36.0))
        .height(Length::Fixed(36.0))
        .padding(0)
        .on_press_maybe((!pending).then_some(Message::ToggleSubscribe))
        .style(move |_theme: &Theme, status| button::Style {
            border: Border {
                radius: 12.0.into(),
                width: 1.0,
                color: if matches!(status, button::Status::Hovered) {
                    Color { a: 0.26 * ink.a, ..ink }
                } else {
                    Color::TRANSPARENT
                },
            },
            ..button::Style::default()
        });
    iced::widget::Tooltip::new(face, text(title).size(12), iced::widget::tooltip::Position::Top).into()
}

/// `.bravais-seam-actions .bravais-chrome-button`: 36px high, 12px bold with a 14px icon; the primary one on a 16%
/// fill, the others at 80% until hovered.
fn chrome_button<'a>(
    glyph: Icon,
    label: &'a str,
    primary: bool,
    ink: Color,
    on_press: Option<Message>,
) -> Element<'a, Message> {
    let enabled = on_press.is_some();
    button(
        container(
            row![icon(glyph, 14.0, ink), text(label).size(12).font(bold()).color(ink)]
                .spacing(8)
                .align_y(Alignment::Center),
        )
        .height(Length::Fill)
        .align_y(Alignment::Center),
    )
    .height(Length::Fixed(36.0))
    .padding([0.0, 12.0])
    .on_press_maybe(on_press)
    .style(move |_theme: &Theme, status| {
        let hot = enabled && matches!(status, button::Status::Hovered | button::Status::Pressed);
        let fill = if primary || hot { 0.16 } else { 0.0 };
        button::Style {
            background: Some(Background::Color(Color { a: fill * ink.a, ..ink })),
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

fn hairline<'a>(ink: Color, alpha: f32) -> Element<'a, Message> {
    container(space())
        .width(Length::Fill)
        .height(Length::Fixed(1.0))
        .style(move |_theme: &Theme| container::Style {
            background: Some(Background::Color(Color { a: alpha * ink.a, ..ink })),
            ..container::Style::default()
        })
        .into()
}

/// The spine of a collection layer (`SpineSeam`): back, fold, the vertical title, the count, the filter mark, play, expand.
fn collection_spine<'a>(host: &Host<'a>, ink: Color) -> Element<'a, Message> {
    let Some(seam) = host.layer.as_ref() else { return space().into() };
    let size = vertical_title_size(&seam.title, 30.0, 18.0);
    let title = button(tracked_col_weight(&seam.title, size, 0.04, ink, iced::font::Weight::ExtraBold))
        .padding(0)
        .on_press(Message::Expand)
        .style(button::text);
    let count = container(tracked_col(&seam.meta, 11.0, 0.1, soft(ink))).clip(true);
    let mut items = column![
        ghost(Icon::ChevronLeft, 16.0, 36.0, ink, Some(Message::Back)),
        ghost(Icon::FoldHorizontal, 16.0, 36.0, ink, Some(Message::Fold)),
        container(title).height(Length::Shrink).clip(true),
        count,
    ]
    .spacing(12)
    .align_x(Alignment::Center);
    if seam.filtering {
        // `.is-filtering`: the spine has no room for the field; pressing it opens the strip.
        items = items.push(ghost(Icon::ListFilter, 16.0, 36.0, host.accent, Some(Message::Expand)));
    }
    items
        .push(space().height(Length::Fill))
        .push(ghost(Icon::Play, 16.0, 36.0, ink, Some(Message::PlayAll)))
        .push(ghost(Icon::Maximize2, 16.0, 36.0, ink, Some(Message::Expand)))
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

fn row_body<'a>(host: &Host<'a>, ink: Color) -> Element<'a, Message> {
    let spine = host.spine;
    let gap = if spine { 10.0 } else { 14.0 };
    let pad_x = (host.window_width * 0.024).max(20.0);
    let fold = ghost(Icon::FoldHorizontal, 16.0, if spine { 34.0 } else { 36.0 }, ink, Some(Message::Fold));
    let mut items: Vec<Element<'a, Message>> = Vec::new();
    if let Some(seam) = host.layer.as_ref() {
        items.push(ghost(Icon::ChevronLeft, 16.0, if spine { 34.0 } else { 36.0 }, ink, Some(Message::Back)));
        items.push(fold);
        items.push(tracked_row(&seam.title, if spine { 15.0 } else { 22.0 }, 0.04, 0.0, ink));
        items.push(text(seam.meta.clone()).size(12).color(soft(ink)).into());
        if let Some(status) = &host.status {
            items.push(text(status.clone()).size(12).color(ink).into());
        }
        items.push(space().width(Length::Fill).into());
        items.push(if spine {
            ghost(Icon::Play, 16.0, 34.0, ink, Some(Message::PlayAll))
        } else {
            chrome_button(Icon::Play, super::tr(host, "libraryBravais.playAll"), true, ink, Some(Message::PlayAll))
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
    let empty = host.current != folia_store::HomeTab::Local && !host.has_track && host.explore_refresh.is_none();
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
    if let Some(enabled) = host.explore_refresh {
        let refresh = enabled.then_some(Message::RefreshExplore);
        rows.push(menu_row(super::tr(host, "home.exploreRefresh"), refresh, ink, shown));
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
    let font = crate::typeface::font(weight);
    let chars =
        label.chars().map(|ch| text(ch.to_string()).size(size).color(color).font(font).into()).collect::<Vec<_>>();
    column(chars).spacing(size * tracking).align_x(Alignment::Center).into()
}

fn extra_bold() -> iced::Font {
    crate::typeface::font(iced::font::Weight::ExtraBold)
}

fn bold() -> iced::Font {
    crate::typeface::font(iced::font::Weight::Bold)
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

/// `BravaisSeamAlbumPoster`: the dotted paper with the cover taped on, the hatch, and the dark block with the
/// kicker, the extruded title, the underline, the facts and the actions over the outlined watermark. Every part
/// enters on its own beat (`bravaisAlbumPoster.css`).
fn album_poster<'a>(host: &Host<'a>, seam: &super::LayerSeam, album: &super::AlbumSeam) -> Element<'a, Message> {
    use crate::poster::{COVER_MARGIN, Cover, Ground, PAD, TITLE_FONT, Underline};
    use crate::poster_motion::Title;
    use iced::widget::canvas;
    let palette = album.palette;
    let entry = album.entry;
    let (ink, paper) = (palette.ink, palette.paper);
    let fade = |color: Color, alpha: f32| Color { a: color.a * alpha, ..color };
    // A part rising in: faded, and lifted by a float only while it moves (a float sits above everything).
    let rise = |element: Element<'a, Message>, (_, below): (f32, f32)| -> Element<'a, Message> {
        if below > 0.01 {
            iced::widget::float(element).translate(move |_, _| Vector::new(0.0, below)).into()
        } else {
            element
        }
    };
    let width = column_layout(host);
    let height = host.window_height;
    let ground: Element<'a, Message> = match host.poster_ground.filter(|_| !host.poster_tilted) {
        Some(cache) => {
            canvas(Ground { palette, composition: album.composition, ghost: album.ghost.clone(), cache, entry })
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        }
        None => space().into(),
    };
    // `.bravais-poster-top`: back and crumbs in ink on the paper, then 「过滤当前页」; it fades in after 200ms.
    let top_ink = fade(ink, entry.top());
    let filter_face = row![
        icon(Icon::ListFilter, 15.0, top_ink),
        text(super::tr(host, "libraryBravais.filterPlaceholder").to_owned())
            .size(13)
            .font(crate::typeface::font(iced::font::Weight::Bold))
            .color(top_ink)
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    let filter = button(container(filter_face).height(Length::Fixed(36.0)).align_y(Alignment::Center))
        .padding([0.0, 12.0])
        .on_press(Message::OpenFilter)
        .style(move |_theme: &Theme, status| button::Style {
            background: matches!(status, button::Status::Hovered | button::Status::Pressed)
                .then_some(Background::Color(Color { a: 0.1 * top_ink.a, ..ink })),
            border: Border { radius: 0.0.into(), width: 1.5, color: top_ink },
            ..button::Style::default()
        });
    let top = row![container(crumbs(host, seam, top_ink, false)).width(Length::Fill), filter]
        .spacing(12)
        .align_y(Alignment::Center)
        .padding(iced::Padding { top: 18.0, right: PAD, bottom: 0.0, left: 20.0 });
    // `.bravais-album-stage`: the cover, `min(56cqw, 33cqh)`, centred in what the column leaves it.
    let desired = ((width - 2.0 * album.inset) * 0.56).min(height * 0.33);
    let scrap: String = format!(
        "{}{}",
        seam.title,
        album.kicker.split_once(" · ").map(|(_, year)| format!(" · {year}")).unwrap_or_default()
    );
    let image = album.cover.clone();
    let composition = album.composition;
    let hidden = album.flying;
    let cover_entry = if album.flown { crate::poster_motion::Entry(None) } else { entry };
    let cover = iced::widget::responsive(move |room| {
        let side = desired.min(room.height - 18.0 - 2.0 * COVER_MARGIN + 24.0).max(48.0);
        let cover = canvas(Cover {
            report: Some(Message::PosterCover),
            image: image.clone(),
            palette,
            composition,
            entry: cover_entry,
            hidden,
            side,
        })
        .width(Length::Fixed(side + 2.0 * COVER_MARGIN))
        .height(Length::Fixed(side + 2.0 * COVER_MARGIN));
        container(cover)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_y(Alignment::Center)
            .padding(iced::Padding { top: 18.0, right: PAD, bottom: 0.0, left: PAD + 18.0 - COVER_MARGIN })
            .into()
    });
    let scrap = container(tracked_col_weight(
        &scrap,
        13.0,
        0.3,
        fade(ink, 0.85 * entry.scrap()),
        iced::font::Weight::ExtraBold,
    ))
    .max_height(height * 0.33 * 0.7)
    .clip(true);
    let stage = stack![
        cover,
        container(scrap).width(Length::Fill).align_x(Alignment::End).padding(iced::Padding {
            top: 24.0,
            right: PAD - 10.0,
            bottom: 0.0,
            left: 0.0
        }),
    ]
    .height(Length::Fill);
    // `.bravais-album-title`: drawn in a canvas so each word can fly in and the hero word can turn.
    let layout = album.title.clone();
    let title_height = layout.height + layout.size * 0.1;
    let title = canvas(Title {
        layout,
        color: palette.title,
        extrude: palette.extrude,
        hero: album.composition.hero,
        entry,
        font: TITLE_FONT,
    })
    .width(Length::Fill)
    .height(Length::Fixed(title_height));
    let kicker_in = entry.kicker();
    let kicker = container(
        text(album.kicker.clone())
            .size(12)
            .font(crate::typeface::font(iced::font::Weight::ExtraBold))
            .color(fade(paper, kicker_in.0)),
    )
    .padding([6.0, 12.0])
    .style(move |_theme: &Theme| container::Style {
        background: Some(Background::Color(fade(ink, kicker_in.0))),
        ..container::Style::default()
    });
    let underline_w = (width - 2.0 * PAD - 2.0 * album.inset) * album.composition.underline / 100.0;
    let underline = canvas(Underline { color: palette.accent, width: underline_w, drawn: entry.underline() })
        .width(Length::Fixed(underline_w))
        .height(Length::Fixed(12.0));
    let title_block = column![
        rise(kicker.into(), kicker_in),
        container(title).padding(iced::Padding::ZERO.top(6.0)),
        container(underline).padding(iced::Padding::ZERO.top(12.0))
    ]
    .padding(iced::Padding { top: 28.0, right: PAD, bottom: 0.0, left: PAD });
    // `.bravais-album-bottom > *`: the facts, the blurb, the status and the actions rise in turn.
    let mut part = 0;
    let mut next = || {
        let beat = entry.bottom(part);
        part += 1;
        beat
    };
    let facts_in = next();
    let mut facts: Vec<Element<'a, Message>> = Vec::new();
    let facts_ink = fade(paper, facts_in.0);
    let dot = || text("·").size(14).color(fade(facts_ink, 0.5));
    if !album.artists.is_empty() {
        let name = text(album.artists.clone())
            .size(19)
            .font(crate::typeface::font(iced::font::Weight::Black))
            .color(facts_ink);
        let rule = container(space()).width(Length::Fill).height(Length::Fixed(2.0)).style(move |_theme: &Theme| {
            container::Style { background: Some(Background::Color(facts_ink)), ..container::Style::default() }
        });
        facts.push(iced::widget::column![name, rule].width(Length::Shrink).into());
    }
    for fact in std::iter::once(&seam.meta).chain(album.facts.iter().filter(|fact| **fact != seam.meta)) {
        if !facts.is_empty() {
            facts.push(dot().into());
        }
        facts.push(
            text(fact.clone()).size(14).font(crate::typeface::font(iced::font::Weight::Bold)).color(facts_ink).into(),
        );
    }
    let mut bottom = column![rise(row(facts).spacing(14).align_y(Alignment::End).wrap().into(), facts_in)].spacing(12);
    if let Some(about) = &album.description {
        let beat = next();
        let blurb = container(
            text(about.clone()).size(13).color(fade(paper, 0.74 * beat.0)).wrapping(iced::widget::text::Wrapping::None),
        )
        .width(Length::Fill)
        .clip(true);
        bottom = bottom.push(rise(blurb.into(), beat));
    }
    if let Some(status) = &host.status {
        let beat = next();
        bottom = bottom.push(rise(text(status.clone()).size(12).color(fade(paper, beat.0)).into(), beat));
    }
    let actions_in = next();
    let alpha = actions_in.0;
    // `.bravais-poster-button`: 46px, a 2px ink border and a hard 5px ink shadow; the first is the accent.
    let poster_button = |glyph: Icon, label: &'a str, primary: bool, message: Option<Message>| {
        let (fill, tint) = if primary { (palette.accent, paper) } else { (paper, ink) };
        let weight = if primary { iced::font::Weight::Black } else { iced::font::Weight::ExtraBold };
        let face = row![
            icon(glyph, 16.0, fade(tint, alpha)),
            text(label).size(15).font(crate::typeface::font(weight)).color(fade(tint, alpha))
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        let element: Element<'a, Message> =
            button(container(face).height(Length::Fixed(46.0)).align_y(Alignment::Center))
                .padding([0.0, 20.0])
                .on_press_maybe(message)
                .style(move |_theme: &Theme, status| {
                    let hot = !primary && matches!(status, button::Status::Hovered | button::Status::Pressed);
                    let fill = if hot {
                        Color {
                            r: paper.r * 0.86 + ink.r * 0.14,
                            g: paper.g * 0.86 + ink.g * 0.14,
                            b: paper.b * 0.86 + ink.b * 0.14,
                            a: 1.0,
                        }
                    } else {
                        fill
                    };
                    button::Style {
                        background: Some(Background::Color(fade(fill, alpha))),
                        border: Border { radius: 0.0.into(), width: 2.0, color: fade(ink, alpha) },
                        shadow: iced::Shadow {
                            color: fade(ink, alpha),
                            offset: Vector::new(5.0, 5.0),
                            blur_radius: 0.0,
                        },
                        ..button::Style::default()
                    }
                })
                .into();
        element
    };
    let mut actions: Vec<Element<'a, Message>> = vec![
        poster_button(
            Icon::Play,
            super::tr(host, "libraryBravais.playAll"),
            true,
            seam.playable.then_some(Message::PlayAll),
        ),
        poster_button(
            Icon::ListPlus,
            super::tr(host, "libraryBravais.enqueueAll"),
            false,
            seam.playable.then_some(Message::EnqueueAll),
        ),
    ];
    if let Some((saved, pending, title)) = &seam.star {
        actions.push(star(*saved, *pending, title.clone(), palette.accent, fade(paper, alpha)));
    }
    actions.push(chrome_button(
        Icon::Ellipsis,
        super::tr(host, "libraryBravaisCollection.more"),
        false,
        fade(paper, alpha),
        Some(Message::CollectionMenu),
    ));
    let actions =
        container(row(actions).spacing(14).align_y(Alignment::Center).wrap()).padding(iced::Padding::ZERO.top(6.0));
    bottom = bottom.push(rise(actions.into(), actions_in));
    let bottom =
        container(bottom).padding(iced::Padding { top: 20.0, right: PAD, bottom: 40.0 + host.bottom, left: PAD });
    let ui = container(column![top, stage, title_block, bottom].width(Length::Fill).height(Length::Fill))
        .padding(iced::Padding { top: 0.0, right: album.inset, bottom: 0.0, left: album.inset });
    let mut layers = stack![ground, ui].width(Length::Fixed(width)).height(Length::Fill);
    if seam.menu {
        // `.bravais-seam-more-anchor > .bravais-seam-menu`: above the actions.
        layers = layers.push(
            container(collection_menu(host, paper))
                .width(Length::Fill)
                .height(Length::Fill)
                .align_y(Alignment::End)
                .padding(iced::Padding { top: 0.0, right: PAD, bottom: host.bottom + 40.0 + 46.0 + 12.0, left: PAD }),
        );
    }
    layers.into()
}

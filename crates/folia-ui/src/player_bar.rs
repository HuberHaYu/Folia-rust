// The transport bar at the bottom: track info, controls, progress and volume.

use folia_core::Repeat;
use folia_i18n::I18n;
use iced::widget::{column, container, row, slider, text};
use iced::{Alignment, Color, Element, Length};
use lucide_static_svg::Icon;

use crate::background::format_time;
use crate::icons::{icon, icon_button};
use crate::playback::{Message, Playback};

pub fn view<'a>(playback: &'a Playback, i18n: &'a I18n, text_color: Color, muted: Color) -> Element<'a, Message> {
    let has_track = playback.current().is_some();
    let (title, artist) = match playback.current() {
        Some(track) => {
            (track.title.as_str(), track.artist.as_deref().unwrap_or_else(|| i18n.t("player.unknownArtist")))
        }
        None => (i18n.t("ui.noTrack"), ""),
    };
    let info = column![text(title).size(15), text(artist).size(12).color(muted)].width(Length::Fixed(220.0));

    let (play_icon, play_label) =
        if playback.is_playing() { (Icon::Pause, "player.pause") } else { (Icon::Play, "player.play") };
    let controls = row![
        icon_button(
            Icon::SkipBack,
            18.0,
            text_color,
            i18n.t("ui.previousTrack"),
            has_track.then_some(Message::Previous)
        ),
        icon_button(play_icon, 22.0, text_color, i18n.t(play_label), has_track.then_some(Message::Toggle)),
        icon_button(Icon::SkipForward, 18.0, text_color, i18n.t("ui.nextTrack"), has_track.then_some(Message::Next)),
    ]
    .align_y(Alignment::Center);

    let duration = playback.duration();
    let position = playback.position().min(duration);
    let progress = row![
        text(format_time(position)).size(12).color(muted),
        slider(0.0..=duration.max(0.001), position, Message::Seeking).on_release(Message::SeekDone).step(0.1),
        text(format_time(duration)).size(12).color(muted),
    ]
    .spacing(10)
    .align_y(Alignment::Center)
    .width(Length::Fill);

    let (repeat_icon, repeat_label, repeat_color) = repeat_of(playback, text_color, muted);
    let volume = row![
        icon_button(repeat_icon, 18.0, repeat_color, i18n.t(repeat_label), Some(Message::CycleRepeat)),
        icon(if playback.settings.volume > 0.0 { Icon::Volume2 } else { Icon::VolumeX }, 18.0, muted),
        slider(0.0..=1.0, playback.settings.volume, Message::Volume)
            .on_release(Message::VolumeDone)
            .step(0.01_f32)
            .width(Length::Fixed(110.0)),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    container(row![info, controls, progress, volume].spacing(20).align_y(Alignment::Center))
        .padding([12, 20])
        .width(Length::Fill)
        .into()
}

/// Transport for the now-playing capsule: controls on the first row, the progress on its own row,
/// so a 512px pill can still show the seek bar the way the original expanded capsule does.
pub fn capsule<'a>(
    playback: &'a Playback,
    i18n: &'a I18n,
    text_color: Color,
    muted: Color,
    daylight: bool,
    openness: f32,
) -> Element<'a, Message> {
    let openness = openness.clamp(0.0, 1.0);
    let (rgb, from, to, border) = if daylight {
        ([1.0, 1.0, 1.0], 0.40, 0.60, Color { r: 1.0, g: 1.0, b: 1.0, a: 0.20 })
    } else {
        ([0.0, 0.0, 0.0], 0.20, 0.40, Color { r: 1.0, g: 1.0, b: 1.0, a: 0.05 })
    };
    let fill = Color { r: rgb[0], g: rgb[1], b: rgb[2], a: from + (to - from) * openness };
    let body: Element<'a, Message> = if openness < 0.45 {
        container(progress(playback, muted)).width(Length::Fill).padding([8, 16]).into()
    } else {
        expanded(playback, i18n, text_color, muted)
    };
    container(body)
        .width(Length::Fill)
        .style(move |_theme| iced::widget::container::Style {
            background: Some(iced::Background::Color(fill)),
            border: iced::Border { radius: 28.0.into(), width: 1.0, color: border },
            ..iced::widget::container::Style::default()
        })
        .into()
}

fn expanded<'a>(playback: &'a Playback, i18n: &'a I18n, text_color: Color, muted: Color) -> Element<'a, Message> {
    let has_track = playback.current().is_some();
    let (title, artist) = match playback.current() {
        Some(track) => {
            (track.title.as_str(), track.artist.as_deref().unwrap_or_else(|| i18n.t("player.unknownArtist")))
        }
        None => (i18n.t("ui.noTrack"), ""),
    };
    let info = column![text(title).size(14), text(artist).size(11).color(muted)].width(Length::Fill);
    let (repeat_icon, repeat_label, repeat_color) = repeat_of(playback, text_color, muted);
    let top = row![
        info,
        transport(playback, i18n, text_color, has_track),
        icon_button(repeat_icon, 16.0, repeat_color, i18n.t(repeat_label), Some(Message::CycleRepeat)),
        icon(if playback.settings.volume > 0.0 { Icon::Volume2 } else { Icon::VolumeX }, 16.0, muted),
        slider(0.0..=1.0, playback.settings.volume, Message::Volume)
            .on_release(Message::VolumeDone)
            .step(0.01_f32)
            .width(Length::Fixed(72.0)),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    container(column![top, progress(playback, muted)].spacing(6)).padding([10, 16]).into()
}

fn transport<'a>(playback: &'a Playback, i18n: &'a I18n, text_color: Color, has_track: bool) -> Element<'a, Message> {
    let (play_icon, play_label) =
        if playback.is_playing() { (Icon::Pause, "player.pause") } else { (Icon::Play, "player.play") };
    row![
        icon_button(
            Icon::SkipBack,
            16.0,
            text_color,
            i18n.t("ui.previousTrack"),
            has_track.then_some(Message::Previous)
        ),
        icon_button(play_icon, 18.0, text_color, i18n.t(play_label), has_track.then_some(Message::Toggle)),
        icon_button(Icon::SkipForward, 16.0, text_color, i18n.t("ui.nextTrack"), has_track.then_some(Message::Next)),
    ]
    .align_y(Alignment::Center)
    .into()
}

fn progress<'a>(playback: &'a Playback, muted: Color) -> Element<'a, Message> {
    let duration = playback.duration();
    let position = playback.position().min(duration);
    row![
        text(format_time(position)).size(12).color(muted),
        slider(0.0..=duration.max(0.001), position, Message::Seeking).on_release(Message::SeekDone).step(0.1),
        text(format_time(duration)).size(12).color(muted),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

fn repeat_of(playback: &Playback, text_color: Color, muted: Color) -> (Icon, &'static str, Color) {
    match playback.settings.repeat {
        Repeat::Off => (Icon::Repeat, "player.loopOff", muted),
        Repeat::All => (Icon::Repeat, "player.loopAll", text_color),
        Repeat::One => (Icon::Repeat1, "player.loopOne", text_color),
    }
}

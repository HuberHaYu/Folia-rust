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

    let (repeat_icon, repeat_label, repeat_color) = match playback.settings.repeat {
        Repeat::Off => (Icon::Repeat, "player.loopOff", muted),
        Repeat::All => (Icon::Repeat, "player.loopAll", text_color),
        Repeat::One => (Icon::Repeat1, "player.loopOne", text_color),
    };
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

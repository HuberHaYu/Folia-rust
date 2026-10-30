// A scrolling list of tracks shared by every page that lists songs; only rows in view are built.

use folia_core::Track;
use folia_i18n::I18n;
use iced::widget::scrollable::Viewport;
use iced::widget::{button, column, row, scrollable, space, text};
use iced::{Alignment, Color, Element, Length};

use crate::background::format_time;

const ROW_HEIGHT: f32 = 52.0;

#[derive(Debug, Default)]
pub struct TrackList {
    offset: f32,
    viewport: f32,
}

impl TrackList {
    pub fn scrolled(&mut self, viewport: Viewport) {
        self.offset = viewport.absolute_offset().y;
        self.viewport = viewport.bounds().height;
    }

    pub fn reset(&mut self) {
        self.offset = 0.0;
    }

    pub fn view<'a, M: Clone + 'a>(
        &self,
        tracks: &'a [Track],
        i18n: &'a I18n,
        muted: Color,
        playing: Option<i64>,
        on_scroll: fn(Viewport) -> M,
        on_play: fn(usize) -> M,
    ) -> Element<'a, M> {
        let total = tracks.len();
        let first = ((self.offset / ROW_HEIGHT).floor() as usize).min(total);
        let visible = ((self.viewport.max(900.0) / ROW_HEIGHT).ceil() as usize + 2).min(total - first);
        let rows = tracks[first..first + visible]
            .iter()
            .enumerate()
            .map(|(i, track)| track_row(first + i, track, i18n, muted, playing == Some(track.id), on_play(first + i)));

        let list = column![space().height(first as f32 * ROW_HEIGHT)]
            .extend(rows)
            .push(space().height((total - first - visible) as f32 * ROW_HEIGHT));
        scrollable(list).on_scroll(on_scroll).height(Length::Fill).into()
    }
}

fn track_row<'a, M: Clone + 'a>(
    index: usize,
    track: &'a Track,
    i18n: &'a I18n,
    muted: Color,
    playing: bool,
    on_press: M,
) -> Element<'a, M> {
    let artist = track.artist.as_deref().unwrap_or_else(|| i18n.t("player.unknownArtist"));
    let album = track.album.as_deref().unwrap_or_else(|| i18n.t("player.unknownAlbum"));
    let content = row![
        text((index + 1).to_string()).size(13).color(muted).width(Length::Fixed(40.0)),
        column![text(&track.title).size(15), text(artist).size(12).color(muted)].width(Length::FillPortion(3)),
        text(album).size(13).color(muted).width(Length::FillPortion(2)),
        text(format_time(track.duration)).size(13).color(muted),
    ]
    .spacing(12)
    .align_y(Alignment::Center);

    button(content)
        .height(Length::Fixed(ROW_HEIGHT))
        .width(Length::Fill)
        .style(if playing { button::secondary } else { button::text })
        .on_press(on_press)
        .into()
}

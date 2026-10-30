// Now playing page: the lyric visualizer filling the window.

use folia_i18n::I18n;
use folia_visualizer::Scene;
use iced::widget::{canvas, container, stack, text};
use iced::{Color, Element, Length};

pub fn view<'a, M: 'a>(scene: Scene<'a>, i18n: &'a I18n, muted: Color) -> Element<'a, M> {
    let has_lyrics = !scene.lyrics.lines.is_empty();
    let visualizer = canvas(scene).width(Length::Fill).height(Length::Fill);
    if has_lyrics {
        return visualizer.into();
    }
    stack![visualizer, container(text(i18n.t("ui.noLyricsAvailable")).color(muted)).center(Length::Fill)].into()
}

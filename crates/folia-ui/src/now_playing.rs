// Now playing: the lyric canvas fills the window, with a vignette and the translation under it.

use folia_i18n::I18n;
use folia_render::{Atmosphere, Program};
use folia_visualizer::Scene;
use iced::widget::{canvas, column, container, shader, stack, text};
use iced::{Alignment, Color, Element, Length, Size};

use crate::overlay::{self, Subtitle};

/// `subtitle` is the translation and the next line. `bottom` is how far the block sits above the window edge.
pub fn view<'a, M: 'a>(
    scene: Scene<'a>,
    i18n: &'a I18n,
    muted: Color,
    background: Color,
    window: Size,
    subtitle: Subtitle<'a>,
    bottom: f32,
) -> Element<'a, M> {
    let has_lyrics = !scene.lyrics.lines.is_empty();
    let font_scale = scene.font_scale;
    let time = scene.time as f32;
    let visualizer = canvas(scene).width(Length::Fill).height(Length::Fill);
    let show_glow = subtitle.translation.is_some() || subtitle.upcoming.is_some();
    let ellipse = overlay::glow_ellipse(
        window.width,
        window.height,
        bottom,
        overlay::translation_px(window.width, font_scale),
        overlay::upcoming_px(window.width, font_scale),
        subtitle.translation,
        subtitle.upcoming,
    );
    let atmosphere = shader(Program::new(Atmosphere {
        time,
        vignette: 0.28,
        grain: 0.04,
        glow_center: ellipse.center,
        glow_radius: ellipse.radius,
        glow_rgb: [background.r, background.g, background.b],
        glow_alpha: if show_glow { overlay::OVERLAY_ALPHA } else { 0.0 },
    }))
    .width(Length::Fill)
    .height(Length::Fill);
    let mut layers = stack![visualizer, atmosphere];
    if show_glow {
        let block = subtitle_block(subtitle, muted, background, window.width, font_scale);
        let subtitle = container(block)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::Center)
            .align_y(Alignment::End)
            .padding(iced::Padding { top: 0.0, right: 16.0, bottom, left: 16.0 });
        layers = layers.push(subtitle);
    } else if !has_lyrics {
        let empty = container(text(i18n.t("ui.noLyricsAvailable")).color(muted)).center(Length::Fill);
        layers = layers.push(empty);
    }
    layers.into()
}

fn subtitle_block<'a, M: 'a>(
    subtitle: Subtitle<'a>,
    muted: Color,
    background: Color,
    width: f32,
    scale: f32,
) -> Element<'a, M> {
    let translation_size = overlay::translation_px(width, scale);
    let base = Color { a: overlay::OVERLAY_ALPHA, ..muted };
    let mut rows = column![].spacing(overlay::row_gap(translation_size)).align_x(Alignment::Center);
    if let Some(translation) = subtitle.translation {
        rows = rows.push(lined(translation, translation_size, base, background));
    }
    if let Some(upcoming) = subtitle.upcoming {
        let (size, alpha) = overlay::upcoming_look(width, scale, subtitle.translation.is_some());
        let color = Color { a: alpha, ..muted };
        rows = rows.push(lined(upcoming, size, color, background));
    }
    rows.into()
}

/// The line, with a 1px-down copy of the theme background standing in for `0 1px 2px`.
fn lined<'a, M: 'a>(content: &'a str, size: f32, color: Color, background: Color) -> Element<'a, M> {
    let shadow = Color { a: overlay::SHADOW_ALPHA * color.a, ..background };
    let ghost = container(text(content).size(size).color(shadow).center()).padding(iced::Padding {
        top: overlay::SHADOW_OFFSET_Y,
        right: 0.0,
        bottom: 0.0,
        left: 0.0,
    });
    stack![ghost, text(content).size(size).color(color).center()].into()
}

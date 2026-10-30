// Lucide icons drawn as tinted SVGs and the icon buttons built from them.

use iced::widget::{button, svg, tooltip};
use iced::{Color, Element, Length};
use lucide_static_svg::Icon;

pub fn icon<'a, M: 'a>(icon: Icon, size: f32, color: Color) -> Element<'a, M> {
    svg(svg::Handle::from_memory(icon.svg_bytes()))
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .style(move |_, _| svg::Style { color: Some(color) })
        .into()
}

/// A borderless icon button with a tooltip; `on_press` of `None` disables it.
pub fn icon_button<'a, M: Clone + 'a>(
    lucide: Icon,
    size: f32,
    color: Color,
    label: &str,
    on_press: Option<M>,
) -> Element<'a, M> {
    let button = button(icon(lucide, size, color)).padding(8).style(button::text).on_press_maybe(on_press);
    tooltip(button, iced::widget::text(label.to_owned()).size(12), tooltip::Position::Top)
        .style(iced::widget::container::rounded_box)
        .into()
}

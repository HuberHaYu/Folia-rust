// Lucide icons drawn as tinted SVGs and the icon buttons built from them.

use std::sync::LazyLock;

use iced::widget::svg;
use iced::{Color, Element, Length};
use lucide_static_svg::Icon;

pub fn icon<'a, M: 'a>(icon: Icon, size: f32, color: Color) -> Element<'a, M> {
    tinted(svg::Handle::from_memory(icon.svg_bytes()), size, color)
}

/// Lucide glyphs the original draws with changed attributes, such as `fill="currentColor"`.
#[derive(Debug, Clone, Copy)]
pub enum Variant {
    FilledPlay,
    FilledPause,
    /// The saved star (`.bravais-seam-star.is-on svg { fill: currentColor }`).
    FilledStar,
    /// `strokeWidth={2.4}`.
    BoldChevronLeft,
    BoldChevronRight,
}

static FILLED_PLAY: LazyLock<svg::Handle> =
    LazyLock::new(|| patched(Icon::Play, "fill=\"none\"", "fill=\"currentColor\""));
static FILLED_PAUSE: LazyLock<svg::Handle> =
    LazyLock::new(|| patched(Icon::Pause, "fill=\"none\"", "fill=\"currentColor\""));
static FILLED_STAR: LazyLock<svg::Handle> =
    LazyLock::new(|| patched(Icon::Star, "fill=\"none\"", "fill=\"currentColor\""));
static BOLD_CHEVRON_LEFT: LazyLock<svg::Handle> =
    LazyLock::new(|| patched(Icon::ChevronLeft, "stroke-width=\"2\"", "stroke-width=\"2.4\""));
static BOLD_CHEVRON_RIGHT: LazyLock<svg::Handle> =
    LazyLock::new(|| patched(Icon::ChevronRight, "stroke-width=\"2\"", "stroke-width=\"2.4\""));

pub fn variant<'a, M: 'a>(variant: Variant, size: f32, color: Color) -> Element<'a, M> {
    let handle = match variant {
        Variant::FilledPlay => &FILLED_PLAY,
        Variant::FilledPause => &FILLED_PAUSE,
        Variant::FilledStar => &FILLED_STAR,
        Variant::BoldChevronLeft => &BOLD_CHEVRON_LEFT,
        Variant::BoldChevronRight => &BOLD_CHEVRON_RIGHT,
    };
    tinted(LazyLock::force(handle).clone(), size, color)
}

fn patched(icon: Icon, from: &str, to: &str) -> svg::Handle {
    svg::Handle::from_memory(icon.svg_str().replacen(from, to, 1).into_bytes())
}

fn tinted<'a, M: 'a>(handle: svg::Handle, size: f32, color: Color) -> Element<'a, M> {
    // The SVG tint replaces RGB and keeps the source alpha, so Color.a has to be the widget opacity.
    let opaque = Color { a: 1.0, ..color };
    svg(handle)
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .opacity(color.a)
        .style(move |_, _| svg::Style { color: Some(opaque) })
        .into()
}

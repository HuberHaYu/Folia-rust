// Dragging the bottom bar to a new height, from `PlayerBottomBarPositioner.tsx` and the bar's positioning mode.

use folia_i18n::I18n;
use folia_store::{BOTTOM_BAR_BASE, bottom_bar_max_offset, clamp_bottom_bar_offset};
use iced::widget::canvas::{self, Frame, LineDash, Path, Stroke};
use iced::widget::{button, canvas as canvas_widget, container, mouse_area, row, space, stack, text, tooltip};
use iced::{Alignment, Background, Border, Color, Element, Length, Point, Rectangle, Renderer, Size, mouse};
use lucide_static_svg::Icon;

use crate::icons::icon;

#[derive(Debug, Clone, Copy)]
pub enum Message {
    Grab,
    Reset,
    Cancel,
    Commit,
}

/// The offset being dragged; the saved one stays untouched until the user commits.
#[derive(Debug, Clone, Copy)]
pub struct Positioning {
    pub live: f32,
    /// Pointer height and offset when the bar was grabbed.
    grab: Option<(f32, f32)>,
}

impl Positioning {
    pub fn new(saved: f32) -> Self {
        Self { live: saved, grab: None }
    }

    pub fn grab(&mut self, pointer_y: Option<f32>) {
        self.grab = pointer_y.map(|y| (y, self.live));
    }

    /// Moving up lifts the bar: `origin.offset + (origin.pointerY - clientY)`, kept inside the window's range.
    pub fn drag(&mut self, pointer_y: f32, window_height: f32) {
        if let Some((from_y, from_offset)) = self.grab {
            self.live = clamp_bottom_bar_offset(from_offset + (from_y - pointer_y), window_height);
        }
    }

    pub fn release(&mut self) {
        self.grab = None;
    }

    pub fn reset(&mut self) {
        self.live = BOTTOM_BAR_BASE;
    }
}

/// What the overlay needs from the shell.
pub struct Look<'a> {
    pub i18n: &'a I18n,
    pub accent: Color,
    pub daylight: bool,
    pub window: Size,
    /// The bar's rectangle, where a press grabs it.
    pub bar: Rectangle,
}

/// The dashed range the bar's bottom may take, the chip at the top, and the grab area over the bar.
pub fn view<'a>(look: Look<'a>) -> Element<'a, Message> {
    let max = bottom_bar_max_offset(look.window.height);
    let inset = if look.window.width >= 768.0 { 32.0 } else { 16.0 };
    let range = Range {
        accent: look.accent,
        rect: Rectangle {
            x: inset,
            y: look.window.height - (BOTTOM_BAR_BASE - 12.0) - ((max - BOTTOM_BAR_BASE).max(0.0) + 24.0),
            width: (look.window.width - 2.0 * inset).max(0.0),
            height: (max - BOTTOM_BAR_BASE).max(0.0) + 24.0,
        },
    };
    let range = canvas_widget(range).width(Length::Fill).height(Length::Fill);
    // The bar is a thing to drag now, not a control: one area over it takes the press.
    let grab = mouse_area(space().width(Length::Fixed(look.bar.width)).height(Length::Fixed(look.bar.height)))
        .on_press(Message::Grab)
        .interaction(mouse::Interaction::Pointer);
    let grab = container(grab).padding(iced::Padding { top: look.bar.y, left: look.bar.x, ..iced::Padding::ZERO });
    let chip = container(chip(&look))
        .width(Length::Fill)
        .align_x(Alignment::Center)
        .padding(iced::Padding { top: 32.0, ..iced::Padding::ZERO });
    stack![range, grab, chip].width(Length::Fill).height(Length::Fill).into()
}

/// `rounded-full border px-4 py-2 text-sm shadow-xl`: the hint, a divider, reset, cancel and the accent check.
fn chip<'a>(look: &Look<'a>) -> Element<'a, Message> {
    let (fill, edge, ink) = if look.daylight {
        (Color { a: 0.8, ..Color::WHITE }, Color { a: 0.1, ..Color::BLACK }, Color::BLACK)
    } else {
        (Color { a: 0.6, ..Color::BLACK }, Color { a: 0.15, ..Color::WHITE }, Color::WHITE)
    };
    let quiet = |lucide: Icon, label: &str, message: Message| -> Element<'a, Message> {
        let press = button(container(icon(lucide, 15.0, Color { a: 0.6, ..ink })).center(Length::Fixed(27.0)))
            .padding(0)
            .on_press(message)
            .style(|_, _| button::Style::default());
        tooltip(press, text(label.to_owned()).size(12), tooltip::Position::Bottom).style(container::rounded_box).into()
    };
    let check_ink = if look.daylight { Color::WHITE } else { Color::BLACK };
    let accent = look.accent;
    let commit = button(container(icon(Icon::Check, 15.0, check_ink)).center(Length::Fixed(27.0)))
        .padding(0)
        .on_press(Message::Commit)
        .style(move |_, _| button::Style {
            background: Some(Background::Color(accent)),
            border: Border { radius: 13.5.into(), ..Border::default() },
            ..button::Style::default()
        });
    let commit = tooltip(commit, text(look.i18n.t("options.save").to_owned()).size(12), tooltip::Position::Bottom)
        .style(container::rounded_box);
    let divider = container(space()).width(Length::Fixed(1.0)).height(Length::Fixed(16.0)).style(move |_| {
        container::Style { background: Some(Background::Color(Color { a: 0.2, ..ink })), ..container::Style::default() }
    });
    let body = row![
        text(look.i18n.t("options.playerBottomBarPositioningHint").to_owned()).size(14).color(ink),
        divider,
        quiet(Icon::RotateCcw, look.i18n.t("options.playerBottomBarReset"), Message::Reset),
        quiet(Icon::X, look.i18n.t("ui.cancel"), Message::Cancel),
        commit,
    ]
    .spacing(12)
    .align_y(Alignment::Center);
    container(body)
        .padding([8, 16])
        .style(move |_| container::Style {
            background: Some(Background::Color(fill)),
            border: Border { radius: 999.0.into(), width: 1.0, color: edge },
            shadow: iced::Shadow {
                color: Color { a: 0.1, ..Color::BLACK },
                offset: iced::Vector::new(0.0, 20.0),
                blur_radius: 25.0,
            },
            ..container::Style::default()
        })
        .into()
}

/// `rounded-3xl border-2 border-dashed`, the accent at 32%.
struct Range {
    accent: Color,
    rect: Rectangle,
}

impl<M> canvas::Program<M> for Range {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let rect = self.rect;
        let path = Path::rounded_rectangle(
            Point::new(rect.x + 1.0, rect.y + 1.0),
            Size::new((rect.width - 2.0).max(0.0), (rect.height - 2.0).max(0.0)),
            23.0.into(),
        );
        let stroke = Stroke::default().with_color(Color { a: 0.32, ..self.accent }).with_width(2.0);
        let stroke = Stroke { line_dash: LineDash { segments: &[6.0, 6.0], offset: 0 }, ..stroke };
        frame.stroke(&path, stroke);
        vec![frame.into_geometry()]
    }
}

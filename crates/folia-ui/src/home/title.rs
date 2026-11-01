// Selectable seam titles: native-style pointer activation, keyboard focus and the original hover transition.

use std::time::Instant;

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{Operation, Tree, operation, tree};
use iced::advanced::{Clipboard, Shell, Widget, renderer};
use iced::{Border, Color, Element, Event, Length, Point, Rectangle, Size, Theme, keyboard, mouse};

use super::Message;
use crate::motion::Tween;

/// `shouldToggleSeamFromTitle`: keyboard activation bypasses pointer selection and the 4px drag threshold.
pub fn seam_title_activates(keyboard: bool, down: Option<Point>, at: Point, has_selection: bool) -> bool {
    keyboard || (!has_selection && down.is_none_or(|down| down.distance(at) <= 4.0))
}

pub fn id() -> iced::widget::Id {
    iced::widget::Id::new("bravais-seam-title")
}

pub struct Title<'a> {
    content: Element<'a, Message>,
    title: String,
    ink: Color,
    accent: Color,
    spine: bool,
    horizontal: bool,
    quoted: bool,
}

impl<'a> Title<'a> {
    pub fn new(
        content: impl Into<Element<'a, Message>>,
        title: &str,
        ink: Color,
        accent: Color,
        spine: bool,
        horizontal: bool,
    ) -> Self {
        Self { content: content.into(), title: title.to_owned(), ink, accent, spine, horizontal, quoted: !spine }
    }

    pub fn quotes(mut self, quoted: bool) -> Self {
        self.quoted = quoted;
        self
    }

    fn bounds(&self, layout: Layout<'_>) -> Rectangle {
        let padding = if self.spine {
            iced::Padding::ZERO
        } else if self.horizontal && !self.quoted {
            iced::Padding::from([4.0, 8.0])
        } else {
            iced::Padding::from([6.0, 10.0])
        };
        layout.bounds().expand(padding)
    }

    // The face contains only the title's glyph leaves, with one quote at each end in the full strip.
    fn glyphs(&self, layout: Layout<'_>, mut visit: impl FnMut(usize, Rectangle)) {
        let mut leaf = 0;
        let skip = usize::from(self.quoted);
        let count = self.title.chars().count();
        leaves(layout, &mut |bounds| {
            if leaf >= skip && leaf < skip + count {
                visit(leaf - skip, bounds);
            }
            leaf += 1;
        });
    }

    fn index_at(&self, layout: Layout<'_>, at: Point) -> usize {
        let mut nearest = (f32::INFINITY, 0);
        self.glyphs(layout, |index, bounds| {
            let distance = bounds.center().distance(at);
            if distance < nearest.0 {
                let after = if self.horizontal { at.x >= bounds.center_x() } else { at.y >= bounds.center_y() };
                nearest = (distance, index + usize::from(after));
            }
        });
        nearest.1
    }

    fn selected(&self, state: &State) -> String {
        let (a, b) = state.selection;
        self.title.chars().skip(a.min(b)).take(a.abs_diff(b)).collect()
    }

    fn pointer(
        &self,
        state: &mut State,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        // iced can pass the final cursor of an event batch even to its earlier press event.
        if let Event::Mouse(mouse::Event::CursorMoved { position }) = event {
            state.pointer = Some(*position);
        }
        let at = state.pointer.or(cursor.position());
        let over = at.is_some_and(|at| self.bounds(layout).contains(at) && viewport.contains(at));
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) if over => {
                let Some(at) = at else { return };
                state.focused = true;
                state.keyboard = false;
                let anchor = self.index_at(layout, at);
                state.down = Some((at, anchor));
                let mut on_text = false;
                self.glyphs(layout, |_, bounds| on_text |= bounds.contains(at));
                if on_text {
                    state.selection = (anchor, anchor);
                }
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) if state.down.is_some() => {
                if let Some((down, anchor)) = state.down
                    && down.distance(*position) > 4.0
                {
                    state.selection = (anchor, self.index_at(layout, *position));
                }
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if let Some((down, _)) = state.down.take() {
                    if let Some(at) = at.filter(|_| over) {
                        let has_selection = !self.selected(state).trim().is_empty();
                        let activate = seam_title_activates(false, Some(down), at, has_selection);
                        tracing::debug!(?down, ?at, has_selection, activate, "seam title released");
                        if activate {
                            shell.publish(Message::TitleToggle { spine: !self.spine, keyboard: false });
                        }
                    }
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::CursorLeft) => state.pointer = None,
            Event::Mouse(mouse::Event::ButtonPressed(_)) if !over => state.unfocus(),
            Event::Window(iced::window::Event::Unfocused) => state.unfocus(),
            _ => {}
        }
    }

    fn keyboard(
        &self,
        state: &mut State,
        event: &Event,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        if !state.focused {
            return;
        }
        match event {
            Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, repeat, .. }) => {
                if modifiers.command() {
                    if key.as_ref() == keyboard::Key::Character("c") && state.selection.0 != state.selection.1 {
                        clipboard.write(iced::advanced::clipboard::Kind::Standard, self.selected(state));
                        shell.capture_event();
                    } else if key.as_ref() == keyboard::Key::Character("a") {
                        state.selection = (0, self.title.chars().count());
                        shell.capture_event();
                    }
                    return;
                }
                match key.as_ref() {
                    keyboard::Key::Named(keyboard::key::Named::Enter) if !repeat => {
                        shell.publish(Message::TitleToggle { spine: !self.spine, keyboard: true });
                        shell.capture_event();
                    }
                    keyboard::Key::Named(keyboard::key::Named::Space) => {
                        state.space = true;
                        shell.capture_event();
                    }
                    _ => {}
                }
            }
            Event::Keyboard(keyboard::Event::KeyReleased {
                key: keyboard::Key::Named(keyboard::key::Named::Space),
                ..
            }) if state.space => {
                state.space = false;
                shell.publish(Message::TitleToggle { spine: !self.spine, keyboard: true });
                shell.capture_event();
            }
            _ => {}
        }
    }
}

struct State {
    title: String,
    spine: bool,
    focused: bool,
    keyboard: bool,
    space: bool,
    down: Option<(Point, usize)>,
    pointer: Option<Point>,
    selection: (usize, usize),
    hover: Tween,
}

impl operation::Focusable for State {
    fn is_focused(&self) -> bool {
        self.focused
    }
    fn focus(&mut self) {
        self.focused = true;
        self.keyboard = true;
    }
    fn unfocus(&mut self) {
        self.focused = false;
        self.space = false;
        self.down = None;
        self.selection = (0, 0);
    }
}

use operation::Focusable;

impl Widget<Message, Theme, iced::Renderer> for Title<'_> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State {
            title: self.title.to_owned(),
            spine: self.spine,
            focused: false,
            keyboard: false,
            space: false,
            down: None,
            pointer: None,
            selection: (0, 0),
            hover: Tween::new(0.0, 140.0, folia_visualizer::Ease::new(0.25, 0.1, 0.25, 1.0), Instant::now()),
        })
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        let state = tree.state.downcast_mut::<State>();
        if state.title != self.title || state.spine != self.spine {
            state.title.clone_from(&self.title);
            state.spine = self.spine;
            state.unfocus();
        }
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &iced::Renderer, limits: &layout::Limits) -> layout::Node {
        self.content.as_widget_mut().layout(&mut tree.children[0], renderer, limits)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        operation.focusable(Some(&id()), layout.bounds(), tree.state.downcast_mut::<State>());
        self.content.as_widget_mut().operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        let now = Instant::now();
        let over = cursor.position().is_some_and(|at| self.bounds(layout).contains(at) && viewport.contains(at));
        state.hover.set(if over { 1.0 } else { 0.0 }, now);
        self.pointer(state, event, layout, cursor, shell, viewport);
        self.keyboard(state, event, clipboard, shell);
        if state.hover.running(now) || shell.is_event_captured() {
            shell.request_redraw();
        }
    }

    fn mouse_interaction(
        &self,
        _tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        _renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        if cursor.position().is_some_and(|at| self.bounds(layout).contains(at) && viewport.contains(at)) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::None
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        use iced::advanced::Renderer;
        let state = tree.state.downcast_ref::<State>();
        let hot = state.hover.value(Instant::now());
        let bounds = self.bounds(layout);
        let Some(clip) = viewport.intersection(&bounds.expand(5.0)) else { return };
        renderer.with_layer(clip, |renderer| {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: if self.spine { bounds.expand(5.0) } else { bounds },
                    border: Border { radius: if self.spine { 13.0.into() } else { 14.0.into() }, ..Border::default() },
                    ..renderer::Quad::default()
                },
                Color { a: self.ink.a * 0.06 * hot, ..self.ink },
            );
            if state.focused && state.keyboard {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: bounds.expand(4.0),
                        border: Border {
                            width: 2.0,
                            radius: if self.spine { 10.0.into() } else { 16.0.into() },
                            color: Color { a: self.accent.a * self.ink.a, ..self.accent },
                        },
                        ..renderer::Quad::default()
                    },
                    Color::TRANSPARENT,
                );
            }
            let (a, b) = state.selection;
            self.glyphs(layout, |index, bounds| {
                if index >= a.min(b) && index < a.max(b) {
                    renderer.fill_quad(
                        renderer::Quad { bounds, ..renderer::Quad::default() },
                        Color { a: self.ink.a, ..theme.extended_palette().primary.weak.color },
                    );
                }
            });
            let style = renderer::Style { text_color: Color { a: self.ink.a * (0.35 + 0.25 * hot), ..self.ink } };
            self.content.as_widget().draw(&tree.children[0], renderer, theme, &style, layout, cursor, viewport);
        });
    }
}

fn leaves(layout: Layout<'_>, visit: &mut impl FnMut(Rectangle)) {
    if layout.children().next().is_none() {
        visit(layout.bounds());
    } else {
        for child in layout.children() {
            leaves(child, visit);
        }
    }
}

impl<'a> From<Title<'a>> for Element<'a, Message> {
    fn from(title: Title<'a>) -> Self {
        Element::new(title)
    }
}

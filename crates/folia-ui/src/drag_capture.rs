// Pointer capture for dragging a list row: presses on the handle strip, then moves and the release anywhere.

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{Operation, Tree};
use iced::advanced::{Clipboard, Shell, Widget, overlay, renderer};
use iced::{Element, Event, Length, Point, Rectangle, Size, Theme, Vector, keyboard, mouse};

/// Wraps a list. While `held` it takes every pointer move and the release, inside the list or not
/// (`setPointerCapture`), and Escape; otherwise a left press on the handle strip is reported.
pub struct Capture<'a, Message> {
    content: Element<'a, Message>,
    held: bool,
    grabbing: bool,
    /// The strip at the left whose presses start a drag, and how far down rows reach, in the list's coordinates.
    handle: Rectangle,
    on_press: Box<dyn Fn(Point) -> Message + 'a>,
    on_move: Box<dyn Fn(Point) -> Message + 'a>,
    on_release: Message,
    on_cancel: Message,
}

impl<'a, Message> Capture<'a, Message> {
    pub fn new(
        content: impl Into<Element<'a, Message>>,
        handle: Rectangle,
        on_press: impl Fn(Point) -> Message + 'a,
        on_move: impl Fn(Point) -> Message + 'a,
        on_release: Message,
        on_cancel: Message,
    ) -> Self {
        Self {
            content: content.into(),
            held: false,
            grabbing: false,
            handle,
            on_press: Box::new(on_press),
            on_move: Box::new(on_move),
            on_release,
            on_cancel,
        }
    }

    /// A press is held (`held`), and it has moved far enough to be a drag (`grabbing`).
    pub fn held(mut self, held: bool, grabbing: bool) -> Self {
        self.held = held;
        self.grabbing = grabbing;
        self
    }
}

impl<Message: Clone> Widget<Message, Theme, iced::Renderer> for Capture<'_, Message> {
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
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
        self.content.as_widget_mut().operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        if self.held {
            match event {
                Event::Mouse(mouse::Event::CursorMoved { position }) => {
                    shell.publish((self.on_move)(Point::new(position.x - bounds.x, position.y - bounds.y)));
                    shell.capture_event();
                    return;
                }
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                    shell.publish(self.on_release.clone());
                    shell.capture_event();
                    return;
                }
                Event::Keyboard(keyboard::Event::KeyPressed {
                    key: keyboard::Key::Named(keyboard::key::Named::Escape),
                    ..
                }) => {
                    shell.publish(self.on_cancel.clone());
                    shell.capture_event();
                    return;
                }
                Event::Mouse(mouse::Event::ButtonPressed(_)) => {
                    shell.capture_event();
                    return;
                }
                _ => {}
            }
        }
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
        if self.held || shell.is_event_captured() {
            return;
        }
        if let (Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)), Some(at)) =
            (event, cursor.position_in(bounds))
            && self.handle.contains(at)
        {
            shell.publish((self.on_press)(at));
            shell.capture_event();
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        if self.grabbing {
            return mouse::Interaction::Grabbing;
        }
        if cursor.position_in(layout.bounds()).is_some_and(|at| self.handle.contains(at)) {
            return mouse::Interaction::Grab;
        }
        self.content.as_widget().mouse_interaction(&tree.children[0], layout, cursor, viewport, renderer)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.as_widget().draw(&tree.children[0], renderer, theme, style, layout, cursor, viewport);
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, iced::Renderer>> {
        self.content.as_widget_mut().overlay(&mut tree.children[0], layout, renderer, viewport, translation)
    }
}

impl<'a, Message: Clone + 'a> From<Capture<'a, Message>> for Element<'a, Message> {
    fn from(capture: Capture<'a, Message>) -> Self {
        Element::new(capture)
    }
}

//! A widget that draws its content at another size than it's laid out at: the
//! mini preview is the real screen, scaled down.
//!
//! The content is only a picture: it takes no input.

use crate::app::Message;
use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer::{self, Quad, Renderer as _};
use iced::advanced::widget::{Operation, Tree, Widget};
use iced::advanced::{Clipboard, Shell, mouse};
use iced::border::Border;
use iced::{
    Color, Element, Event, Length, Point, Rectangle, Renderer, Size, Theme, Transformation, Vector,
    window,
};

/// How the content is laid out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fit {
    /// At this size, then scaled to fit the widget, keeping its proportions.
    Whole(Size),
    /// This wide and as tall as fills the widget, then scaled to the widget's width.
    Fill(f32),
    /// At the size it wants (up to this wide), then scaled to fit the widget, up
    /// to this much.
    Natural { max_width: f32, max_scale: f32 },
}

/// A frame around the content: rounds its corners (onto `corners`, what's behind
/// the widget) and outlines it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub radius: f32,
    pub corners: Color,
    pub outline: Color,
}

pub struct Scaled<'a> {
    content: Element<'a, Message>,
    fit: Fit,
    frame: Option<Frame>,
}

pub fn scaled<'a>(content: impl Into<Element<'a, Message>>, fit: Fit) -> Scaled<'a> {
    Scaled {
        content: content.into(),
        fit,
        frame: None,
    }
}

impl Scaled<'_> {
    pub fn frame(mut self, frame: Frame) -> Self {
        self.frame = Some(frame);
        self
    }

    /// Where content of size `content` goes in `bounds`, and its scale.
    fn place(&self, bounds: Rectangle, content: Size) -> (Rectangle, f32) {
        let most = match self.fit {
            Fit::Natural { max_scale, .. } => max_scale,
            Fit::Whole(_) | Fit::Fill(_) => f32::INFINITY,
        };
        let scale = (bounds.width / content.width.max(1.0))
            .min(bounds.height / content.height.max(1.0))
            .min(most);
        // Whole pixels, so the frame and the picture's edges are sharp.
        let size = Size::new(
            (content.width * scale).round(),
            (content.height * scale).round(),
        );
        let at = Point::new(
            (bounds.x + (bounds.width - size.width) / 2.0).round(),
            (bounds.y + (bounds.height - size.height) / 2.0).round(),
        );
        (Rectangle::new(at, size), scale)
    }
}

/// How far the rounding border reaches out of the picture. (A border's inside
/// corners are its outside radius minus its width.)
const REACH: f32 = 16.0;

impl Widget<Message, Theme, Renderer> for Scaled<'_> {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let size = limits.max();
        let inner = match self.fit {
            Fit::Whole(size) => layout::Limits::new(size, size),
            Fit::Fill(width) => {
                let scale = size.width / width.max(1.0);
                let size = Size::new(width, (size.height / scale.max(0.001)).round());
                layout::Limits::new(size, size)
            }
            Fit::Natural { max_width, .. } => {
                layout::Limits::new(Size::ZERO, Size::new(max_width, f32::INFINITY))
            }
        };
        let content = self
            .content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, &inner);
        layout::Node::with_children(size, vec![content])
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let Some(content) = layout.children().next() else {
            return;
        };
        let from = content.bounds();
        if bounds.width < 1.0 || bounds.height < 1.0 || from.width < 1.0 || from.height < 1.0 {
            return;
        }
        let (view, scale) = self.place(bounds, from.size());
        renderer.with_layer(view, |renderer| {
            let transformation = Transformation::translate(view.x, view.y)
                * Transformation::scale(scale)
                * Transformation::translate(-from.x, -from.y);
            renderer.with_transformation(transformation, |renderer| {
                self.content.as_widget().draw(
                    &tree.children[0],
                    renderer,
                    theme,
                    style,
                    content,
                    mouse::Cursor::Unavailable,
                    &from,
                );
            });
        });
        if let Some(frame) = self.frame {
            let around = view.expand(REACH);
            renderer.with_layer(around, |renderer| {
                renderer.fill_quad(
                    Quad {
                        bounds: around,
                        border: Border {
                            color: frame.corners,
                            width: REACH,
                            radius: (frame.radius + REACH).into(),
                        },
                        ..Quad::default()
                    },
                    Color::TRANSPARENT,
                );
                renderer.fill_quad(
                    Quad {
                        bounds: view,
                        border: Border {
                            color: frame.outline,
                            width: 1.0,
                            radius: frame.radius.into(),
                        },
                        ..Quad::default()
                    },
                    Color::TRANSPARENT,
                );
            });
        }
    }

    fn operate(
        &mut self,
        _tree: &mut Tree,
        _layout: Layout<'_>,
        _renderer: &Renderer,
        _operation: &mut dyn Operation,
    ) {
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        // Widgets settle how they look (enabled, not hovered…) on every redraw.
        if let (Event::Window(window::Event::RedrawRequested(_)), Some(content)) =
            (event, layout.children().next())
        {
            self.content.as_widget_mut().update(
                &mut tree.children[0],
                event,
                content,
                mouse::Cursor::Unavailable,
                renderer,
                clipboard,
                shell,
                &content.bounds(),
            );
        }
    }

    fn overlay<'b>(
        &'b mut self,
        _tree: &'b mut Tree,
        _layout: Layout<'b>,
        _renderer: &Renderer,
        _viewport: &Rectangle,
        _translation: Vector,
    ) -> Option<iced::advanced::overlay::Element<'b, Message, Theme, Renderer>> {
        None
    }
}

impl<'a> From<Scaled<'a>> for Element<'a, Message> {
    fn from(scaled: Scaled<'a>) -> Self {
        Element::new(scaled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::widget::Space;

    #[test]
    fn content_is_centered_and_keeps_its_proportions() {
        let widget = |fit| scaled(Space::new(), fit);
        let bounds = Rectangle::new(Point::new(100.0, 50.0), Size::new(448.0, 600.0));
        let size = Size::new(1440.0, 900.0);
        let (view, scale) = widget(Fit::Whole(size)).place(bounds, size);
        assert!((scale - 448.0 / 1440.0).abs() < 1e-6);
        assert_eq!(
            view,
            Rectangle::new(Point::new(100.0, 210.0), Size::new(448.0, 280.0))
        );
        // A tall shape is limited by the height.
        let tall = Size::new(620.0, 1000.0);
        let (view, _) = widget(Fit::Whole(tall)).place(bounds, tall);
        assert_eq!(view.size(), Size::new(372.0, 600.0));
        // Small things only grow so much.
        let fit = Fit::Natural {
            max_width: 500.0,
            max_scale: 2.0,
        };
        assert_eq!(widget(fit).place(bounds, Size::new(100.0, 40.0)).1, 2.0);
        assert!(widget(fit).place(bounds, Size::new(500.0, 60.0)).1 < 1.0);
    }
}

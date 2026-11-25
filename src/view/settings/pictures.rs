//! The small diagrams on picture cards: a screen with what the choice puts on it.

use crate::theme::{DIAGRAM, with_alpha};
use iced::mouse::Cursor;
use iced::widget::canvas::{self, Frame, Geometry, LineCap, Path, Stroke};
use iced::{Color, Point, Rectangle, Renderer, Size, Theme};
use std::f32::consts::TAU;

pub const DIAGRAM_SIZE: Size = Size::new(72.0, 46.0);

/// What a diagram shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Picture {
    // Visualizers
    Bars,
    Radial,
    Wave,
    // Backgrounds
    Solid,
    AlbumArt,
    Ambient,
    /// A shader of the user's own.
    Shader,
    // Now playing
    Header,
    Spotlight,
    Minimal,
    Nothing,
}

pub struct Diagram {
    pub picture: Picture,
    pub color: Color,
}

/// Made-up bar heights.
const HEIGHTS: [f32; 10] = [0.45, 0.75, 1.0, 0.7, 0.9, 0.55, 0.75, 0.6, 0.35, 0.5];

impl<M> canvas::Program<M> for Diagram {
    type State = ();

    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let (w, h) = (bounds.width, bounds.height);
        let color = self.color;
        let faint = with_alpha(color, 0.4);
        let c = Point::new(w / 2.0, h / 2.0);
        let rect = |x: f32, y: f32, w: f32, h: f32, r: f32| {
            Path::rounded_rectangle(Point::new(x, y), Size::new(w, h), r.into())
        };
        let line = |color: Color, width: f32| {
            Stroke::default()
                .with_color(color)
                .with_width(width)
                .with_line_cap(LineCap::Round)
        };

        // The screen.
        let screen = rect(1.0, 1.0, w - 2.0, h - 2.0, 5.0);
        match self.picture {
            Picture::Solid => frame.fill(&screen, with_alpha(color, 0.35)),
            Picture::Ambient => {
                for (x, y, r, a) in [
                    (0.3, 0.4, 12.0, 0.3),
                    (0.55, 0.62, 13.0, 0.4),
                    (0.74, 0.36, 10.0, 0.3),
                ] {
                    let at = Point::new(w * x, h * y);
                    frame.fill(&Path::circle(at, r), with_alpha(color, a));
                }
            }
            Picture::AlbumArt => {
                let (s, x) = (h - 18.0, c.x);
                frame.fill(&rect(x - s / 2.0, 9.0, s, s, 3.0), with_alpha(color, 0.35));
                frame.fill(
                    &Path::circle(Point::new(x + s * 0.18, 9.0 + s * 0.3), 3.5),
                    color,
                );
                let hills = Path::new(|p| {
                    p.move_to(Point::new(x - s / 2.0, 9.0 + s));
                    p.line_to(Point::new(x - s * 0.15, 9.0 + s * 0.5));
                    p.line_to(Point::new(x + s * 0.1, 9.0 + s * 0.78));
                    p.line_to(Point::new(x + s * 0.25, 9.0 + s * 0.62));
                    p.line_to(Point::new(x + s / 2.0, 9.0 + s));
                    p.close();
                });
                frame.fill(&hills, color);
            }
            _ => {}
        }
        frame.stroke(&screen, line(DIAGRAM, 1.5));

        match self.picture {
            Picture::Bars => {
                let (bar, pitch) = (3.0, 4.7);
                let x0 = c.x - (HEIGHTS.len() as f32 * pitch - (pitch - bar)) / 2.0;
                for (i, v) in HEIGHTS.iter().enumerate() {
                    let len = v * (h - 18.0);
                    let x = x0 + i as f32 * pitch;
                    frame.fill(&rect(x, h - 9.0 - len, bar, len, 0.0), color);
                }
            }
            Picture::Radial => {
                let r = 8.5;
                frame.stroke(&Path::circle(c, r), line(color, 1.5));
                for i in 0..16 {
                    let a = i as f32 / 16.0 * TAU;
                    let len = 2.5 + 5.5 * HEIGHTS[(i * 3) % HEIGHTS.len()];
                    let at = |d: f32| Point::new(c.x + d * a.sin(), c.y - d * a.cos());
                    frame.stroke(
                        &Path::line(at(r + 2.5), at(r + 2.5 + len)),
                        line(color, 1.5),
                    );
                }
            }
            Picture::Wave => {
                let wave = Path::new(|p| {
                    let (x0, x1) = (10.0, w - 10.0);
                    let y = |x: f32| {
                        let t = (x - x0) / (x1 - x0);
                        c.y + 11.0 * (t * TAU * 1.5).sin() * (1.0 - 0.45 * t)
                    };
                    p.move_to(Point::new(x0, y(x0)));
                    let mut x = x0;
                    while x < x1 {
                        x = (x + 1.5).min(x1);
                        p.line_to(Point::new(x, y(x)));
                    }
                });
                frame.stroke(&wave, line(color, 2.0));
            }
            Picture::Shader => {
                // Angle brackets around a slash.
                let mark = Path::new(|p| {
                    p.move_to(Point::new(c.x - 9.0, c.y - 7.0));
                    p.line_to(Point::new(c.x - 16.0, c.y));
                    p.line_to(Point::new(c.x - 9.0, c.y + 7.0));
                    p.move_to(Point::new(c.x + 9.0, c.y - 7.0));
                    p.line_to(Point::new(c.x + 16.0, c.y));
                    p.line_to(Point::new(c.x + 9.0, c.y + 7.0));
                    p.move_to(Point::new(c.x + 3.0, c.y - 9.0));
                    p.line_to(Point::new(c.x - 3.0, c.y + 9.0));
                });
                frame.stroke(&mark, line(color, 2.0));
            }
            Picture::Header => {
                frame.fill(&rect(c.x - 17.0, 7.0, 10.0, 10.0, 2.0), color);
                frame.fill(&rect(c.x - 4.0, 7.5, 21.0, 3.5, 1.5), color);
                frame.fill(&rect(c.x - 4.0, 13.0, 14.0, 3.0, 1.5), faint);
                small_bars(&mut frame, c.x, h - 8.0, 12.0, faint);
            }
            Picture::Spotlight => {
                frame.fill(&rect(9.0, 9.0, 18.0, 18.0, 3.0), color);
                frame.fill(&rect(9.0, 30.0, 18.0, 3.0, 1.5), color);
                frame.fill(&rect(9.0, 35.0, 12.0, 2.5, 1.2), faint);
                small_bars(&mut frame, w * 0.68, h - 10.0, 18.0, faint);
            }
            Picture::Minimal => {
                frame.fill(&rect(c.x - 15.0, 8.0, 30.0, 3.0, 1.5), color);
                small_bars(&mut frame, c.x, h - 8.0, 15.0, faint);
            }
            Picture::Nothing => small_bars(&mut frame, c.x, h - 9.0, 18.0, faint),
            Picture::Solid | Picture::AlbumArt | Picture::Ambient => {}
        }
        vec![frame.into_geometry()]
    }
}

/// A few bars centered on `x`, up to `height` tall.
fn small_bars(frame: &mut Frame, x: f32, base: f32, height: f32, color: Color) {
    let heights = &HEIGHTS[..7];
    let (bar, pitch) = (2.5, 4.2);
    let x0 = x - (heights.len() as f32 * pitch - (pitch - bar)) / 2.0;
    for (i, v) in heights.iter().enumerate() {
        let len = v * height;
        frame.fill_rectangle(
            Point::new(x0 + i as f32 * pitch, base - len),
            Size::new(bar, len),
            color,
        );
    }
}

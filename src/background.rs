//! Window background: base color, album-art or ambient layer, dim overlay.

use crate::settings::{AppearanceSettings, BackgroundMode, Motion};
use iced::mouse::Cursor;
use iced::widget::canvas::{self, Frame, Geometry, gradient};
use iced::widget::image::{FilterMethod, Handle};
use iced::{Color, Point, Radians, Rectangle, Renderer, Size, Theme};
use std::f32::consts::TAU;

/// The three cover copies: size and center as fractions of the container, spiral period, direction.
const COPIES: [(f32, f32, f32, f32, f32); 3] = [
    (0.88, 0.50, 0.50, 90.0, 1.0),
    (0.52, 0.28, 0.34, 60.0, -1.0),
    (0.40, 0.74, 0.70, 44.0, 1.0),
];
const AMBIENT_PERIOD: f32 = 70.0;
/// Below this Dim, add a soft scrim behind the header so its text stays readable.
const SCRIM_BELOW_DIM: u8 = 35;
const BASE: Color = crate::theme::BG;

#[derive(Clone, Copy)]
pub enum LayerKind<'a> {
    Art(&'a Handle),
    Ambient(&'a Handle),
}

#[derive(Clone, Copy)]
pub struct Layer<'a> {
    pub kind: LayerKind<'a>,
    pub opacity: f32,
}

pub struct Background<'a> {
    pub settings: &'a AppearanceSettings,
    /// Seconds on a monotonic clock, so motion is frame-rate independent.
    pub time: f32,
    /// Back to front (the outgoing layer first during a cross-fade).
    pub layers: Vec<Layer<'a>>,
    /// Beat pulse, 0..=1 (already scaled by the Intensity setting).
    pub pulse: f32,
}

/// How much the background swells at full pulse.
const PULSE_SCALE: f32 = 0.06;

/// Where and how big a copy is at time `t`, plus its rotation.
pub fn copy_transform(
    index: usize,
    motion: Motion,
    t: f32,
    origin: Point,
    side: f32,
) -> (Rectangle, f32) {
    let (size, cx, cy, period, dir) = COPIES[index];
    let mut center = Point::new(origin.x + cx * side, origin.y + cy * side);
    let mut d = size * side;
    let mut angle = 0.0;
    match motion {
        Motion::Still => {}
        Motion::Spiral => angle = dir * TAU * (t / period).fract(),
        Motion::Drift => {
            // Ease out and back; each half-cycle lasts half the spiral period.
            let s = 0.5 - 0.5 * (TAU * t / period).cos();
            center.x += 0.06 * side * s;
            center.y -= 0.06 * side * s;
            d *= 1.0 + 0.12 * s;
        }
    }
    (
        Rectangle::new(
            Point::new(center.x - d / 2.0, center.y - d / 2.0),
            Size::new(d, d),
        ),
        angle,
    )
}

fn draw_image(frame: &mut Frame, handle: &Handle, rect: Rectangle, angle: f32, opacity: f32) {
    frame.draw_image(
        rect,
        canvas::Image {
            handle: handle.clone(),
            filter_method: FilterMethod::Linear,
            rotation: Radians(angle),
            border_radius: 0.0.into(),
            opacity,
            snap: false,
        },
    );
}

impl<M> canvas::Program<M> for Background<'_> {
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
        let s = self.settings;
        let (w, h) = (bounds.width, bounds.height);

        if s.bg_mode == BackgroundMode::Solid {
            frame.fill_rectangle(
                Point::ORIGIN,
                bounds.size(),
                crate::controls::rgb(s.solid_color.rgb()),
            );
            return vec![frame.into_geometry()];
        }
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), BASE);

        let center = Point::new(w / 2.0, h / 2.0);
        for layer in &self.layers {
            match layer.kind {
                LayerKind::Art(handle) => {
                    // A square container 1.5× the longer side, so rotating copies never expose corners.
                    let side = 1.5 * w.max(h);
                    let origin = Point::new(center.x - side / 2.0, center.y - side / 2.0);
                    for i in 0..COPIES.len() {
                        let (rect, angle) = copy_transform(i, s.motion, self.time, origin, side);
                        draw_image(
                            &mut frame,
                            handle,
                            swell(rect, self.pulse),
                            angle,
                            layer.opacity,
                        );
                    }
                }
                LayerKind::Ambient(handle) => {
                    // Big enough to cover the window at any rotation.
                    let side = 1.25 * (w * w + h * h).sqrt() * (1.0 + PULSE_SCALE * self.pulse);
                    let t = self.time;
                    let (dx, dy, angle) = match s.motion {
                        Motion::Still => (0.0, 0.0, 0.0),
                        Motion::Spiral => (0.0, 0.0, TAU * (t / AMBIENT_PERIOD).fract()),
                        Motion::Drift => (
                            0.03 * side * (TAU * t / 52.0).sin(),
                            0.03 * side * (TAU * t / 46.0).cos(),
                            0.12 * (TAU * t / 40.0).sin(),
                        ),
                    };
                    let rect = Rectangle::new(
                        Point::new(center.x - side / 2.0 + dx, center.y - side / 2.0 + dy),
                        Size::new(side, side),
                    );
                    draw_image(&mut frame, handle, rect, angle, layer.opacity);
                }
            }
        }
        vec![frame.into_geometry()]
    }
}

/// Dim overlay and header scrim. A separate canvas because iced draws a canvas's
/// images above its shapes, so these can't share a frame with the art.
pub struct Overlay {
    pub dim_pct: u8,
    /// Beat pulse, 0..=1: briefly lifts the dim so the background brightens.
    pub pulse: f32,
}

/// Grow a rectangle about its center for the beat pulse.
fn swell(rect: Rectangle, pulse: f32) -> Rectangle {
    let k = 1.0 + PULSE_SCALE * pulse;
    let size = Size::new(rect.width * k, rect.height * k);
    let c = rect.center();
    Rectangle::new(
        Point::new(c.x - size.width / 2.0, c.y - size.height / 2.0),
        size,
    )
}

impl<M> canvas::Program<M> for Overlay {
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
        let dim = self.dim_pct as f32 / 100.0 * (1.0 - 0.3 * self.pulse);
        frame.fill_rectangle(
            Point::ORIGIN,
            bounds.size(),
            Color::from_rgba8(12, 12, 16, dim),
        );

        if self.dim_pct < SCRIM_BELOW_DIM {
            let strength = 0.5 * (1.0 - self.dim_pct as f32 / SCRIM_BELOW_DIM as f32);
            let scrim = gradient::Linear::new(Point::ORIGIN, Point::new(0.0, 180.0))
                .add_stop(0.0, Color::from_rgba(0.0, 0.0, 0.0, strength))
                .add_stop(1.0, Color::TRANSPARENT);
            frame.fill_rectangle(Point::ORIGIN, Size::new(bounds.width, 180.0), scrim);
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pulse_swells_about_the_center() {
        let r = Rectangle::new(Point::new(100.0, 100.0), Size::new(200.0, 100.0));
        assert_eq!(swell(r, 0.0), r);
        let big = swell(r, 1.0);
        assert!((big.width - 200.0 * (1.0 + PULSE_SCALE)).abs() < 1e-3);
        assert!((big.center().x - r.center().x).abs() < 1e-3);
        assert!((big.center().y - r.center().y).abs() < 1e-3);
    }

    #[test]
    fn spiral_periods_and_directions() {
        let o = Point::ORIGIN;
        // Copy A turns clockwise once every 90 s; B counter-clockwise every 60 s.
        let (_, a) = copy_transform(0, Motion::Spiral, 22.5, o, 1000.0);
        assert!((a - TAU / 4.0).abs() < 1e-4);
        let (_, b) = copy_transform(1, Motion::Spiral, 15.0, o, 1000.0);
        assert!((b + TAU / 4.0).abs() < 1e-4);
    }

    #[test]
    fn drift_peaks_at_half_period() {
        let o = Point::ORIGIN;
        let (still, _) = copy_transform(0, Motion::Drift, 0.0, o, 1000.0);
        let (peak, _) = copy_transform(0, Motion::Drift, 45.0, o, 1000.0);
        assert!((still.width - 880.0).abs() < 1e-3);
        assert!((peak.width - 880.0 * 1.12).abs() < 1e-2);
        assert!((peak.center().x - (500.0 + 60.0)).abs() < 1e-2);
        assert!((peak.center().y - (500.0 - 60.0)).abs() < 1e-2);
    }
}

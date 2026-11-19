//! Custom settings controls drawn on canvas.
//!
//! Each control takes a `focused` flag from the app's keyboard-focus model and
//! draws a 2px #F5F5F7 focus ring when set.

use crate::theme::{self, BAR, CUSTOM_RING, TEXT, TEXT_2, TRACK, ZERO_TICK};
use iced::alignment::Vertical;
use iced::mouse::{self, Cursor};
use iced::widget::canvas::{self, Action, Event, Frame, Geometry, Path, Stroke};
use iced::widget::image::{FilterMethod, Handle};
use iced::widget::{Canvas, text::Alignment};
use iced::{Color, Element, Length, Point, Rectangle, Renderer, Size, Theme};

type OnChange<'a, T, M> = Box<dyn Fn(T) -> M + 'a>;

fn label(
    frame: &mut Frame,
    content: &str,
    at: Point,
    align: Alignment,
    color: Color,
    font: iced::Font,
) {
    frame.fill_text(canvas::Text {
        content: content.to_string(),
        position: at,
        color,
        size: 14.0.into(),
        font,
        align_x: align,
        align_y: Vertical::Top,
        ..canvas::Text::default()
    });
}

fn pill(frame: &mut Frame, x: f32, y: f32, w: f32, h: f32, color: Color) {
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let r = (w.min(h) / 2.0).min(3.0);
    frame.fill(
        &Path::rounded_rectangle(Point::new(x, y), Size::new(w, h), r.into()),
        color,
    );
}

/// The keyboard focus ring.
pub fn focus_ring(frame: &mut Frame, rect: Rectangle, radius: f32) {
    frame.stroke(
        &Path::rounded_rectangle(rect.position(), rect.size(), radius.into()),
        Stroke::default().with_color(TEXT).with_width(2.0),
    );
}

fn snap(v: f32, min: f32, max: f32, step: f32) -> f32 {
    (min + ((v - min) / step).round() * step).clamp(min, max)
}

fn pointer_or_default(state: &DragState, bounds: Rectangle, cursor: Cursor) -> mouse::Interaction {
    if state.dragging.is_some() {
        mouse::Interaction::Grabbing
    } else if cursor.is_over(bounds) {
        mouse::Interaction::Pointer
    } else {
        mouse::Interaction::default()
    }
}

#[derive(Default)]
pub struct DragState {
    dragging: Option<usize>,
}

// ---------------------------------------------------------------------------
// Horizontal slider

const LABEL_ROW: f32 = 20.0;
const TRACK_AREA: f32 = 22.0;
const TRACK_INSET: f32 = 8.0;
const LABEL_GAP: f32 = 6.0;
/// Room below the track so the focus ring isn't clipped.
const TRACK_PAD: f32 = 4.0;
const SLIDER_HEIGHT: f32 = LABEL_ROW + LABEL_GAP + TRACK_AREA + TRACK_PAD;
/// The label-less slider (volume) uses a full 44px hit area.
const COMPACT_HEIGHT: f32 = 44.0;

pub struct Slider<'a, M> {
    /// `None` for the compact, label-less variant.
    labels: Option<(String, String)>,
    value: f32,
    min: f32,
    max: f32,
    step: f32,
    accent: Color,
    focused: bool,
    on_change: OnChange<'a, f32, M>,
}

pub fn slider<'a, M: 'a>(
    label: impl Into<String>,
    display: impl Into<String>,
    value: f32,
    (min, max, step): (f32, f32, f32),
    accent: Color,
    focused: bool,
    on_change: impl Fn(f32) -> M + 'a,
) -> Element<'a, M> {
    Canvas::new(Slider {
        labels: Some((label.into(), display.into())),
        value,
        min,
        max,
        step,
        accent,
        focused,
        on_change: Box::new(on_change),
    })
    .width(Length::Fill)
    .height(SLIDER_HEIGHT)
    .into()
}

/// A slider with no label row, for tight spots like the volume popover.
pub fn compact_slider<'a, M: 'a>(
    value: f32,
    (min, max, step): (f32, f32, f32),
    accent: Color,
    focused: bool,
    on_change: impl Fn(f32) -> M + 'a,
) -> Canvas<Slider<'a, M>, M> {
    Canvas::new(Slider {
        labels: None,
        value,
        min,
        max,
        step,
        accent,
        focused,
        on_change: Box::new(on_change),
    })
    .height(COMPACT_HEIGHT)
}

fn track_x(bounds: Rectangle) -> (f32, f32) {
    (TRACK_INSET, bounds.width - TRACK_INSET)
}

fn fraction_at(bounds: Rectangle, cursor: Cursor) -> Option<f32> {
    let p = cursor.position()?;
    let (x0, x1) = track_x(bounds);
    Some(((p.x - bounds.x - x0) / (x1 - x0)).clamp(0.0, 1.0))
}

impl<M> Slider<'_, M> {
    fn track_cy(&self) -> f32 {
        if self.labels.is_some() {
            LABEL_ROW + LABEL_GAP + TRACK_AREA / 2.0
        } else {
            COMPACT_HEIGHT / 2.0
        }
    }
}

impl<M> canvas::Program<M> for Slider<'_, M> {
    type State = DragState;

    fn update(
        &self,
        state: &mut DragState,
        event: &Event,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> Option<Action<M>> {
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                if cursor.is_over(bounds) =>
            {
                state.dragging = Some(0);
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) if state.dragging.is_some() => {}
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                if state.dragging.is_some() =>
            {
                state.dragging = None;
                return Some(Action::capture());
            }
            _ => return None,
        }
        let t = fraction_at(bounds, cursor)?;
        let v = snap(
            self.min + t * (self.max - self.min),
            self.min,
            self.max,
            self.step,
        );
        if (v - self.value).abs() > f32::EPSILON {
            Some(Action::publish((self.on_change)(v)).and_capture())
        } else {
            Some(Action::capture())
        }
    }

    fn draw(
        &self,
        _: &DragState,
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        if let Some((name, display)) = &self.labels {
            label(
                &mut frame,
                name,
                Point::ORIGIN,
                Alignment::Left,
                TEXT,
                theme::INTER_MEDIUM,
            );
            label(
                &mut frame,
                display,
                Point::new(bounds.width, 0.0),
                Alignment::Right,
                TEXT_2,
                theme::INTER,
            );
        }

        let cy = self.track_cy();
        let (x0, x1) = track_x(bounds);
        let t = ((self.value - self.min) / (self.max - self.min)).clamp(0.0, 1.0);
        let hx = x0 + t * (x1 - x0);
        pill(&mut frame, x0, cy - 3.0, hx - 10.0 - x0, 6.0, self.accent);
        pill(&mut frame, hx + 10.0, cy - 3.0, x1 - hx - 10.0, 6.0, TRACK);
        frame.fill(
            &Path::rounded_rectangle(
                Point::new(hx - 3.0, cy - 11.0),
                Size::new(6.0, 22.0),
                3.0.into(),
            ),
            BAR,
        );
        if self.focused {
            focus_ring(
                &mut frame,
                Rectangle::new(
                    Point::new(1.0, cy - 14.0),
                    Size::new(bounds.width - 2.0, 28.0),
                ),
                8.0,
            );
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &DragState,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> mouse::Interaction {
        pointer_or_default(state, bounds, cursor)
    }
}

// ---------------------------------------------------------------------------
// Range slider (two handles)

pub struct RangeSlider<'a, M> {
    label: String,
    display: String,
    lo: f32,
    hi: f32,
    max: f32,
    min_gap: f32,
    accent: Color,
    /// Which handle has keyboard focus (0 = low, 1 = high).
    focused: Option<usize>,
    on_change: OnChange<'a, (f32, f32), M>,
}

/// A 0..=`max` range slider with integer steps whose handles stay `min_gap` apart.
#[allow(clippy::too_many_arguments)]
pub fn range_slider<'a, M: 'a>(
    label: impl Into<String>,
    display: impl Into<String>,
    (lo, hi): (f32, f32),
    max: f32,
    min_gap: f32,
    accent: Color,
    focused: Option<usize>,
    on_change: impl Fn((f32, f32)) -> M + 'a,
) -> Element<'a, M> {
    Canvas::new(RangeSlider {
        label: label.into(),
        display: display.into(),
        lo,
        hi,
        max,
        min_gap,
        accent,
        focused,
        on_change: Box::new(on_change),
    })
    .width(Length::Fill)
    .height(SLIDER_HEIGHT)
    .into()
}

/// Move one handle of a range by a keyboard step, keeping the gap.
pub fn step_range(
    (lo, hi): (f32, f32),
    handle: usize,
    step: crate::focus::Step,
    max: f32,
    min_gap: f32,
) -> (f32, f32) {
    if handle == 0 {
        let v = step.apply(lo, (0.0, hi - min_gap, 1.0));
        (v, hi)
    } else {
        let v = step.apply(hi, (lo + min_gap, max, 1.0));
        (lo, v)
    }
}

impl<M> canvas::Program<M> for RangeSlider<'_, M> {
    type State = DragState;

    fn update(
        &self,
        state: &mut DragState,
        event: &Event,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> Option<Action<M>> {
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                if cursor.is_over(bounds) =>
            {
                let t = fraction_at(bounds, cursor)? * self.max;
                // Grab the nearer handle; ties go to the side the cursor is on.
                let handle = if (t - self.lo).abs() < (t - self.hi).abs()
                    || ((t - self.lo).abs() == (t - self.hi).abs() && t < self.lo)
                {
                    0
                } else {
                    1
                };
                state.dragging = Some(handle);
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) if state.dragging.is_some() => {}
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                if state.dragging.is_some() =>
            {
                state.dragging = None;
                return Some(Action::capture());
            }
            _ => return None,
        }
        let v = (fraction_at(bounds, cursor)? * self.max).round();
        let (lo, hi) = match state.dragging {
            Some(0) => (v.min(self.hi - self.min_gap).max(0.0), self.hi),
            _ => (self.lo, v.max(self.lo + self.min_gap).min(self.max)),
        };
        if lo != self.lo || hi != self.hi {
            Some(Action::publish((self.on_change)((lo, hi))).and_capture())
        } else {
            Some(Action::capture())
        }
    }

    fn draw(
        &self,
        _: &DragState,
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        label(
            &mut frame,
            &self.label,
            Point::ORIGIN,
            Alignment::Left,
            TEXT,
            theme::INTER_MEDIUM,
        );
        label(
            &mut frame,
            &self.display,
            Point::new(bounds.width, 0.0),
            Alignment::Right,
            TEXT_2,
            theme::INTER,
        );

        let cy = LABEL_ROW + LABEL_GAP + TRACK_AREA / 2.0;
        let (x0, x1) = track_x(bounds);
        let lx = x0 + self.lo / self.max * (x1 - x0);
        let hx = x0 + self.hi / self.max * (x1 - x0);
        pill(&mut frame, x0, cy - 3.0, lx - 10.0 - x0, 6.0, TRACK);
        pill(
            &mut frame,
            lx + 10.0,
            cy - 3.0,
            hx - lx - 20.0,
            6.0,
            self.accent,
        );
        pill(&mut frame, hx + 10.0, cy - 3.0, x1 - hx - 10.0, 6.0, TRACK);
        for (k, x) in [lx, hx].into_iter().enumerate() {
            frame.fill(
                &Path::rounded_rectangle(
                    Point::new(x - 3.0, cy - 11.0),
                    Size::new(6.0, 22.0),
                    3.0.into(),
                ),
                BAR,
            );
            if self.focused == Some(k) {
                focus_ring(
                    &mut frame,
                    Rectangle::new(Point::new(x - 8.0, cy - 14.0), Size::new(16.0, 28.0)),
                    6.0,
                );
            }
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &DragState,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> mouse::Interaction {
        pointer_or_default(state, bounds, cursor)
    }
}

// ---------------------------------------------------------------------------
// Vertical band slider (−12..=+12 dB)

const BAND_TRACK: f32 = 84.0;
const BAND_TOP: f32 = 20.0 + 8.0;
const BAND_HEIGHT: f32 = BAND_TOP + BAND_TRACK + 8.0 + 20.0;

pub struct BandSlider<'a, M> {
    label: &'static str,
    display: String,
    db: i8,
    accent: Color,
    focused: bool,
    on_change: OnChange<'a, i8, M>,
}

pub fn band_slider<'a, M: 'a>(
    label: &'static str,
    db: i8,
    accent: Color,
    focused: bool,
    on_change: impl Fn(i8) -> M + 'a,
) -> Element<'a, M> {
    Canvas::new(BandSlider {
        label,
        display: crate::settings::format_db(db),
        db,
        accent,
        focused,
        on_change: Box::new(on_change),
    })
    .width(Length::Fill)
    .height(BAND_HEIGHT)
    .into()
}

impl<M> canvas::Program<M> for BandSlider<'_, M> {
    type State = DragState;

    fn update(
        &self,
        state: &mut DragState,
        event: &Event,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> Option<Action<M>> {
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                if cursor.is_over(bounds) =>
            {
                state.dragging = Some(0);
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) if state.dragging.is_some() => {}
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                if state.dragging.is_some() =>
            {
                state.dragging = None;
                return Some(Action::capture());
            }
            _ => return None,
        }
        let p = cursor.position()?;
        let t = ((bounds.y + BAND_TOP + BAND_TRACK - p.y) / BAND_TRACK).clamp(0.0, 1.0);
        let db = (t * 24.0 - 12.0).round() as i8;
        if db != self.db {
            Some(Action::publish((self.on_change)(db)).and_capture())
        } else {
            Some(Action::capture())
        }
    }

    fn draw(
        &self,
        _: &DragState,
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let cx = bounds.width / 2.0;
        label(
            &mut frame,
            &self.display,
            Point::new(cx, 0.0),
            Alignment::Center,
            TEXT,
            theme::INTER,
        );

        let top = BAND_TOP;
        let zero = top + BAND_TRACK / 2.0;
        pill(&mut frame, cx - 3.0, top, 6.0, BAND_TRACK, TRACK);
        frame.fill_rectangle(
            Point::new(cx - 9.0, zero - 1.0),
            Size::new(18.0, 2.0),
            ZERO_TICK,
        );

        let y = top + BAND_TRACK * (1.0 - (self.db as f32 + 12.0) / 24.0);
        let (a, b) = if y < zero { (y, zero) } else { (zero, y) };
        if b - a > 0.5 {
            frame.fill_rectangle(Point::new(cx - 3.0, a), Size::new(6.0, b - a), self.accent);
        }
        frame.fill(
            &Path::rounded_rectangle(
                Point::new(cx - 11.0, y - 3.0),
                Size::new(22.0, 6.0),
                3.0.into(),
            ),
            BAR,
        );
        label(
            &mut frame,
            self.label,
            Point::new(cx, top + BAND_TRACK + 8.0),
            Alignment::Center,
            TEXT_2,
            theme::INTER,
        );
        if self.focused {
            focus_ring(
                &mut frame,
                Rectangle::new(
                    Point::new(cx - 18.0, top - 8.0),
                    Size::new(36.0, BAND_TRACK + 16.0),
                ),
                10.0,
            );
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &DragState,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> mouse::Interaction {
        pointer_or_default(state, bounds, cursor)
    }
}

// ---------------------------------------------------------------------------
// Switch

pub struct Switch<M> {
    on: bool,
    accent: Color,
    focused: bool,
    message: M,
}

pub fn switch<'a, M: Clone + 'a>(
    on: bool,
    accent: Color,
    focused: bool,
    message: M,
) -> Element<'a, M> {
    Canvas::new(Switch {
        on,
        accent,
        focused,
        message,
    })
    .width(52.0)
    .height(44.0)
    .into()
}

impl<M: Clone> canvas::Program<M> for Switch<M> {
    type State = ();

    fn update(
        &self,
        _: &mut (),
        event: &Event,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> Option<Action<M>> {
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                if cursor.is_over(bounds) =>
            {
                Some(Action::publish(self.message.clone()).and_capture())
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let x = bounds.width - 44.0;
        let y = (bounds.height - 26.0) / 2.0;
        frame.fill(
            &Path::rounded_rectangle(Point::new(x, y), Size::new(44.0, 26.0), 13.0.into()),
            if self.on { self.accent } else { TRACK },
        );
        let kx = if self.on { x + 44.0 - 13.0 } else { x + 13.0 };
        frame.fill(&Path::circle(Point::new(kx, y + 13.0), 10.0), Color::WHITE);
        if self.focused {
            focus_ring(
                &mut frame,
                Rectangle::new(Point::new(x - 4.0, y - 4.0), Size::new(52.0 - 1.0, 34.0)),
                17.0,
            );
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(&self, _: &(), bounds: Rectangle, cursor: Cursor) -> mouse::Interaction {
        if cursor.is_over(bounds) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}

// ---------------------------------------------------------------------------
// Color swatches

/// What a swatch dot shows.
pub enum SwatchFill {
    /// One color, or a gradient through several.
    Stops(Vec<[u8; 3]>),
    /// A tiny image (the album cover).
    Image(Handle),
    /// The "+" custom swatch before anything is picked.
    Add,
}

pub struct Swatches<'a, M> {
    swatches: Vec<SwatchFill>,
    selected: usize,
    /// Faint inner ring so very dark swatches stay visible on the panel.
    inner_ring: bool,
    focused: bool,
    on_pick: OnChange<'a, usize, M>,
}

const SWATCH: f32 = 44.0;
const SWATCH_GAP: f32 = 12.0;
const SWATCH_MIN_GAP: f32 = 6.0;
/// Room around the dots for the keyboard focus ring.
const SWATCH_MARGIN: f32 = 4.0;

pub fn swatches<'a, M: 'a>(
    swatches: Vec<SwatchFill>,
    selected: usize,
    inner_ring: bool,
    focused: bool,
    on_pick: impl Fn(usize) -> M + 'a,
) -> Element<'a, M> {
    Canvas::new(Swatches {
        swatches,
        selected,
        inner_ring,
        focused,
        on_pick: Box::new(on_pick),
    })
    .width(Length::Fill)
    .height(SWATCH + 2.0 * SWATCH_MARGIN)
    .into()
}

impl<M> Swatches<'_, M> {
    /// Swatch size and gap for this width. A long row tightens the gap first and
    /// only then shrinks the swatches, so it never runs past the panel edge.
    fn layout(&self, width: f32) -> (f32, f32) {
        let n = self.swatches.len().max(1) as f32;
        let avail = width - 2.0 * SWATCH_MARGIN;
        if n < 2.0 {
            return (SWATCH.min(avail), SWATCH_GAP);
        }
        let gap = ((avail - n * SWATCH) / (n - 1.0)).clamp(SWATCH_MIN_GAP, SWATCH_GAP);
        let size = ((avail - (n - 1.0) * gap) / n).clamp(24.0, SWATCH);
        (size, gap)
    }

    fn hit(&self, bounds: Rectangle, cursor: Cursor) -> Option<usize> {
        let p = cursor.position_in(bounds)?;
        let (size, gap) = self.layout(bounds.width);
        let x = p.x - SWATCH_MARGIN;
        if x < 0.0 {
            return None;
        }
        let i = (x / (size + gap)) as usize;
        let local = x - i as f32 * (size + gap);
        (i < self.swatches.len() && local <= size).then_some(i)
    }
}

pub fn rgb(c: [u8; 3]) -> Color {
    Color::from_rgb8(c[0], c[1], c[2])
}

impl<M> canvas::Program<M> for Swatches<'_, M> {
    type State = ();

    fn update(
        &self,
        _: &mut (),
        event: &Event,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> Option<Action<M>> {
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let i = self.hit(bounds, cursor)?;
                Some(Action::publish((self.on_pick)(i)).and_capture())
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let (size, gap) = self.layout(bounds.width);
        for (i, fill) in self.swatches.iter().enumerate() {
            let x = SWATCH_MARGIN + i as f32 * (size + gap);
            let c = Point::new(x + size / 2.0, SWATCH_MARGIN + SWATCH / 2.0);
            let r = size / 2.0 - 4.0;
            match fill {
                SwatchFill::Image(handle) => {
                    frame.draw_image(
                        Rectangle::new(Point::new(c.x - r, c.y - r), Size::new(2.0 * r, 2.0 * r)),
                        canvas::Image {
                            handle: handle.clone(),
                            filter_method: FilterMethod::Linear,
                            rotation: iced::Radians(0.0),
                            border_radius: r.into(),
                            opacity: 1.0,
                            snap: false,
                        },
                    );
                }
                SwatchFill::Stops(stops) => {
                    let inner = Path::circle(c, r);
                    if stops.len() == 1 {
                        frame.fill(&inner, rgb(stops[0]));
                    } else {
                        let mut g = canvas::gradient::Linear::new(
                            Point::new(c.x - r * 0.7, c.y - r * 0.7),
                            Point::new(c.x + r * 0.7, c.y + r * 0.7),
                        );
                        for (k, s) in stops.iter().enumerate() {
                            g = g.add_stop(k as f32 / (stops.len() - 1) as f32, rgb(*s));
                        }
                        frame.fill(&inner, g);
                    }
                }
                SwatchFill::Add => {
                    frame.stroke(
                        &Path::circle(c, size / 2.0 - 1.0),
                        Stroke {
                            line_dash: canvas::LineDash {
                                segments: &[4.0, 4.0],
                                offset: 0,
                            },
                            ..Stroke::default().with_color(CUSTOM_RING).with_width(2.0)
                        },
                    );
                    let plus = Path::new(|p| {
                        p.move_to(Point::new(c.x - 6.0, c.y));
                        p.line_to(Point::new(c.x + 6.0, c.y));
                        p.move_to(Point::new(c.x, c.y - 6.0));
                        p.line_to(Point::new(c.x, c.y + 6.0));
                    });
                    frame.stroke(
                        &plus,
                        Stroke::default()
                            .with_color(TEXT_2)
                            .with_width(2.0)
                            .with_line_cap(canvas::LineCap::Round),
                    );
                }
            }
            if self.inner_ring && !matches!(fill, SwatchFill::Add) {
                frame.stroke(
                    &Path::circle(c, r - 0.5),
                    Stroke::default()
                        .with_color(Color::from_rgba(1.0, 1.0, 1.0, 0.2))
                        .with_width(1.0),
                );
            }
            if i == self.selected {
                frame.stroke(
                    &Path::circle(c, size / 2.0 - 1.0),
                    Stroke::default().with_color(TEXT).with_width(2.0),
                );
            }
        }
        if self.focused {
            // Ring around the whole row; arrows move the selection inside it.
            let n = self.swatches.len() as f32;
            let w = n * size + (n - 1.0) * gap + 2.0 * SWATCH_MARGIN - 2.0;
            focus_ring(
                &mut frame,
                Rectangle::new(Point::new(1.0, 1.0), Size::new(w, bounds.height - 2.0)),
                26.0,
            );
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(&self, _: &(), bounds: Rectangle, cursor: Cursor) -> mouse::Interaction {
        if self.hit(bounds, cursor).is_some() {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}

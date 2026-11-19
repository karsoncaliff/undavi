//! The tuning meter: live input level, gate, level range and beats, shown over the
//! bars while the Tuning panel is open so its sliders have something to point at.

use crate::app::{App, Message};
use crate::settings::GATE_OFF;
use crate::theme::{
    self, INTER_MEDIUM, INTER_SEMIBOLD, TEXT, TEXT_2, TEXT_3, TRACK, tracked, with_alpha,
};
use iced::mouse::Cursor;
use iced::widget::canvas::{self, Frame, Geometry, Path};
use iced::widget::{Canvas, column, container, row, text};
use iced::{Alignment, Color, Element, Length, Point, Rectangle, Renderer, Size, Theme};

/// Meters span this range, in dB.
const DB_RANGE: (f32, f32) = (-100.0, 0.0);
const WIDTH: f32 = 300.0;
const LABEL_W: f32 = 56.0;
const VALUE_W: f32 = 64.0;
/// The beat dot lights up above this pulse.
const HIT: f32 = 0.5;

fn db_pos(db: f32) -> f32 {
    ((db - DB_RANGE.0) / (DB_RANGE.1 - DB_RANGE.0)).clamp(0.0, 1.0)
}

/// `−42 dB`, or a dash without a reading.
fn db_text(db: Option<f32>) -> String {
    match db {
        Some(db) if db > DB_RANGE.0 => {
            let v = db.round() as i32;
            if v < 0 {
                format!("\u{2212}{} dB", -v)
            } else {
                format!("{v} dB")
            }
        }
        Some(_) => format!("<\u{2212}{} dB", -DB_RANGE.0 as i32),
        None => "\u{2013}".into(),
    }
}

/// One horizontal meter: a fill, an optional shaded band and an optional marker line.
struct Bar {
    /// Fill, 0..=1.
    value: f32,
    color: Color,
    /// Shaded range behind the fill (the Tuning level range), 0..=1.
    band: Option<(f32, f32)>,
    /// A tick across the bar (the gate threshold), 0..=1.
    marker: Option<f32>,
}

const BAR_H: f32 = 6.0;
const CANVAS_H: f32 = 14.0;

impl canvas::Program<Message> for Bar {
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
        let w = bounds.width;
        let y = (CANVAS_H - BAR_H) / 2.0;
        let pill = |x0: f32, x1: f32| {
            Path::rounded_rectangle(
                Point::new(x0, y),
                Size::new((x1 - x0).max(0.0), BAR_H),
                (BAR_H / 2.0).into(),
            )
        };
        frame.fill(&pill(0.0, w), TRACK);
        if let Some((lo, hi)) = self.band {
            frame.fill(&pill(lo * w, hi * w), with_alpha(TEXT, 0.16));
        }
        if self.value > 0.0 {
            frame.fill(&pill(0.0, (self.value * w).max(BAR_H)), self.color);
        }
        // Small ticks at the band's ends, so it reads even under the fill.
        if let Some((lo, hi)) = self.band {
            for x in [lo * w, hi * w] {
                let x = x.clamp(1.0, w - 1.0);
                frame.fill_rectangle(
                    Point::new(x - 1.0, 1.0),
                    Size::new(2.0, CANVAS_H - 2.0),
                    TEXT_3,
                );
            }
        }
        if let Some(m) = self.marker {
            let x = (m * w).clamp(1.0, w - 1.0);
            frame.fill_rectangle(Point::new(x - 1.0, 0.0), Size::new(2.0, CANVAS_H), TEXT);
        }
        vec![frame.into_geometry()]
    }
}

/// The beat light: a dot that fills with the pulse.
struct Dot {
    pulse: f32,
    color: Color,
}

impl canvas::Program<Message> for Dot {
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
        let c = Point::new(bounds.width / 2.0, bounds.height / 2.0);
        frame.fill(&Path::circle(c, 5.0), TRACK);
        let lit = if self.pulse >= HIT { 1.0 } else { self.pulse };
        frame.fill(&Path::circle(c, 5.0), with_alpha(self.color, lit));
        vec![frame.into_geometry()]
    }
}

fn meter_row<'a>(
    label: &'a str,
    meter: impl Into<Element<'a, Message>>,
    value: Element<'a, Message>,
) -> Element<'a, Message> {
    row![
        text(label)
            .font(INTER_MEDIUM)
            .size(13)
            .color(TEXT_2)
            .width(LABEL_W),
        container(meter).width(Length::Fill),
        container(value).align_right(VALUE_W),
    ]
    .spacing(10)
    .align_y(Alignment::Center)
    .into()
}

fn bar<'a>(b: Bar) -> Element<'a, Message> {
    Canvas::new(b).width(Length::Fill).height(CANVAS_H).into()
}

fn value<'a>(s: String) -> Element<'a, Message> {
    text(s).size(12).color(TEXT).into()
}

impl App {
    /// The meter card shown over the bars while Tuning is open.
    pub(crate) fn tuning_meter(&self) -> Element<'_, Message> {
        let t = &self.tuning;
        let accent = self.accent();
        let m = self.meter;
        let gate_on = t.gate_db > GATE_OFF;
        let sound = self.gate.is_open();

        // Input: overall level against the noise gate.
        let input = Bar {
            value: m.input_db.map_or(0.0, db_pos),
            color: if sound { accent } else { TEXT_3 },
            band: None,
            marker: gate_on.then(|| db_pos(t.gate_db as f32)),
        };
        // Bars: the loudest bar against the level range that maps to empty..full.
        let bars = Bar {
            value: m.loudest_db.map_or(0.0, db_pos),
            color: accent,
            band: Some((db_pos(t.floor_db as f32), db_pos(t.ceiling_db as f32))),
            marker: None,
        };
        let pulse = self.beat.pulse();
        let beat = Bar {
            value: pulse,
            color: accent,
            band: None,
            marker: None,
        };

        let mut status = vec![if m.input_db.is_none() {
            "No audio".to_string()
        } else if sound {
            "Sound".to_string()
        } else if gate_on {
            "Silent (below the gate)".to_string()
        } else {
            "Silent".to_string()
        }];
        if t.auto_gain {
            status.push(format!("Auto-gain \u{00D7}{:.1}", self.processor.gain()));
        }

        container(
            column![
                text(tracked("Meter"))
                    .font(INTER_SEMIBOLD)
                    .size(11)
                    .color(TEXT_3),
                meter_row("Input", bar(input), value(db_text(m.input_db))),
                meter_row("Bars", bar(bars), value(db_text(m.loudest_db))),
                meter_row(
                    "Beat",
                    bar(beat),
                    Canvas::new(Dot {
                        pulse,
                        color: accent
                    })
                    .width(14)
                    .height(CANVAS_H)
                    .into(),
                ),
                text(status.join("  \u{00B7}  ")).size(12).color(TEXT_3),
            ]
            .spacing(8),
        )
        .width(WIDTH)
        .padding([14, 16])
        .style(theme::panel_translucent)
        .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_labels() {
        assert_eq!(db_text(Some(-41.6)), "\u{2212}42 dB");
        assert_eq!(db_text(Some(0.2)), "0 dB");
        assert_eq!(db_text(Some(-130.0)), "<\u{2212}100 dB");
        assert_eq!(db_text(None), "\u{2013}");
        assert_eq!(db_pos(-50.0), 0.5);
        assert_eq!(db_pos(10.0), 1.0);
    }
}

//! The signal monitor: what the visualizer is working with, to help tune it, in the
//! settings window's Sound section. While it's on and the settings are open, it keeps
//! a few seconds of history: input level, the loudest bar, beats and frame times, plus
//! the latest spectrum. Its rows are built in `view/settings/monitor.rs`; this module
//! holds the data, the numbers and tips derived from it, and the graphs.

use crate::app::Message;
use crate::settings::{FreqScale, GATE_OFF, TuningSettings, slider_to_hz};
use crate::theme::{TEXT, TEXT_2, TEXT_3, TRACK, with_alpha};
use iced::mouse::Cursor;
use iced::widget::canvas::{self, Frame, Geometry, LineDash, Path, Stroke};
use iced::{Color, Point, Rectangle, Renderer, Size, Theme};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// How much history the graphs show.
pub const HISTORY: Duration = Duration::from_secs(8);
/// Graphs span this range, in dB.
const DB_RANGE: (f32, f32) = (-100.0, 0.0);
/// The spectrum's frequency axis, in Hz.
const HZ_RANGE: (f32, f32) = (20.0, 20_000.0);
/// A beat counts as a hit when the pulse rises past this…
const HIT: f32 = 0.5;
/// …and the next one needs it to fall below this first.
const REARM: f32 = 0.3;
/// A bar this close to the top counts as full height.
const FULL: f32 = 0.99;

/// One frame's readings.
#[derive(Clone, Copy, Debug)]
pub struct Sample {
    pub at: Instant,
    /// Time since the previous frame, in seconds.
    pub dt: f32,
    /// Input level (RMS of the mix) in dBFS; `None` without audio.
    pub input_db: Option<f32>,
    /// Loudest bar before the level range, in dB; `None` in silence.
    pub loudest_db: Option<f32>,
    /// The noise gate let sound through.
    pub sound: bool,
    /// Glow beat pulse and background pulse, 0..=1.
    pub pulse: f32,
    pub bg_pulse: f32,
    /// A beat started this frame.
    pub hit: bool,
    /// Share of bars at full height, and their average height (0..=1).
    pub full: f32,
    pub mean: f32,
}

/// Everything the monitor shows, gathered each frame while it's on.
#[derive(Default)]
pub struct Monitor {
    samples: VecDeque<Sample>,
    /// The latest mix spectrum (dBFS per FFT bin), kept through silence.
    pub spectrum: Vec<f32>,
    pub sample_rate: f32,
    pub fft_size: usize,
    /// The bars as drawn (one channel's run), for the spectrum overlay.
    pub bars: Vec<f32>,
    /// How long the last frame's update took (audio, analysis, animation).
    pub update_cost: Duration,
    /// The pulse hasn't fallen back since the last hit.
    held: bool,
}

/// Reading for one frame, before the hit is worked out.
pub struct Reading<'a> {
    pub now: Instant,
    pub dt: f32,
    pub input_db: Option<f32>,
    pub loudest_db: Option<f32>,
    pub sound: bool,
    pub pulse: f32,
    pub bg_pulse: f32,
    /// What the bars show this frame (all values).
    pub visible: &'a [f32],
    /// Values per channel (the first run is kept for the spectrum overlay).
    pub per_channel: usize,
    /// This frame's mix spectrum, when there was audio.
    pub spectrum: Option<(&'a [f32], f32, usize)>,
}

impl Monitor {
    /// Add one frame and drop what's older than [`HISTORY`].
    pub fn record(&mut self, r: Reading<'_>) {
        let hit = !self.held && r.pulse >= HIT;
        if hit {
            self.held = true;
        } else if r.pulse < REARM {
            self.held = false;
        }
        let n = r.visible.len().max(1) as f32;
        let full = r.visible.iter().filter(|&&v| v >= FULL).count() as f32 / n;
        let mean = r.visible.iter().sum::<f32>() / n;
        self.samples.push_back(Sample {
            at: r.now,
            dt: r.dt,
            input_db: r.input_db,
            loudest_db: r.loudest_db,
            sound: r.sound,
            pulse: r.pulse,
            bg_pulse: r.bg_pulse,
            hit,
            full,
            mean,
        });
        while self
            .samples
            .front()
            .is_some_and(|s| r.now.saturating_duration_since(s.at) > HISTORY)
        {
            self.samples.pop_front();
        }
        if let Some((mags, sample_rate, fft_size)) = r.spectrum {
            self.spectrum.clear();
            self.spectrum.extend_from_slice(mags);
            self.sample_rate = sample_rate;
            self.fft_size = fft_size;
        }
        self.bars.clear();
        self.bars
            .extend_from_slice(&r.visible[..r.per_channel.min(r.visible.len())]);
    }

    /// Forget the history (it was turned off, or the settings closed).
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn samples(&self) -> &VecDeque<Sample> {
        &self.samples
    }

    fn recent(&self, span: Duration) -> impl Iterator<Item = &Sample> {
        let last = self.samples.back().map(|s| s.at);
        self.samples
            .iter()
            .filter(move |s| last.is_some_and(|l| l.saturating_duration_since(s.at) <= span))
    }

    /// Frames per second over the last second.
    pub fn fps(&self) -> Option<f32> {
        let dts: Vec<f32> = self
            .recent(Duration::from_secs(1))
            .map(|s| s.dt)
            .filter(|&d| d > 0.0)
            .collect();
        let total: f32 = dts.iter().sum();
        (dts.len() >= 2 && total > 0.0).then(|| dts.len() as f32 / total)
    }

    /// Average and longest frame time over the last two seconds, in ms.
    pub fn frame_times(&self) -> Option<(f32, f32)> {
        let dts: Vec<f32> = self
            .recent(Duration::from_secs(2))
            .map(|s| s.dt)
            .filter(|&d| d > 0.0)
            .collect();
        if dts.is_empty() {
            return None;
        }
        let avg = dts.iter().sum::<f32>() / dts.len() as f32;
        let max = dts.iter().copied().fold(0.0, f32::max);
        Some((avg * 1000.0, max * 1000.0))
    }

    /// Beats in the history.
    pub fn hits(&self) -> usize {
        self.samples.iter().filter(|s| s.hit).count()
    }

    /// Tempo from the gaps between hits (median), when there are enough of them.
    pub fn bpm(&self) -> Option<f32> {
        let times: Vec<Instant> = self
            .samples
            .iter()
            .filter(|s| s.hit)
            .map(|s| s.at)
            .collect();
        let mut gaps: Vec<f32> = times
            .windows(2)
            .map(|w| (w[1] - w[0]).as_secs_f32())
            .filter(|g| (0.25..=1.5).contains(g))
            .collect();
        if gaps.len() < 3 {
            return None;
        }
        gaps.sort_by(f32::total_cmp);
        Some(60.0 / gaps[gaps.len() / 2])
    }

    /// Of the frames with sound: share of full-height bars and average height.
    pub fn heights(&self) -> Option<(f32, f32)> {
        let sound: Vec<&Sample> = self.samples.iter().filter(|s| s.sound).collect();
        if sound.is_empty() {
            return None;
        }
        let n = sound.len() as f32;
        Some((
            sound.iter().map(|s| s.full).sum::<f32>() / n,
            sound.iter().map(|s| s.mean).sum::<f32>() / n,
        ))
    }

    /// Share of frames (with a reading) where `pick` holds.
    fn share(
        &self,
        value: impl Fn(&Sample) -> Option<f32>,
        pick: impl Fn(f32) -> bool,
    ) -> Option<f32> {
        let values: Vec<f32> = self.samples.iter().filter_map(value).collect();
        (values.len() >= 10)
            .then(|| values.iter().filter(|&&v| pick(v)).count() as f32 / values.len() as f32)
    }

    /// Whether the history covers enough time to judge.
    fn settled(&self) -> bool {
        match (self.samples.front(), self.samples.back()) {
            (Some(a), Some(b)) => b.at - a.at >= Duration::from_secs(3),
            _ => false,
        }
    }

    /// Suggestions for the level settings, from what the history shows.
    pub fn level_tips(&self, t: &TuningSettings) -> Vec<&'static str> {
        let mut tips = Vec::new();
        if !self.settled() {
            return tips;
        }
        let playing = self.samples.iter().filter(|s| s.input_db.is_some()).count() >= 10;
        if t.gate_db > GATE_OFF
            && playing
            && self
                .share(|s| s.input_db, |db| db < t.gate_db as f32)
                .is_some_and(|f| f > 0.5)
        {
            tips.push("The input is below the Noise gate most of the time. Lower the gate.");
        }
        if let Some((full, mean)) = self.heights() {
            if full > 0.15 {
                tips.push(
                    "Many bars hit full height. Lower Sensitivity or raise the top of the Level range.",
                );
            } else if mean < 0.12 && !t.auto_gain {
                tips.push(
                    "The bars stay low. Raise Sensitivity, lower the top of the Level range, or turn on Auto-gain.",
                );
            }
        }
        if self
            .share(|s| s.loudest_db, |db| db < t.floor_db as f32)
            .is_some_and(|f| f > 0.5)
        {
            tips.push("Most of the music is below the bottom of the Level range. Lower it.");
        }
        tips
    }

    /// Suggestions for beat detection.
    pub fn beat_tips(&self) -> Vec<&'static str> {
        let mut tips = Vec::new();
        let sound_s: f32 = self.samples.iter().filter(|s| s.sound).map(|s| s.dt).sum();
        if sound_s < 4.0 {
            return tips;
        }
        let rate = self.hits() as f32 / sound_s;
        if self.hits() == 0 {
            tips.push("No beats found. Raise Beat sensitivity, or try the other glow source.");
        } else if rate > 4.0 {
            tips.push("Beats fire very often. Lower Beat sensitivity.");
        }
        tips
    }
}

// ---------------------------------------------------------------------------
// Graphs

fn db_y(db: f32, h: f32) -> f32 {
    let t = ((db - DB_RANGE.0) / (DB_RANGE.1 - DB_RANGE.0)).clamp(0.0, 1.0);
    h * (1.0 - t)
}

fn hz_x(hz: f32, w: f32) -> f32 {
    let (lo, hi) = (HZ_RANGE.0.ln(), HZ_RANGE.1.ln());
    w * ((hz.max(1.0).ln() - lo) / (hi - lo)).clamp(0.0, 1.0)
}

fn x_hz(x: f32, w: f32) -> f32 {
    let (lo, hi) = (HZ_RANGE.0.ln(), HZ_RANGE.1.ln());
    (lo + (hi - lo) * (x / w.max(1.0))).exp()
}

fn line(color: Color, width: f32) -> Stroke<'static> {
    Stroke::default().with_color(color).with_width(width)
}

fn dashed(color: Color) -> Stroke<'static> {
    Stroke {
        line_dash: LineDash {
            segments: &[4.0, 4.0],
            offset: 0,
        },
        ..line(color, 1.0)
    }
}

/// Faint horizontal lines every 20 dB.
fn db_grid(frame: &mut Frame, w: f32, h: f32) {
    for db in [-80.0, -60.0, -40.0, -20.0] {
        let y = db_y(db, h);
        frame.stroke(
            &Path::line(Point::new(0.0, y), Point::new(w, y)),
            line(with_alpha(TRACK, 0.8), 1.0),
        );
    }
}

/// A polyline through `points`, skipping gaps (`None`).
fn polyline(frame: &mut Frame, points: impl Iterator<Item = Option<Point>>, stroke: Stroke<'_>) {
    let mut run: Vec<Point> = Vec::new();
    let flush = |run: &mut Vec<Point>, frame: &mut Frame| {
        if run.len() >= 2 {
            let path = Path::new(|p| {
                p.move_to(run[0]);
                for &q in &run[1..] {
                    p.line_to(q);
                }
            });
            frame.stroke(&path, stroke);
        }
        run.clear();
    };
    for p in points {
        match p {
            Some(p) => run.push(p),
            None => flush(&mut run, frame),
        }
    }
    flush(&mut run, frame);
}

/// x position of a sample in a history graph (now at the right edge).
fn time_x(s: &Sample, last: Instant, w: f32) -> f32 {
    let age = last.saturating_duration_since(s.at).as_secs_f32();
    w * (1.0 - age / HISTORY.as_secs_f32())
}

/// Input level and loudest bar over time, against the gate and level range.
pub struct LevelGraph<'a> {
    pub monitor: &'a Monitor,
    pub tuning: &'a TuningSettings,
    pub accent: Color,
}

impl canvas::Program<Message> for LevelGraph<'_> {
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
        let t = self.tuning;
        db_grid(&mut frame, w, h);
        // The level range: what maps to empty..full bars.
        let (top, bottom) = (db_y(t.ceiling_db as f32, h), db_y(t.floor_db as f32, h));
        frame.fill_rectangle(
            Point::new(0.0, top),
            Size::new(w, bottom - top),
            with_alpha(TEXT, 0.06),
        );
        for y in [top, bottom] {
            frame.stroke(
                &Path::line(Point::new(0.0, y), Point::new(w, y)),
                line(TEXT_3, 1.0),
            );
        }
        if t.gate_db > GATE_OFF {
            let y = db_y(t.gate_db as f32, h);
            frame.stroke(
                &Path::line(Point::new(0.0, y), Point::new(w, y)),
                dashed(TEXT),
            );
        }
        let samples = self.monitor.samples();
        if let Some(last) = samples.back().map(|s| s.at) {
            let at = |s: &Sample, db: Option<f32>| {
                db.map(|db| Point::new(time_x(s, last, w), db_y(db, h)))
            };
            polyline(
                &mut frame,
                samples.iter().map(|s| at(s, s.input_db)),
                line(TEXT_2, 1.5),
            );
            polyline(
                &mut frame,
                samples.iter().map(|s| at(s, s.loudest_db)),
                line(self.accent, 2.0),
            );
        }
        vec![frame.into_geometry()]
    }
}

/// The mix spectrum (with Tilt applied, as the bars see it), the bars' frequency
/// range and level range, and the bars themselves.
pub struct SpectrumGraph<'a> {
    pub monitor: &'a Monitor,
    pub tuning: &'a TuningSettings,
    pub accent: Color,
}

impl canvas::Program<Message> for SpectrumGraph<'_> {
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
        let t = self.tuning;
        let m = self.monitor;
        db_grid(&mut frame, w, h);
        for (hz, label) in [(100.0, "100"), (1000.0, "1k"), (10_000.0, "10k")] {
            let x = hz_x(hz, w);
            frame.stroke(
                &Path::line(Point::new(x, 0.0), Point::new(x, h)),
                line(with_alpha(TRACK, 0.8), 1.0),
            );
            frame.fill_text(canvas::Text {
                content: label.into(),
                position: Point::new(x + 3.0, h - 13.0),
                color: TEXT_3,
                size: 10.0.into(),
                ..canvas::Text::default()
            });
        }
        // The frequencies the bars cover.
        let lo = slider_to_hz(t.freq_lo as f32);
        let hi = slider_to_hz(t.freq_hi as f32).max(lo * 1.01);
        let (x0, x1) = (hz_x(lo, w), hz_x(hi, w));
        frame.fill_rectangle(
            Point::new(x0, 0.0),
            Size::new(x1 - x0, h),
            with_alpha(TEXT, 0.05),
        );
        let (top, bottom) = (db_y(t.ceiling_db as f32, h), db_y(t.floor_db as f32, h));
        for y in [top, bottom] {
            frame.stroke(
                &Path::line(Point::new(x0, y), Point::new(x1, y)),
                line(TEXT_3, 1.0),
            );
        }

        // The bars, standing on the level range's floor and reaching its ceiling at full height.
        if !m.bars.is_empty() {
            let edges: Vec<f32> = bar_edges(t.freq_scale, lo, hi, m.bars.len());
            for (i, v) in m.bars.iter().enumerate() {
                let (a, b) = (hz_x(edges[i], w), hz_x(edges[i + 1], w));
                let bh = v.clamp(0.0, 1.0) * (bottom - top);
                frame.fill_rectangle(
                    Point::new(a + 0.5, bottom - bh),
                    Size::new((b - a - 1.0).max(1.0), bh),
                    with_alpha(self.accent, 0.25),
                );
            }
        }

        // The spectrum, one point per pixel column (the loudest bin under it).
        if m.fft_size > 0 && !m.spectrum.is_empty() {
            let hz_per_bin = m.sample_rate / m.fft_size as f32;
            let last = m.spectrum.len() - 1;
            let tilt = t.tilt_db as f32;
            let points = (0..w as usize).map(|px| {
                let (f0, f1) = (x_hz(px as f32, w), x_hz(px as f32 + 1.0, w));
                let k0 = (f0 / hz_per_bin).ceil() as usize;
                let k1 = ((f1 / hz_per_bin).floor() as usize).min(last);
                let db = if k0 <= k1 {
                    m.spectrum[k0..=k1]
                        .iter()
                        .copied()
                        .fold(f32::NEG_INFINITY, f32::max)
                } else {
                    let x = ((f0 * f1).sqrt() / hz_per_bin).min(last as f32);
                    let a = x.floor() as usize;
                    let b = (a + 1).min(last);
                    m.spectrum[a] + (m.spectrum[b] - m.spectrum[a]) * (x - a as f32)
                };
                let center = (f0 * f1).sqrt();
                let db = db + tilt * (center / 1000.0).log2();
                Some(Point::new(px as f32, db_y(db, h)))
            });
            polyline(&mut frame, points, line(TEXT, 1.25));
        }
        vec![frame.into_geometry()]
    }
}

/// Band edges for `bars` bars (`bars + 1` values).
fn bar_edges(scale: FreqScale, lo: f32, hi: f32, bars: usize) -> Vec<f32> {
    let mut edges: Vec<f32> = scale.edges(lo, hi, bars).collect();
    edges.resize(bars + 1, hi);
    edges
}

/// Beat pulses over time, with hits marked along the top.
pub struct BeatGraph<'a> {
    pub monitor: &'a Monitor,
    pub show_bg: bool,
    pub accent: Color,
}

impl canvas::Program<Message> for BeatGraph<'_> {
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
        let top = 8.0;
        let y = |p: f32| top + (h - top) * (1.0 - p.clamp(0.0, 1.0));
        let threshold = y(HIT);
        frame.stroke(
            &Path::line(Point::new(0.0, threshold), Point::new(w, threshold)),
            dashed(TEXT_3),
        );
        let samples = self.monitor.samples();
        if let Some(last) = samples.back().map(|s| s.at) {
            if self.show_bg {
                polyline(
                    &mut frame,
                    samples
                        .iter()
                        .map(|s| Some(Point::new(time_x(s, last, w), y(s.bg_pulse)))),
                    line(TEXT_2, 1.25),
                );
            }
            polyline(
                &mut frame,
                samples
                    .iter()
                    .map(|s| Some(Point::new(time_x(s, last, w), y(s.pulse)))),
                line(self.accent, 2.0),
            );
            for s in samples.iter().filter(|s| s.hit) {
                frame.fill(
                    &Path::circle(Point::new(time_x(s, last, w), 3.0), 3.0),
                    self.accent,
                );
            }
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading(now: Instant, pulse: f32, visible: &[f32]) -> Reading<'_> {
        Reading {
            now,
            dt: 1.0 / 60.0,
            input_db: Some(-30.0),
            loudest_db: Some(-20.0),
            sound: true,
            pulse,
            bg_pulse: 0.0,
            visible,
            per_channel: visible.len(),
            spectrum: None,
        }
    }

    #[test]
    fn hits_tempo_and_history_length() {
        let mut m = Monitor::default();
        let start = Instant::now();
        let bars = [0.5, 1.0];
        // 120 BPM: a pulse every 0.5 s, 60 fps, for 10 s.
        for i in 0..600 {
            let t = i as f32 / 60.0;
            let pulse = if (t % 0.5) < 0.1 { 1.0 } else { 0.0 };
            m.record(reading(start + Duration::from_secs_f32(t), pulse, &bars));
        }
        // Older frames are dropped.
        let span = m.samples().back().unwrap().at - m.samples().front().unwrap().at;
        assert!(span <= HISTORY);
        let bpm = m.bpm().expect("tempo");
        assert!((bpm - 120.0).abs() < 3.0, "{bpm}");
        assert!((m.fps().unwrap() - 60.0).abs() < 1.0);
        let (full, mean) = m.heights().unwrap();
        assert_eq!(full, 0.5);
        assert_eq!(mean, 0.75);
        // Half the bars are pinned at full height.
        let tips = m.level_tips(&TuningSettings::default());
        assert!(tips.iter().any(|t| t.contains("full height")), "{tips:?}");
    }

    #[test]
    fn a_held_pulse_is_one_hit() {
        let mut m = Monitor::default();
        let start = Instant::now();
        for i in 0..30 {
            m.record(reading(start + Duration::from_millis(i * 16), 0.9, &[0.1]));
        }
        assert_eq!(m.hits(), 1);
        assert_eq!(m.bpm(), None);
    }

    #[test]
    fn axes() {
        assert_eq!(db_y(0.0, 100.0), 0.0);
        assert_eq!(db_y(-100.0, 100.0), 100.0);
        assert!((x_hz(hz_x(1000.0, 300.0), 300.0) - 1000.0).abs() < 0.5);
        assert_eq!(bar_edges(FreqScale::Log, 20.0, 20_000.0, 4).len(), 5);
    }
}

//! The visualizer and the seek bar.
//!
//! Every visualizer is a shader (`shaders/mod.rs`): Bars, Radial and Wave are
//! built in, and the shaders folder can add more. This module hands one the
//! numbers it needs each frame (each bar's value and peak, the palette, the glow,
//! the music; see `assets/shaders/visualizer.wgsl`) and works out the settings the
//! rest of the app reads from it: how many bars to compute, and how mirroring and
//! split map the spectrum onto them. `shader.rs` draws it on the GPU.

use crate::app::Message;
use crate::art::NEUTRAL_ACCENT;
use crate::audio::Signal;
use crate::backdrop::{Cover, Place};
use crate::settings::{Channel, GlowSource, Mirror, SeekHandle, Split};
use crate::shaders::settings::Resolved;
use crate::shaders::{self, Prepared};
use crate::theme::{SEEK_HANDLE, TEXT, TEXT_2, TRACK, with_alpha};
use iced::mouse::{self, Cursor};
use iced::widget::canvas::{self, Action, Event, Frame, Geometry, LineCap, LineJoin, Path, Stroke};
use iced::{Color, Point, Rectangle, Renderer, Size, Theme};
use shader::UNIFORM_FLOATS;
use std::f32::consts::TAU;

mod shader;

/// The most bars (and so values handed to the shader).
pub const MAX_BARS: usize = 96;
/// Bars for a visualizer without a `bar_count` setting.
const DEFAULT_BARS: usize = 64;
/// The most palette stops the shader takes.
const MAX_STOPS: usize = 8;

/// The settings the app itself reads from the visualizer, by their reserved ids
/// (`bar_count`, `mirror`, `split`, `channel`, `glow`, `glow_source`,
/// `glow_amount`; see docs/shaders.md). Missing ones take these defaults.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VisEngine {
    pub bar_count: usize,
    pub mirror: Mirror,
    pub split: Split,
    pub channel: Channel,
    pub glow: bool,
    pub glow_source: GlowSource,
    /// 0..=1.
    pub glow_amount: f32,
}

impl Default for VisEngine {
    fn default() -> Self {
        Self {
            bar_count: DEFAULT_BARS,
            mirror: Mirror::Off,
            split: Split::Off,
            channel: Channel::Mono,
            glow: false,
            glow_source: GlowSource::Bass,
            glow_amount: 0.5,
        }
    }
}

impl VisEngine {
    pub fn read(r: &Resolved) -> Self {
        let d = Self::default();
        Self {
            bar_count: r
                .int("bar_count")
                .map_or(d.bar_count, |n| (n.max(1) as usize).min(MAX_BARS)),
            mirror: r
                .choice("mirror")
                .and_then(Mirror::from_label)
                .unwrap_or(d.mirror),
            split: r
                .choice("split")
                .and_then(Split::from_label)
                .unwrap_or(d.split),
            channel: r
                .choice("channel")
                .and_then(Channel::from_label)
                .unwrap_or(d.channel),
            glow: r.toggle("glow").unwrap_or(d.glow),
            glow_source: r
                .choice("glow_source")
                .and_then(GlowSource::from_label)
                .unwrap_or(d.glow_source),
            glow_amount: r
                .float("glow_amount")
                .map_or(d.glow_amount, |a| (a / 100.0).clamp(0.0, 1.0)),
        }
    }
}

/// One frame of the visualizer: which shader, with what.
pub struct Visualizer<'a> {
    pub place: Place,
    /// One value per bin (half the bars when mirrored left/right), 0..=1.
    pub values: &'a [f32],
    pub peaks: &'a [f32],
    pub engine: VisEngine,
    pub shader: &'a Prepared,
    pub settings: &'a Resolved,
    /// Beat-glow strength this frame, 0..=1 (with the glow amount applied).
    pub glow: f32,
    /// The cover (Radial's middle, and `cover()` in any visualizer).
    pub cover: Option<&'a Cover>,
    /// Bar colors, resolved from the palette (Album colors come from the cover).
    pub stops: Vec<[u8; 3]>,
    /// The palette runs along each bar rather than across them all.
    pub along: bool,
    pub audio: shaders::Audio,
    pub colors: [[u8; 3]; 4],
    pub accent: [u8; 3],
    /// Seconds of music played.
    pub time: f32,
}

/// One color, or every stop the same.
pub fn is_solid(stops: &[[u8; 3]]) -> bool {
    stops.windows(2).all(|w| w[0] == w[1])
}

/// How bars map onto the processed values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarMap {
    /// Bar `j` shows value `j`, bass on the left.
    Straight,
    /// Bass in the center, the same spectrum on both sides.
    Mirrored,
    /// Bass in the center, each half with its own signal (left/right, or mid/side).
    /// Values hold the left half's spectrum, then the right half's.
    Split,
}

/// The signals behind the bars: one spectrum, or one per half when split.
pub fn bar_signals(e: &VisEngine) -> &'static [Signal] {
    match (BarMap::for_engine(e), e.split, e.channel) {
        (BarMap::Split, Split::MidSide, _) => &[Signal::Mix, Signal::Side],
        (BarMap::Split, _, _) => &[Signal::Left, Signal::Right],
        (_, _, Channel::Mono) => &[Signal::Mix],
        (_, _, Channel::Left) => &[Signal::Left],
        (_, _, Channel::Right) => &[Signal::Right],
    }
}

impl BarMap {
    pub fn for_engine(e: &VisEngine) -> Self {
        match (e.mirror.left_right(), e.split) {
            (false, _) => BarMap::Straight,
            (true, Split::Off) => BarMap::Mirrored,
            (true, Split::Stereo | Split::MidSide) => BarMap::Split,
        }
    }

    /// Values needed for `n` bars (across all channels).
    pub fn bins(self, n: usize) -> usize {
        match self {
            BarMap::Straight => n,
            BarMap::Mirrored => n.div_ceil(2),
            BarMap::Split => 2 * n.div_ceil(2),
        }
    }

    /// The value bar `j` of `n` shows.
    pub fn value(self, src: &[f32], j: usize, n: usize) -> f32 {
        let get = |i: usize| src.get(i).copied().unwrap_or(0.0);
        let mid = (n as f32 - 1.0) / 2.0;
        let from_center = (j as f32 - mid).abs().floor() as usize;
        match self {
            BarMap::Straight => get(j),
            BarMap::Mirrored => get(from_center),
            BarMap::Split => {
                let right = n.div_ceil(2);
                if j as f32 == mid {
                    // The single center bar of an odd count belongs to both sides.
                    (get(0) + get(right)) / 2.0
                } else if (j as f32) < mid {
                    get(from_center)
                } else {
                    get(right + from_center)
                }
            }
        }
    }
}

fn rgb(c: [u8; 3]) -> [f32; 4] {
    [
        c[0] as f32 / 255.0,
        c[1] as f32 / 255.0,
        c[2] as f32 / 255.0,
        1.0,
    ]
}

// Rows of `UndaviVis` (assets/shaders/visualizer.wgsl).
const ROW_SIZE: usize = 0;
const ROW_INFO: usize = 1;
const ROW_HALO: usize = 2;
const ROW_LEVELS: usize = 3;
const ROW_EXTRA: usize = 4;
const ROW_BANDS: usize = 5;
const ROW_ACCENT: usize = 9;
const ROW_COLORS: usize = 10;
const ROW_STOPS: usize = 14;
const ROW_VALUES: usize = 22;
const ROW_PEAKS: usize = 46;

impl Visualizer<'_> {
    fn stops(&self) -> &[[u8; 3]] {
        if self.stops.is_empty() {
            &[NEUTRAL_ACCENT]
        } else {
            &self.stops[..self.stops.len().min(MAX_STOPS)]
        }
    }

    /// The uniforms for a `w` × `h` (logical) area. The pixel size and the sRGB
    /// flag are filled in on the GPU side, which knows them.
    pub fn uniforms(&self, w: f32, h: f32) -> Vec<f32> {
        let n = self.engine.bar_count.clamp(1, MAX_BARS);
        let map = BarMap::for_engine(&self.engine);
        let stops = self.stops();
        let mut u = vec![0.0; UNIFORM_FLOATS];
        let mut row = |i: usize, v: [f32; 4]| u[4 * i..4 * i + 4].copy_from_slice(&v);
        row(ROW_SIZE, [w, h, self.time, 1.0]);
        let along = self.along && !is_solid(stops);
        row(
            ROW_INFO,
            [
                self.glow,
                stops.len() as f32,
                0.0,
                if along { 1.0 } else { 0.0 },
            ],
        );
        // The glow takes the palette's middle color.
        row(ROW_HALO, rgb(stops[stops.len() / 2]));
        let a = &self.audio;
        row(ROW_LEVELS, [a.bass, a.mids, a.treble, a.beat]);
        let has_cover = if self.cover.is_some() { 1.0 } else { 0.0 };
        row(ROW_EXTRA, [a.level, n as f32, has_cover, 0.0]);
        row(ROW_ACCENT, rgb(self.accent));
        for (i, c) in self.colors.iter().enumerate() {
            row(ROW_COLORS + i, rgb(*c));
        }
        for (i, c) in stops.iter().enumerate() {
            row(ROW_STOPS + i, rgb(*c));
        }
        let mut array = |first_row: usize, values: &[f32]| {
            let start = 4 * first_row;
            u[start..start + values.len()].copy_from_slice(values);
        };
        array(ROW_BANDS, &a.bands);
        let per_bar = |src: &[f32]| -> Vec<f32> { (0..n).map(|j| map.value(src, j, n)).collect() };
        array(ROW_VALUES, &per_bar(self.values));
        array(ROW_PEAKS, &per_bar(self.peaks));
        u
    }
}

impl iced::widget::shader::Program<Message> for Visualizer<'_> {
    type State = ();
    type Primitive = shader::Primitive;

    fn draw(&self, _: &(), _: Cursor, bounds: Rectangle) -> shader::Primitive {
        shader::Primitive {
            place: self.place,
            uniforms: self.uniforms(bounds.width, bounds.height),
            settings: self.settings.pack(),
            cover: self.cover.cloned(),
            shader: self.shader.compiled.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// Seek bar

/// Width reserved for each time label, and the gap to the bar.
const LABEL_W: f32 = 46.0;
const LABEL_GAP: f32 = 14.0;
/// A seek bar narrower than this has no time labels.
const LABEL_ROOM: f32 = 260.0;

/// How the seek bar is drawn, in px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SeekLook {
    /// Of the curve, from its middle.
    pub amplitude: f32,
    pub wavelength: f32,
    /// Thickness of the lines.
    pub stroke: f32,
    pub handle: SeekHandle,
    /// Width and height of a bar handle.
    pub handle_size: (f32, f32),
    /// Brightness of the part still to play, 0..=1.
    pub remaining: f32,
    /// Show the elapsed and remaining time.
    pub times: bool,
}

impl SeekLook {
    /// The handle's width and height.
    fn handle(&self) -> (f32, f32) {
        match self.handle {
            SeekHandle::Bar => self.handle_size,
            SeekHandle::Dot => {
                let d = (self.handle_size.1 * 0.5).max(self.stroke + 6.0);
                (d, d)
            }
            SeekHandle::None => (0.0, 0.0),
        }
    }

    /// Space between the handle and the lines (their round ends).
    fn gap(&self) -> f32 {
        match self.handle {
            SeekHandle::None => 2.0,
            _ => (self.handle_size.0 * 1.25).max(3.0),
        }
    }
}

pub struct Seek<M> {
    /// Current position and track length in seconds, or `None` when nothing
    /// seekable is playing.
    pub time: Option<(f32, f32)>,
    /// Wave phase in radians (animates while playing).
    pub phase: f32,
    pub accent: Color,
    /// 0..=1, for auto-hide fading.
    pub alpha: f32,
    /// Keyboard focus ring.
    pub focused: bool,
    pub look: SeekLook,
    pub on_seek: fn(f32) -> M,
}

#[derive(Default)]
pub struct SeekState {
    drag: Option<f32>,
    /// Where the pointer is over the bar (0..=1), for the time preview.
    hover: Option<f32>,
}

/// `m:ss`, or `h:mm:ss` from an hour up.
pub fn format_time(seconds: f32) -> String {
    let s = seconds.max(0.0).round() as u64;
    let (h, m, s) = (s / 3600, (s / 60) % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

impl<M> Seek<M> {
    /// The bar's horizontal extent, between the time labels.
    fn x_range(&self, bounds: Rectangle) -> (f32, f32) {
        // Inset by half a stroke so round caps aren't clipped.
        let side = if self.labels(bounds) {
            LABEL_W + LABEL_GAP
        } else {
            0.0
        };
        let inset = side + self.look.stroke / 2.0;
        (inset, (bounds.width - inset).max(inset + 1.0))
    }

    /// Whether the times show.
    fn labels(&self, bounds: Rectangle) -> bool {
        self.time.is_some() && self.look.times && bounds.width >= LABEL_ROOM
    }

    /// How far the curve is from the bar's middle, `x` from its start.
    fn curve(&self, x: f32) -> f32 {
        let turn = TAU * x / self.look.wavelength.max(1.0) - self.phase;
        -self.look.amplitude * turn.sin()
    }

    /// The same for the whole line: the curve as far as `played` (and its round end),
    /// and straight from there.
    fn line(&self, x: f32, played: f32) -> f32 {
        if played > 0.0 && x <= played + self.look.stroke / 2.0 {
            self.curve(x.min(played))
        } else {
            0.0
        }
    }

    fn fraction_at(&self, bounds: Rectangle, p: Point) -> f32 {
        let (x0, x1) = self.x_range(bounds);
        ((p.x - bounds.x - x0) / (x1 - x0)).clamp(0.0, 1.0)
    }

    fn over_bar(&self, bounds: Rectangle, p: Point) -> bool {
        let (x0, x1) = self.x_range(bounds);
        let x = p.x - bounds.x;
        let reach = self.look.handle().0.max(8.0);
        bounds.contains(p) && x >= x0 - reach && x <= x1 + reach
    }
}

impl<M> canvas::Program<M> for Seek<M> {
    type State = SeekState;

    fn update(
        &self,
        state: &mut SeekState,
        event: &Event,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> Option<Action<M>> {
        self.time?;
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let p = cursor.position()?;
                if !self.over_bar(bounds, p) {
                    return None;
                }
                state.drag = Some(self.fraction_at(bounds, p));
                Some(Action::request_redraw().and_capture())
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                let hover = self
                    .over_bar(bounds, *position)
                    .then(|| self.fraction_at(bounds, *position));
                if state.drag.is_some() {
                    state.drag = Some(self.fraction_at(bounds, *position));
                    return Some(Action::request_redraw().and_capture());
                }
                if hover != state.hover {
                    state.hover = hover;
                    return Some(Action::request_redraw());
                }
                None
            }
            Event::Mouse(mouse::Event::CursorLeft) => {
                state.hover = None;
                Some(Action::request_redraw())
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                let f = state.drag.take()?;
                Some(Action::publish((self.on_seek)(f)).and_capture())
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        state: &SeekState,
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let alpha = self.alpha;
        if alpha <= 0.0 {
            return vec![frame.into_geometry()];
        }
        let (x0, x1) = self.x_range(bounds);
        let cy = bounds.height / 2.0;
        let look = self.look;
        let (handle_w, handle_h) = look.handle();
        let line = |color: Color| {
            Stroke::default()
                .with_color(with_alpha(color, alpha))
                .with_width(look.stroke)
                .with_line_cap(LineCap::Round)
        };

        let Some((position, length)) = self.time else {
            frame.stroke(
                &Path::line(Point::new(x0, cy), Point::new(x1, cy)),
                line(TRACK),
            );
            return vec![frame.into_geometry()];
        };
        let progress = state.drag.unwrap_or(if length > 0.0 {
            (position / length).clamp(0.0, 1.0)
        } else {
            0.0
        });

        // Time labels: elapsed (or the time under the pointer) and remaining.
        let preview = state.drag.or(state.hover);
        let (left, left_color) = match preview {
            Some(f) => (format_time(f * length), self.accent),
            None => (format_time(position), TEXT_2),
        };
        let label = |content: String,
                     x: f32,
                     align: iced::widget::text::Alignment,
                     color: Color| canvas::Text {
            content,
            position: Point::new(x, cy),
            color: with_alpha(color, alpha),
            size: 13.0.into(),
            font: crate::theme::INTER_MEDIUM,
            align_x: align,
            align_y: iced::alignment::Vertical::Center,
            ..canvas::Text::default()
        };
        if self.labels(bounds) {
            frame.fill_text(label(
                left,
                LABEL_W,
                iced::widget::text::Alignment::Right,
                left_color,
            ));
            let remaining = (length - progress * length).max(0.0);
            frame.fill_text(label(
                format!("\u{2212}{}", format_time(remaining)),
                bounds.width - LABEL_W,
                iced::widget::text::Alignment::Left,
                TEXT_2,
            ));
        }

        let hx = x0 + handle_w / 2.0 + progress * (x1 - x0 - handle_w);
        let wave_end = hx - handle_w / 2.0 - look.gap() - look.stroke / 2.0;
        if wave_end > x0 {
            let wave = Path::new(|p| {
                let at = |x: f32| Point::new(x, cy + self.curve(x - x0));
                p.move_to(at(x0));
                let mut x = x0;
                while x < wave_end {
                    x = (x + 2.0).min(wave_end);
                    p.line_to(at(x));
                }
            });
            frame.stroke(&wave, line(self.accent).with_line_join(LineJoin::Round));
        }
        let rest_start = hx + handle_w / 2.0 + look.gap() + look.stroke / 2.0;
        if rest_start < x1 {
            frame.stroke(
                &Path::line(Point::new(rest_start, cy), Point::new(x1, cy)),
                line(with_alpha(TEXT, look.remaining)),
            );
        }
        // Hover marker: a faint tick where a click would seek to.
        if let (Some(f), None) = (state.hover, state.drag) {
            let x = x0 + handle_w / 2.0 + f * (x1 - x0 - handle_w);
            let tall = (look.handle_size.1 * 0.5).max(look.stroke + 8.0);
            // On the line, but whole however high the curve goes.
            let room = ((bounds.height - tall) / 2.0).max(0.0);
            let y = cy + self.line(x - x0, wave_end - x0).clamp(-room, room);
            frame.fill(
                &Path::rounded_rectangle(
                    Point::new(x - 1.5, y - tall / 2.0),
                    Size::new(3.0, tall),
                    1.5.into(),
                ),
                with_alpha(TEXT, 0.45 * alpha),
            );
        }
        if handle_w > 0.0 {
            frame.fill(
                &Path::rounded_rectangle(
                    Point::new(hx - handle_w / 2.0, cy - handle_h / 2.0),
                    Size::new(handle_w, handle_h),
                    (handle_w / 2.0).into(),
                ),
                with_alpha(SEEK_HANDLE, alpha),
            );
        }
        if self.focused {
            crate::controls::focus_ring(
                &mut frame,
                Rectangle::new(
                    Point::new(x0 - 10.0, 1.0),
                    Size::new(x1 - x0 + 20.0, bounds.height - 2.0),
                ),
                12.0,
            );
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &SeekState,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> mouse::Interaction {
        if self.time.is_none() {
            mouse::Interaction::default()
        } else if state.drag.is_some() {
            mouse::Interaction::Grabbing
        } else if cursor.position().is_some_and(|p| self.over_bar(bounds, p)) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bars() -> (Prepared, Resolved) {
        let lib = shaders::Library::new(None);
        let p = lib
            .prepared(shaders::BARS, shaders::Kind::Visualizer)
            .unwrap()
            .clone();
        let mut saved = crate::settings::ShaderValues::new();
        saved.insert("bar_count".into(), 20.into());
        saved.insert("mirror".into(), "Left/right".into());
        saved.insert("split".into(), "Off".into());
        saved.insert("glow".into(), true.into());
        saved.insert("glow_amount".into(), 80.into());
        let r = p.decls.resolve(Some(&saved), &[]);
        (p, r)
    }

    #[test]
    fn reserved_settings_drive_the_engine() {
        let (_, r) = bars();
        let e = VisEngine::read(&r);
        assert_eq!(e.bar_count, 20);
        assert_eq!(e.mirror, Mirror::LeftRight);
        assert_eq!(BarMap::for_engine(&e), BarMap::Mirrored);
        assert!(e.glow);
        assert!((e.glow_amount - 0.8).abs() < 1e-6);
        assert_eq!(bar_signals(&e), &[Signal::Mix]);
        // A shader without them gets the defaults.
        let none = crate::shaders::settings::Decls::default().resolve(None, &[]);
        assert_eq!(VisEngine::read(&none), VisEngine::default());
    }

    #[test]
    fn uniforms_carry_each_bars_value() {
        let (p, r) = bars();
        let engine = VisEngine::read(&r);
        let values: Vec<f32> = (0..10).map(|i| i as f32 / 10.0).collect();
        let vis = Visualizer {
            place: Place::Screen,
            values: &values,
            peaks: &values,
            engine,
            shader: &p,
            settings: &r,
            glow: 0.5,
            cover: None,
            stops: vec![[255, 0, 0], [0, 0, 255]],
            along: true,
            audio: shaders::Audio::default(),
            colors: [[0; 3]; 4],
            accent: [0; 3],
            time: 3.0,
        };
        let u = vis.uniforms(800.0, 400.0);
        assert_eq!(&u[0..3], &[800.0, 400.0, 3.0]);
        assert_eq!(u[4], 0.5, "glow");
        assert_eq!(u[7], 1.0, "along");
        assert_eq!(u[4 * ROW_EXTRA + 1], 20.0, "bar count");
        // Mirrored left/right: the middle bars show the bass (value 0), the ends the highs.
        let bar = |j: usize| u[4 * ROW_VALUES + j];
        assert_eq!(bar(9), 0.0);
        assert_eq!(bar(10), 0.0);
        assert_eq!(bar(0), 0.9);
        // One color: nothing to run along.
        let solid = Visualizer {
            stops: vec![[9, 9, 9]],
            ..vis
        };
        assert_eq!(solid.uniforms(800.0, 400.0)[7], 0.0);
    }

    #[test]
    fn the_hover_marker_is_on_the_line() {
        let seek = Seek {
            time: Some((50.0, 100.0)),
            phase: 0.4,
            accent: Color::WHITE,
            alpha: 1.0,
            focused: false,
            look: SeekLook {
                amplitude: 6.0,
                wavelength: 100.0,
                stroke: 7.0,
                handle: SeekHandle::Bar,
                handle_size: (8.0, 36.0),
                remaining: 1.0,
                times: true,
            },
            on_seek: |f: f32| f,
        };
        // The curve, as far as it's drawn (and its round end)…
        for x in [0.0, 12.0, 25.0, 80.0, 199.0] {
            assert_eq!(seek.line(x, 200.0), seek.curve(x));
        }
        assert_eq!(seek.line(203.0, 200.0), seek.curve(200.0));
        assert!(seek.curve(25.0).abs() > 1.0 && seek.curve(25.0).abs() <= 6.0);
        // …and the middle from there, or where nothing has played yet.
        assert_eq!(seek.line(204.0, 200.0), 0.0);
        assert_eq!(seek.line(0.0, -5.0), 0.0);
        // A straight line has nothing to follow.
        let flat = Seek {
            look: SeekLook {
                amplitude: 0.0,
                ..seek.look
            },
            ..seek
        };
        assert_eq!(flat.line(25.0, 200.0), 0.0);
    }

    #[test]
    fn time_formatting() {
        assert_eq!(format_time(0.0), "0:00");
        assert_eq!(format_time(96.4), "1:36");
        assert_eq!(format_time(3725.0), "1:02:05");
    }

    #[test]
    fn left_right_puts_bass_in_center() {
        let src = [0.0, 1.0, 2.0, 3.0, 4.0, 5.0];
        let bars = |map: BarMap, n: usize| -> Vec<f32> {
            (0..n)
                .map(|j| map.value(&src[..map.bins(n)], j, n))
                .collect()
        };
        // Odd count: a single center bar.
        assert_eq!(BarMap::Mirrored.bins(5), 3);
        assert_eq!(bars(BarMap::Mirrored, 5), [2.0, 1.0, 0.0, 1.0, 2.0]);
        // Even count: two center bars share the bass.
        assert_eq!(BarMap::Mirrored.bins(4), 2);
        assert_eq!(bars(BarMap::Mirrored, 4), [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(bars(BarMap::Straight, 4), [0.0, 1.0, 2.0, 3.0]);
    }

    #[test]
    fn stereo_puts_each_channel_on_its_side() {
        // Left spectrum is 0, 1, 2; right spectrum is 3, 4, 5 (bass first).
        let src = [0.0, 1.0, 2.0, 3.0, 4.0, 5.0];
        let bars = |n: usize| -> Vec<f32> {
            (0..n)
                .map(|j| BarMap::Split.value(&src[..BarMap::Split.bins(n)], j, n))
                .collect()
        };
        assert_eq!(BarMap::Split.bins(6), 6);
        assert_eq!(bars(6), [2.0, 1.0, 0.0, 3.0, 4.0, 5.0]);
        // Odd count: the center bar averages both channels' bass.
        assert_eq!(BarMap::Split.bins(5), 6);
        assert_eq!(bars(5), [2.0, 1.0, 1.5, 4.0, 5.0]);
    }
}

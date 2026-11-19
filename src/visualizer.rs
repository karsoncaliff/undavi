//! Drawing the bars (all styles) and the seek bar.

use crate::audio::Signal;
use crate::controls::rgb;
use crate::settings::{Channel, Corners, GradientDir, Split, VisualSettings, VisualStyle};
use crate::theme::{SEEK_HANDLE, TEXT, TEXT_2, TRACK, with_alpha};
use iced::mouse::{self, Cursor};
use iced::widget::canvas::{
    self, Action, Event, Fill, Frame, Geometry, LineCap, LineJoin, Path, Stroke,
};
use iced::widget::image::{FilterMethod, Handle};
use iced::{Color, Point, Radians, Rectangle, Renderer, Size, Theme, Vector};
use std::f32::consts::{PI, TAU};

/// Bar length the layout is proportioned for: in windows too short for it, bars
/// (and their widths) scale down together, as they always have. Longer Max length
/// settings just use more of the height.
const REFERENCE_LENGTH: f32 = 400.0;
const MIN_VISIBLE: f32 = 4.0;
const CAP_HEIGHT: f32 = 4.0;
const CAP_GAP: f32 = 6.0;
const CORNER_ROUNDED: f32 = 2.5;

pub struct Bars<'a> {
    /// One value per bin (half the bars when mirrored left/right), 0..=1.
    pub values: &'a [f32],
    pub peaks: &'a [f32],
    pub visuals: &'a VisualSettings,
    /// Beat-glow strength this frame, 0..=1 (already scaled by the Intensity setting).
    pub glow: f32,
    /// Round album cover for the middle of the Radial style.
    pub cover: Option<&'a Handle>,
    /// Cover rotation in radians (turns slowly while music plays).
    pub spin: f32,
    /// Bar colors, resolved from the palette (Album colors come from the cover).
    pub stops: Vec<[u8; 3]>,
}

/// One color, or every stop the same.
pub fn is_solid(stops: &[[u8; 3]]) -> bool {
    stops.windows(2).all(|w| w[0] == w[1])
}

/// Color at `t` (0..=1) along a palette's stops.
fn sample(stops: &[[u8; 3]], t: f32) -> [u8; 3] {
    if stops.len() == 1 {
        return stops[0];
    }
    let x = t.clamp(0.0, 1.0) * (stops.len() - 1) as f32;
    let i = (x.floor() as usize).min(stops.len() - 2);
    let f = x - i as f32;
    let (a, b) = (stops[i], stops[i + 1]);
    [0, 1, 2].map(|k| (a[k] as f32 + (b[k] as f32 - a[k] as f32) * f).round() as u8)
}

/// How far toward white the bars brighten at full glow.
const GLOW_BRIGHTEN: f32 = 0.4;
/// Halo rings: count, spacing in px, and peak opacity.
const HALO_LAYERS: usize = 5;
const HALO_STEP: f32 = 2.5;
const HALO_ALPHA: f32 = 0.16;

fn brighten(c: [u8; 3], t: f32) -> Color {
    let c = rgb(c);
    Color::from_rgb(
        c.r + (1.0 - c.r) * t,
        c.g + (1.0 - c.g) * t,
        c.b + (1.0 - c.b) * t,
    )
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
pub fn bar_signals(v: &VisualSettings) -> &'static [Signal] {
    match (BarMap::for_visuals(v), v.split, v.channel) {
        (BarMap::Split, Split::MidSide, _) => &[Signal::Mix, Signal::Side],
        (BarMap::Split, _, _) => &[Signal::Left, Signal::Right],
        (_, _, Channel::Mono) => &[Signal::Mix],
        (_, _, Channel::Left) => &[Signal::Left],
        (_, _, Channel::Right) => &[Signal::Right],
    }
}

impl BarMap {
    pub fn for_visuals(v: &VisualSettings) -> Self {
        match (v.mirror.left_right(), v.split) {
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

impl Bars<'_> {
    fn fill_for(&self, scale: f32, origin: Point, total_w: f32, base_y: f32, tip_y: f32) -> Fill {
        let stops = &self.stops;
        let lift = GLOW_BRIGHTEN * self.glow;
        if is_solid(&self.stops) {
            return brighten(stops[0], lift).into();
        }
        let (start, end) = match self.visuals.gradient {
            GradientDir::Across => (origin, Point::new(origin.x + total_w * scale, origin.y)),
            GradientDir::Along => (Point::new(0.0, base_y), Point::new(0.0, tip_y)),
        };
        if start == end {
            return brighten(stops[0], lift).into();
        }
        let mut g = canvas::gradient::Linear::new(start, end);
        for (k, s) in stops.iter().enumerate() {
            g = g.add_stop(k as f32 / (stops.len() - 1) as f32, brighten(*s, lift));
        }
        g.into()
    }

    /// Halo color: the palette's middle stop, brightened like the bars.
    fn halo_color(&self) -> Color {
        let stops = &self.stops;
        brighten(stops[stops.len() / 2], GLOW_BRIGHTEN * self.glow)
    }
}

/// Soft halo around one bar run: stacked, growing, fading rounded rects.
fn draw_halo(frame: &mut Frame, bar: Rectangle, scale: f32, color: Color, glow: f32) {
    for k in 1..=HALO_LAYERS {
        let e = k as f32 * HALO_STEP * scale;
        let alpha = HALO_ALPHA * glow * (1.0 - k as f32 / (HALO_LAYERS as f32 + 1.0));
        let rect = Path::rounded_rectangle(
            Point::new(bar.x - e, bar.y - e),
            Size::new(bar.width + 2.0 * e, bar.height + 2.0 * e),
            (bar.width / 2.0 + e).into(),
        );
        frame.fill(&rect, Color { a: alpha, ..color });
    }
}

/// Fill a vertical run from `from` toward `to` (either direction), segmented or solid.
#[allow(clippy::too_many_arguments)]
fn draw_run(
    frame: &mut Frame,
    x: f32,
    w: f32,
    from: f32,
    to: f32,
    v: &VisualSettings,
    scale: f32,
    fill: Fill,
) {
    let dir = if to < from { -1.0 } else { 1.0 };
    let len = (to - from).abs();
    let radius_for = |h: f32| {
        let r = match v.corners {
            Corners::Square => 0.0,
            Corners::Rounded => CORNER_ROUNDED * scale,
            Corners::Pill => w / 2.0,
        };
        r.min(h / 2.0).min(w / 2.0)
    };
    let mut rect = |a: f32, b: f32| {
        let (y0, y1) = if a < b { (a, b) } else { (b, a) };
        let h = y1 - y0;
        if h <= 0.01 {
            return;
        }
        let path =
            Path::rounded_rectangle(Point::new(x, y0), Size::new(w, h), radius_for(h).into());
        frame.fill(&path, fill);
    };

    if !v.segments {
        rect(from, from + dir * len);
        return;
    }
    let seg = v.segment_size as f32 * scale;
    let gap = v.segment_gap as f32 * scale;
    // Mirrored halves start half a gap off the center line so they match.
    let start = if v.mirror.up_down() { gap / 2.0 } else { 0.0 };
    if len <= start {
        // Too short for a full block: still show the minimum visible sliver.
        rect(from + dir * start, from + dir * (start + len));
        return;
    }
    let mut offset = start;
    while offset < len {
        // The last block is cut to fit the bar's height.
        rect(from + dir * offset, from + dir * (offset + seg).min(len));
        offset += seg + gap;
    }
}

impl Bars<'_> {
    /// The original look: a row of vertical bars.
    fn draw_bars(&self, frame: &mut Frame, bounds: Rectangle) {
        let v = self.visuals;
        let n = v.bar_count as usize;
        let bw = v.bar_width as f32;
        let sp = v.spacing as f32;
        let total_w = n as f32 * bw + (n as f32 - 1.0) * sp;
        let up_down = v.mirror.up_down();

        // Peak caps stick out above the max height; leave room for them.
        let extra = if v.peak_caps {
            CAP_GAP + CAP_HEIGHT
        } else {
            0.0
        };
        let extra = if up_down { 2.0 * extra } else { extra };
        let max_len = v.max_length as f32;
        let scale = (bounds.width / total_w)
            .min(bounds.height / (max_len.min(REFERENCE_LENGTH) + extra))
            .min(1.0);
        let origin = Point::new((bounds.width - total_w * scale) / 2.0, 0.0);
        let center_y = bounds.height / 2.0;
        // A full bar (both halves, when mirrored up/down), never taller than the space.
        let full = (max_len * scale)
            .min(bounds.height - extra * scale)
            .max(0.0);
        // Off: bars grow up from a baseline at the bottom of the full-length box.
        let base_y = if up_down {
            center_y
        } else {
            center_y + full / 2.0
        };
        let half = if up_down { full / 2.0 } else { full };

        let map = BarMap::for_visuals(v);
        let value = |j: usize, src: &[f32]| map.value(src, j, n);

        let min_len = if up_down {
            MIN_VISIBLE / 2.0
        } else {
            MIN_VISIBLE
        } * scale;

        // Glow halos go first so no halo covers a neighboring bar.
        if self.glow > 0.01 {
            let color = self.halo_color();
            for j in 0..n {
                let x = origin.x + j as f32 * (bw + sp) * scale;
                let len = (value(j, self.values) * half).max(min_len);
                let (y0, y1) = if up_down {
                    (base_y - len, base_y + len)
                } else {
                    (base_y - len, base_y)
                };
                let bar = Rectangle::new(Point::new(x, y0), Size::new(bw * scale, y1 - y0));
                draw_halo(frame, bar, scale, color, self.glow);
            }
        }

        for j in 0..n {
            let x = origin.x + j as f32 * (bw + sp) * scale;
            let w = bw * scale;
            let len = (value(j, self.values) * half).max(min_len);

            let directions: &[f32] = if up_down { &[-1.0, 1.0] } else { &[-1.0] };
            for &dir in directions {
                let tip = base_y + dir * len;
                let fill = self.fill_for(scale, origin, total_w, base_y, tip);
                draw_run(frame, x, w, base_y, tip, v, scale, fill);

                if v.peak_caps {
                    let peak_len = (value(j, self.peaks) * half).max(len);
                    let cap_near = base_y + dir * (peak_len + CAP_GAP * scale);
                    let cap_far = cap_near + dir * CAP_HEIGHT * scale;
                    let (y0, y1) = if cap_near < cap_far {
                        (cap_near, cap_far)
                    } else {
                        (cap_far, cap_near)
                    };
                    let cap_fill = match (v.gradient, self.stops.last()) {
                        // "Along": both tips use the end color.
                        (GradientDir::Along, Some(&last)) if !is_solid(&self.stops) => {
                            brighten(last, GLOW_BRIGHTEN * self.glow).into()
                        }
                        _ => fill,
                    };
                    let r = match v.corners {
                        Corners::Square => 0.0,
                        _ => (CAP_HEIGHT * scale / 2.0).min(w / 2.0),
                    };
                    frame.fill(
                        &Path::rounded_rectangle(
                            Point::new(x, y0),
                            Size::new(w, y1 - y0),
                            r.into(),
                        ),
                        cap_fill,
                    );
                }
            }
        }
    }

    /// Bars in a ring around the album cover. Each bar is drawn in a frame rotated
    /// so "up" points outward, which reuses the bar drawing (corners, segments, gradients).
    fn draw_radial(&self, frame: &mut Frame, bounds: Rectangle) {
        let v = self.visuals;
        let n = v.bar_count as usize;
        let up_down = v.mirror.up_down();
        let left_right = v.mirror.left_right();
        let center = Point::new(bounds.width / 2.0, bounds.height / 2.0);

        let extra = if v.peak_caps {
            CAP_GAP + CAP_HEIGHT
        } else {
            0.0
        };
        let outer = (bounds.width.min(bounds.height) / 2.0 - 8.0 - extra).max(40.0);
        let scale = (outer / 300.0).min(1.0);
        let inner = outer * 0.42;
        // Max length can shorten the bars; the ring already fills the space.
        let reach = (outer - inner).min(v.max_length as f32 * scale);
        // Up/down mirroring also grows bars inward, toward the cover.
        let reach_in = if up_down { inner * 0.35 } else { 0.0 };
        let cover_r = inner - reach_in - 10.0 * scale;
        // Keep bars from overlapping at the inner edge.
        let w = (v.bar_width as f32 * scale)
            .min(TAU * inner / n as f32 * 0.8)
            .max(1.0);
        let min_len = MIN_VISIBLE * scale;
        let stops = &self.stops;
        let lift = GLOW_BRIGHTEN * self.glow;

        // With left/right mirroring the bass (middle index) sits at the top.
        let mid = if left_right {
            (n as f32 - 1.0) / 2.0
        } else {
            -0.5
        };
        let angle = |j: usize| -PI / 2.0 + TAU * (j as f32 - mid) / n as f32;
        let map = BarMap::for_visuals(v);
        let value = |j: usize, src: &[f32]| map.value(src, j, n);

        // The cover in the middle (or a faint ring when there's none).
        if cover_r > 8.0 {
            match self.cover {
                Some(handle) => {
                    let r = cover_r * (1.0 + 0.04 * self.glow);
                    frame.draw_image(
                        Rectangle::new(
                            Point::new(center.x - r, center.y - r),
                            Size::new(2.0 * r, 2.0 * r),
                        ),
                        canvas::Image {
                            handle: handle.clone(),
                            filter_method: FilterMethod::Linear,
                            rotation: Radians(self.spin),
                            border_radius: 0.0.into(),
                            opacity: 1.0,
                            snap: false,
                        },
                    );
                }
                None => frame.stroke(
                    &Path::circle(center, cover_r),
                    Stroke::default()
                        .with_color(with_alpha(TRACK, 0.6))
                        .with_width(2.0),
                ),
            }
        }

        let rotated = |frame: &mut Frame, j: usize, f: &mut dyn FnMut(&mut Frame)| {
            frame.with_save(|frame| {
                frame.translate(Vector::new(center.x, center.y));
                frame.rotate(angle(j) + PI / 2.0);
                f(frame);
            });
        };

        if self.glow > 0.01 {
            let color = self.halo_color();
            for j in 0..n {
                let len = (value(j, self.values) * reach).max(min_len);
                let len_in = value(j, self.values) * reach_in;
                rotated(frame, j, &mut |frame| {
                    let bar = Rectangle::new(
                        Point::new(-w / 2.0, -(inner + len)),
                        Size::new(w, len + len_in),
                    );
                    draw_halo(frame, bar, scale, color, self.glow);
                });
            }
        }

        for j in 0..n {
            let len = (value(j, self.values) * reach).max(min_len);
            let len_in = value(j, self.values) * reach_in;
            let fill: Fill = match v.gradient {
                GradientDir::Along if !is_solid(&self.stops) => {
                    let mut g = canvas::gradient::Linear::new(
                        Point::new(0.0, -inner),
                        Point::new(0.0, -(inner + len)),
                    );
                    for (k, c) in stops.iter().enumerate() {
                        g = g.add_stop(k as f32 / (stops.len() - 1) as f32, brighten(*c, lift));
                    }
                    g.into()
                }
                // "Across" runs around the ring.
                _ => brighten(sample(stops, j as f32 / n.max(2) as f32), lift).into(),
            };
            let peak = (value(j, self.peaks) * reach).max(len);
            rotated(frame, j, &mut |frame| {
                draw_run(frame, -w / 2.0, w, -inner, -(inner + len), v, scale, fill);
                if len_in > 0.5 {
                    draw_run(
                        frame,
                        -w / 2.0,
                        w,
                        -inner,
                        -(inner - len_in),
                        v,
                        scale,
                        fill,
                    );
                }
                if v.peak_caps {
                    let near = -(inner + peak + CAP_GAP * scale);
                    let r = if v.corners == Corners::Square {
                        0.0
                    } else {
                        (CAP_HEIGHT * scale / 2.0).min(w / 2.0)
                    };
                    frame.fill(
                        &Path::rounded_rectangle(
                            Point::new(-w / 2.0, near - CAP_HEIGHT * scale),
                            Size::new(w, CAP_HEIGHT * scale),
                            r.into(),
                        ),
                        fill,
                    );
                }
            });
        }
    }

    /// A smooth curve through the spectrum with a soft fill underneath.
    fn draw_wave(&self, frame: &mut Frame, bounds: Rectangle) {
        let v = self.visuals;
        let n = (v.bar_count as usize).max(2);
        let up_down = v.mirror.up_down();
        let map = BarMap::for_visuals(v);
        let max_len = v.max_length as f32;
        let scale = (bounds.height / max_len.min(REFERENCE_LENGTH)).min(1.0);
        let width = (bounds.width - 48.0).clamp(40.0, 1400.0);
        let x0 = (bounds.width - width) / 2.0;
        let center_y = bounds.height / 2.0;
        let full = (max_len * scale).min(bounds.height);
        let base_y = if up_down {
            center_y
        } else {
            center_y + full / 2.0
        };
        let half = if up_down { full / 2.0 } else { full };
        let line_w = (v.bar_width as f32 / 2.0 + 1.0) * scale.max(0.5);
        let stops = &self.stops;
        let lift = GLOW_BRIGHTEN * self.glow;

        let heights: Vec<f32> = (0..n)
            .map(|j| {
                let value = map.value(self.values, j, n);
                (value * half).max(1.5 * scale)
            })
            .collect();
        let points = |dir: f32| -> Vec<Point> {
            heights
                .iter()
                .enumerate()
                .map(|(j, h)| Point::new(x0 + width * j as f32 / (n - 1) as f32, base_y + dir * h))
                .collect()
        };
        let top = points(-1.0);
        let bottom = if up_down { points(1.0) } else { Vec::new() };

        // Colors: across = left to right; along = from the baseline out to the peaks.
        let gradient = |alpha: f32| -> Fill {
            if is_solid(&self.stops) {
                return with_alpha(brighten(stops[0], lift), alpha).into();
            }
            let (a, b) = match v.gradient {
                GradientDir::Across => (Point::new(x0, 0.0), Point::new(x0 + width, 0.0)),
                GradientDir::Along => (Point::new(0.0, base_y), Point::new(0.0, base_y - half)),
            };
            let mut g = canvas::gradient::Linear::new(a, b);
            for (k, c) in stops.iter().enumerate() {
                g = g.add_stop(
                    k as f32 / (stops.len() - 1) as f32,
                    with_alpha(brighten(*c, lift), alpha),
                );
            }
            g.into()
        };
        let stroke_with = |fill: Fill, width: f32| Stroke {
            style: fill.style,
            width,
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            ..Stroke::default()
        };

        // Soft fill under (or between) the curves.
        let area = Path::new(|p| {
            p.move_to(Point::new(top[0].x, base_y));
            smooth_through(p, &top, true);
            if up_down {
                let back: Vec<Point> = bottom.iter().rev().copied().collect();
                p.line_to(back[0]);
                smooth_through(p, &back, false);
            } else {
                p.line_to(Point::new(top[n - 1].x, base_y));
            }
            p.close();
        });
        frame.fill(&area, gradient(0.22 + 0.1 * self.glow));

        let curves: Vec<&Vec<Point>> = if up_down {
            vec![&top, &bottom]
        } else {
            vec![&top]
        };
        for curve in curves {
            let path = Path::new(|p| {
                p.move_to(curve[0]);
                smooth_through(p, curve, false);
            });
            if self.glow > 0.01 {
                for k in 1..=HALO_LAYERS {
                    let alpha =
                        HALO_ALPHA * self.glow * (1.0 - k as f32 / (HALO_LAYERS as f32 + 1.0));
                    frame.stroke(
                        &path,
                        stroke_with(gradient(alpha), line_w + 2.0 * k as f32 * HALO_STEP * scale),
                    );
                }
            }
            frame.stroke(&path, stroke_with(gradient(1.0), line_w));
        }
    }
}

/// Continue `p` through `points` with a Catmull-Rom spline (as cubic Béziers).
/// With `from_first`, first draws a line to the first point.
fn smooth_through(p: &mut canvas::path::Builder, points: &[Point], from_first: bool) {
    if points.is_empty() {
        return;
    }
    if from_first {
        p.line_to(points[0]);
    }
    let at = |i: isize| points[i.clamp(0, points.len() as isize - 1) as usize];
    for i in 0..points.len() as isize - 1 {
        let (p0, p1, p2, p3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
        let c1 = Point::new(p1.x + (p2.x - p0.x) / 6.0, p1.y + (p2.y - p0.y) / 6.0);
        let c2 = Point::new(p2.x - (p3.x - p1.x) / 6.0, p2.y - (p3.y - p1.y) / 6.0);
        p.bezier_curve_to(c1, c2, p2);
    }
}

impl<M> canvas::Program<M> for Bars<'_> {
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
        match self.visuals.style {
            VisualStyle::Bars => self.draw_bars(&mut frame, bounds),
            VisualStyle::Radial => self.draw_radial(&mut frame, bounds),
            VisualStyle::Wave => self.draw_wave(&mut frame, bounds),
        }
        vec![frame.into_geometry()]
    }
}

// ---------------------------------------------------------------------------
// Seek bar

const SEEK_HEIGHT: f32 = 40.0;
const STROKE: f32 = 7.0;
const AMPLITUDE: f32 = 6.0;
const WAVELENGTH: f32 = 100.0;
const HANDLE_W: f32 = 8.0;
const HANDLE_H: f32 = 36.0;
const HANDLE_GAP: f32 = 10.0;
/// Width reserved for each time label, and the gap to the bar.
const LABEL_W: f32 = 46.0;
const LABEL_GAP: f32 = 14.0;

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
    pub on_seek: fn(f32) -> M,
}

#[derive(Default)]
pub struct SeekState {
    drag: Option<f32>,
    /// Where the pointer is over the bar (0..=1), for the time preview.
    hover: Option<f32>,
}

pub fn seek_height() -> f32 {
    SEEK_HEIGHT
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
        let side = if self.time.is_some() {
            LABEL_W + LABEL_GAP
        } else {
            0.0
        };
        (side + STROKE / 2.0, bounds.width - side - STROKE / 2.0)
    }

    fn fraction_at(&self, bounds: Rectangle, p: Point) -> f32 {
        let (x0, x1) = self.x_range(bounds);
        ((p.x - bounds.x - x0) / (x1 - x0)).clamp(0.0, 1.0)
    }

    fn over_bar(&self, bounds: Rectangle, p: Point) -> bool {
        let (x0, x1) = self.x_range(bounds);
        let x = p.x - bounds.x;
        bounds.contains(p) && x >= x0 - HANDLE_W && x <= x1 + HANDLE_W
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
        let line = |color: Color| {
            Stroke::default()
                .with_color(with_alpha(color, alpha))
                .with_width(STROKE)
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

        let hx = x0 + HANDLE_W / 2.0 + progress * (x1 - x0 - HANDLE_W);
        let wave_end = hx - HANDLE_W / 2.0 - HANDLE_GAP - STROKE / 2.0;
        if wave_end > x0 {
            let wave = Path::new(|p| {
                let y = |x: f32| cy - AMPLITUDE * (TAU * (x - x0) / WAVELENGTH - self.phase).sin();
                p.move_to(Point::new(x0, y(x0)));
                let mut x = x0;
                while x < wave_end {
                    x = (x + 2.0).min(wave_end);
                    p.line_to(Point::new(x, y(x)));
                }
            });
            frame.stroke(&wave, line(self.accent));
        }
        let rest_start = hx + HANDLE_W / 2.0 + HANDLE_GAP + STROKE / 2.0;
        if rest_start < x1 {
            frame.stroke(
                &Path::line(Point::new(rest_start, cy), Point::new(x1, cy)),
                line(TEXT),
            );
        }
        // Hover marker: a faint tick where a click would seek to.
        if let (Some(f), None) = (state.hover, state.drag) {
            let x = x0 + HANDLE_W / 2.0 + f * (x1 - x0 - HANDLE_W);
            frame.fill(
                &Path::rounded_rectangle(
                    Point::new(x - 1.5, cy - 9.0),
                    Size::new(3.0, 18.0),
                    1.5.into(),
                ),
                with_alpha(TEXT, 0.45 * alpha),
            );
        }
        frame.fill(
            &Path::rounded_rectangle(
                Point::new(hx - HANDLE_W / 2.0, cy - HANDLE_H / 2.0),
                Size::new(HANDLE_W, HANDLE_H),
                4.0.into(),
            ),
            with_alpha(SEEK_HANDLE, alpha),
        );
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

    #[test]
    fn palette_sampling() {
        let stops = [[0, 0, 0], [100, 200, 50], [200, 0, 250]];
        assert_eq!(sample(&stops, 0.0), [0, 0, 0]);
        assert_eq!(sample(&stops, 0.5), [100, 200, 50]);
        assert_eq!(sample(&stops, 1.0), [200, 0, 250]);
        assert_eq!(sample(&stops, 0.25), [50, 100, 25]);
        assert_eq!(sample(&[[9, 9, 9]], 0.7), [9, 9, 9]);
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

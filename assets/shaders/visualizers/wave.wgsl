// Wave: a smooth curve through the spectrum with a soft fill underneath.
//
// One of Undavi's built-in visualizers, written the same way as your own (see
// docs/shaders.md). Copy it into your shaders folder to start from it.
//
// @section Line
// @setting bar_count: int(16, 96) = 43 "Points"
// @setting bar_width: int(2, 16) = 8 "Line width" "px"
// @setting max_length: int(100, 1200, 10) = 400 "Max height" "px"
// @section Mirror
// @setting mirror: choice("Off", "Up/down", "Left/right", "Both") = "Up/down" "Mirror"
// @setting split: choice("Off", "Stereo", "Mid/Side") = "Stereo" "Split" if mirror = "Left/right" | "Both"
// @help Stereo: the left channel on the left, the right on the right. Mid/Side: what both channels share on the left, what differs on the right.
// @setting channel: choice("Mono", "Left", "Right") = "Mono" "Channel" if mirror = "Off" | "Up/down" or split = "Off"
// @help Mono mixes both channels together.
// @section Glow
// @setting glow: toggle = false "Glow on the beat"
// @help The line brightens with the bass or drums
// @setting glow_source: choice("Bass", "Drums") = "Bass" "Sync to" if glow
// @setting glow_amount: int(0, 100) = 50 "Intensity" "%" if glow

const REFERENCE_LENGTH: f32 = 400.0;

fn visualizer(uv: vec2<f32>, t: f32) -> vec4<f32> {
    let size = resolution();
    let p = uv * size;
    let n = max(bar_count(), 2);
    let up_down = settings.mirror == MIRROR_UP_DOWN || settings.mirror == MIRROR_BOTH;
    let max_len = f32(settings.max_length);
    let scale = min(size.y / min(max_len, REFERENCE_LENGTH), 1.0);
    let span = clamp(size.x - 48.0, 40.0, 1400.0);
    let x0 = (size.x - span) * 0.5;
    let full = min(max_len * scale, size.y);
    let base = select(size.y * 0.5 + full * 0.5, size.y * 0.5, up_down);
    let half = select(full, full * 0.5, up_down);
    let line_w = (f32(settings.bar_width) * 0.5 + 1.0) * max(scale, 0.5);

    // The curve's height (and slope) at this x: a Catmull-Rom spline through the
    // points, never quite flat.
    let x = clamp(p.x, x0, x0 + span);
    let step = span / f32(n - 1);
    let fi = clamp((x - x0) / step, 0.0, f32(n - 1));
    let i = min(i32(floor(fi)), n - 2);
    let ti = fi - f32(i);
    let low = 1.5 * scale;
    let p0 = max(bar(i - 1) * half, low);
    let p1 = max(bar(i) * half, low);
    let p2 = max(bar(i + 1) * half, low);
    let p3 = max(bar(i + 2) * half, low);
    let h = catmull_rom(p0, p1, p2, p3, ti);
    let slope = catmull_rom_slope(p0, p1, p2, p3, ti) / step;

    // The distance to the line (and its mirror), from |y - f(x)| / sqrt(1 + f'²);
    // past the ends, to the end points, so the line gets round caps.
    let up = base - p.y;
    let off = abs(p.x - x);
    let dy = select(abs(up - h), min(abs(up - h), abs(up + h)), up_down);
    let d_line = select(dy / sqrt(1.0 + slope * slope), length(vec2<f32>(off, dy)), off > 0.0);
    let d = d_line - line_w * 0.5;

    // Along: out from the baseline; across: left to right.
    let c = select(bar_color((p.x - x0) / span), bar_color(abs(up) / max(half, 0.001)), along());

    // A soft fill under (or between) the curves, the glow, then the line.
    let in_fill = select(up >= 0.0 && up <= h, abs(up) <= h, up_down) && off <= 0.0;
    var out = vec4<f32>(c, select(0.0, 0.22 + 0.1 * glow(), in_fill));
    out = over(out, vec4<f32>(glow_color(), halo(d, 14.0 * scale)));
    return over(out, vec4<f32>(c, coverage(d)));
}

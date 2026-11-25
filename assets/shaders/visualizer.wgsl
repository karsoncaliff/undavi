// The prelude for every visualizer, Undavi's own (shaders/visualizers/) and
// custom ones (docs/shaders.md): a file with `fn visualizer(uv, t) -> vec4<f32>`
// is placed before this one, after common.wgsl, and can call the public helpers
// below. Internal names start with `undavi_`. The composer (src/shaders/mod.rs)
// adds the file's `settings` struct and `undavi_custom_visualizer`, which calls it.

struct UndaviVis {
    // Logical width and height, clock (s), logical size of one screen pixel.
    size: vec4<f32>,
    // Glow (0..1), palette stop count, 1 when the target is sRGB (colors then
    // need converting to linear), 1 when the palette runs along each bar.
    info: vec4<f32>,
    // Glow color (0..1 RGB), unused.
    halo: vec4<f32>,
    // Bass, mids, treble, beat (0..1).
    levels: vec4<f32>,
    // Overall level, bar count, has a cover (0/1), unused.
    extra: vec4<f32>,
    // 16 spectrum bands.
    bands: array<vec4<f32>, 4>,
    // The accent (0..1 RGB).
    accent: vec4<f32>,
    // Four colors from the cover.
    colors: array<vec4<f32>, 4>,
    // The bar palette's stops (up to 8).
    stops: array<vec4<f32>, 8>,
    // What each bar shows (up to 96, 0..1), and its peak cap.
    values: array<vec4<f32>, 24>,
    peaks: array<vec4<f32>, 24>,
};

@group(0) @binding(0) var<uniform> undavi_v: UndaviVis;
@group(0) @binding(1) var undavi_cover_tex: texture_2d<f32>;
@group(0) @binding(2) var undavi_samp: sampler;

// ---------------------------------------------------------------------------
// The music and the colors.

// The visualizer's area in logical pixels.
fn resolution() -> vec2<f32> {
    return undavi_v.size.xy;
}

fn bass() -> f32 {
    return undavi_v.levels.x;
}
fn mids() -> f32 {
    return undavi_v.levels.y;
}
fn treble() -> f32 {
    return undavi_v.levels.z;
}
fn beat() -> f32 {
    return undavi_v.levels.w;
}
fn level() -> f32 {
    return undavi_v.extra.x;
}

fn band(i: i32) -> f32 {
    let k = u32(clamp(i, 0, 15));
    return undavi_v.bands[k / 4u][k % 4u];
}

fn palette(i: i32) -> vec3<f32> {
    return undavi_v.colors[u32(clamp(i, 0, 3))].rgb;
}

fn accent() -> vec3<f32> {
    return undavi_v.accent.rgb;
}

// How many bars there are: the visualizer's `bar_count` setting (64 without one).
fn bar_count() -> i32 {
    return i32(undavi_v.extra.y);
}

// What bar `i` shows (0..1), with the `mirror`, `split` and `channel` settings
// applied.
fn bar(i: i32) -> f32 {
    let k = u32(clamp(i, 0, max(bar_count() - 1, 0)));
    return undavi_v.values[k / 4u][k % 4u];
}

// Where bar `i`'s peak cap is (0..1).
fn peak(i: i32) -> f32 {
    let k = u32(clamp(i, 0, max(bar_count() - 1, 0)));
    return undavi_v.peaks[k / 4u][k % 4u];
}

// Catmull-Rom spline through p1..p2 (p0 and p3 shape it), at `t` (0..1), and
// its slope.
fn catmull_rom(p0: f32, p1: f32, p2: f32, p3: f32, t: f32) -> f32 {
    let t2 = t * t;
    let t3 = t2 * t;
    return 0.5 * ((2.0 * p1) + (-p0 + p2) * t + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
        + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t3);
}
fn catmull_rom_slope(p0: f32, p1: f32, p2: f32, p3: f32, t: f32) -> f32 {
    let t2 = t * t;
    return 0.5 * ((-p0 + p2) + 2.0 * (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t
        + 3.0 * (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t2);
}

// The bars as a smooth curve: `x` from 0 (first bar) to 1 (last bar).
fn spectrum(x: f32) -> f32 {
    let n = bar_count();
    if (n < 2) {
        return bar(0);
    }
    let f = clamp(x, 0.0, 1.0) * f32(n - 1);
    let i = i32(floor(f));
    let t = f - f32(i);
    return max(catmull_rom(bar(i - 1), bar(i), bar(i + 1), bar(i + 2), t), 0.0);
}

// The beat glow (0..1): the `glow` and `glow_amount` settings, on the beat.
fn glow() -> f32 {
    return undavi_v.info.x;
}

// The glow's color: the middle of the palette.
fn glow_color() -> vec3<f32> {
    return undavi_v.halo.rgb;
}

// The bar palette at `t` (0..1), brightened with the glow.
fn bar_color(t: f32) -> vec3<f32> {
    let c = undavi_gradient(t);
    return c + (vec3<f32>(1.0) - c) * 0.4 * glow();
}

// Whether the palette should run along each bar (Colors → Gradient → "Along"),
// rather than across them all. False for a single color.
fn along() -> bool {
    return undavi_v.info.w > 0.5;
}

fn has_cover() -> bool {
    return undavi_v.extra.z > 0.5;
}

// The cover at `uv` (0..1 across it); dark gray when there's none.
fn cover(uv: vec2<f32>) -> vec3<f32> {
    if (!has_cover()) {
        return vec3<f32>(0.15, 0.15, 0.17);
    }
    return textureSampleLevel(undavi_cover_tex, undavi_samp, clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0).rgb;
}

// ---------------------------------------------------------------------------
// Drawing shapes.

// The size of one screen pixel, in the logical pixels `uv * resolution()` uses.
fn px() -> f32 {
    return undavi_v.size.w;
}

// Inigo Quilez's rounded box: the signed distance from `p` to a box of half
// size `half` centered at the origin, with corners rounded by `r`.
fn round_box(p: vec2<f32>, half: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - half + vec2<f32>(r);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

// How much of a pixel `d` from a shape's edge is inside it (anti-aliased).
fn coverage(d: f32) -> f32 {
    return clamp(0.5 - d / px(), 0.0, 1.0);
}

// A soft glow outside a shape, `d` away, fading out over `reach` (with the beat).
fn halo(d: f32, reach: f32) -> f32 {
    let k = 1.0 - clamp(d / max(reach, 0.001), 0.0, 1.0);
    return 0.38 * glow() * k * k * step(0.0, d);
}

// `top` drawn over `under` (straight alpha).
fn over(under: vec4<f32>, top: vec4<f32>) -> vec4<f32> {
    let a = top.a + under.a * (1.0 - top.a);
    if (a <= 0.0) {
        return vec4<f32>(0.0);
    }
    let rgb = (top.rgb * top.a + under.rgb * under.a * (1.0 - top.a)) / a;
    return vec4<f32>(rgb, a);
}

// The distance from a point to one run of a bar: along its axis from `base`
// in direction `dir` (+1 or -1) for `len`, `w` wide, corners rounded by up to
// `corner`. `x` is the point's offset across the bar, `o` its position along the
// axis. With `seg` > 0 the run is a stack of blocks `seg` long with `gap`
// between them, starting `start` from the base.
fn bar_run(x: f32, o: f32, base: f32, dir: f32, len: f32, w: f32, corner: f32, seg: f32, gap: f32, start: f32) -> f32 {
    let s = (o - base) * dir;
    if (seg <= 0.0) {
        let r = min(corner, min(len * 0.5, w * 0.5));
        return round_box(vec2<f32>(x, s - len * 0.5), vec2<f32>(w * 0.5, len * 0.5), r);
    }
    if (len <= start) {
        // Too short for a full block: still a sliver.
        let r = min(corner, min(len * 0.5, w * 0.5));
        return round_box(vec2<f32>(x, s - start - len * 0.5), vec2<f32>(w * 0.5, len * 0.5), r);
    }
    let period = seg + gap;
    let count = ceil((len - start) / period);
    let i = clamp(floor((s - start) / period), 0.0, count - 1.0);
    var d = 1e9;
    for (var k = -1; k <= 1; k = k + 1) {
        let j = i + f32(k);
        if (j < 0.0 || j > count - 1.0) {
            continue;
        }
        // The last block is cut to fit the bar's length.
        let s0 = start + j * period;
        let s1 = min(s0 + seg, len);
        let h = s1 - s0;
        if (h <= 0.01) {
            continue;
        }
        let r = min(corner, min(h * 0.5, w * 0.5));
        d = min(d, round_box(vec2<f32>(x, s - (s0 + s1) * 0.5), vec2<f32>(w * 0.5, h * 0.5), r));
    }
    return d;
}

// ---------------------------------------------------------------------------
// Internals.

// The bar palette at `t`, between its stops.
fn undavi_gradient(t: f32) -> vec3<f32> {
    let n = i32(undavi_v.info.y);
    if (n <= 1) {
        return undavi_v.stops[0].rgb;
    }
    let x = clamp(t, 0.0, 1.0) * f32(n - 1);
    let i = min(i32(floor(x)), n - 2);
    let f = x - f32(i);
    return mix(undavi_v.stops[u32(i)].rgb, undavi_v.stops[u32(i + 1)].rgb, f);
}

// Colors are worked out like the rest of the UI (gamma space); an sRGB target
// expects linear values.
fn undavi_out(c: vec3<f32>, a: f32) -> vec4<f32> {
    var rgb = clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
    if (undavi_v.info.z > 0.5) {
        rgb = select(pow((rgb + 0.055) / 1.055, vec3<f32>(2.4)), rgb / 12.92, rgb <= vec3<f32>(0.04045));
    }
    return vec4<f32>(rgb, clamp(a, 0.0, 1.0));
}

struct UndaviFullOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn undavi_vs_full(@builtin(vertex_index) i: u32) -> UndaviFullOut {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var out: UndaviFullOut;
    out.pos = vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(uv.x, 1.0 - uv.y);
    return out;
}

@fragment
fn undavi_fs_custom(in: UndaviFullOut) -> @location(0) vec4<f32> {
    let c = undavi_custom_visualizer(in.uv, undavi_v.size.z);
    return undavi_out(c.rgb, c.a);
}

// The prelude for every background, Undavi's own (shaders/backgrounds/) and
// custom ones (docs/shaders.md): a file with `fn scene(uv, t) -> vec3<f32>` is
// placed before this one, after common.wgsl, and can call the public helpers
// below. Internal names start with `undavi_`. The composer (src/shaders/mod.rs)
// adds the file's `settings` struct, `undavi_custom_scene` (which calls `scene`)
// and `undavi_custom_warp` (a custom warp picked as the background's animation).
//
// The scene is drawn at half resolution into an offscreen texture, then blurred
// and graded; see backdrop.rs. While the cover changes, it's drawn twice (once
// per cover) and the two are mixed.

struct UndaviScene {
    // Logical width and height, clock (s), unused.
    size: vec4<f32>,
    // The outgoing and current cover slots (0 none, 1 A, 2 B), how far the
    // cross-fade between them is (1 = done), unused.
    fade: vec4<f32>,
    // The beat pulse (0..1, with its intensity), how much to swell (a fraction
    // of the size), unused ×2.
    pulse: vec4<f32>,
    // The four colors from the cover (or the defaults), 0..1 RGB.
    colors: array<vec4<f32>, 4>,
    // What's under everything.
    base: vec4<f32>,
};

struct UndaviAudio {
    // Bass, mids, treble, beat pulse (0..1).
    levels: vec4<f32>,
    // Overall level (0..1), unused ×3.
    extra: vec4<f32>,
    // 16 spectrum bands, low to high (0..1), four per vector.
    bands: array<vec4<f32>, 4>,
    // The accent color (0..1 RGB).
    accent: vec4<f32>,
};

@group(0) @binding(0) var<uniform> undavi_u: UndaviScene;
@group(0) @binding(1) var undavi_cover_a: texture_2d<f32>;
@group(0) @binding(2) var undavi_cover_b: texture_2d<f32>;
@group(0) @binding(3) var undavi_samp: sampler;
@group(0) @binding(4) var<uniform> undavi_audio: UndaviAudio;

// Which cover `cover()` reads while the scene is drawn (see `undavi_fs`).
var<private> undavi_slot: f32;

// ---------------------------------------------------------------------------
// The music and the colors.

// The window's size in logical pixels.
fn resolution() -> vec2<f32> {
    return undavi_u.size.xy;
}

// The music, 0..1: low, middle and high frequencies, the beat pulse (jumps on
// a hit and fades), and the overall level.
fn bass() -> f32 {
    return undavi_audio.levels.x;
}
fn mids() -> f32 {
    return undavi_audio.levels.y;
}
fn treble() -> f32 {
    return undavi_audio.levels.z;
}
fn beat() -> f32 {
    return undavi_audio.levels.w;
}
fn level() -> f32 {
    return undavi_audio.extra.x;
}

// One of 16 spectrum bands (0 = lowest, 15 = highest), 0..1.
fn band(i: i32) -> f32 {
    let k = u32(clamp(i, 0, 15));
    return undavi_audio.bands[k / 4u][k % 4u];
}

// One of four colors taken from the cover (0..3).
fn palette(i: i32) -> vec3<f32> {
    return undavi_u.colors[u32(clamp(i, 0, 3))].rgb;
}

// The accent color.
fn accent() -> vec3<f32> {
    return undavi_audio.accent.rgb;
}

// The background's beat pulse (0..1): its `pulse` settings, on the beat.
fn pulse() -> f32 {
    return undavi_u.pulse.x;
}

// How much the beat swells the picture now (a fraction of its size), from the
// `pulse_style` setting; `album` and `ambient` apply it.
fn swell() -> f32 {
    return undavi_u.pulse.y;
}

fn has_cover() -> bool {
    return undavi_slot > 0.5;
}

// The current cover at `uv` (0..1 across it), or the Ambient circles when
// there's none.
fn cover(uv: vec2<f32>) -> vec3<f32> {
    if (!has_cover()) {
        return undavi_ambient_at(uv);
    }
    return undavi_cover(undavi_slot, clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)));
}

// ---------------------------------------------------------------------------
// Animations. `q` is a point relative to the window's center, in units of its
// shorter side; each returns the point to take the color from.

// Liquid: Inigo Quilez's domain warping, f(p + fbm(p + fbm(p))), drifting through
// the noise over time so the picture flows like oil on water.
fn liquid(q: vec2<f32>, t: f32) -> vec2<f32> {
    let p = q * 0.8;
    let a = vec2<f32>(
        fbm(p + vec2<f32>(0.0, 0.0) + vec2<f32>(0.05 * t, 0.02 * t)),
        fbm(p + vec2<f32>(5.2, 1.3) - vec2<f32>(0.03 * t, 0.04 * t)),
    );
    let b = vec2<f32>(
        fbm(p + 1.5 * a + vec2<f32>(1.7, 9.2) + vec2<f32>(0.02 * t, 0.0)),
        fbm(p + 1.5 * a + vec2<f32>(8.3, 2.8) - vec2<f32>(0.0, 0.03 * t)),
    );
    return q + 0.13 * b;
}

// Warp: the Brown–Conrady radial lens model (k1, k2 breathing between barrel
// and pincushion), with a damped ripple running out from the middle.
fn lens(q: vec2<f32>, t: f32) -> vec2<f32> {
    let r2 = dot(q, q);
    let k1 = 0.32 * sin(0.21 * t);
    let k2 = 0.12 * sin(0.13 * t + 1.7);
    var w = q * (1.0 + k1 * r2 + k2 * r2 * r2);
    let r = sqrt(r2);
    if (r > 1e-4) {
        let ripple = 0.022 * sin(TAU * (3.0 * r - 0.22 * t)) * exp(-1.6 * r);
        w = w + (q / r) * ripple;
    }
    return w;
}

// Spiral: the classic twirl ("whirl") filter, turning points by an angle that
// falls off smoothly with the distance from the middle, winding in and out,
// plus a slow turn of the whole picture.
fn twirl(q: vec2<f32>, t: f32) -> vec2<f32> {
    let r = length(q);
    let falloff = 1.0 - smoothstep(0.0, 0.8, r);
    let strength = 3.2 * sin(0.11 * t);
    let angle = strength * falloff * falloff + 0.04 * t;
    return rotate(q, -angle);
}

// `p` (logical px) moved by animation `kind`: 0 Liquid, 1 Warp, 2 Spiral, 3 or
// more the custom warp picked in its place.
fn animate(p: vec2<f32>, kind: i32, t: f32) -> vec2<f32> {
    let size = resolution();
    let m = min(size.x, size.y);
    let center = size * 0.5;
    let q = (p - center) / m;
    var w = q;
    if (kind == 0) {
        w = liquid(q, t);
    } else if (kind == 1) {
        w = lens(q, t);
    } else if (kind == 2) {
        w = twirl(q, t);
    } else {
        w = undavi_custom_warp(q, t);
    }
    return center + w * m;
}

// ---------------------------------------------------------------------------
// Pictures. `motion`: 0 still, 1 drift, 2 rotate (slowly, over time `t`).

// The cover laid out over the window at `p` (logical px): `arrange` 0 three
// soft-edged copies, 1 one large cover, 2 tiles; `zoom` 1 = normal size.
fn album(p: vec2<f32>, arrange: i32, zoom: f32, motion: i32, t: f32) -> vec3<f32> {
    let size = resolution();
    var c = undavi_u.base.rgb;
    if (arrange == 0) {
        // A square container 1.5× the longer side, so turning copies never
        // expose its corners.
        let side = 1.5 * max(size.x, size.y) * zoom;
        let origin = size * 0.5 - vec2<f32>(side * 0.5);
        c = undavi_copy_over(c, p, undavi_copy(origin, side, 0.88, vec2<f32>(0.50, 0.50), 90.0, 1.0, motion, t));
        c = undavi_copy_over(c, p, undavi_copy(origin, side, 0.52, vec2<f32>(0.28, 0.34), 60.0, -1.0, motion, t));
        c = undavi_copy_over(c, p, undavi_copy(origin, side, 0.40, vec2<f32>(0.74, 0.70), 44.0, 1.0, motion, t));
    } else if (arrange == 1) {
        // Big enough to cover the window at any angle and drift.
        let side = 1.15 * length(size) * zoom;
        var center = size * 0.5;
        var angle = 0.0;
        if (motion == 2) {
            angle = TAU * fract(t / 120.0);
        } else if (motion == 1) {
            center = center + 0.04 * side * vec2<f32>(sin(TAU * t / 52.0), cos(TAU * t / 46.0));
            angle = 0.08 * sin(TAU * t / 40.0);
        }
        let half = side * 0.5 * (1.0 + swell());
        c = cover(clamp(undavi_local(p, vec4<f32>(center, half, angle)), vec2<f32>(0.0), vec2<f32>(1.0)));
    } else {
        // Drift pans the grid diagonally; Rotate moves it in a slow circle.
        let tile = 300.0 * zoom;
        var offset = vec2<f32>(0.0);
        if (motion == 1) {
            offset = vec2<f32>(16.0, 10.0) * t;
        } else if (motion == 2) {
            let a = TAU * t / 40.0;
            offset = 0.5 * tile * vec2<f32>(cos(a), sin(a));
        }
        c = cover(fract((p - offset) / (tile * (1.0 + swell()))));
    }
    return undavi_saturate_by(c, 1.35);
}

// The Ambient picture at `p` (logical px): four soft circles in the cover's
// colors, in a container big enough to cover the window at any angle.
fn ambient(p: vec2<f32>, motion: i32, t: f32) -> vec3<f32> {
    let size = resolution();
    let side = 1.25 * length(size) * (1.0 + swell());
    var center = size * 0.5;
    var angle = 0.0;
    if (motion == 2) {
        angle = TAU * fract(t / 70.0);
    } else if (motion == 1) {
        center = center + 0.03 * side * vec2<f32>(sin(TAU * t / 52.0), cos(TAU * t / 46.0));
        angle = 0.12 * sin(TAU * t / 40.0);
    }
    return undavi_ambient_at(undavi_local(p, vec4<f32>(center, side * 0.5, angle)));
}

// ---------------------------------------------------------------------------
// Internals.

fn undavi_saturate_by(c: vec3<f32>, amount: f32) -> vec3<f32> {
    let l = luma(c);
    return clamp(vec3<f32>(l) + (c - vec3<f32>(l)) * amount, vec3<f32>(0.0), vec3<f32>(1.0));
}

fn undavi_cover(slot: f32, uv: vec2<f32>) -> vec3<f32> {
    let a = textureSampleLevel(undavi_cover_a, undavi_samp, uv, 0.0).rgb;
    let b = textureSampleLevel(undavi_cover_b, undavi_samp, uv, 0.0).rgb;
    return select(a, b, slot > 1.5);
}

// Where `p` lands in a turned square (center x, y, half size, angle): 0..1 across it.
fn undavi_local(p: vec2<f32>, rect: vec4<f32>) -> vec2<f32> {
    return rotate(p - rect.xy, -rect.w) / (2.0 * rect.z) + vec2<f32>(0.5);
}

// One of the three copies: its square (center, half size, angle) at time `t`.
fn undavi_copy(origin: vec2<f32>, side: f32, size: f32, at: vec2<f32>, period: f32, dir: f32, motion: i32, t: f32) -> vec4<f32> {
    var center = origin + at * side;
    var d = size * side;
    var angle = 0.0;
    if (motion == 2) {
        angle = dir * TAU * fract(t / period);
    } else if (motion == 1) {
        // Ease out and back; each half-cycle lasts half the period.
        let s = 0.5 - 0.5 * cos(TAU * t / period);
        center = center + vec2<f32>(0.06, -0.06) * side * s;
        d = d * (1.0 + 0.12 * s);
    }
    return vec4<f32>(center, d * 0.5 * (1.0 + swell()), angle);
}

// A soft-edged copy of the cover over `under`.
fn undavi_copy_over(under: vec3<f32>, p: vec2<f32>, rect: vec4<f32>) -> vec3<f32> {
    let l = undavi_local(p, rect);
    let d = length(l - vec2<f32>(0.5)) * 2.0;
    let alpha = 1.0 - smoothstep(0.6, 1.0, d);
    if (alpha <= 0.0) {
        return under;
    }
    return mix(under, cover(clamp(l, vec2<f32>(0.0), vec2<f32>(1.0))), alpha);
}

fn undavi_circle(c: vec3<f32>, l: vec2<f32>, center: vec2<f32>, diameter: f32, color: vec3<f32>) -> vec3<f32> {
    let d = length(l - center);
    let a = 1.0 - smoothstep(diameter * 0.5 - 0.02, diameter * 0.5 + 0.02, d);
    return mix(c, color, a);
}

// The Ambient circles at `l` (0..1 across their container); the blur afterwards
// softens them.
fn undavi_ambient_at(l: vec2<f32>) -> vec3<f32> {
    var c = vec3<f32>(0.078, 0.078, 0.086);
    c = undavi_circle(c, l, vec2<f32>(0.32, 0.36), 0.52, undavi_u.colors[0].rgb);
    c = undavi_circle(c, l, vec2<f32>(0.70, 0.30), 0.46, undavi_u.colors[1].rgb);
    c = undavi_circle(c, l, vec2<f32>(0.62, 0.72), 0.44, undavi_u.colors[2].rgb);
    c = undavi_circle(c, l, vec2<f32>(0.28, 0.72), 0.40, undavi_u.colors[3].rgb);
    return undavi_saturate_by(c, 1.3);
}

struct UndaviVOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// One triangle covering the target.
@vertex
fn undavi_vs(@builtin(vertex_index) i: u32) -> UndaviVOut {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var out: UndaviVOut;
    out.pos = vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(uv.x, 1.0 - uv.y);
    return out;
}

@fragment
fn undavi_fs(in: UndaviVOut) -> @location(0) vec4<f32> {
    let t = undavi_u.size.z;
    let fade = undavi_u.fade;
    undavi_slot = fade.y;
    var c = undavi_custom_scene(in.uv, t);
    if (fade.z < 1.0) {
        undavi_slot = fade.x;
        c = mix(undavi_custom_scene(in.uv, t), c, fade.z);
    }
    return vec4<f32>(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}

// The blurred scene onto the window: color grading (saturation, contrast,
// brightness, accent tint), the dim (lifted on the beat), the scrim behind the
// header, a vignette and film grain.

struct Composite {
    // Logical width and height, background clock (s), unused.
    size: vec4<f32>,
    // Saturation, brightness, contrast (1 = unchanged), tint amount (0..1).
    grade: vec4<f32>,
    // Tint color (0..1 RGB), unused.
    tint: vec4<f32>,
    // Dim (0..1, beat lift applied), vignette, grain (0..1), scrim strength.
    fx: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: Composite;
@group(0) @binding(1) var src: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;

struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs(@builtin(vertex_index) i: u32) -> VOut {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var out: VOut;
    out.pos = vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(uv.x, 1.0 - uv.y);
    return out;
}

fn luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.299, 0.587, 0.114));
}

// The PCG hash (Jarzynski and Olano, "Hash Functions for GPU Rendering", JCGT 2020).
fn pcg(v: u32) -> u32 {
    let state = v * 747796405u + 2891336453u;
    let word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    return (word >> 22u) ^ word;
}

// A pixel and frame number to 0..1.
fn hash(x: u32, y: u32, frame: u32) -> f32 {
    return f32(pcg(x + pcg(y + pcg(frame)))) / 4294967295.0;
}

@fragment
fn fs(in: VOut) -> @location(0) vec4<f32> {
    var c = textureSampleLevel(src, samp, in.uv, 0.0).rgb;

    // Grade: saturation, contrast about mid-gray, brightness, then a tint that
    // keeps each pixel's lightness and takes the accent's hue.
    let l = luma(c);
    c = vec3<f32>(l) + (c - vec3<f32>(l)) * u.grade.x;
    c = (c - vec3<f32>(0.5)) * u.grade.z + vec3<f32>(0.5);
    c = c * u.grade.y;
    if (u.grade.w > 0.0) {
        let k = max(luma(c), 0.0) / max(luma(u.tint.rgb), 0.004);
        c = mix(c, u.tint.rgb * k, u.grade.w);
    }
    c = clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));

    // Dim toward the app's near-black.
    c = mix(c, vec3<f32>(12.0, 12.0, 16.0) / 255.0, u.fx.x);

    // Scrim: darker behind the header so its text stays readable.
    let y = in.uv.y * u.size.y;
    if (u.fx.w > 0.0 && y < 180.0) {
        c = c * (1.0 - u.fx.w * (1.0 - y / 180.0));
    }

    // Vignette: darker toward the corners, following the window's shape.
    if (u.fx.y > 0.0) {
        let d = length((in.uv - vec2<f32>(0.5)) * 2.0) / 1.41421356;
        let t = clamp((d - 0.35) / 0.65, 0.0, 1.0);
        c = c * (1.0 - 0.9 * u.fx.y * t * t * (3.0 - 2.0 * t));
    }

    // Grain: fresh noise per pixel, 24 times a second, centered on zero so it
    // doesn't gray the picture.
    if (u.fx.z > 0.0) {
        let frame = u32(max(u.size.z, 0.0) * 24.0);
        let n = hash(u32(in.pos.x), u32(in.pos.y), frame) - 0.5;
        c = c + vec3<f32>(n * 0.16 * u.fx.z);
    }

    return vec4<f32>(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}

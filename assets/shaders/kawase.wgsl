// Dual Kawase blur (Marius Bjørge, "Bandwidth-Efficient Rendering", SIGGRAPH
// 2015): a chain of half-size downsamples, then upsamples back, each reading a
// few bilinear taps. The radius comes from the number of steps and the offset.

struct Pass {
    // Half a texel of the source texture (x, y), tap offset, unused.
    params: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: Pass;
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

fn tap(uv: vec2<f32>) -> vec4<f32> {
    return textureSampleLevel(src, samp, uv, 0.0);
}

@fragment
fn down(in: VOut) -> @location(0) vec4<f32> {
    let h = u.params.xy * u.params.z;
    var sum = tap(in.uv) * 4.0;
    sum = sum + tap(in.uv - h);
    sum = sum + tap(in.uv + h);
    sum = sum + tap(in.uv + vec2<f32>(h.x, -h.y));
    sum = sum + tap(in.uv - vec2<f32>(h.x, -h.y));
    return sum / 8.0;
}

@fragment
fn up(in: VOut) -> @location(0) vec4<f32> {
    let h = u.params.xy * u.params.z;
    var sum = tap(in.uv + vec2<f32>(-h.x * 2.0, 0.0));
    sum = sum + tap(in.uv + vec2<f32>(-h.x, h.y)) * 2.0;
    sum = sum + tap(in.uv + vec2<f32>(0.0, h.y * 2.0));
    sum = sum + tap(in.uv + vec2<f32>(h.x, h.y)) * 2.0;
    sum = sum + tap(in.uv + vec2<f32>(h.x * 2.0, 0.0));
    sum = sum + tap(in.uv + vec2<f32>(h.x, -h.y)) * 2.0;
    sum = sum + tap(in.uv + vec2<f32>(0.0, -h.y * 2.0));
    sum = sum + tap(in.uv + vec2<f32>(-h.x, -h.y)) * 2.0;
    return sum / 12.0;
}

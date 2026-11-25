// Shared by every Undavi shader that custom shaders build on (scene.wgsl and
// visualizer.wgsl): math helpers and noise. The public names here (TAU, rotate,
// luma, snoise, fbm) are documented in docs/shaders.md.

const TAU: f32 = 6.28318530718;

// `p` turned by `a` radians.
fn rotate(p: vec2<f32>, a: f32) -> vec2<f32> {
    let s = sin(a);
    let c = cos(a);
    return vec2<f32>(c * p.x - s * p.y, s * p.x + c * p.y);
}

// Perceived brightness of a color.
fn luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.299, 0.587, 0.114));
}

// ---------------------------------------------------------------------------
// 2D simplex noise, ported to WGSL from "webgl-noise" (noise2D.glsl) by Ian
// McEwan and Stefan Gustavson, https://github.com/stegu/webgl-noise, under the
// MIT License:
//
// Copyright (C) 2011 Ashima Arts, (C) 2011-2016 Stefan Gustavson
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.

fn undavi_mod289_3(x: vec3<f32>) -> vec3<f32> {
    return x - floor(x * (1.0 / 289.0)) * 289.0;
}

fn undavi_mod289_2(x: vec2<f32>) -> vec2<f32> {
    return x - floor(x * (1.0 / 289.0)) * 289.0;
}

fn undavi_permute(x: vec3<f32>) -> vec3<f32> {
    return undavi_mod289_3(((x * 34.0) + 10.0) * x);
}

// Simplex noise at `v`, about -1..1.
fn snoise(v: vec2<f32>) -> f32 {
    let C = vec4<f32>(0.211324865405187, 0.366025403784439, -0.577350269189626, 0.024390243902439);
    var i = floor(v + dot(v, C.yy));
    let x0 = v - i + dot(i, C.xx);
    var i1 = vec2<f32>(0.0, 1.0);
    if (x0.x > x0.y) {
        i1 = vec2<f32>(1.0, 0.0);
    }
    var x12 = x0.xyxy + C.xxzz;
    x12 = vec4<f32>(x12.xy - i1, x12.zw);
    i = undavi_mod289_2(i);
    let p = undavi_permute(undavi_permute(i.y + vec3<f32>(0.0, i1.y, 1.0)) + i.x + vec3<f32>(0.0, i1.x, 1.0));
    var m = max(vec3<f32>(0.5) - vec3<f32>(dot(x0, x0), dot(x12.xy, x12.xy), dot(x12.zw, x12.zw)), vec3<f32>(0.0));
    m = m * m;
    m = m * m;
    let x = 2.0 * fract(p * C.www) - 1.0;
    let h = abs(x) - 0.5;
    let ox = floor(x + 0.5);
    let a0 = x - ox;
    m = m * (1.79284291400159 - 0.85373472095314 * (a0 * a0 + h * h));
    let g = vec3<f32>(a0.x * x0.x + h.x * x0.y, a0.yz * x12.xz + h.yz * x12.yw);
    return 130.0 * dot(m, g);
}

// Fractal Brownian motion: two octaves of simplex noise, about -1..1. Domain
// warping turns every extra octave into finer, coastline-like edges; two keep
// the shapes broad and liquid.
fn fbm(p: vec2<f32>) -> f32 {
    var sum = 0.0;
    var amp = 0.6;
    var q = p;
    for (var i = 0; i < 2; i = i + 1) {
        sum = sum + amp * snoise(q);
        q = rotate(q, 0.5) * 2.02;
        amp = amp * 0.5;
    }
    return sum;
}

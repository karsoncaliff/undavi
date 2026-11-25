// Plasma (a scene): the classic demoscene effect, a few sine waves added up,
// colored with Inigo Quilez's cosine palette and leaning toward the cover's
// colors. The bass makes the rings swell and the beat brightens it.
//
// `uv` runs 0..1 across the window (y down); return the color there (0..1 RGB).
// The Effects are applied to the result; `blur` is one of the ids Undavi reads
// itself (see docs/shaders.md), so declaring it adds the blur too.
//
// @setting scale: float(1, 8, 0.5) = 3 "Scale"
// @help How many waves fit across the window.
// @setting cover_colors: int(0, 100, 5) = 35 "Cover colors" "%"
// @help How far it leans toward the cover's colors.
// @setting blur: int(0, 120) = 0 "Blur" "px"

// A rainbow that cycles as `x` grows: a + b * cos(TAU * (c * x + d)).
fn cosine_palette(x: f32) -> vec3<f32> {
    let d = vec3<f32>(0.0, 0.33, 0.67);
    return vec3<f32>(0.5) + vec3<f32>(0.5) * cos(TAU * (vec3<f32>(x) + d));
}

fn scene(uv: vec2<f32>, t: f32) -> vec3<f32> {
    let size = resolution();
    let p = (uv - vec2<f32>(0.5)) * vec2<f32>(size.x / size.y, 1.0) * settings.scale;
    var v = sin(p.x + 0.4 * t);
    v = v + sin(1.3 * p.y - 0.3 * t);
    v = v + sin(0.7 * (p.x + p.y) + 0.25 * t);
    let center = vec2<f32>(sin(0.2 * t), cos(0.15 * t)) * 1.5;
    v = v + sin(length(p + center) * (2.0 + 2.0 * bass()));

    let rainbow = cosine_palette(0.15 * v + 0.03 * t);
    let k = 0.5 + 0.5 * sin(1.2 * v);
    let cover_tone = mix(palette(0), palette(2), k);
    return mix(rainbow, cover_tone, f32(settings.cover_colors) / 100.0) * (0.85 + 0.3 * beat());
}

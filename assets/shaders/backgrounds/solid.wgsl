// Solid: one flat color. The global Dim doesn't darken it (the color is already
// what you picked); the other effects still apply.
//
// One of Undavi's built-in backgrounds, written the same way as your own (see
// docs/shaders.md).
//
// @no-dim
// @setting color: color(#141416, #000000, #0f1424, #1c1322) = #141416 "Color"

fn scene(uv: vec2<f32>, t: f32) -> vec3<f32> {
    return settings.color;
}

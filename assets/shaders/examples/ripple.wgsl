// Ripple (a warp): rings running out from the middle, like a drop in water,
// pushed further by the bass and the beat.
//
// @setting rings: int(1, 10) = 4 "Rings"
// @setting strength: int(0, 300, 10) = 100 "Strength" "%"

fn warp(q: vec2<f32>, t: f32) -> vec2<f32> {
    let r = length(q);
    if (r < 0.0001) {
        return q;
    }
    // A damped sine wave travelling outward.
    let wave = sin(TAU * (f32(settings.rings) * r - 0.6 * t)) * exp(-2.0 * r);
    let strength = (0.01 + 0.04 * bass() + 0.03 * beat()) * f32(settings.strength) / 100.0;
    return q + (q / r) * wave * strength;
}

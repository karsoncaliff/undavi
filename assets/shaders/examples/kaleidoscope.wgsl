// Kaleidoscope (a warp): the picture folded into six mirrored slices around the
// middle, turning slowly. The bass pulls it in a little.
//
// `q` is a point relative to the window's center, in units of its shorter side;
// return the point whose color should appear there. `t` is the background clock
// in seconds (it follows Speed and pauses with the music).
//
// Its settings show up under Album art or Ambient while it's the animation:
//
// @setting slices: int(2, 16) = 6 "Slices"
// @setting turning: int(0, 200, 5) = 100 "Turning" "%"
// @setting bass_pull: toggle = true "Pull in with the bass"

fn warp(q: vec2<f32>, t: f32) -> vec2<f32> {
    let slice = TAU / f32(settings.slices);
    var r = length(q);
    if (settings.bass_pull != 0) {
        r = r * (1.0 - 0.15 * bass());
    }
    // The angle, turning with time, folded into the first slice and mirrored.
    var a = atan2(q.y, q.x) + 0.05 * t * f32(settings.turning) / 100.0;
    a = a - slice * floor(a / slice);
    a = abs(a - slice * 0.5);
    return vec2<f32>(cos(a), sin(a)) * r;
}

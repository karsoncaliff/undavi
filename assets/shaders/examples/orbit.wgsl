// Orbit (a visualizer): the spectrum wrapped around a circle as one closed,
// glowing curve (a polar plot), turning slowly. The bass swells the circle and
// the beat brightens it.
//
// @setting size: int(20, 80) = 45 "Size" "%"
// @setting reach: int(0, 60) = 35 "Reach" "%"
// @help How far the loudest sounds push the curve out.
// @setting turning: int(0, 200, 5) = 100 "Turning" "%"
// @setting halo: toggle = true "Halo"
// @setting fill: toggle = true "Fill inside"

fn visualizer(uv: vec2<f32>, t: f32) -> vec4<f32> {
    let size = resolution();
    let p = (uv - vec2<f32>(0.5)) * size;
    let unit = min(size.x, size.y) * 0.5;
    let r = length(p) / unit;
    // The angle, turning with time, as 0..1 around the circle; mirrored so the
    // curve meets itself smoothly (bass at the top and bottom).
    let turn = fract(atan2(p.x, -p.y) / TAU + 0.02 * t * f32(settings.turning) / 100.0 + 1.0);
    let x = 1.0 - abs(2.0 * turn - 1.0);
    let radius = f32(settings.size) / 100.0 + 0.1 * bass() + f32(settings.reach) / 100.0 * spectrum(x);

    // Distance to the curve, in pixels: a crisp line plus a soft glow.
    let d = abs(r - radius) * unit;
    let line = 1.0 - smoothstep(1.0, 2.5, d);
    let halo = select(0.0, exp(-d / (10.0 + 20.0 * beat())) * 0.6, settings.halo != 0);
    // A faint fill inside.
    let fill = select(0.0, 0.12, settings.fill != 0 && r < radius);
    let color = bar_color(x);
    return vec4<f32>(color, clamp(max(line, max(halo, fill)), 0.0, 1.0));
}

// Ambient: four soft circles in the cover's colors (or calm defaults).
//
// One of Undavi's built-in backgrounds, written the same way as your own (see
// docs/shaders.md). Copy it into your shaders folder to start from it.
//
// @section Picture
// @setting animated: toggle = false "Animated"
// @help The circles themselves flow, ripple or twist
// @setting motion: choice("Still", "Drift", "Rotate") = "Rotate" "Motion" if !animated
// @setting animation: choice("Liquid", "Warp", "Spiral", warps) = "Liquid" "Motion" if animated
// @setting speed: int(25, 200, 5) = 100 "Speed" "%" if animated or motion != "Still"
// @setting blur: int(0, 150) = 90 "Blur" "px"
// @section Beat
// @setting pulse: toggle = false "Pulse with the beat"
// @help The background reacts to the bass or drums
// @setting pulse_source: choice("Bass", "Drums") = "Bass" "Sync to" if pulse
// @setting pulse_amount: int(0, 100) = 50 "Intensity" "%" if pulse
// @setting pulse_style: choice("Swell", "Brighten", "Zoom", "Focus") = "Swell" "Style" if pulse
// @help Swell: grows a little and brightens. Brighten: only brightens. Zoom: only grows. Focus: less blur on the beat.

fn scene(uv: vec2<f32>, t: f32) -> vec3<f32> {
    var p = uv * resolution();
    var motion = settings.motion;
    if (settings.animated != 0) {
        p = animate(p, settings.animation, t);
        motion = MOTION_STILL;
    }
    return ambient(p, motion, t);
}

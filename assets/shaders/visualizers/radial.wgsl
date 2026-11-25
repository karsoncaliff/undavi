// Radial: bars in a ring around the album cover, which turns while music plays.
//
// One of Undavi's built-in visualizers, written the same way as your own (see
// docs/shaders.md). Copy it into your shaders folder to start from it.
//
// @section Bars
// @setting bar_count: int(16, 96) = 43 "Bar count"
// @setting bar_width: int(2, 16) = 8 "Bar width" "px"
// @setting max_length: int(100, 1200, 10) = 400 "Max length" "px"
// @setting corners: choice("Square", "Rounded", "Pill") = "Square" "Corners"
// @section Mirror
// @setting mirror: choice("Off", "Up/down", "Left/right", "Both") = "Up/down" "Mirror"
// @help Up/down also grows bars inward, toward the cover; Left/right puts the bass at the top.
// @setting split: choice("Off", "Stereo", "Mid/Side") = "Stereo" "Split" if mirror = "Left/right" | "Both"
// @help Stereo: the left channel on one side, the right on the other. Mid/Side: what both channels share on one side, what differs on the other.
// @setting channel: choice("Mono", "Left", "Right") = "Mono" "Channel" if mirror = "Off" | "Up/down" or split = "Off"
// @help Mono mixes both channels together.
// @section Details
// @setting segments: toggle = false "Segments"
// @help Each bar as a stack of blocks, like an LED meter.
// @setting segment_size: int(2, 16) = 6 "Segment size" "px" if segments
// @setting segment_gap: int(1, 8) = 3 "Segment gap" "px" if segments
// @setting peak_caps: toggle = false "Peak caps"
// @help Small caps that mark each bar's recent peak.
// @section Glow
// @setting glow: toggle = false "Glow on the beat"
// @help Bars brighten with the bass or drums
// @setting glow_source: choice("Bass", "Drums") = "Bass" "Sync to" if glow
// @setting glow_amount: int(0, 100) = 50 "Intensity" "%" if glow

const PI: f32 = 3.14159265359;
const MIN_VISIBLE: f32 = 4.0;
const CAP_HEIGHT: f32 = 4.0;
const CAP_GAP: f32 = 6.0;

fn visualizer(uv: vec2<f32>, t: f32) -> vec4<f32> {
    let size = resolution();
    let rel = uv * size - size * 0.5;
    let n = bar_count();
    let nf = f32(n);
    let up_down = settings.mirror == MIRROR_UP_DOWN || settings.mirror == MIRROR_BOTH;
    let left_right = settings.mirror == MIRROR_LEFT_RIGHT || settings.mirror == MIRROR_BOTH;
    let caps = settings.peak_caps != 0;

    let extra = select(0.0, CAP_GAP + CAP_HEIGHT, caps);
    let outer = max(min(size.x, size.y) * 0.5 - 8.0 - extra, 40.0);
    let scale = min(outer / 300.0, 1.0);
    let inner = outer * 0.42;
    // Max length can shorten the bars; the ring already fills the space.
    let reach = min(outer - inner, f32(settings.max_length) * scale);
    // Up/down mirroring also grows bars inward, toward the cover.
    let reach_in = select(0.0, inner * 0.35, up_down);
    let cover_r = inner - reach_in - 10.0 * scale;
    // Keep bars from overlapping at the inner edge.
    let w = max(min(f32(settings.bar_width) * scale, TAU * inner / nf * 0.8), 1.0);
    let min_len = MIN_VISIBLE * scale;
    var corner = w * 0.5;
    if (settings.corners == CORNERS_SQUARE) {
        corner = 0.0;
    } else if (settings.corners == CORNERS_ROUNDED) {
        corner = 2.5 * scale;
    }
    let segments = settings.segments != 0;
    let seg = select(0.0, f32(settings.segment_size) * scale, segments);
    let gap = f32(settings.segment_gap) * scale;
    let start = select(0.0, gap * 0.5, up_down);
    // With left/right mirroring the bass (the middle bar) sits at the top.
    let mid = select(-0.5, (nf - 1.0) * 0.5, left_right);

    var out = vec4<f32>(0.0);

    // The cover in the middle, turning once every 24 s (or a faint ring).
    if (cover_r > 8.0) {
        let dist = length(rel);
        if (has_cover()) {
            let r = cover_r * (1.0 + 0.04 * glow());
            let spin = TAU * fract(t / 24.0);
            let cuv = rotate(rel, -spin) / (2.0 * r) + vec2<f32>(0.5);
            out = vec4<f32>(cover(cuv), coverage(dist - r));
        } else {
            out = vec4<f32>(vec3<f32>(0.224, 0.227, 0.255), 0.6 * coverage(abs(dist - cover_r) - 1.0));
        }
    }

    // The bar this pixel's angle falls on; bar k points at -90° + 360° (k - mid) / n.
    let f = (atan2(rel.y, rel.x) + PI * 0.5) / TAU * nf + mid;
    // Wrapped into 0..n first, so the index never goes negative.
    let k0 = i32(round(f - nf * floor(f / nf)));

    // The glow first, from nearby bars, so no glow covers a bar.
    if (glow() > 0.0) {
        var clear = 1.0;
        for (var dk = -2; dk <= 2; dk = dk + 1) {
            let k = (k0 + dk + n) % n;
            let local = rotate(rel, -(TAU * (f32(k) - mid) / nf));
            let o = -local.y;
            let len = max(bar(k) * reach, min_len);
            let len_in = bar(k) * reach_in;
            let o0 = select(inner, inner - len_in, len_in > 0.5);
            let o1 = inner + len;
            let d = round_box(vec2<f32>(local.x, o - (o0 + o1) * 0.5), vec2<f32>(w * 0.5, (o1 - o0) * 0.5), w * 0.5);
            clear = clear * (1.0 - halo(d, 14.0 * scale));
        }
        out = over(out, vec4<f32>(glow_color(), 1.0 - clear));
    }

    for (var dk = -1; dk <= 1; dk = dk + 1) {
        let k = (k0 + dk + n) % n;
        // Turned so the bar points straight out: `o` is the distance from the
        // middle, `local.x` the offset across the bar.
        let local = rotate(rel, -(TAU * (f32(k) - mid) / nf));
        let o = -local.y;
        let len = max(bar(k) * reach, min_len);
        let len_in = bar(k) * reach_in;
        let joined = len_in > 0.5;
        var d: f32;
        if (joined && !segments) {
            // Outward and inward halves as one shape, rounded at both tips.
            let h = (len + len_in) * 0.5;
            d = round_box(vec2<f32>(local.x, o - (inner + (len - len_in) * 0.5)), vec2<f32>(w * 0.5, h), min(corner, min(h, w * 0.5)));
        } else {
            d = bar_run(local.x, o, inner, 1.0, len, w, corner, seg, gap, start);
            if (joined) {
                d = min(d, bar_run(local.x, o, inner, -1.0, len_in, w, corner, seg, gap, start));
            }
        }
        // Along: from the inner edge (0) out to the tip (1); across: around the ring.
        let color = select(bar_color(f32(k) / max(nf, 2.0)), bar_color((o - inner) / len), along());
        out = over(out, vec4<f32>(color, coverage(d)));

        if (caps) {
            let peak_len = max(peak(k) * reach, len);
            let cap_r = select(min(CAP_HEIGHT * scale * 0.5, w * 0.5), 0.0, settings.corners == CORNERS_SQUARE);
            let dc = bar_run(local.x, o, inner + peak_len + CAP_GAP * scale, 1.0, CAP_HEIGHT * scale, w, cap_r, 0.0, 0.0, 0.0);
            out = over(out, vec4<f32>(color, coverage(dc)));
        }
    }
    return out;
}

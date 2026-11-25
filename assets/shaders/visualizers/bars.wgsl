// Bars: a row of vertical bars, Undavi's original look.
//
// One of Undavi's built-in visualizers, written the same way as your own (see
// docs/shaders.md). Copy it into your shaders folder to start from it.
//
// @section Bars
// @setting bar_count: int(16, 96) = 43 "Bar count"
// @setting bar_width: int(2, 16) = 8 "Bar width" "px"
// @setting max_length: int(100, 1200, 10) = 400 "Max length" "px"
// @setting spacing: int(0, 20) = 12 "Spacing" "px"
// @setting corners: choice("Square", "Rounded", "Pill") = "Square" "Corners"
// @section Mirror
// @setting mirror: choice("Off", "Up/down", "Left/right", "Both") = "Up/down" "Mirror"
// @setting split: choice("Off", "Stereo", "Mid/Side") = "Stereo" "Split" if mirror = "Left/right" | "Both"
// @help Stereo: the left channel on the left, the right on the right. Mid/Side: what both channels share on the left, what differs on the right.
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

// Bar length the layout is proportioned for: in windows too short for it, the
// bars (and their widths) scale down together. Longer Max lengths use more of
// the height.
const REFERENCE_LENGTH: f32 = 400.0;
// The shortest a bar gets, so silence still shows a row.
const MIN_VISIBLE: f32 = 4.0;
const CAP_HEIGHT: f32 = 4.0;
const CAP_GAP: f32 = 6.0;

fn visualizer(uv: vec2<f32>, t: f32) -> vec4<f32> {
    let size = resolution();
    let p = uv * size;
    let n = bar_count();
    let bw = f32(settings.bar_width);
    let sp = f32(settings.spacing);
    let total = f32(n) * bw + f32(n - 1) * sp;
    let up_down = settings.mirror == MIRROR_UP_DOWN || settings.mirror == MIRROR_BOTH;
    let caps = settings.peak_caps != 0;

    // Peak caps stick out past the longest bar; leave room for them.
    var extra = select(0.0, CAP_GAP + CAP_HEIGHT, caps);
    if (up_down) {
        extra = extra * 2.0;
    }
    let max_len = f32(settings.max_length);
    let scale = min(min(size.x / total, size.y / (min(max_len, REFERENCE_LENGTH) + extra)), 1.0);
    let origin = (size.x - total * scale) * 0.5;
    // A full bar (both halves, when mirrored), never taller than the space.
    let full = max(min(max_len * scale, size.y - extra * scale), 0.0);
    // Mirrored bars grow both ways from the middle; otherwise up from a baseline.
    let base = select(size.y * 0.5 + full * 0.5, size.y * 0.5, up_down);
    let half = select(full, full * 0.5, up_down);
    let min_len = select(MIN_VISIBLE, MIN_VISIBLE * 0.5, up_down) * scale;
    let w = bw * scale;
    let pitch = (bw + sp) * scale;
    var corner = w * 0.5;
    if (settings.corners == CORNERS_SQUARE) {
        corner = 0.0;
    } else if (settings.corners == CORNERS_ROUNDED) {
        corner = 2.5 * scale;
    }
    let segments = settings.segments != 0;
    let seg = select(0.0, f32(settings.segment_size) * scale, segments);
    let gap = f32(settings.segment_gap) * scale;
    // Mirrored halves start half a gap off the center line, so they match.
    let start = select(0.0, gap * 0.5, up_down);

    let k0 = i32(floor((p.x - origin) / pitch));
    var out = vec4<f32>(0.0);

    // The glow first, from every bar within reach, so no glow covers a bar.
    if (glow() > 0.0) {
        let reach = 14.0 * scale;
        let r = min(i32(ceil(reach / pitch)) + 1, 8);
        var clear = 1.0;
        for (var k = k0 - r; k <= k0 + r; k = k + 1) {
            if (k < 0 || k >= n) {
                continue;
            }
            let cx = origin + f32(k) * pitch + w * 0.5;
            let len = max(bar(k) * half, min_len);
            let y0 = base - len;
            let y1 = select(base, base + len, up_down);
            let d = round_box(
                vec2<f32>(p.x - cx, p.y - (y0 + y1) * 0.5),
                vec2<f32>(w * 0.5, (y1 - y0) * 0.5),
                w * 0.5,
            );
            clear = clear * (1.0 - halo(d, reach));
        }
        out = vec4<f32>(glow_color(), 1.0 - clear);
    }

    // The bar under this pixel and its neighbors (for their anti-aliased edges).
    for (var k = k0 - 1; k <= k0 + 1; k = k + 1) {
        if (k < 0 || k >= n) {
            continue;
        }
        let x = p.x - (origin + f32(k) * pitch + w * 0.5);
        let len = max(bar(k) * half, min_len);
        var d: f32;
        if (up_down && !segments) {
            // One shape over both halves, rounded at both tips.
            d = round_box(vec2<f32>(x, p.y - base), vec2<f32>(w * 0.5, len), min(corner, min(len, w * 0.5)));
        } else {
            d = bar_run(x, p.y, base, -1.0, len, w, corner, seg, gap, start);
            if (up_down) {
                d = min(d, bar_run(x, p.y, base, 1.0, len, w, corner, seg, gap, start));
            }
        }
        // Along each bar: from the baseline (0) to its tip (1); across: left to right.
        let color = select(
            bar_color((p.x - origin) / (total * scale)),
            bar_color(abs(p.y - base) / len),
            along(),
        );
        out = over(out, vec4<f32>(color, coverage(d)));

        if (caps) {
            let peak_len = max(peak(k) * half, len);
            let near = peak_len + CAP_GAP * scale;
            let cap = CAP_HEIGHT * scale;
            let cap_r = select(min(cap * 0.5, w * 0.5), 0.0, settings.corners == CORNERS_SQUARE);
            var dc = bar_run(x, p.y, base - near, -1.0, cap, w, cap_r, 0.0, 0.0, 0.0);
            if (up_down) {
                dc = min(dc, bar_run(x, p.y, base + near, 1.0, cap, w, cap_r, 0.0, 0.0, 0.0));
            }
            // Along each bar, the caps take the palette's end color.
            let cap_color = select(color, bar_color(1.0), along());
            out = over(out, vec4<f32>(cap_color, coverage(dc)));
        }
    }
    return out;
}

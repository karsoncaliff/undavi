// LED meter (a visualizer): the classic hi-fi spectrum display, a grid of lit
// cells with a falling peak in each column, colored with the bar palette (low
// cells in its first colors, high ones in its last).
//
// `uv` runs 0..1 across the visualizer's area (y down). Return the color there
// and how opaque it is (0..1); the background shows through where it's 0.
//
// `bar_count` is one of the ids Undavi reads itself (see docs/shaders.md): it
// sets how many bars the spectrum is split into, so `bar(i)` has one per column.
//
// @setting bar_count: int(8, 64) = 24 "Columns"
// @setting rows: int(8, 48) = 24 "Rows"
// @setting peaks: toggle = true "Falling peaks"
// @setting unlit: int(0, 30) = 8 "Unlit cells" "%"
// @help How brightly the cells that aren't lit still show.

fn visualizer(uv: vec2<f32>, t: f32) -> vec4<f32> {
    let columns = f32(bar_count());
    let rows = f32(settings.rows);
    // The meter fills the middle of the area, with a margin around it.
    let p = (uv - vec2<f32>(0.08, 0.12)) / vec2<f32>(0.84, 0.76);
    if (p.x < 0.0 || p.x > 1.0 || p.y < 0.0 || p.y > 1.0) {
        return vec4<f32>(0.0);
    }
    let column = floor(p.x * columns);
    let row = floor((1.0 - p.y) * rows);
    // The gap between cells.
    let cell = fract(vec2<f32>(p.x * columns, (1.0 - p.y) * rows));
    let in_cell = step(0.12, cell.x) * step(cell.x, 0.88) * step(0.15, cell.y) * step(cell.y, 0.85);

    let height = bar(i32(column)) * rows;
    let peak_row = floor(peak(i32(column)) * rows);
    let lit = step(row + 0.5, height);
    let is_peak = select(0.0, 1.0, settings.peaks != 0 && row == peak_row && peak_row > 0.0);
    let color = bar_color(row / rows);
    // Unlit cells glow faintly, so the grid is always there.
    let alpha = in_cell * max(max(lit, is_peak), f32(settings.unlit) / 100.0);
    return vec4<f32>(color, alpha);
}

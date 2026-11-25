# Shaders

Every background and every visualizer in Undavi is a shader, a small program
that runs on your graphics card. The built-in ones (Bars, Radial and Wave; Solid,
Album art and Ambient) are written the same way you'd write your own, and their
source is in [`assets/shaders/visualizers/`](../assets/shaders/visualizers/) and
[`assets/shaders/backgrounds/`](../assets/shaders/backgrounds/).

- **Visualizer:** pick one under **Visualizer → Style**.
- **Background:** pick one under **Background → Style**.

Each shader declares its own settings (see [Settings](#settings)), and the settings
window shows those, under the styles. Only a few settings are the same for every
shader: the **Colors** section, and the **Effects** (Dim, saturation, brightness,
contrast, accent tint, vignette and grain) at the end of Background.

Your own shaders come from your shaders folder:

| Platform | Folder |
|---|---|
| Linux | `~/.config/undavi/shaders/` |
| Windows | `%APPDATA%\undavi\shaders\` |
| macOS | `~/Library/Application Support/undavi/shaders/` |

**Open folder** under the styles creates the folder, with five examples, the
first time. Examples added in later versions of Undavi show up in an existing
folder by themselves; one you delete stays deleted, and one you edit is left
alone (the folder's `.examples` file keeps track). Every `.wgsl` file in it shows
up in a list. Undavi re-reads a file whenever you save it, so you can edit with
the app open and watch the result. **Make a copy** puts a copy of the built-in
shader in use in the folder (as `bars-copy.wgsl`, say), as a starting point.

Shaders are written in [WGSL](https://www.w3.org/TR/WGSL/), the shading language
of WebGPU.

## Three kinds

A file defines one of these three functions. Which one decides what it does,
and which list it shows up in.

### `warp`: move the art around

```wgsl
fn warp(q: vec2<f32>, t: f32) -> vec2<f32>
```

`q` is a point on the screen, relative to the window's center and measured in
units of the window's shorter side (so `length(q) == 0.5` touches its edge).
Return the point whose color should appear there. Returning `q` changes nothing.

A warp isn't a background of its own: it becomes one more choice under **Motion**
when Album art or Ambient is **Animated**, next to the built-in Liquid, Warp and
Spiral. Everything else of that background still applies (Layout, Zoom, Blur,
the pulse), and the warp's own settings show in a section named after its file.

```wgsl
// Ripple: rings running out from the middle, pushed further by the bass.
// @setting rings: int(1, 10) = 4 "Rings"
fn warp(q: vec2<f32>, t: f32) -> vec2<f32> {
    let r = length(q);
    if (r < 0.0001) {
        return q;
    }
    let wave = sin(TAU * (f32(settings.rings) * r - 0.6 * t)) * exp(-2.0 * r);
    return q + (q / r) * wave * (0.01 + 0.04 * bass());
}
```

### `scene`: draw the whole background

```wgsl
fn scene(uv: vec2<f32>, t: f32) -> vec3<f32>
```

`uv` runs from 0 to 1 across the window (y points down). Return the color there,
red, green and blue from 0 to 1. The cover isn't drawn for you, but `cover(uv)`
reads it, and `album(…)` and `ambient(…)` draw it the way the built-ins do (see
`album-art.wgsl`). The Effects are applied to what you return, and the Blur when
the shader declares a `blur` setting.

```wgsl
// The cover, mirrored into four quadrants and tinted by the beat.
// @setting blur: int(0, 120) = 40 "Blur" "px"
fn scene(uv: vec2<f32>, t: f32) -> vec3<f32> {
    let m = abs(uv - vec2<f32>(0.5)) * 2.0;
    return cover(m) * (0.8 + 0.4 * beat());
}
```

In both, `t` is the background clock in seconds. It follows the `speed` setting
(when the background has one) and pauses when the music does.

### `visualizer`: draw the visualizer

```wgsl
fn visualizer(uv: vec2<f32>, t: f32) -> vec4<f32>
```

`uv` runs from 0 to 1 across the visualizer's area (the space the bars take; y
points down). Return the color there and how opaque it is: red, green, blue and
alpha, all 0 to 1. Where alpha is 0, the background shows through. `t` is the
seconds of music played.

```wgsl
// Bars as soft columns of light, in the bar palette.
// @setting softness: float(0.005, 0.1, 0.005) = 0.02 "Softness"
fn visualizer(uv: vec2<f32>, t: f32) -> vec4<f32> {
    let height = spectrum(uv.x);
    let lit = smoothstep(height, height - settings.softness, 1.0 - uv.y);
    return vec4<f32>(bar_color(uv.x), lit);
}
```

The palette and gradient (Colors) apply to every visualizer, through
`bar_color`.

## Settings

A shader lists its settings in comments, anywhere in the file (the built-ins put
them at the top). Undavi builds the controls from them, remembers the
values for each shader separately (and in presets), and hands them to the shader
as `settings.<id>`:

```text
// @section Bars
// @setting bar_count: int(16, 96) = 43 "Bar count"
// @setting max_length: int(100, 1200, 10) = 400 "Max length" "px"
// @setting corners: choice("Square", "Rounded", "Pill") = "Square" "Corners"
// @setting segments: toggle = false "Segments"
// @help Each bar as a stack of blocks, like an LED meter.
// @setting segment_size: int(2, 16) = 6 "Segment size" "px" if segments
// @setting tint: color(#ff8800, #4b7bf5) = #ff8800 "Tint"
```

Each `@setting` line is `id: kind = default "Label"`, then an optional unit in
quotes, then an optional `if` condition.

| Kind | Control | In the shader |
|---|---|---|
| `int(min, max)` or `int(min, max, step)` | slider | `i32` |
| `float(min, max)` or `float(min, max, step)` | slider | `f32` |
| `toggle` | switch | `i32`, 1 when on (`settings.segments != 0`) |
| `choice("A", "B", …)` | buttons (in several rows when they don't fit in one) | `i32`, the option's position, with a constant for each: `CORNERS_SQUARE`, `CORNERS_ROUNDED`, … |
| `color(#rrggbb, …)` | swatches (the listed ones, as choices) | `vec3<f32>`, 0..1 |

In a background, a choice can end with `warps`, as in
`choice("Liquid", "Warp", warps)`: every custom warp in the folder is added as an
option after the listed ones.

- `@section Name` starts a new group; the settings after it go there. It also shows
  in the tree on the left.
- `@help Some text` adds a line of explanation under the setting before it.
- `if segments` shows the setting only while the `segments` switch is on;
  `if !segments` while it's off. `if mirror = "Off" | "Up/down"` shows it for
  those options, `if motion != "Still"` for any other. Join several with `or`, or
  with `and` (not both in one condition).
- `@no-dim` (a background only) leaves the Dim out, as Solid does.

Ids are lowercase letters, digits and `_`, and can't be a word WGSL keeps for
itself (`layout`, `type`, `loop`, …). **Reset these settings** puts
the shader in use back to its defaults. Saved values that no longer fit (a file
edited to a smaller range, an option removed) fall back to the default.

### Settings Undavi reads

A few ids mean something to Undavi itself, not only to the shader. Declare one to
get that behavior, with whatever range and label you like:

| Id | Kind | In | What Undavi does with it |
|---|---|---|---|
| `bar_count` | int | visualizer | splits the spectrum into this many bars (`bar_count()`, `bar(i)`); 64 without it |
| `mirror` | choice of `"Off"`, `"Up/down"`, `"Left/right"`, `"Both"` | visualizer | which bar shows which frequencies (left/right repeats them back to front) |
| `split` | choice of `"Off"`, `"Stereo"`, `"Mid/Side"` | visualizer | with left/right mirroring, a different signal on each half |
| `channel` | choice of `"Mono"`, `"Left"`, `"Right"` | visualizer | which channel the bars follow |
| `glow`, `glow_source`, `glow_amount` | toggle; `"Bass"`/`"Drums"`; int (%) | visualizer | the beat glow in `glow()` and `bar_color` |
| `blur` | int or float (px) | background | the blur over the scene |
| `speed` | int (%) | background | how fast `t` runs |
| `pulse`, `pulse_source`, `pulse_amount`, `pulse_style` | toggle; `"Bass"`/`"Drums"`; int (%); `"Swell"`/`"Brighten"`/`"Zoom"`/`"Focus"` | background | the beat pulse (`pulse()`, `swell()`, the dim lifting) |

A warp's ids are added to its background's, so they mustn't be one of those.

## What you can use

Your file is placed in front of one of Undavi's own shaders, so you can call
these in any kind:

| Name | Gives |
|---|---|
| `settings` | your settings (see above) |
| `TAU` | 2π |
| `rotate(p: vec2<f32>, a: f32) -> vec2<f32>` | `p` turned by `a` radians |
| `luma(c: vec3<f32>) -> f32` | a color's perceived brightness |
| `snoise(p: vec2<f32>) -> f32` | 2D simplex noise, about −1..1 |
| `fbm(p: vec2<f32>) -> f32` | two octaves of simplex noise, about −1..1 |
| `resolution() -> vec2<f32>` | the area's size in logical pixels |
| `bass() -> f32`, `mids() -> f32`, `treble() -> f32` | how loud the lows, middles and highs are, 0..1 |
| `beat() -> f32` | the beat pulse: jumps toward 1 on a hit, then fades |
| `level() -> f32` | the overall input level, 0..1 |
| `band(i: i32) -> f32` | one of 16 spectrum bands, `0` (about 30 Hz) to `15` (16 kHz), 0..1 |
| `palette(i: i32) -> vec3<f32>` | one of four colors taken from the cover (`0`–`3`) |
| `accent() -> vec3<f32>` | the accent color |
| `has_cover() -> bool` | whether there's a cover |
| `cover(uv: vec2<f32>) -> vec3<f32>` | the current cover (0..1 across it); in a background, the Ambient circles when there's none, in a visualizer, dark gray |

In a background only:

| Name | Gives |
|---|---|
| `pulse() -> f32`, `swell() -> f32` | the background's beat pulse (its `pulse` settings), and how much it should grow |
| `animate(p, kind, t)` | `p` (logical px) moved by animation `kind`: 0 Liquid, 1 Warp, 2 Spiral, more for custom warps |
| `album(p, arrangement, zoom, motion, t)` | the cover at `p` the way Album art draws it |
| `ambient(p, motion, t)` | the Ambient circles at `p` |

In a visualizer only:

| Name | Gives |
|---|---|
| `bar_count() -> i32` | how many bars there are |
| `bar(i: i32) -> f32` | what bar `i` shows, 0..1, with mirroring and split applied |
| `peak(i: i32) -> f32` | where bar `i`'s peak is, 0..1 |
| `spectrum(x: f32) -> f32` | the bars as a smooth curve, `x` from 0 (first bar) to 1 (last) |
| `bar_color(t: f32) -> vec3<f32>` | the bar palette at `t` (0..1), brightened by the glow |
| `glow() -> f32` | the beat glow, 0..1 (the `glow` settings) |
| `px() -> f32` | one physical pixel, in logical pixels (for crisp edges) |
| `round_box(p, half, r) -> f32`, `coverage(d) -> f32`, `halo(d, reach) -> f32`, `over(under, top) -> vec4<f32>` | the signed distance to a rounded box, how much of a pixel it covers, a soft glow around it, and `top` laid over `under`: what the built-in Bars use |

The audio values use Sound's **Level range**, like the bars, so raising or
lowering it changes how far they move.

Anything else from WGSL works as usual (`sin`, `mix`, `smoothstep`, `atan2`,
`fract`, …). Undavi's own names start with `undavi_`; avoid that prefix, and
don't define a function with one of the names above.

## Mistakes

Undavi checks every file before it reaches the graphics card: both the code and
the `@setting` lines. If a file has an error, it's marked **error** in the list,
and picking it shows the message with the line and column, for example:

```text
error: no definition in scope for identifier: `nope`
  ┌─ broken.wgsl:3:16
  │
3 │     return q * nope;
  │                ^^^^ unknown identifier
```

While the picked shader has an error, Bars (or Album art) stands in, and the
settings say why. Your pick is kept: fix the file and save, and it's back within a
second. A file can't take a built-in's name (`bars.wgsl`, say); copies get their
own.

## Performance

A background runs for every pixel at half the window's resolution, a visualizer
for every pixel of its area, every frame. Simple math is free; long loops and
many noise calls add up, especially in full screen. A shader that runs for a very
long time (an endless loop, say) can stall the screen until the graphics driver
resets the card. If that happens, move the file out of the folder.

## Examples

Undavi puts these in a new shaders folder. Each declares a few settings of its
own. Their source is in [`assets/shaders/examples/`](../assets/shaders/examples/).

| File | Kind | What it does |
|---|---|---|
| `kaleidoscope.wgsl` | warp | Folds the picture into turning, mirrored slices; the bass pulls it in. |
| `ripple.wgsl` | warp | Rings running out from the middle on the bass and the beat. |
| `plasma.wgsl` | scene | The classic demoscene plasma, colored with the cover's palette. |
| `led-meter.wgsl` | visualizer | A hi-fi LED spectrum display: lit cells and falling peaks. |
| `orbit.wgsl` | visualizer | The spectrum wrapped around a circle as one glowing curve. |

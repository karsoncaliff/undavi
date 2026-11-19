# Architecture

Undavi is one Rust crate with a library (`src/lib.rs`) and a tiny binary
(`src/main.rs`, which calls `undavi::run()`). The UI is built with
[iced](https://iced.rs) 0.14; audio comes from [cpal](https://github.com/RustAudio/cpal).

## Modules

The library has two layers. The **public modules** are UI-free and documented as
an API; the **app modules** are internal and may change freely.

### Public (the engine)

| Module | Responsibility |
|---|---|
| `audio` | Lists sources, captures one (`Capture`), and turns samples into spectra (`Analyzer`). |
| `spectrum` | Noise gate (`Gate`), bar heights (`Processor`) and beat detection (`BeatGlow`). Pure math. |
| `settings` | The settings model, presets, the settings file, and migrations. |
| `lyrics` | LRC and Lyricsfile parsing, timing helpers, and the LRCLIB lookup thread. |
| `media` | Now-playing info and transport commands (MPRIS on Linux). |

### Internal (the app)

| Module | Responsibility |
|---|---|
| `app` | `App`: all state, `Message`, the update loop and the per-frame `tick`. |
| `view` | Builds the widget tree every frame: header, stage, panels, picker. |
| `layout` | Decides what fits at the current window size (floating panels, compact header). |
| `platform` | Detects what the platform supports, so the view can hide the rest. |
| `visualizer` | Draws the bars (all styles, mirroring, gradients, glow) and the seek bar. |
| `karaoke` | Draws and animates the lyrics. |
| `background` | Draws the background layers (solid, album art, ambient) and the dim overlay. |
| `art` | Downloads album art, blurs it and picks colors from it, on its own thread. |
| `controls` | Custom canvas controls: sliders, range sliders, switches, swatches. |
| `focus` | Keyboard focus: Tab order, arrow-key steps, Enter/Space. |
| `meter` | The tuning meter shown while the Tuning panel is open. |
| `theme` | Colors, fonts and shared widget styles. |
| `icons` | The icon set. |
| `about` | The About panel's content: version, credits, licenses, the User-Agent. |

## Threads

| Thread | Owner | Does |
|---|---|---|
| UI (main) | iced | `update`, `view` and drawing. Everything else reports back to it. |
| Audio | cpal | Folds each buffer to left/right pairs and pushes them into a lock-free ring buffer. Never blocks. |
| `undavi-mpris` | `media` | Polls the player every 500 ms and runs commands. |
| `undavi-art` | `art` | Downloads, decodes and blurs album art. |
| `undavi-lyrics` | `lyrics` | Finds lyrics for the current song. |
| `undavi-app-capture` | `audio::apps` | Runs a PipeWire loop for an app source: our capture stream, and links to the app's streams as they appear. |
| `undavi-pw-list` | `audio::apps` | Lists the playing apps for the source menu (short-lived, at most 500 ms). |

The worker threads share a small `Mutex`-guarded snapshot with the UI. The UI
only reads the snapshot (cheap, every frame) and sends requests over a channel,
so a slow network or player never stalls drawing.

## A frame

The clock is iced's `window::frames()` subscription (one message per display
refresh), or a timer when the Appearance frame-rate cap is 30 or 60 fps. Each
tick (`App::tick`):

1. **Keep audio alive.** Reopen the stream if it died, stalled, or never opened
   (`keep_audio_alive`).
2. **Analyze.** Drain the capture into the `Analyzer` and compute the spectra the
   bars need, looking back by the visual delay.
3. **Gate.** The `Gate` decides from the input level whether there is sound.
4. **Process.** The `Processor` updates the bar heights; `BeatGlow`s update the
   bar glow and the background pulse.
5. **Follow the player.** Read the media snapshot, ask for art and lyrics for
   the current track.
6. **Ease.** Accent and album colors ease toward their targets; auto-hide fades
   the controls.
7. **Rest.** When nothing has moved for a second, switch to idle mode: a 250 ms
   timer instead of every frame.
8. **Save.** Settings changed more than a second ago are written to disk.

Then iced calls `view`, which rebuilds the widget tree from `App`. The focus
registry is rebuilt with it, so Tab order always matches what's on screen.

See [Audio pipeline](audio-pipeline.md) for steps 2 to 4 in detail.

## Platform support

| | Linux | Windows | macOS |
|---|---|---|---|
| System audio | PipeWire sink monitor | WASAPI loopback (untested) | Default input only |
| Single apps, Music player | PipeWire | Not yet | Not yet |
| Track info and controls | MPRIS | Not yet | Not yet |
| Always on top | X11 only (Wayland has no way to ask) | Yes | Yes |

`platform::Platform` records what's available, and the view leaves out controls
that wouldn't do anything.

## The patched iced_wgpu

`vendor/iced_wgpu` is a copy of iced_wgpu 0.14.0 with two fixes, used through
`[patch.crates-io]`:

1. iced picked a 16-bit float surface on some NVIDIA/Vulkan drivers, which made
   every color much lighter.
2. Full-color SVGs (the wordmark) got dark edges, because premultiplied pixels
   were blended as straight alpha.

`vendor/iced_wgpu/PATCHED.md` has the details. Drop the copy once upstream iced
has both fixes.

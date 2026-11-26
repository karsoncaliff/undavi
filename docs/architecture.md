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
| `settings` | The settings model, sections and their presets (`presets`), the keyboard shortcuts (`controls`), the settings file, and upgrades of older ones (`versions`). |
| `lyrics` | LRC and Lyricsfile parsing, timing helpers, and the LRCLIB lookup thread. |
| `media` | Now-playing info and transport commands (MPRIS on Linux, the system's media controls on Windows). |

### Internal (the app)

| Module | Responsibility |
|---|---|
| `app` | `App`: all state, `Message`, the update loop and the per-frame `tick`. `app::settings` is the settings window's state and what its controls do, undo and search included. |
| `view` | Builds the widget tree every frame: header, stage, About panel, picker, update pop-up. `view::playback` is the playback bar in each of its styles, `view::quick` the quick settings, and `view::scaled` draws widgets at another size (the preview). |
| `view::settings` | The settings window: the tree (`navigation`), each section's rows (`sections`, `rows`, `pictures`, and `monitor` for the signal monitor), the preset menu and the Presets page (`presets`) and the live preview (`preview`). |
| `layout` | Decides what fits at the current window size (floating panel, compact header, the settings window's columns). |
| `platform` | Detects what the platform supports, so the view can hide the rest, and the reduced-motion setting. |
| `visualizer` | The visualizer's uniforms for the GPU (bar values, palette, glow), the settings it reads from the shader (`VisEngine`: bar count, mirror, split, channel, glow), and the seek bar. |
| `visualizer::shader` | The GPU visualizer: one full-area pass of the visualizer's shader (`assets/shaders/visualizer.wgsl` plus Bars, Radial, Wave or a custom one), with its settings. |
| `karaoke` | Draws and animates the lyrics. |
| `background` | The background's uniforms each frame (covers and their cross-fade, pulse, effects), and the settings it reads from the shader (`BgEngine`: blur, speed, pulse, `@no-dim`). |
| `shaders` | Every visualizer and background shader: the built-ins (`assets/shaders/visualizers/`, `assets/shaders/backgrounds/`) and the shaders folder; composes them with their prelude, checks them (naga), reloads files, and the audio they see. |
| `shaders::settings` | The `// @setting` declarations: parsing them, resolving saved values against them, conditions, and packing them into the shader's `settings` uniform. |
| `backdrop` | The GPU background: a wgpu pipeline that runs the background's shader (`assets/shaders/scene.wgsl` plus Solid, Album art, Ambient or a custom one), then a dual Kawase blur, grading, dim, vignette and grain. |
| `art` | Downloads and decodes album art and picks colors from it, on its own thread. |
| `controls` | Custom canvas controls: sliders, range sliders, switches, swatches. |
| `focus` | Keyboard focus: Tab order, arrow-key steps, Enter/Space. |
| `meter` | The level meter at the top of the Sound section, and its beat row under the switches of what follows the beat. |
| `monitor` | The signal monitor (in Sound): a few seconds of history, stats, tuning tips and its graphs. |
| `theme` | Colors, fonts and shared widget styles. |
| `icons` | The icon set: Material Symbols, from `assets/icons/ui/`. |
| `about` | The About panel's content: version, credits, licenses, the User-Agent. |
| `update` | Looks for a newer GitHub release and installs it. |

## Threads

| Thread | Owner | Does |
|---|---|---|
| UI (main) | iced | `update`, `view` and drawing. Everything else reports back to it. |
| Audio | cpal | Folds each buffer to left/right pairs and pushes them into a lock-free ring buffer. Never blocks. |
| `undavi-mpris` (Linux), `undavi-media` (Windows) | `media` | Polls the player every 500 ms and runs commands. |
| `undavi-art` | `art` | Downloads and decodes album art and picks its colors. |
| `undavi-lyrics` | `lyrics` | Finds lyrics for the current song. |
| `undavi-app-capture` | `audio::linux`, `audio::windows` | Records an app source. Linux: a PipeWire loop with our capture stream, and links to the app's streams as they appear. Windows: a loopback for each of the app's processes that plays sound. |
| `undavi-pw-list` (Linux), `undavi-app-list` (Windows) | `audio::linux`, `audio::windows` | Lists the playing apps for the source menu (short-lived, at most 500 ms). |
| `undavi-motion` | `platform` | Asks the desktop for its reduced-motion setting when the settings window opens (short-lived, at most a second). |
| `undavi-open` | `about` | Windows: opens a link or folder (short-lived). |
| `undavi-update` | `app` | Checks for a new release at startup, and downloads and installs one (short-lived; reports back through an iced `Task`). |

The worker threads share a small `Mutex`-guarded snapshot with the UI. The UI
only reads the snapshot (cheap, every frame) and sends requests over a channel,
so a slow network or player never stalls drawing.

## A frame

The clock is iced's `window::frames()` subscription (one message per display
refresh), or a timer when the frame-rate cap (General) is 30 or 60 fps. Each
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

The settings window's preview is the same screen built again for another window
size (`App::still_screen`), as a picture: nothing in it takes focus, and
`view::scaled` lays it out at its real size, draws it scaled down and passes it no
input. The GPU background and visualizer keep separate buffers and textures for it
(`backdrop::Place`), so both can be on screen at once.

See [Audio pipeline](audio-pipeline.md) for steps 2 to 4 in detail.

## Platform support

| | Linux | Windows | macOS |
|---|---|---|---|
| System audio | PipeWire sink monitor | WASAPI loopback | Default input only |
| Single apps, Music player | PipeWire | WASAPI process loopback (Windows 10 version 2004 and newer) | Not yet |
| Track info and controls | MPRIS | The system's media controls (`Windows.Media.Control`) | Not yet |
| The player's volume | MPRIS | The app's volume in the volume mixer | Not yet |
| Album art | From the address the player gives | From the player itself | Not yet |
| `.lrc` files next to the song | Yes | No: players don't say which file it is | Not yet |
| Always on top | X11 only (Wayland has no way to ask) | Yes | Yes |
| Reduced motion | The desktop portal | "Animation effects" in the accessibility settings | Not yet |
| Graphics | Vulkan, or OpenGL | DirectX 12 where the GPU has it (Vulkan made full screen glitch on NVIDIA), or else Vulkan or OpenGL | Metal |
| Updates | Release tarball, `install.sh` | Release zip, `install.ps1` | Not yet |

`platform::Platform` records what's available, and the view leaves out controls
that wouldn't do anything. What's specific to a system is in a module of that name
(`audio::linux`, `media::windows`, …).

## The patched iced_wgpu

`vendor/iced_wgpu` is a copy of iced_wgpu 0.14.0 with two fixes, used through
`[patch.crates-io]`:

1. iced picked a 16-bit float surface on some NVIDIA/Vulkan drivers, which made
   every color much lighter.
2. Full-color SVGs (the wordmark) got dark edges, because premultiplied pixels
   were blended as straight alpha.

`vendor/iced_wgpu/PATCHED.md` has the details. Drop the copy once upstream iced
has both fixes.

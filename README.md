<p align="center">
  <img src="assets/undavi/title.svg" width="220" alt="Undavi">
</p>
<p align="center">
  An audio visualizer for your desktop: whatever is playing, as a live spectrum,<br>
  with the album art, playback controls and time-synced lyrics.
</p>
<p align="center">
  <a href="https://github.com/karsoncaliff/undavi/releases/latest"><img src="https://img.shields.io/github/v/release/karsoncaliff/undavi" alt="Latest release"></a>
  <a href="https://github.com/karsoncaliff/undavi/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/karsoncaliff/undavi/ci.yml?label=CI" alt="CI status"></a>
  <img src="https://img.shields.io/badge/platform-Linux%20%7C%20Windows-blue" alt="Platform: Linux and Windows">
  <a href="CONTRIBUTING.md#building"><img src="https://img.shields.io/badge/rust-1.90%2B-orange?logo=rust" alt="Rust 1.90+"></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/karsoncaliff/undavi" alt="License: GPL-3.0"></a>
</p>

https://github.com/user-attachments/assets/113cb537-b453-4312-9901-2768c247f9f7

---

## Features

- **Live spectrum of any audio.** System audio (whatever is playing), a single
  app such as Spotify or Firefox, whichever app your music player is, a specific
  output, or a microphone. Reconnects by itself if the device or audio server goes
  away.
- **Three visualizers:** bars, a ring of bars around the spinning album cover, or a
  smooth wave. Mirror them, split them into left/right or mid/side, add segments,
  peak caps and a glow on the beat, and color them with built-in palettes, your own
  gradient, or colors taken from the album art.
- **Backgrounds** of blurred album art, soft ambient color or a solid color. The
  art and the ambient color can drift, rotate or flow like liquid, and pulse with
  the bass or drums. Color effects, a vignette and film grain go over any of them.
- **Your own shaders,** for visualizers and backgrounds alike. A
  [WGSL file](docs/shaders.md) can follow the music, add its own sliders and
  switches to the settings, and reloads when you save it. Start from a copy of a
  built-in one, or from the five examples.
- **Synced lyrics,** word by word when available, from
  [LRCLIB](https://lrclib.net) or a `.lrc` file next to the song. Click a line
  to jump to it.
- **Now playing:** title, artist and cover in the header, as a large spotlight
  beside the bars, or as one small line. Play, pause, skip, seek, repeat and
  volume work with your music player.
- **Playback bar:** on the background, in a card or docked along the bottom, full or
  compact, with the seek bar's curve, lines and handle the way you like them.
- **Live preview:** the settings show the whole screen as you change it, in the
  shape of a regular window, an ultrawide screen or a narrow window. You can also
  search the settings, undo one you changed, and Cancel puts everything back.
- **Presets** for each section of the settings, built-in and your own, to mix and
  match, and groups of them to switch several sections at once.
- **Quick settings:** star the settings you change most, and they're one click away.
- **Fine tuning:** sensitivity, spread, rise and fall times, gravity, level range,
  contrast, frequency range and scale (log, mel, linear), a three-band EQ,
  auto-gain, a noise gate, beat sensitivity and a visual delay for Bluetooth
  audio. A live meter and a signal monitor show what the sliders are doing. Or
  press Auto-calibrate.
- **Easy on the battery:** drawing slows to a crawl when nothing is moving, the
  frame rate can be capped, and silence can freeze the bars.
- **Keyboard friendly:** every control is reachable with Tab and adjustable with
  the arrow keys, and every shortcut can be changed.
- **Updates:** Undavi tells you about new releases and can install them for you
  (with pacman, it leaves that to pacman). You can turn this off, or also get unstable
  versions to try the latest features.
- **Linux and Windows,** with the same features on both.

## Requirements

| | Linux | Windows |
|---|---|---|
| **System** | Wayland or X11; see [below](#linux-distributions) | 64-bit Windows 11, or Windows 10 version 2004 and newer |
| **Sound** | PipeWire as the sound server, for system audio and single apps. Without it, Undavi can still visualize an input such as a microphone. | Nothing more |
| **Music player** | One that supports MPRIS, for track info, album art, lyrics and playback controls. Most do: Spotify, Firefox, Chromium-based browsers, VLC, Rhythmbox, Elisa, Lollypop, Strawberry, and mpv with the `mpv-mpris` plugin. | One that shows up in the media controls (next to the volume in the quick settings). Most do: Spotify, browsers, Media Player, and many more. |
| **Graphics** | Vulkan, or OpenGL 3.3 / OpenGL ES 3.0 | DirectX 12, Vulkan or OpenGL 3.3 |
| **Building** | Rust 1.90 or newer, plus the libraries in [CONTRIBUTING.md](CONTRIBUTING.md#building) | Rust 1.90 or newer, plus the [Visual Studio C++ build tools](CONTRIBUTING.md#building) |

### Linux distributions

Undavi is developed on Arch Linux, and every change is built and tested on
Ubuntu 24.04. It should work on any distribution that uses PipeWire for sound,
which most have by default since 2022, including:

- Fedora 34 and newer
- Ubuntu 22.10 and newer, including 24.04 LTS
- Debian 12 and newer (GNOME; other desktops may still use PulseAudio)
- Linux Mint 22 and newer
- Pop!_OS 22.04 and newer
- openSUSE Tumbleweed
- Arch Linux, Manjaro and EndeavourOS

To check yours, run `pactl info | grep "Server Name"`: it should mention
PipeWire. Distributions that still use PulseAudio (such as Ubuntu 22.04 LTS)
can switch to PipeWire, or use Undavi with a microphone input.

macOS builds may run, but aren't supported yet: there are no playback controls there,
and it can't capture system audio.

## Installing

Already have 0.9.0? It can't update itself, so see [MIGRATING.md](MIGRATING.md).

### Windows

Download `undavi-<version>-x86_64-windows-setup.exe` from the
[latest release](https://github.com/karsoncaliff/undavi/releases/latest) and run it.
Undavi is then in the Start menu.

It's installed for you alone, to `%LOCALAPPDATA%\Programs\Undavi` unless you pick
another folder, and doesn't need administrator rights. Windows may warn about an app it
doesn't know, since the builds aren't signed: **More info**, then **Run anyway**.

To remove it, use **Installed apps** in the Windows settings.

If you'd rather see what an installer does, there's also a zip with a
[script](install.ps1) that does the same. Unpack `undavi-<version>-x86_64-windows.zip`,
right-click `install.ps1` in the folder and pick **Run with PowerShell**.

### Linux

Each [release](https://github.com/karsoncaliff/undavi/releases) has a ready-made
build for 64-bit PCs, for distributions with glibc 2.39 or newer (Ubuntu 24.04,
Fedora 40, Debian 13, Linux Mint 22, Arch and newer):

```sh
tar xf undavi-0.10.0-x86_64-linux.tar.gz
cd undavi-0.10.0-x86_64-linux
sudo ./install.sh
```

**Sudo is required for**
- Copying the executable into `/usr/local/bin`.
- Installing desktop shortcuts, icons, and license files.
- Removing those same files when uninstalling.

If you have any concerns, you can view the installation script [here](install.sh).

### Arch Linux

```sh
git clone https://github.com/karsoncaliff/undavi
cd undavi/packaging/arch
makepkg -si
```

### From source

Install Rust and the build dependencies listed in
[CONTRIBUTING.md](CONTRIBUTING.md#building), then:

```sh
git clone https://github.com/karsoncaliff/undavi
cd undavi
cargo build --release --locked
sudo ./install.sh
```

Either way, Undavi is installed to `/usr/local` with its menu entry and icons.
Run `undavi` or find it in your app launcher.

On Windows, the last line is `powershell -ExecutionPolicy Bypass -File install.ps1`.

### Uninstalling

To remove it on Linux, run
`sudo ./install.sh --uninstall` from the same folder.

## Using Undavi

Start your music, then open Undavi. The bars follow **System Audio** by default;
click the source button at the top to pick something else:

- **Apps:** only one app, such as Spotify or a browser, so notifications and
  other sounds don't move the bars. **Music player** follows whichever app is
  playing the current song. (On Linux, with PipeWire only.)
- **Outputs** and **Inputs:** a specific device, or a microphone.

Undavi remembers your choice.

https://github.com/user-attachments/assets/4d3b0ed7-97ae-42ae-b219-8fa4f2743f4a

Click the Undavi logo for the About panel below it. If the song's info is stuck, the
button next to the logo connects to your music player again, and looks for the cover and
the lyrics anew. The controls fade out when the mouse is still and come back when you
move it.

The sliders button next to the gear (or Q) opens the **quick settings**: the settings
you've starred in the settings window.

### The settings window

The gear at the top right (or S) opens the settings. Every change applies right
away, to the app behind the window and to the live preview beside the settings.
**Done** (or Esc) keeps your changes, and **Cancel** puts everything back the way
it was when you opened it.

- **Find things** in the tree on the left, or with the search above it: Enter goes to
  the setting that matches best. The arrows fold sections, and the groups in them,
  away; the button above the search expands or collapses every section.
- **Stars:** the star in front of a setting puts it in the quick settings. (Cancel
  leaves the stars as they are.)
- **Undo:** a setting you changed has an undo button next to its name, and then one
  to redo it. Loading a preset is a new start.
- **Presets:** each section has its own. Pick one from the menu at the top right of
  the section, or save your own there; rename, reorder and delete yours with the
  pencil. **Presets**, at the end of the tree, lists all of them, and is where you make
  preset groups: a preset for each section you like, loaded together.
- **Preview:** the whole screen in the shape of a regular window, an ultrawide
  screen or a narrow window, or only the part the section affects. **Peek at full
  size** hides the settings for a look at the real thing.

### Sound

How the bars react to sound: sensitivity, speed, gravity, the level and
frequency ranges, EQ, auto-gain, the noise gate, beats and a visual delay. A live
meter shows what the sliders are working with.

For a closer look, turn on the signal monitor below it. It graphs the input level
against the noise gate and level range, the spectrum with the bars laid over it, and
beats and tempo, and lists the frame rate, audio source and player. It also suggests
which slider to move when the bars sit too low, max out, or miss the beat. Each part
can be hidden.

https://github.com/user-attachments/assets/19622230-3d25-403f-bfd1-58fe59f63e71

### Visualizer and Colors

How the bars look: bars, a ring around the cover, a wave, or a [visualizer
shader of your own](docs/shaders.md); mirroring and stereo or mid/side split;
segments, peak caps and glow (with a meter of the beat it follows). Everything is
drawn on the GPU, and each visualizer has its own settings, declared in its shader
(yours can too). The palette and the accent color are in Colors.

https://github.com/user-attachments/assets/82fd0dca-c728-4cf5-9f08-5ec6a7761231

### Background

The picture behind the bars: a solid color, the album art, soft Ambient circles
in the cover's colors, or a [shader of your own](docs/shaders.md). The album art
can be three moving copies of the cover, one large cover or a grid of tiles that
stay still, drift or rotate, or come alive with an animated Liquid, Warp or
Spiral effect, at the speed and zoom you like, with a beat pulse that swells,
brightens, zooms or brings the art into focus. Effects go over any of them: dim,
saturation, brightness, contrast, an accent tint, a vignette and film grain.

### Now playing, Playback bar, Lyrics, Controls and General

How the current track is shown, how the playback bar looks (its style, what's in it,
and the seek bar's curve, thickness, handle and times), the lyrics' size, glow and
timing, the keyboard shortcuts, how far a step of seeking or volume goes, what the mouse
does over the bars, when the controls hide, the frame rate, (where the desktop allows it)
staying on top, whether Undavi checks for updates and installs them, and whether that
includes unstable versions.

https://github.com/user-attachments/assets/4f4a0219-77c5-40e9-aba1-8365805c0257

### Keyboard shortcuts

These are the shortcuts to start with. Change them, or add ones for the previous and next
track, the volume and more, under Controls in the settings.

| Key | Does |
|---|---|
| Space | Play or pause |
| ← / → | Seek 5 seconds back or forward |
| F or F11 | Full screen (or double-click the bars) |
| S | Open or close the settings |
| Q | Open or close the quick settings |
| T, V, B, I | Open the settings at Sound, Visualizer, Background, General |
| R | Refresh the song's info |
| Ctrl + D | Star the setting that has the focus |
| Esc | Close whatever is open, then leave full screen |
| Tab / Shift+Tab | Move between controls |
| ← → ↑ ↓, Page Up/Down, Home/End | Adjust the focused control |
| Enter or Space | Press the focused button or switch |

## Privacy

Undavi has no accounts, ads or telemetry. It only goes online for three things:

- **Lyrics:** the title, artist, album and length of the song that's playing are
  sent to [lrclib.net](https://lrclib.net). Lyrics are kept in memory while Undavi
  runs and never saved to disk.
- **Album art:** downloaded from the address your music player provides. (Not on
  Windows, where the player provides the picture itself.)
- **Updates:** when Undavi starts, it asks GitHub for the latest release. Turn
  this off in the settings (General), or with "Don't ask again" in the update pop-up.

## Settings

Settings and presets are saved automatically in
`~/.config/undavi/settings.json` (`%APPDATA%\undavi` on Windows,
`~/Library/Application Support/undavi` on macOS). To start over, quit Undavi and
delete that file.

## Roadmap

Undavi is a personal project, so there are no dates. Items move up
or down depending on bug reports and what people ask for.

The full roadmap for future features can be found [here](ROADMAP.md).

### In Development
- [ ] Static effects that don't depend on audio. (LFO)
- [ ] Particles in the background with custom effects.
- [ ] Reactions (When X, do Y).
- [ ] Modulation grid.

## Troubleshooting

- **The bars don't move.** Check that the source button says the right device.
  If it says **Default input**, Undavi can't capture system audio here (PipeWire
  isn't running), so it's listening to the microphone.
- **No track info, playback controls or lyrics.** Your player isn't running, or
  doesn't support MPRIS (Linux) or the media controls (Windows). Lyrics also need the
  song to be in LRCLIB, or a `.lrc` file next to it (Linux: players on Windows don't
  say which file they're playing).
- **An app doesn't move the bars on Windows.** Some music apps have an "exclusive
  mode", which keeps their sound from everything else. Turn it off in the app.
- **"Always on top" is missing.** Wayland doesn't let apps keep themselves on
  top; use your compositor's window rules instead.
- **Rendering problems or a blank window.** Try another graphics backend. On Linux,
  OpenGL: `WGPU_BACKEND=gl undavi`. On Windows, set `WGPU_BACKEND` to `dx12` or
  `vulkan` first (in PowerShell, `$env:WGPU_BACKEND = 'dx12'`).

## Contributing

Bug reports and pull requests are welcome. See
[CONTRIBUTING.md](CONTRIBUTING.md) to get started, and [docs/](docs/README.md)
for how Undavi works.

## License

Undavi is free software under the [GNU General Public License, version 3
only](LICENSE), with the additional terms in [NOTICE](NOTICE). The Undavi name
and logo are covered by the [trademark policy](TRADEMARKS.md): forks are welcome,
under their own name.

Undavi uses lyrics from [LRCLIB](https://lrclib.net), the
[Inter](https://rsms.me/inter/) typeface (SIL Open Font License 1.1) and
[Material Symbols](https://fonts.google.com/icons) icons (Apache License 2.0), and
is built with [iced](https://iced.rs) and [cpal](https://github.com/RustAudio/cpal).

## Credits

- [LRCLIB](https://lrclib.net): Lyric syncing
- [Inter](https://rsms.me/inter/): UI fonts
- [Material Symbols](https://fonts.google.com/icons): UI icons
- Dependencies: For their wonderful libraries
- [Spicy Lyrics](https://github.com/spikerko/spicy-lyrics/): Inspiration for the lyric features
- [NCS](https://ncs.io/): The music played in the previews

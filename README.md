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
  <img src="https://img.shields.io/badge/platform-Linux-blue?logo=linux&logoColor=white" alt="Platform: Linux">
  <a href="CONTRIBUTING.md#building"><img src="https://img.shields.io/badge/rust-1.90%2B-orange?logo=rust" alt="Rust 1.90+"></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/karsoncaliff/undavi" alt="License: GPL-3.0"></a>
</p>

https://github.com/user-attachments/assets/aaf0d770-fbcc-4a4b-94b2-1f113691ff23

---

## Features

- **Live spectrum of any audio.** System audio (whatever is playing), a single
  app such as Spotify or Firefox, whichever app your music player is, a specific
  output, or a microphone. Reconnects by itself if the device or audio server goes
  away.
- **Three styles:** bars, a ring of bars around the spinning album cover, or a
  smooth wave. Mirror them, split them into left/right or mid/side, add
  segments and peak caps, and color them with built-in palettes, your own
  gradient, or colors taken from the album art.
- **Synced lyrics,** word by word when available, from
  [LRCLIB](https://lrclib.net) or a `.lrc` file next to the song. Click a line
  to jump to it.
- **Now playing:** title, artist and cover in the header, as a large spotlight
  beside the bars, or as one small line. Play, pause, skip, seek, repeat and
  volume work with your music player.
- **Backgrounds** of blurred album art, soft ambient color or a solid color,
  slowly moving, and pulsing with the bass or drums if you like.
- **Fine tuning:** sensitivity, spread, rise and fall times, gravity, level range,
  contrast, frequency range and scale (log, mel, linear), a three-band EQ,
  auto-gain, a noise gate, beat sensitivity and a visual delay for Bluetooth
  audio, with a live meter that shows what the sliders are doing. Or press
  Auto-calibrate.
- **Presets** for each panel, so you can switch looks in one click.
- **Easy on the battery:** drawing slows to a crawl when nothing is moving, the
  frame rate can be capped, and silence can freeze the bars.
- **Keyboard friendly:** every control is reachable with Tab and adjustable with
  the arrow keys.

## Requirements

| | Needed |
|---|---|
| **Sound** | PipeWire as the sound server, for system audio and single apps. Without it, Undavi can still visualize an input such as a microphone. |
| **Music player** | One that supports MPRIS, for track info, album art, lyrics and playback controls. Most do: Spotify, Firefox, Chromium-based browsers, VLC, Rhythmbox, Elisa, Lollypop, Strawberry, and mpv with the `mpv-mpris` plugin. |
| **Graphics** | Vulkan, or OpenGL 3.3 / OpenGL ES 3.0. |
| **Desktop** | Wayland or X11. |
| **Building** | Rust 1.90 or newer, plus the libraries in [CONTRIBUTING.md](CONTRIBUTING.md#building). |

### Distributions

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

Windows and macOS builds may run, but aren't supported yet: there are no
playback controls there, and macOS can't capture system audio.

## Installing

### Download

Each [release](https://github.com/karsoncaliff/undavi/releases) has a ready-made
build for 64-bit PCs, for distributions with glibc 2.39 or newer (Ubuntu 24.04,
Fedora 40, Debian 13, Linux Mint 22, Arch and newer):

```sh
tar xf undavi-0.9.0-x86_64-linux.tar.gz
cd undavi-0.9.0-x86_64-linux
sudo packaging/install.sh
```

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
sudo packaging/install.sh
```

Either way, Undavi is installed to `/usr/local` with its menu entry and icons.
Run `undavi` or find it in your app launcher. To remove it, run
`sudo packaging/install.sh --uninstall` from the same folder.

## Using Undavi

Start your music, then open Undavi. The bars follow **System Audio** by default;
click the source button at the top to pick something else:

- **Apps:** only one app, such as Spotify or a browser, so notifications and
  other sounds don't move the bars. **Music player** follows whichever app is
  playing the current song. (PipeWire only.)
- **Outputs** and **Inputs:** a specific device, or a microphone.

Undavi remembers your choice.

https://github.com/user-attachments/assets/05674d5f-cc36-4b51-bfb2-3cb30b21ec6b

The three buttons at the top right open the side panels. Click the Undavi logo
for the About panel. The controls fade out when the mouse is still and come back
when you move it.

### Tuning

How the bars react to sound: sensitivity, speed, gravity, the level and
frequency ranges, EQ, auto-gain, the noise gate, beats and a visual delay. A live
meter shows what the sliders are working with while the panel is open.

https://github.com/user-attachments/assets/fedd0698-02ad-4055-898b-51e841b00bc9

### Visuals

How the bars look: bars, a ring around the cover, or a wave; mirroring and
stereo or mid/side split; segments, peak caps, colors and glow.

https://github.com/user-attachments/assets/44707b18-74da-44a6-a1d7-0af56a35a9f0

### Appearance

The background, how the current track is shown, lyrics, the accent color and the
window.

https://github.com/user-attachments/assets/e6c1f41a-5f51-4cd2-b4f2-3570a6738fb6

### Keyboard shortcuts

| Key | Does |
|---|---|
| Space | Play or pause |
| ← / → | Seek 5 seconds back or forward |
| F or F11 | Full screen (or double-click the bars) |
| T, V, A | Open or close Tuning, Visuals, Appearance |
| Esc | Close whatever is open, then leave full screen |
| Tab / Shift+Tab | Move between controls |
| ← → ↑ ↓, Page Up/Down, Home/End | Adjust the focused control |
| Enter or Space | Press the focused button or switch |

## Privacy

Undavi has no accounts, ads or telemetry. It only goes online for two things:

- **Lyrics:** the title, artist, album and length of the song that's playing are
  sent to [lrclib.net](https://lrclib.net). Lyrics are kept in memory while Undavi
  runs and never saved to disk.
- **Album art:** downloaded from the address your music player provides.

## Settings

Settings and presets are saved automatically in
`~/.config/undavi/settings.json` (`%APPDATA%\undavi` on Windows,
`~/Library/Application Support/undavi` on macOS). To start over, quit Undavi and
delete that file.

## Troubleshooting

- **The bars don't move.** Check that the source button says the right device.
  If it says **Default input**, Undavi can't capture system audio here (PipeWire
  isn't running), so it's listening to the microphone.
- **No track info, playback controls or lyrics.** Your player doesn't support
  MPRIS, or isn't running. Lyrics also need the song to be in LRCLIB.
- **"Always on top" is missing.** Wayland doesn't let apps keep themselves on
  top; use your compositor's window rules instead.
- **Rendering problems or a blank window.** Try the OpenGL backend:
  `WGPU_BACKEND=gl undavi`.

## Contributing

Bug reports and pull requests are welcome. See
[CONTRIBUTING.md](CONTRIBUTING.md) to get started, and [docs/](docs/README.md)
for how Undavi works.

## License

Undavi is free software under the [GNU General Public License, version 3
only](LICENSE), with the additional terms in [NOTICE](NOTICE). The Undavi name
and logo are covered by the [trademark policy](TRADEMARKS.md): forks are welcome,
under their own name.

Undavi uses lyrics from [LRCLIB](https://lrclib.net) and the
[Inter](https://rsms.me/inter/) typeface (SIL Open Font License 1.1), and is
built with [iced](https://iced.rs) and [cpal](https://github.com/RustAudio/cpal).

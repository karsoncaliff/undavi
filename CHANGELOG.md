# Changelog

All notable changes to Undavi are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.0-alpha.2] - 2026-11-26

### Fixed

- Windows: going full screen could show the desktop through Undavi, or a white patch,
  for a few seconds (seen with NVIDIA). Undavi now draws with DirectX 12 on Windows,
  where the GPU has it. `WGPU_BACKEND` still picks another one.

## [1.0.0-alpha] - 2026-11-25

An unstable version, for trying the latest features.

### Added

- Windows support (64-bit Windows 11, or Windows 10 version 2004 and newer), with what
  Undavi does on Linux:
  - Track info, album art, synced lyrics and playback controls for any player that shows
    up in the media controls of Windows. The volume is the player's in the volume mixer.
  - Single apps, Music player and outputs as audio sources, next to system audio and
    inputs.
  - An installer, or a zip with an install script (`install.ps1`) instead. Either adds
    Undavi to the Start menu and to Installed apps, and gets updates of its own kind.
  - Reduced motion, following "Animation effects" in the accessibility settings.
  - An icon for `undavi.exe`, and no console window next to it.
- "Unstable versions" under General in the settings: updates to early builds from the dev
  branch, only for trying the latest features.

## [0.10.0] - 2026-11-25

0.9.0 can't update itself, so this version has to be installed by hand. See the
[migration guide](https://github.com/karsoncaliff/undavi/blob/main/MIGRATING.md).

### Added

- Update checks: on startup Undavi looks for a newer release on GitHub and
  offers to install it, with "Update automatically" and "Don't ask again"
  options. Installs from a release tarball or `install.sh` are updated in place
  (asking for your password when needed); pacman installs are pointed to pacman.
  The switches are under General in the settings.
- A "Word by word" switch under Lyrics. Turn it off to light up
  whole lines even when the lyrics have word timing.
- Auto-hide: "Only when the mouse moves", so keys, clicks and scrolling don't
  bring the controls back.
- Gravity strength (Sound → Response), from 25% to 250%.
- An animated background (Background → Animated) for both Album art and
  Ambient: Liquid (domain-warped simplex noise), Warp (a breathing
  Brown–Conrady lens with a ripple) or Spiral (a twirl).
- Custom visualizers: a shader with `fn visualizer(uv, t)` in the shaders
  folder, listed under Visualizer → Style next to Bars, Radial and Wave. It gets
  each bar's value and peak, a smooth spectrum curve, the bar palette and the
  glow, plus the music. Two examples (an LED meter and a polar "orbit") come
  with it.
- Shader settings: a shader declares its own settings in `// @setting` comments
  (sliders, switches, choices and colors, in groups, with help text and
  conditions), and the settings show them. Values are kept per shader and in
  presets. Undavi reads a few ids itself (`bar_count`, `mirror`, `glow`, `blur`,
  `pulse`, …), so a custom shader can opt into those. See `docs/shaders.md`.
- "Make a copy" puts a built-in shader in the shaders folder, to start your own
  from it.
- The visualizer is drawn on the GPU: bars, segments and caps are signed-distance
  rounded boxes with a real soft glow, the Wave is evaluated per pixel, and
  Radial's cover is drawn by the same shader.
- Custom background shaders: `.wgsl` files in `~/.config/undavi/shaders/`. A
  shader either warps the art (one more Motion choice when the background is
  Animated) or draws the whole scene (listed under Background → Style), and can follow the bass, mids, treble, beat and 16 spectrum
  bands. Files reload when saved; errors are shown with their line instead of
  crashing. Comes with three examples. See `docs/shaders.md`.
- The Album art and Ambient backgrounds are drawn on the GPU: the blur (dual
  Kawase), color effects, vignette and grain update instantly, and the copies
  are blurred together as one picture.
- Album art background options: layout (Copies, Single, Tiles), motion speed,
  zoom, an Effects section (saturation, brightness, contrast, accent tint,
  vignette, grain), and pulse styles (Swell, Brighten, Zoom, Focus).
- A signal monitor (a switch at the top of Sound, with the monitor below it): graphs
  of the input level against the noise gate and level range, the spectrum with the
  bars over it, and beat pulses with hits and tempo, plus frame rate, audio source
  and player details, and tips for the Sound sliders. Each part can be hidden.
- A live preview in the settings window: the whole screen, drawn small, in the
  shape of a regular window, an ultrawide screen or a narrow window; or only the
  part of the screen the section affects, filling the preview. "Peek at full size"
  hides the settings for a look at the real thing.
- Built-in presets for every section, the name of the preset in use (with an
  "Edited" tag once the settings differ from it), and a menu to save over it, save
  a new one or discard the changes.
- Search in the settings. Enter goes to the setting that matches best.
- Undo and redo buttons next to the name of a setting you changed.
- The sections in the tree, and the groups in them, fold away, and a button above the
  search expands or collapses the whole tree.
- A **Presets** page in the settings with every section's presets, to load, rename,
  reorder or delete, and preset groups: presets of several sections, loaded together.
- A **Playback bar** section: on the background (as before), in a card or docked along
  the bottom of the window, full or compact, with what's in it and the seek bar's
  curve height, length and speed, thickness, handle, brightness and times.
- A **Controls** section: every keyboard shortcut can be changed (and there are new ones,
  for the previous and next track, the volume, mute and more), with how far a step of
  seeking or volume goes, and what double-clicking and scrolling over the bars do.
- Quick settings (the sliders button, or Q): the settings you've starred, in a panel
  under the header. Every setting has a star in front of its name (Ctrl + D stars the
  one that has the focus).
- A button next to the logo (or R) that refreshes the song's info, cover and lyrics, for
  when the connection to the music player is stuck.
- The beat meter shows under "Glow on the beat" and "Pulse with the beat" while
  they're on.
- Cancel, which puts every setting and preset back the way it was when the
  settings were opened.

### Changed

- One settings window (the gear, or S) replaces the Tuning, Visuals and Appearance
  panels, with nine sections: **Sound** (T), **Visualizer** (V), **Colors**,
  **Background** (B, now with the effects), **Now playing**, **Playback bar**,
  **Lyrics**, **Controls** and **General** (I). Each has its own presets. Saved settings and presets move over by
  themselves (format version 4): a preset becomes one in each section it had
  settings for, with the same name. The A shortcut is gone.
- The tuning meter is at the top of Sound.
- The About panel opens on the left, under the wordmark.
- The icons are Google's Material Symbols.
- The source and settings buttons are see-through.
- Shortcuts only react to their exact keys: Shift + S no longer opens the settings.
- Every visualizer and background is a shader now, built-ins included: Bars,
  Radial and Wave, and Solid, Album art and Ambient are `.wgsl` files that
  declare their settings like any other. Visualizer shows the settings of the
  visualizer in use (only Colors are shared by all of them), and Background those of
  the background. Glow is a setting of each visualizer. Each shader remembers its own
  settings, so Bars and Wave can have different bar counts. Settings files and
  presets from before move over by themselves (format version 2).
- The color effects, vignette and grain now apply to the Solid background too.
- The background motion "Spiral" is now called "Rotate", since it turns the art
  rather than spiralling it. Saved settings keep working.

### Fixed

- Scrollbars now use the accent color.
- The wordmark in the About panel (and other icons) could be drawn with a doubled or
  missing row or column of pixels.
- In small windows, the volume control squeezed the button next to it.
- The mark under the pointer on the seek bar follows its curve.
- With up/down mirroring, solid bars are one continuous shape instead of
  two halves pinched at the center line (Bars and Radial).
- Album art and `.lrc` files that are far too big (or aren't files at all) are
  skipped instead of being read whole.

## [0.9.0] - 2026-11-19

The first public release.

### Added

- Live spectrum of system audio (PipeWire), a single app, whichever app the
  music player is, a specific output, or an input, with automatic reconnection
  when the device or audio server goes away. The chosen source is remembered.
- Bars, Radial and Wave styles; up/down and left/right mirroring; stereo and
  mid/side split; mono, left or right channel; segments; peak caps; rounded
  corners; a maximum bar length.
- Palettes: Silver, Undavi, Aurora, Ember, colors from the album art, and custom
  gradients; bar glow on the bass or drums.
- Tuning: sensitivity, spread, rise and fall times, gravity, peak hold, level
  range, contrast, frequency range, tilt and scale (log, mel, linear), FFT detail,
  bar value, three-band EQ, auto-gain with adapt speed, noise gate, silence
  behavior (fade out, idle wave, freeze), beat sensitivity, visual delay, and
  auto-calibrate, with a live meter while tuning.
- Now playing over MPRIS: title, artist, album and cover in the header, a
  spotlight beside the bars, or one small line; play, pause, previous, next, seek,
  repeat and volume.
- Time-synced lyrics from LRCLIB or a `.lrc` file next to the song, word by word
  when available, with size, glow and timing settings; click a line to seek.
  Lyrics are never saved to disk.
- Backgrounds: blurred album art, ambient color or solid color, with drift or
  spiral motion and an optional pulse with the beat.
- Accent colors, including Lilac and Sky from the Undavi gradient and a color
  from the album art.
- Presets for the Tuning, Visuals and Appearance panels.
- Keyboard navigation for every control, auto-hiding controls, full screen,
  always on top (where the platform allows), a frame-rate cap, and a low-power
  idle mode.
- A layout that adapts to narrow windows: floating panels and a compact header.
- About panel with credits and licenses; app icon, desktop entry, AppStream
  metadata, an install script and an Arch PKGBUILD.
- Settings saved in a versioned file, with automatic migration from older
  versions and from Muvijo, Undavi's previous name.

[1.0.0-alpha.2]: https://github.com/karsoncaliff/undavi/compare/v1.0.0-alpha...v1.0.0-alpha.2
[1.0.0-alpha]: https://github.com/karsoncaliff/undavi/compare/v0.10.0...v1.0.0-alpha
[0.10.0]: https://github.com/karsoncaliff/undavi/compare/v0.9.0...v0.10.0
[0.9.0]: https://github.com/karsoncaliff/undavi/releases/tag/v0.9.0

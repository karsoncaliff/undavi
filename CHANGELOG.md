# Changelog

All notable changes to Undavi are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.9.0] - 2026-11-18

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

[Unreleased]: https://github.com/karsoncaliff/undavi/compare/v0.9.0...HEAD
[0.9.0]: https://github.com/karsoncaliff/undavi/releases/tag/v0.9.0

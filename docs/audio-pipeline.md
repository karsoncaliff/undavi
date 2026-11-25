# Audio pipeline

How a buffer of captured samples becomes bar heights, glow and silence. The
numbers here are the constants in the code; if you change one, change it here
too.

```text
cpal ─▶ Capture ─▶ ring buffer ─▶ Analyzer ─▶ Gate ─▶ Processor ─▶ bars, peak caps
                                      │                  
                                      └──────────▶ BeatGlow ─▶ bar glow, background pulse
```

## 1. Capture (`audio::Capture`)

- **Which device.** "System Audio" is the default output's monitor on PipeWire
  (`sink_default`), a WASAPI loopback of the default output on Windows, and the
  default input everywhere else (labeled "Default input" there). Other sources
  are outputs (their monitors) and inputs, as cpal lists them.
- **Single apps (Linux).** An app source records only that app, so other
  sounds don't move the bars. cpal can't record another app's playback stream,
  so `audio::linux` does it with PipeWire directly: it opens its own capture
  stream (stereo `f32` at 48 kHz, not connected by the session manager) and
  links the output ports of every playback stream (`Stream/Output/Audio`) whose
  app matches. PipeWire mixes several links into one input, and links are added
  and removed as the app's streams come and go, so apps with several streams
  (Spotify) or a new stream per playback (Firefox) are followed. Apps are matched
  by `application.name`, then the executable, then the node name.
- **Single apps (Windows).** `audio::windows` uses WASAPI's process loopback
  (stereo `f32` at 48 kHz), which records what a process and those it started play,
  on whatever device. It looks for the app's audio sessions every 500 ms and opens
  one for each process that has any, except those started by another one of them.
  They all deliver steadily, silence included, so they're added up in step; one
  that falls more than 100 ms behind counts as silent. Apps are matched by their
  name: for packaged apps the one in the Start menu, for the rest the executable's
  "File description", or else its file name.
- **Music player.** The same, with the app chosen from the player. On Linux, its
  MPRIS bus name parts (without `org`, `instance…` and the like) and display name
  are compared with the stream's application name and executable
  (`audio::player_matches`), so following Spotify, then a browser tab, just works.
  On Windows, the player's app ID leads to its executable and display name through
  the Start menu (`audio::windows::player_names`). An app with exactly that name is
  it; if there is none, the names are compared the same way as on Linux.
  When the target isn't playing, the stage says "Waiting for …" after 1.5 s.
- **Format.** The device's default config. `i16`, `i32` and `f32` samples are
  converted to `f32` in -1.0..=1.0.
- **Channels.** Every frame is folded to a left/right pair: mono is copied to both
  sides, and channels beyond the first two (center, LFE, surrounds) are shared
  equally, so `(left + right) / 2` is always the plain mono mix.
- **Hand-off.** Pairs go into a lock-free ring buffer of about a second. If the UI
  falls behind, the newest samples are dropped; the audio thread never waits.
- **Errors.** Xruns, refused real-time scheduling and "rerouted to the new default
  device" leave the stream running. Anything else marks it dead, and the app
  reopens it: first after 500 ms, then every 2 s. A stream that delivers nothing
  for 5 s while the player says it's playing is reopened quietly. The
  "Reconnecting" message only appears after 1.5 s, so quick recoveries don't flash.

## 2. Analysis (`audio::Analyzer`)

- **History.** 131,072 samples per side: the largest FFT plus 500 ms of visual
  delay at up to 192 kHz.
- **Window.** The latest `size` samples, ending `delay` samples ago (Sound →
  Visual delay), with a Hann window. `size` is 2048, 4096 or 8192 (Sound →
  Detail: Fast, Balanced, Detailed).
- **Spectrum.** A real FFT, scaled by `2 / sum(window)` so a full-scale sine reads
  0 dBFS, then `20·log10` with a floor at -140 dB. The mix is always computed;
  left, right and side `(left − right) / 2` only when the bars need them.
- **Level.** RMS of the mix over the window's last 2048 samples (about 40 ms),
  scaled so a full-scale sine reads 0 dB. This feeds the gate and the meter.
- **Staleness.** If no samples arrived for 250 ms (plus the visual delay), there
  is no audio at all and the gate closes.

## 3. Gate (`spectrum::Gate`)

Decides whether there is sound to show.

- Opens as soon as the level reaches the threshold (Sound → Noise gate,
  -100 to -30 dB; the lowest position turns the gate off).
- Closes once the level has stayed more than 3 dB below the threshold for 250 ms,
  so short gaps between notes and a level hovering at the threshold don't
  flicker.
- Anything at or below -110 dB is digital silence and always closes it; so does
  no audio at all.

While the gate is closed, the bars and beat detectors get no input, and the app
is free to go idle.

## 4. Bar heights (`spectrum::Processor`)

One `Processor` turns one or two spectra into one value per bar (with Left/right
split, the left half's values come first). Per frame:

1. **Bin.** The frequency range (Sound → Range, 20 Hz to 20 kHz) is cut into one
   band per bar along the frequency scale: equal octaves (Log), equal mels (Mel),
   or equal Hz (Linear). A band covering several FFT bins takes the loudest bin
   or the average power (Sound → Bar value); a band narrower than one bin
   interpolates at its center.
2. **Tilt.** `tilt × log2(center / 1 kHz)` dB, pivoting at 1 kHz.
3. **Normalize.** The level range (Sound → Level range) maps floor..ceiling to
   0..1, then the contrast curve raises it to `2^((contrast − 50) / 50)`
   (0.5 to 2).
4. **EQ.** Bass, mids and highs bands are Gaussians centered at 0.08, 0.45 and
   0.85 of the 20 Hz to 20 kHz log axis (widths 0.20, 0.22, 0.20). Their gains are
   mixed by weight and applied as linear gain.
5. **Sensitivity.** Multiplies by 0.35 + 0.65 × sensitivity / 100.
6. **Spread.** A Gaussian blur across neighboring bars with σ = spread / 100 × 2.5
   bars, never across the two halves of a split.
7. **Silence.** With no input, Sound → When it's silent decides: *Fade out* keeps
   the targets at zero, *Idle wave* sets them to a slow wave 12% high, and
   *Freeze* stops here and leaves everything as it was.
8. **Rise and fall.** Each bar eases toward its target with time constant
   `rise_ms` going up and `fall_ms` going down. With Gravity, bars instead fall
   with constant acceleration `g = 2 / (3 × fall)²`, so a full bar lands in three
   fall times; a bar resting on its target has no speed.
9. **Auto-gain.** Tracks the tallest bar with an instant attack and a release
   set by Adapt speed (8, 3 or 1 s), and scales so that peak sits at 0.95. The
   factor is limited to 0.25 to 8, and holds still during silence so music
   doesn't come back blown up. Internal heights stay unscaled, so turning
   auto-gain on or off never makes the bars jump.
10. **Peak caps.** Each cap holds for `peak_hold_ms`, then falls at the fall rate
    (or with gravity).

**Auto-calibrate** listens for 5 s, collects the tallest bar of every frame with
something in it (at least ten frames), and picks the Sensitivity that puts their
75th percentile at 80% height.

## 5. Beats (`spectrum::BeatGlow`)

Two detectors, chosen per effect (Visualizer → Glow, Background → Pulse with the
beat). Both read the mix spectrum and give a pulse in 0..1.

- **Bass** compares the 30–150 Hz power with its own recent average (1.5 s) and
  a peak that falls 4 dB/s, so it works at any volume. The pulse rises in about
  20 ms and settles in about 220 ms.
- **Drums** looks for spectral flux (rising amplitude) in a kick band
  (40–150 Hz) and a snare band (1.5–6 kHz). A hit is flux above its 0.6 s average
  times a ratio; snare hits count 75%. Hits are at least 110 ms apart, and the
  pulse decays with a 150 ms time constant.

**Beat sensitivity** (Sound → Beats) sets the bar for a hit. For drums, the
ratio goes from 2.4× (0) through 1.6× (50) to 1.25× (100). For bass, below 50 the
bottom part of each swell is ignored (up to half of it); above 50, swells count
up to twice as much.

The background pulse eases the detector's output again (60 ms up, 350 ms down)
so it breathes instead of flickering.

## 6. Mapping values to bars (`visualizer::BarMap`)

| Mirror | Split | Values | Bar order |
|---|---|---|---|
| Off, Up/down | – | one per bar, from the chosen Channel | bass on the left |
| Left/right, Both | Off | one per pair of bars | bass in the middle, the same spectrum on both sides |
| Left/right, Both | Stereo or Mid/Side | one per bar | bass in the middle; left/mid on the left, right/side on the right |

//! From spectra to what's drawn: bar heights, peak caps, beats and silence.
//!
//! Pure math with no audio or UI, so it runs the same in tests. Each frame:
//!
//! 1. A [`Gate`] decides from the input level whether there's sound at all.
//! 2. A [`Processor`] turns one or two [`SpectrumFrame`]s into bar heights:
//!    bin into bars along the frequency scale, normalize between the level floor
//!    and ceiling, apply contrast, tilt and the band EQ, Sensitivity, Spread,
//!    rise/fall (or gravity) and auto-gain. Without sound it fades, idles or
//!    freezes, as [`SilenceMode`] says.
//! 3. A [`BeatGlow`] follows the bass or detects drum hits for the glow effects.
//!
//! `docs/audio-pipeline.md` in the repository walks through every step.
//!
//! # Examples
//!
//! ```
//! use undavi::settings::TuningSettings;
//! use undavi::spectrum::{Processor, SpectrumFrame};
//!
//! // A flat spectrum at -20 dBFS, as an `audio::Analyzer` would produce at 48 kHz.
//! let magnitudes = vec![-20.0; 4096 / 2 + 1];
//! let frame = SpectrumFrame { magnitudes_db: &magnitudes, sample_rate: 48_000.0, fft_size: 4096 };
//!
//! let mut processor = Processor::default();
//! let tuning = TuningSettings::default();
//! for _ in 0..60 {
//!     processor.update(std::slice::from_ref(&frame), 32, &tuning, 1.0 / 60.0);
//! }
//! assert_eq!(processor.visible().len(), 32);
//! assert!(processor.visible().iter().all(|&h| h > 0.3 && h <= 1.0));
//! ```

use crate::settings::{
    BarValue, GATE_OFF, GlowSource, SilenceMode, TuningSettings, contrast_exponent, hz_to_log_pos,
    slider_to_hz,
};
use std::f32::consts::TAU;

/// Auto-gain: the height (0..=1) it scales the tallest bar to.
pub const AGC_TARGET: f32 = 0.95;
/// Auto-gain: the quietest peak it scales up from, and the gain limits.
const AGC_MIN_PEAK: f32 = 0.05;
const AGC_RANGE: (f32, f32) = (0.25, 8.0);
/// The height (0..=1) of the slow wave shown in silence with
/// [`SilenceMode::Wave`].
pub const IDLE_LEVEL: f32 = 0.12;
/// Gravity: a bar falls its full height in this many fall times.
const GRAVITY_FALLS: f32 = 3.0;
/// Auto-calibrate: how long to listen.
pub const CALIBRATE_S: f32 = 5.0;
/// Auto-calibrate: where typical peaks should land.
const CALIBRATE_TARGET: f32 = 0.8;

/// (center, width) of the bass / mids / highs EQ bands on the 20 Hz–20 kHz log axis.
const BANDS: [(f32, f32); 3] = [(0.08, 0.20), (0.45, 0.22), (0.85, 0.20)];

/// Sensitivity slider (0..=100) → height multiplier (0.35×..=1.0×).
pub fn sensitivity_gain(sensitivity: u8) -> f32 {
    0.35 + 0.65 * (sensitivity as f32 / 100.0)
}

/// Inverse of [`sensitivity_gain`], clamped to the slider range.
fn sensitivity_for_gain(gain: f32) -> u8 {
    (((gain - 0.35) / 0.65) * 100.0).round().clamp(0.0, 100.0) as u8
}

/// Smooth EQ gain (in dB) at a position on the log-frequency axis.
///
/// Each band contributes its gain weighted by a Gaussian; weights are normalized
/// so overlapping bands never add up to more than any single band's gain.
pub fn eq_gain_db(pos: f32, bands_db: [f32; 3]) -> f32 {
    let mut sum = 0.0;
    let mut weight = 0.0;
    for ((center, width), db) in BANDS.iter().zip(bands_db) {
        let z = (pos - center) / width;
        let w = (-0.5 * z * z).exp();
        sum += w * db;
        weight += w;
    }
    sum / weight.max(1.0)
}

struct Calibration {
    elapsed: f32,
    frame_peaks: Vec<f32>,
}

/// Turns FFT magnitudes into animated bar heights. One per visualizer.
pub struct Processor {
    /// Smoothed bar heights before auto-gain, one per value the bars need
    /// (see `BarMap::bins`). With stereo, the left channel's values come first.
    heights: Vec<f32>,
    /// What gets drawn: heights with auto-gain applied, clamped to 0..=1.
    visible: Vec<f32>,
    /// Peak-cap positions, 0..=1.
    peaks: Vec<f32>,
    peak_hold: Vec<f32>,
    /// Gravity: how fast each bar and cap is falling, in heights per second.
    fall_speed: Vec<f32>,
    peak_speed: Vec<f32>,
    agc_peak: f32,
    /// The auto-gain factor applied this frame (1 when auto-gain is off).
    gain: f32,
    /// Loudest bar this frame in dB (after tilt), for the tuning meter.
    loudest_db: Option<f32>,
    /// Clock for the idle animation, in seconds.
    idle_time: f32,
    calibration: Option<Calibration>,
    // Scratch buffers reused every frame.
    targets: Vec<f32>,
    scratch: Vec<f32>,
}

impl Default for Processor {
    fn default() -> Self {
        Self {
            heights: Vec::new(),
            visible: Vec::new(),
            peaks: Vec::new(),
            peak_hold: Vec::new(),
            fall_speed: Vec::new(),
            peak_speed: Vec::new(),
            agc_peak: 0.0,
            gain: 1.0,
            loudest_db: None,
            idle_time: 0.0,
            calibration: None,
            targets: Vec::new(),
            scratch: Vec::new(),
        }
    }
}

/// Frequency-domain input for one frame.
pub struct SpectrumFrame<'a> {
    /// Magnitude per FFT bin in dBFS (length `fft_size / 2 + 1`).
    pub magnitudes_db: &'a [f32],
    /// Sample rate of the analyzed audio, in Hz.
    pub sample_rate: f32,
    /// FFT size the magnitudes came from, in samples.
    pub fft_size: usize,
}

impl Processor {
    /// Peak-cap positions this frame, 0..=1, one per value (like [`Processor::visible`]).
    pub fn peaks(&self) -> &[f32] {
        &self.peaks
    }

    /// Start listening for [`CALIBRATE_S`] seconds to find a Sensitivity that puts
    /// typical peaks at about 80% height. [`Processor::update`] returns it when done.
    pub fn start_calibration(&mut self) {
        self.calibration = Some(Calibration {
            elapsed: 0.0,
            frame_peaks: Vec::new(),
        });
    }

    /// Stop a running calibration without a result.
    pub fn cancel_calibration(&mut self) {
        self.calibration = None;
    }

    /// Seconds left in the running calibration, if any.
    pub fn calibration_remaining(&self) -> Option<f32> {
        self.calibration
            .as_ref()
            .map(|c| (CALIBRATE_S - c.elapsed).max(0.0))
    }

    /// Run one frame. `bins` is the number of values to compute, split evenly
    /// between `frames` (one spectrum, or one per half). No frames means silence,
    /// handled as `tuning.when_silent` says.
    /// Returns `Some(sensitivity)` when a calibration just finished.
    pub fn update(
        &mut self,
        frames: &[SpectrumFrame<'_>],
        bins: usize,
        tuning: &TuningSettings,
        dt: f32,
    ) -> Option<u8> {
        let dt = dt.clamp(0.0, 0.25);
        self.resize(bins);
        let silent = frames.is_empty();

        // 3–5. Bin, normalize and EQ (or fall to silence without input).
        self.targets.clear();
        self.loudest_db = None;
        let channels = frames.len().max(1);
        for frame in frames {
            self.bin(frame, bins / channels, tuning);
        }
        self.targets.resize(bins, 0.0);

        let calibrated = self.calibrate(dt);
        if silent && tuning.when_silent == SilenceMode::Freeze {
            // Everything stays where it is until sound comes back.
            return calibrated;
        }

        // 6. Sensitivity.
        let gain = sensitivity_gain(tuning.sensitivity);
        for t in &mut self.targets {
            *t *= gain;
        }

        // 7. Spread: Gaussian blur across neighboring bars.
        self.spread(tuning.spread, channels);

        if silent && tuning.when_silent == SilenceMode::Wave {
            self.idle_time += dt;
            self.idle_targets(channels, self.agc_gain(tuning));
        }

        // 8. Rise / fall (per-bar attack and release).
        let fall_s = tuning.fall_ms as f32 / 1000.0;
        let rise = 1.0 - (-dt / (tuning.rise_ms as f32 / 1000.0)).exp();
        let fall = 1.0 - (-dt / fall_s).exp();
        // Gravity: constant acceleration that drops a full bar in GRAVITY_FALLS fall
        // times at 100% strength (stronger pulls harder, so it drops sooner).
        let gravity = tuning.gravity.then(|| {
            2.0 / (GRAVITY_FALLS * fall_s).powi(2) * tuning.gravity_strength as f32 / 100.0
        });
        for i in 0..self.heights.len() {
            let (h, t) = (&mut self.heights[i], self.targets[i]);
            if t > *h {
                *h += (t - *h) * rise;
                self.fall_speed[i] = 0.0;
            } else if let Some(g) = gravity {
                self.fall_speed[i] += g * dt;
                *h -= self.fall_speed[i] * dt;
                if *h <= t {
                    // Landed on the target: rest there instead of building up speed.
                    *h = t;
                    self.fall_speed[i] = 0.0;
                }
            } else {
                *h += (t - *h) * fall;
            }
        }

        // 9. Auto-gain: follow a slowly decaying peak and scale toward the target height.
        // It holds still during silence, so music coming back isn't blown up.
        if !silent {
            let tallest = self.heights.iter().copied().fold(0.0, f32::max);
            let release = tuning.agc_speed.release_s();
            self.agc_peak = tallest.max(self.agc_peak * (-dt / release).exp());
        }
        let agc = self.agc_gain(tuning);
        self.gain = agc;

        // 10. Clamp. Internal heights stay un-scaled so rise/fall is continuous
        // when auto-gain toggles; only the visible values carry the factor.
        let hold = tuning.peak_hold_ms as f32 / 1000.0;
        for i in 0..self.heights.len() {
            let v = (self.heights[i] * agc).clamp(0.0, 1.0);
            self.visible[i] = v;
            // Peak caps: hold, then fall at the Fall time's rate (or with gravity).
            if v >= self.peaks[i] {
                self.peaks[i] = v;
                self.peak_hold[i] = hold;
                self.peak_speed[i] = 0.0;
            } else if self.peak_hold[i] > 0.0 {
                self.peak_hold[i] -= dt;
            } else if let Some(g) = gravity {
                self.peak_speed[i] += g * dt;
                self.peaks[i] -= self.peak_speed[i] * dt;
                if self.peaks[i] <= v {
                    self.peaks[i] = v;
                    self.peak_speed[i] = 0.0;
                }
            } else {
                self.peaks[i] = (self.peaks[i] * (1.0 - fall)).max(v);
            }
        }

        calibrated
    }

    /// Auto-gain factor for the current peak (1 when auto-gain is off).
    fn agc_gain(&self, tuning: &TuningSettings) -> f32 {
        if tuning.auto_gain {
            (AGC_TARGET / self.agc_peak.max(AGC_MIN_PEAK)).clamp(AGC_RANGE.0, AGC_RANGE.1)
        } else {
            1.0
        }
    }

    /// Advance a running calibration with this frame's targets (before Sensitivity).
    fn calibrate(&mut self, dt: f32) -> Option<u8> {
        let cal = self.calibration.as_mut()?;
        cal.elapsed += dt;
        let frame_peak = self.targets.iter().copied().fold(0.0, f32::max);
        if frame_peak > 0.02 {
            cal.frame_peaks.push(frame_peak);
        }
        if cal.elapsed < CALIBRATE_S {
            return None;
        }
        let mut peaks = std::mem::take(&mut cal.frame_peaks);
        self.calibration = None;
        if peaks.len() < 10 {
            return None;
        }
        // "Typical peak": the 75th percentile of per-frame maxima.
        peaks.sort_by(f32::total_cmp);
        let typical = peaks[(peaks.len() * 3) / 4];
        Some(sensitivity_for_gain(CALIBRATE_TARGET / typical))
    }

    /// A slow, low wave across each channel's bars, pre-divided by the auto-gain
    /// so it shows at the same height whatever the gain is.
    fn idle_targets(&mut self, channels: usize, agc: f32) {
        let run = (self.targets.len() / channels).max(1);
        let t = self.idle_time;
        for (i, target) in self.targets.iter_mut().enumerate() {
            let x = (i % run) as f32 / run as f32;
            let a = 0.5 + 0.5 * (TAU * (1.2 * x - 0.25 * t)).sin();
            let b = 0.5 + 0.5 * (TAU * (2.7 * x + 0.17 * t)).sin();
            *target = IDLE_LEVEL * (0.35 + 0.65 * a * (0.6 + 0.4 * b)) / agc;
        }
    }

    /// The auto-gain factor applied to the last frame (1 when auto-gain is off).
    pub fn gain(&self) -> f32 {
        self.gain
    }

    /// The loudest bar's level in the last frame, in dB (after tilt); `None` in silence.
    pub fn loudest_db(&self) -> Option<f32> {
        self.loudest_db
    }

    /// Values to draw this frame (auto-gain applied, clamped to 0..=1).
    pub fn visible(&self) -> &[f32] {
        &self.visible
    }

    fn resize(&mut self, bins: usize) {
        if self.heights.len() != bins {
            self.heights = vec![0.0; bins];
            self.visible = vec![0.0; bins];
            self.peaks = vec![0.0; bins];
            self.peak_hold = vec![0.0; bins];
            self.fall_speed = vec![0.0; bins];
            self.peak_speed = vec![0.0; bins];
        }
    }

    fn bin(&mut self, frame: &SpectrumFrame<'_>, bins: usize, tuning: &TuningSettings) {
        let lo = slider_to_hz(tuning.freq_lo as f32);
        let hi = slider_to_hz(tuning.freq_hi as f32).max(lo * 1.01);
        let hz_per_bin = frame.sample_rate / frame.fft_size as f32;
        let mags = frame.magnitudes_db;
        let last = mags.len().saturating_sub(1);
        let mut edges = tuning.freq_scale.edges(lo, hi, bins);
        let mut f1 = edges.next().unwrap_or(lo);
        let bands = [
            tuning.bass_db as f32,
            tuning.mids_db as f32,
            tuning.highs_db as f32,
        ];
        let floor = tuning.floor_db as f32;
        let span = (tuning.ceiling_db as f32 - floor).max(1.0);
        let exponent = contrast_exponent(tuning.contrast);
        let tilt = tuning.tilt_db as f32;

        for _ in 0..bins {
            let f0 = f1;
            f1 = edges.next().unwrap_or(hi);
            let center = (f0 * f1).sqrt();

            let k0 = (f0 / hz_per_bin).ceil() as usize;
            let k1 = ((f1 / hz_per_bin).floor() as usize).min(last);
            let db = if k0 <= k1 && k0 <= last {
                // Several FFT bins fall in this bar.
                let band = &mags[k0..=k1];
                match tuning.bar_value {
                    BarValue::Loudest => band.iter().copied().fold(f32::NEG_INFINITY, f32::max),
                    // Average the power, not the dB values.
                    BarValue::Average => {
                        let power: f32 = band.iter().map(|db| 10f32.powf(db / 10.0)).sum();
                        10.0 * (power / band.len() as f32).max(1e-12).log10()
                    }
                }
            } else {
                // Narrower than one FFT bin (low frequencies): interpolate at the center.
                let x = (center / hz_per_bin).min(last as f32);
                let a = x.floor() as usize;
                let b = (a + 1).min(last);
                let t = x - a as f32;
                mags[a] * (1.0 - t) + mags[b] * t
            };

            // Tilt pivots at 1 kHz: positive values lift the treble.
            let db = db + tilt * (center / 1000.0).log2();
            self.loudest_db = Some(self.loudest_db.map_or(db, |l| l.max(db)));
            // 4. Normalize: floor..ceiling → 0..1, then shape with the contrast curve.
            let level = ((db - floor) / span).clamp(0.0, 1.0).powf(exponent);
            // 5. Band EQ as a linear gain on the normalized level.
            let eq = 10f32.powf(eq_gain_db(hz_to_log_pos(center), bands) / 20.0);
            self.targets.push(level * eq);
        }
    }

    /// Blur within each channel's run of values, never across channels.
    fn spread(&mut self, spread: u8, channels: usize) {
        let sigma = spread as f32 / 100.0 * 2.5;
        if sigma < 0.05 || self.targets.len() < 3 {
            return;
        }
        let run = (self.targets.len() / channels).max(1) as isize;
        let radius = (sigma * 3.0).ceil() as isize;
        let kernel: Vec<f32> = (-radius..=radius)
            .map(|d| (-0.5 * (d as f32 / sigma).powi(2)).exp())
            .collect();
        let n = self.targets.len() as isize;
        self.scratch.clear();
        for i in 0..n {
            let lo = i / run * run;
            let span = lo..(lo + run).min(n);
            let (mut sum, mut wsum) = (0.0, 0.0);
            for (j, w) in (-radius..=radius).zip(&kernel) {
                let k = i + j;
                if span.contains(&k) {
                    sum += self.targets[k as usize] * w;
                    wsum += w;
                }
            }
            self.scratch.push(sum / wsum);
        }
        std::mem::swap(&mut self.targets, &mut self.scratch);
    }
}

// ---------------------------------------------------------------------------
// Noise gate

/// Input at or below this is digital silence, whatever the gate is set to.
const SILENCE_DB: f32 = -110.0;
/// The gate closes this far below its threshold, so a level hovering right at the
/// threshold doesn't flicker on and off.
const GATE_HYSTERESIS_DB: f32 = 3.0;
/// …and only after staying below it this long, so short gaps between notes don't close it.
const GATE_HOLD_S: f32 = 0.25;

/// Decides whether the input is sound or silence (hiss, a paused player).
#[derive(Default)]
pub struct Gate {
    open: bool,
    below_for: f32,
}

impl Gate {
    /// Feed this frame's input level (`None` when no audio arrives) and the gate
    /// threshold from the settings; returns whether there's sound to show.
    pub fn update(&mut self, level_db: Option<f32>, threshold_db: i8, dt: f32) -> bool {
        let threshold = if threshold_db <= GATE_OFF {
            SILENCE_DB
        } else {
            (threshold_db as f32).max(SILENCE_DB)
        };
        match level_db {
            None => {
                self.open = false;
                self.below_for = 0.0;
            }
            Some(db) if db >= threshold => {
                self.open = true;
                self.below_for = 0.0;
            }
            Some(db) if self.open => {
                let close_at = if threshold_db <= GATE_OFF {
                    threshold
                } else {
                    threshold - GATE_HYSTERESIS_DB
                };
                if db < close_at {
                    self.below_for += dt;
                    if self.below_for >= GATE_HOLD_S {
                        self.open = false;
                    }
                } else {
                    self.below_for = 0.0;
                }
            }
            Some(_) => {}
        }
        self.open
    }

    /// Whether the last [`Gate::update`] found sound.
    pub fn is_open(&self) -> bool {
        self.open
    }

    #[cfg(test)]
    pub(crate) fn set_open(&mut self, open: bool) {
        self.open = open;
    }
}

// ---------------------------------------------------------------------------
// Beat glow

/// Bass glow: low-end band, in Hz.
const BASS_BAND: (f32, f32) = (30.0, 150.0);
/// Drum onsets: kick band and snare/hat band, in Hz.
const KICK_BAND: (f32, f32) = (40.0, 150.0);
const SNARE_BAND: (f32, f32) = (1500.0, 6000.0);
/// At the default beat sensitivity, a hit must exceed the recent average flux by
/// this factor (see [`onset_ratio`]).
const ONSET_RATIO: f32 = 1.6;
/// Minimum time between drum hits, in seconds.
const ONSET_REFRACTORY: f32 = 0.11;

/// Turns the spectrum into a 0..=1 "pulse" for the beat glow.
#[derive(Default)]
pub struct BeatGlow {
    pulse: f32,
    // Bass mode
    bass_avg: f32,
    bass_peak: f32,
    // Drums mode
    prev: Vec<f32>,
    flux_avg: [f32; 2],
    since_hit: f32,
}

/// Beat sensitivity (0..=100) → how far a drum hit must rise above the recent
/// average: 2.4× at 0, 1.6× at the default 50, 1.25× at 100.
fn onset_ratio(sensitivity: u8) -> f32 {
    1.0 + (ONSET_RATIO - 1.0) * 2f32.powf((50.0 - sensitivity as f32) / 40.0)
}

/// Beat sensitivity applied to a 0..=1 bass level: below 50 it takes a stronger
/// swell to register (up to half the range is ignored), above 50 swells count for
/// more (up to 2×).
fn bass_response(x: f32, sensitivity: u8) -> f32 {
    let s = sensitivity as f32;
    if s <= 50.0 {
        let threshold = 0.5 * (50.0 - s) / 50.0;
        ((x - threshold) / (1.0 - threshold)).max(0.0)
    } else {
        (x * (1.0 + (s - 50.0) / 50.0)).min(1.0)
    }
}

fn band_bins(frame: &SpectrumFrame<'_>, (lo, hi): (f32, f32)) -> std::ops::Range<usize> {
    let hz = frame.sample_rate / frame.fft_size as f32;
    let last = frame.magnitudes_db.len();
    let a = ((lo / hz).floor() as usize).clamp(1, last);
    let b = ((hi / hz).ceil() as usize + 1).clamp(a, last);
    a..b
}

impl BeatGlow {
    /// The current pulse, 0..=1: 0 between beats, jumping up on a hit and fading.
    pub fn pulse(&self) -> f32 {
        self.pulse
    }

    #[cfg(test)]
    pub(crate) fn set_pulse(&mut self, pulse: f32) {
        self.pulse = pulse;
    }

    /// Run one frame (`None` is silence). `sensitivity` is the beat
    /// sensitivity, 0..=100.
    pub fn update(
        &mut self,
        frame: Option<&SpectrumFrame<'_>>,
        source: GlowSource,
        sensitivity: u8,
        dt: f32,
    ) -> f32 {
        let dt = dt.clamp(0.0, 0.25);
        let Some(frame) = frame else {
            self.pulse *= (-dt / 0.2).exp();
            self.prev.clear();
            return self.pulse;
        };
        match source {
            GlowSource::Bass => self.update_bass(frame, sensitivity, dt),
            GlowSource::Drums => self.update_drums(frame, sensitivity, dt),
        }
        self.pulse
    }

    /// Follows the low end relative to its own recent range, so it works at any volume.
    fn update_bass(&mut self, frame: &SpectrumFrame<'_>, sensitivity: u8, dt: f32) {
        let mags = frame.magnitudes_db;
        let range = band_bins(frame, BASS_BAND);
        let power: f32 = mags[range.clone()]
            .iter()
            .map(|db| 10f32.powf(db / 10.0))
            .sum();
        let db = 10.0 * (power / range.len().max(1) as f32).max(1e-12).log10();

        if self.bass_avg == 0.0 {
            self.bass_avg = db;
            self.bass_peak = db;
        }
        self.bass_avg += (db - self.bass_avg) * (1.0 - (-dt / 1.5).exp());
        self.bass_peak = db.max(self.bass_peak - dt * 4.0); // falls 4 dB/s

        let target = if db < -70.0 {
            0.0
        } else {
            let x =
                ((db - self.bass_avg) / (self.bass_peak - self.bass_avg).max(3.0)).clamp(0.0, 1.0);
            bass_response(x, sensitivity).powf(1.5)
        };
        let tau = if target > self.pulse { 0.02 } else { 0.22 };
        self.pulse += (target - self.pulse) * (1.0 - (-dt / tau).exp());
    }

    /// Spectral flux onsets in the kick and snare bands.
    fn update_drums(&mut self, frame: &SpectrumFrame<'_>, sensitivity: u8, dt: f32) {
        let ratio = onset_ratio(sensitivity);
        let mags = frame.magnitudes_db;
        if self.prev.len() != mags.len() {
            self.prev = mags.iter().map(|db| 10f32.powf(db / 20.0)).collect();
            return;
        }
        let mut hit = 0.0f32;
        for (i, band) in [KICK_BAND, SNARE_BAND].into_iter().enumerate() {
            let range = band_bins(frame, band);
            let n = range.len().max(1) as f32;
            let flux: f32 = range
                .map(|k| (10f32.powf(mags[k] / 20.0) - self.prev[k]).max(0.0))
                .sum::<f32>()
                / n;
            let avg = &mut self.flux_avg[i];
            if flux > *avg * ratio && flux > 1e-5 {
                // Snare hits count a little less than kicks.
                let weight = if i == 0 { 1.0 } else { 0.75 };
                hit =
                    hit.max(weight * (0.6 + 0.4 * ((flux / avg.max(1e-9) - ratio) / 3.0).min(1.0)));
            }
            *avg += (flux - *avg) * (1.0 - (-dt / 0.6).exp());
        }
        for (p, db) in self.prev.iter_mut().zip(mags) {
            *p = 10f32.powf(db / 20.0);
        }

        self.since_hit += dt;
        if hit > 0.0 && self.since_hit >= ONSET_REFRACTORY {
            self.pulse = self.pulse.max(hit);
            self.since_hit = 0.0;
        } else {
            self.pulse *= (-dt / 0.15).exp();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beat_sensitivity_curves() {
        assert!((onset_ratio(50) - ONSET_RATIO).abs() < 1e-6);
        assert!(onset_ratio(0) > onset_ratio(50) && onset_ratio(50) > onset_ratio(100));
        assert_eq!(bass_response(0.3, 0), 0.0);
        assert_eq!(bass_response(0.3, 50), 0.3);
        assert!((bass_response(0.3, 100) - 0.6).abs() < 1e-6);
    }

    #[test]
    fn sensitivity_range() {
        assert!((sensitivity_gain(0) - 0.35).abs() < 1e-6);
        assert!((sensitivity_gain(100) - 1.0).abs() < 1e-6);
        assert_eq!(sensitivity_for_gain(sensitivity_gain(72)), 72);
    }
}

//! Audio capture and spectrum analysis.
//!
//! A [`Capture`] records a [`Source`] (system audio, an output device or an input)
//! on cpal's audio thread and hands the samples over through a lock-free ring
//! buffer. An [`Analyzer`] keeps a history of those samples and turns the latest
//! window into a magnitude spectrum in dBFS, for the mono mix and, on request, for
//! the left, right or side [`Signal`].
//!
//! # Examples
//!
//! Analyzing audio that comes from somewhere other than a live capture:
//!
//! ```
//! use undavi::audio::{Analyzer, Signal};
//!
//! let mut analyzer = Analyzer::default();
//! // One second of a 1 kHz tone, as interleaved left/right pairs.
//! let tone: Vec<f32> = (0..48_000)
//!     .flat_map(|i| {
//!         let s = (std::f32::consts::TAU * 1000.0 * i as f32 / 48_000.0).sin();
//!         [s, s]
//!     })
//!     .collect();
//! analyzer.push(&tone);
//! analyzer.compute(&[], 0);
//!
//! // The loudest bin is the one closest to 1 kHz.
//! let spectrum = analyzer.spectrum(Signal::Mix);
//! let loudest = (0..spectrum.len()).max_by(|&a, &b| spectrum[a].total_cmp(&spectrum[b]));
//! let hz_per_bin = 48_000.0 / analyzer.size() as f32;
//! assert_eq!(loudest, Some((1000.0 / hz_per_bin).round() as usize));
//! ```

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use realfft::{RealFftPlanner, RealToComplex};
use ringbuf::traits::{Consumer, Observer, Producer, Split};
use ringbuf::{HeapCons, HeapRb};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as apps;
#[cfg(windows)]
pub(crate) mod windows;
#[cfg(windows)]
use self::windows as apps;

/// The default FFT size (samples per analysis window).
pub const FFT_SIZE: usize = 4096;
/// The largest FFT size an [`Analyzer`] supports.
pub const MAX_FFT_SIZE: usize = 8192;
/// Samples kept per side: the largest FFT plus the longest visual delay
/// (500 ms) at up to 192 kHz.
const HISTORY: usize = 1 << 17;
/// The input level is measured over this many of the latest samples (~40 ms at 48 kHz).
const LEVEL_WINDOW: usize = 2048;

/// Something to capture: an entry in the source picker.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Source {
    /// Stable identifier: the `DeviceId` as text for devices, the app key for
    /// [`SourceKind::App`], empty for [`SourceKind::SystemOutput`] and
    /// [`SourceKind::Player`].
    pub id: String,
    /// Name to show, e.g. "System Audio" or the device's description.
    pub label: String,
    /// What kind of source this is.
    pub kind: SourceKind,
}

/// The kinds of [`Source`], in the order the picker lists them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    /// Whatever is playing on the default output (loopback / sink monitor).
    SystemOutput,
    /// Whichever app the music player is (see [`Capture::follow_player`]), so
    /// other sounds don't move the bars. See [`has_app_capture`].
    Player,
    /// One app (all of its playback streams). See [`has_app_capture`].
    App,
    /// What's playing on a specific output device.
    Output,
    /// A microphone or other input.
    Input,
}

impl Source {
    /// The default entry: what's playing (where the host can capture it), or else
    /// the default input, labeled as such.
    pub fn system(host: &cpal::Host) -> Self {
        Self {
            id: String::new(),
            label: if has_loopback(host) {
                "System Audio"
            } else {
                "Default input"
            }
            .into(),
            kind: SourceKind::SystemOutput,
        }
    }
}

/// Whether `host` can capture what's playing (PipeWire monitors, WASAPI loopback).
/// Elsewhere (e.g. macOS, plain ALSA) "System Audio" can only be the default input.
pub fn has_loopback(host: &cpal::Host) -> bool {
    #[cfg(target_os = "linux")]
    if host.id() == cpal::HostId::PipeWire {
        return true;
    }
    let _ = host;
    cfg!(windows)
}

/// Lists sources for the picker. The first entry is always [`Source::system`].
pub fn list_sources(host: &cpal::Host) -> Vec<Source> {
    let mut out = vec![Source::system(host)];
    let Ok(devices) = host.devices() else {
        return out;
    };
    let mut outputs = Vec::new();
    let mut inputs = Vec::new();
    for device in devices {
        let (Ok(desc), Ok(id)) = (device.description(), device.id()) else {
            continue;
        };
        let name = desc.name().to_string();
        // Nodes without a description (e.g. other apps' recording streams) aren't useful here.
        if name.is_empty() || name == "unknown" {
            continue;
        }
        // PipeWire exposes synthetic "default_*" devices; System Audio already covers the sink.
        if name == "default_sink" || name == "default_output" {
            continue;
        }
        let id = id.to_string();
        if name == "default_input" {
            inputs.insert(
                0,
                Source {
                    id,
                    label: "Default input".into(),
                    kind: SourceKind::Input,
                },
            );
            continue;
        }
        // "What's playing on X": a sink whose monitor we can capture (PipeWire), or an
        // output we can open as a loopback (WASAPI).
        let monitored = desc.supports_output() && (desc.supports_input() || cfg!(windows));
        if monitored {
            outputs.push(Source {
                id,
                label: name,
                kind: SourceKind::Output,
            });
        } else if desc.supports_input() {
            inputs.push(Source {
                id,
                label: name,
                kind: SourceKind::Input,
            });
        }
    }
    out.extend(app_sources(host));
    out.extend(outputs);
    out.extend(inputs);
    out
}

/// "Music player" and every app playing right now, where single apps can be
/// captured.
fn app_sources(host: &cpal::Host) -> Vec<Source> {
    #[cfg(any(target_os = "linux", windows))]
    if has_app_capture(host) {
        let mut out = vec![Source {
            id: String::new(),
            label: "Music player".into(),
            kind: SourceKind::Player,
        }];
        out.extend(apps::playing_apps().into_iter().map(|a| Source {
            id: a.key,
            label: a.label,
            kind: SourceKind::App,
        }));
        return out;
    }
    let _ = host;
    Vec::new()
}

/// Whether single apps can be captured with this host: PipeWire, or WASAPI on
/// Windows 10 version 2004 and newer.
pub fn has_app_capture(host: &cpal::Host) -> bool {
    #[cfg(target_os = "linux")]
    if host.id() == cpal::HostId::PipeWire {
        return true;
    }
    #[cfg(windows)]
    if host.id() == cpal::HostId::Wasapi {
        return apps::supported();
    }
    let _ = host;
    false
}

/// An app with at least one playback stream.
#[cfg(any(target_os = "linux", windows))]
#[derive(Clone, Debug, PartialEq, Eq)]
struct PlayingApp {
    /// Stable identifier: the app's name in lowercase.
    key: String,
    /// Name to show, e.g. "Spotify".
    label: String,
}

/// What an app capture records.
#[cfg(any(target_os = "linux", windows))]
#[derive(Clone, Debug, PartialEq, Eq)]
enum Target {
    /// Nothing (yet).
    None,
    /// Every stream of the app with this key.
    App(String),
    /// Every stream of the app that matches this player (see [`player_matches`]).
    Player {
        /// What the player calls itself to the system, as in
        /// [`Track::player_bus`](crate::media::Track::player_bus).
        bus: String,
        /// The player's display name, e.g. "Mozilla Firefox".
        identity: String,
    },
}

/// Lowercase letters and digits only: "Mozilla Firefox" → "mozillafirefox".
fn squash(s: &str) -> String {
    s.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Whether a player (`bus` and `identity`: see
/// [`Track::player_bus`](crate::media::Track::player_bus) and
/// [`Track::player_name`](crate::media::Track::player_name)) is the app with these
/// names. Players and streams name themselves inconsistently ("firefox" vs
/// "Firefox", "chromium.instance42" vs "Google Chrome" / "chrome"), so this
/// compares squashed names, allowing one to contain the other.
pub fn player_matches(bus: &str, identity: &str, app_names: &[String]) -> bool {
    // Every meaningful part of the bus name: not instance numbers, and not the
    // domain parts of reverse-DNS names (`org.kde.elisa`, `io.github.….Celluloid`).
    const GENERIC: [&str; 9] = [
        "org", "com", "net", "io", "app", "github", "gitlab", "kde", "gnome",
    ];
    let parts = bus.split('.').filter(|p| {
        !p.starts_with("instance") && !GENERIC.contains(&p.to_ascii_lowercase().as_str())
    });
    let player: Vec<String> = parts
        .chain([identity])
        .map(squash)
        .filter(|s| s.len() >= 3)
        .collect();
    app_names
        .iter()
        .map(|n| squash(n))
        .filter(|a| a.len() >= 3)
        .any(|a| {
            player
                .iter()
                .any(|p| *p == a || p.contains(a.as_str()) || a.contains(p.as_str()))
        })
}

fn find_device(host: &cpal::Host, source: &Source) -> Option<cpal::Device> {
    if source.kind == SourceKind::SystemOutput {
        // PipeWire: capture the default sink's monitor.
        #[cfg(target_os = "linux")]
        if host.id() == cpal::HostId::PipeWire {
            let id = cpal::DeviceId::new(cpal::HostId::PipeWire, "sink_default");
            if let Some(d) = host.device_by_id(&id) {
                return Some(d);
            }
        }
        // WASAPI: an input stream on an output device is a loopback capture.
        #[cfg(windows)]
        if let Some(d) = host.default_output_device() {
            return Some(d);
        }
        // Elsewhere (e.g. macOS) there is no loopback; fall back to the default input.
        return host.default_input_device();
    }
    host.devices().ok()?.find(|d| {
        d.id()
            .map(|id| id.to_string() == source.id)
            .unwrap_or(false)
    })
}

/// Fold one frame of `channels` samples into a left/right pair. Mono is copied to
/// both sides. Channels past the first two (center, LFE, surrounds) are shared
/// equally, so the average of the pair is the plain mono mix of every channel.
fn stereo_pair(frame: impl Iterator<Item = f32>) -> (f32, f32) {
    let (mut l, mut r, mut rest, mut count) = (0.0, 0.0, 0.0, 0usize);
    for (i, s) in frame.enumerate() {
        match i {
            0 => l = s,
            1 => r = s,
            _ => rest += s,
        }
        count += 1;
    }
    match count {
        0 => (0.0, 0.0),
        1 => (l, l),
        n => {
            let n = n as f32;
            ((2.0 * l + rest) / n, (2.0 * r + rest) / n)
        }
    }
}

/// Whether a stream error means the stream is gone and must be opened again.
/// Glitches (xruns), refused real-time scheduling and "rerouted to the new default
/// device" leave a stream running.
fn is_fatal(kind: cpal::ErrorKind) -> bool {
    !matches!(
        kind,
        cpal::ErrorKind::Xrun | cpal::ErrorKind::RealtimeDenied | cpal::ErrorKind::DeviceChanged
    )
}

/// What a [`Capture`] records from.
enum Backend {
    /// A device, through cpal (kept alive; dropping it stops the stream).
    Device(#[allow(dead_code)] cpal::Stream),
    /// One app, or the player's app.
    #[cfg(any(target_os = "linux", windows))]
    App(apps::AppCapture),
}

/// A running capture stream. Dropping it stops capture.
///
/// Samples are folded to a left/right pair (see the module docs) and buffered for
/// about a second; drain them every frame with [`Analyzer::pull`].
pub struct Capture {
    backend: Backend,
    consumer: HeapCons<f32>,
    sample_rate: u32,
    /// Set from the audio thread when the stream dies (device unplugged, server restarted).
    dead: Arc<AtomicBool>,
}

impl Capture {
    /// Open `source` on `host` and start recording.
    ///
    /// # Errors
    ///
    /// Returns a readable message when the device is gone, has no usable stream
    /// configuration, or refuses to open or start the stream.
    pub fn start(host: &cpal::Host, source: &Source) -> Result<Self, String> {
        if matches!(source.kind, SourceKind::App | SourceKind::Player) {
            return Self::start_app(source);
        }
        let device = find_device(host, source).ok_or("audio device not found")?;
        let supported = device
            .default_input_config()
            .or_else(|_| device.default_output_config())
            .map_err(|e| format!("no usable stream config: {e}"))?;
        let channels = supported.channels().max(1) as usize;
        let sample_rate = supported.sample_rate();
        let config = cpal::StreamConfig {
            channels: channels as u16,
            sample_rate,
            buffer_size: cpal::BufferSize::Default,
        };

        // About 1 s of audio as interleaved left/right pairs; the UI drains it every frame.
        let rb = HeapRb::<f32>::new(
            2 * (sample_rate as usize)
                .next_power_of_two()
                .max(MAX_FFT_SIZE * 2),
        );
        let (mut producer, consumer) = rb.split();
        let mut pairs: Vec<f32> = Vec::with_capacity(8192);
        let dead = Arc::new(AtomicBool::new(false));
        let on_error = {
            let dead = dead.clone();
            move |e: cpal::Error| {
                if is_fatal(e.kind()) {
                    if !dead.swap(true, Ordering::Relaxed) {
                        eprintln!("undavi: audio stream lost: {e}");
                    }
                } else if e.kind() != cpal::ErrorKind::Xrun {
                    // Xruns (a dropped buffer now and then) are routine; not worth a line each.
                    eprintln!("undavi: audio stream error: {e}");
                }
            }
        };

        macro_rules! build {
            ($t:ty, $to_f32:expr) => {
                device.build_input_stream::<$t, _, _>(
                    config,
                    move |data: &[$t], _| {
                        pairs.clear();
                        for frame in data.chunks(channels) {
                            let (l, r) = stereo_pair(frame.iter().map(|&s| $to_f32(s)));
                            pairs.push(l);
                            pairs.push(r);
                        }
                        // If the UI falls behind, newest samples are dropped; fine for a visualizer.
                        // Only whole pairs go in, so left and right never swap.
                        let room = producer.vacant_len() & !1;
                        producer.push_slice(&pairs[..pairs.len().min(room)]);
                    },
                    on_error,
                    None,
                )
            };
        }

        let stream = match supported.sample_format() {
            cpal::SampleFormat::I16 => build!(i16, |s: i16| s as f32 / 32768.0),
            cpal::SampleFormat::I32 => build!(i32, |s: i32| s as f32 / 2_147_483_648.0),
            _ => build!(f32, |s: f32| s),
        }
        .map_err(|e| format!("could not open stream: {e}"))?;
        stream
            .play()
            .map_err(|e| format!("could not start stream: {e}"))?;

        Ok(Self {
            backend: Backend::Device(stream),
            consumer,
            sample_rate,
            dead,
        })
    }

    /// Capture one app, or (for [`SourceKind::Player`]) nothing until
    /// [`Capture::follow_player`] says which app the player is.
    #[cfg(any(target_os = "linux", windows))]
    fn start_app(source: &Source) -> Result<Self, String> {
        let rb = HeapRb::<f32>::new(2 * (apps::RATE as usize).next_power_of_two());
        let (mut producer, consumer) = rb.split();
        let dead = Arc::new(AtomicBool::new(false));
        let target = match source.kind {
            SourceKind::App => Target::App(source.id.clone()),
            _ => Target::None,
        };
        let capture = apps::AppCapture::start(
            target,
            move |pairs: &[f32]| {
                let room = producer.vacant_len() & !1;
                producer.push_slice(&pairs[..pairs.len().min(room)]);
            },
            dead.clone(),
        )?;
        Ok(Self {
            backend: Backend::App(capture),
            consumer,
            sample_rate: apps::RATE,
            dead,
        })
    }

    #[cfg(not(any(target_os = "linux", windows)))]
    fn start_app(_source: &Source) -> Result<Self, String> {
        Err("capturing single apps isn't supported on this platform".into())
    }

    /// For a [`SourceKind::Player`] capture: record the app behind this player (its
    /// [`Track::player_bus`](crate::media::Track::player_bus) and
    /// [`Track::player_name`](crate::media::Track::player_name)), or nothing for
    /// `None`. Cheap to call every frame; does nothing for other captures.
    pub fn follow_player(&mut self, player: Option<(&str, &str)>) {
        #[cfg(any(target_os = "linux", windows))]
        if let Backend::App(capture) = &mut self.backend {
            capture.set_target(match player {
                Some((bus, identity)) => Target::Player {
                    bus: bus.to_string(),
                    identity: identity.to_string(),
                },
                None => Target::None,
            });
        }
        let _ = player;
    }

    /// For app captures, how many of the app's playback streams are being
    /// recorded (0 while it isn't playing); `None` for devices.
    pub fn app_streams(&self) -> Option<usize> {
        match &self.backend {
            Backend::Device(_) => None,
            #[cfg(any(target_os = "linux", windows))]
            Backend::App(capture) => Some(capture.linked()),
        }
    }

    /// Samples per second (per channel) of the stream.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// The stream reported that it stopped for good; open a new one.
    pub fn is_dead(&self) -> bool {
        self.dead.load(Ordering::Relaxed)
    }
}

/// A signal the analyzer can turn into a spectrum.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Signal {
    /// The mono mix (also the "mid" of mid/side), (left + right) / 2.
    Mix,
    /// The left channel.
    Left,
    /// The right channel.
    Right,
    /// What differs between the channels, (left − right) / 2.
    Side,
}

/// Keeps a history of left/right samples and turns a window of them into spectra.
///
/// Feed it with [`Analyzer::pull`] (from a [`Capture`]) or [`Analyzer::push`], then
/// call [`Analyzer::compute`] once per frame and read the results with
/// [`Analyzer::spectrum`] and [`Analyzer::level_db`].
pub struct Analyzer {
    /// Ring buffers of `HISTORY` samples, one per side.
    left: Vec<f32>,
    right: Vec<f32>,
    /// Half a pair left over from the last drain (not expected, but keeps sides aligned).
    pending: Option<f32>,
    write: usize,
    size: usize,
    window: Vec<f32>,
    window_sum: f32,
    fft: Arc<dyn RealToComplex<f32>>,
    input: Vec<f32>,
    output: Vec<realfft::num_complex::Complex<f32>>,
    scratch: Vec<realfft::num_complex::Complex<f32>>,
    drain: Vec<f32>,
    /// Spectrum of the mono mix.
    magnitudes_db: Vec<f32>,
    /// Other signals' spectra, filled by [`Analyzer::compute`] when asked for.
    left_db: Vec<f32>,
    right_db: Vec<f32>,
    side_db: Vec<f32>,
    /// RMS level of the mix at the end of the last analyzed window, in dBFS.
    level_db: f32,
}

impl Default for Analyzer {
    fn default() -> Self {
        Self::new(FFT_SIZE)
    }
}

impl Analyzer {
    /// An analyzer with FFT windows of `size` samples (a power of two, at most
    /// [`MAX_FFT_SIZE`]). Larger windows resolve low notes better but react later.
    ///
    /// # Panics
    ///
    /// Panics if `size` is larger than [`MAX_FFT_SIZE`] or smaller than 2.
    pub fn new(size: usize) -> Self {
        assert!(
            (2..=MAX_FFT_SIZE).contains(&size),
            "FFT size {size} out of range"
        );
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(size);
        // Hann window.
        let window: Vec<f32> = (0..size)
            .map(|i| {
                let x = i as f32 / (size - 1) as f32;
                0.5 - 0.5 * (std::f32::consts::TAU * x).cos()
            })
            .collect();
        let window_sum = window.iter().sum();
        Self {
            left: vec![0.0; HISTORY],
            right: vec![0.0; HISTORY],
            pending: None,
            write: 0,
            size,
            window,
            window_sum,
            input: fft.make_input_vec(),
            output: fft.make_output_vec(),
            scratch: fft.make_scratch_vec(),
            fft,
            drain: vec![0.0; 8192],
            magnitudes_db: vec![-120.0; size / 2 + 1],
            left_db: vec![-120.0; size / 2 + 1],
            right_db: vec![-120.0; size / 2 + 1],
            side_db: vec![-120.0; size / 2 + 1],
            level_db: -140.0,
        }
    }

    /// The FFT size, in samples. Spectra have `size / 2 + 1` bins.
    pub fn size(&self) -> usize {
        self.size
    }

    /// Switch FFT size, keeping the captured history.
    ///
    /// # Panics
    ///
    /// Like [`Analyzer::new`], for a size out of range.
    pub fn set_size(&mut self, size: usize) {
        if size != self.size {
            let left = std::mem::take(&mut self.left);
            let right = std::mem::take(&mut self.right);
            let (write, pending) = (self.write, self.pending);
            *self = Self::new(size);
            self.left = left;
            self.right = right;
            self.write = write;
            self.pending = pending;
        }
    }

    /// Move everything `capture` recorded since the last call into the history.
    /// Returns `false` if nothing arrived.
    pub fn pull(&mut self, capture: &mut Capture) -> bool {
        let mut got = false;
        while capture.consumer.occupied_len() > 0 {
            let n = capture.consumer.pop_slice(&mut self.drain);
            if n == 0 {
                break;
            }
            got = true;
            let drain = std::mem::take(&mut self.drain);
            self.push(&drain[..n]);
            self.drain = drain;
        }
        got
    }

    /// Add samples to the history: interleaved left/right pairs, -1.0..=1.0.
    /// [`Analyzer::pull`] does this for a [`Capture`]; call it directly to analyze
    /// audio from anywhere else.
    pub fn push(&mut self, interleaved: &[f32]) {
        for &s in interleaved {
            match self.pending.take() {
                None => self.pending = Some(s),
                Some(l) => {
                    self.left[self.write] = l;
                    self.right[self.write] = s;
                    self.write = (self.write + 1) % HISTORY;
                }
            }
        }
    }

    /// Window + FFT the samples ending `delay` samples ago into `magnitudes_db`
    /// (dBFS, 0 dB = full-scale sine), plus each of `signals` other than the mix
    /// (read them back with [`Analyzer::spectrum`]). Also measures the input level.
    pub fn compute(&mut self, signals: &[Signal], delay: usize) {
        let delay = delay.min(HISTORY - self.size);
        self.measure_level(delay);
        self.fft(Signal::Mix, delay);
        for &signal in signals {
            if signal != Signal::Mix {
                self.fft(signal, delay);
            }
        }
    }

    /// The spectrum of `signal` from the last [`Analyzer::compute`] that asked for it.
    pub fn spectrum(&self, signal: Signal) -> &[f32] {
        match signal {
            Signal::Mix => &self.magnitudes_db,
            Signal::Left => &self.left_db,
            Signal::Right => &self.right_db,
            Signal::Side => &self.side_db,
        }
    }

    /// RMS level of the mix from the last [`Analyzer::compute`], in dBFS.
    pub fn level_db(&self) -> f32 {
        self.level_db
    }

    /// Sample of `signal` at ring position `k`.
    fn sample(&self, signal: Signal, k: usize) -> f32 {
        let (l, r) = (self.left[k], self.right[k]);
        match signal {
            Signal::Mix => (l + r) * 0.5,
            Signal::Left => l,
            Signal::Right => r,
            Signal::Side => (l - r) * 0.5,
        }
    }

    fn measure_level(&mut self, delay: usize) {
        let n = LEVEL_WINDOW.min(self.size);
        let end = self.write + HISTORY - delay;
        let sum: f32 = (end - n..end)
            .map(|i| self.sample(Signal::Mix, i % HISTORY).powi(2))
            .sum();
        // RMS relative to a full-scale sine (whose RMS is 1/√2), to match the spectrum.
        let rms = (sum / n as f32).sqrt() * std::f32::consts::SQRT_2;
        self.level_db = 20.0 * rms.max(1e-7).log10();
    }

    fn fft(&mut self, signal: Signal, delay: usize) {
        let start = self.write + 2 * HISTORY - self.size - delay;
        for i in 0..self.size {
            let s = self.sample(signal, (start + i) % HISTORY);
            self.input[i] = s * self.window[i];
        }
        if self
            .fft
            .process_with_scratch(&mut self.input, &mut self.output, &mut self.scratch)
            .is_err()
        {
            return;
        }
        let scale = 2.0 / self.window_sum;
        let out = match signal {
            Signal::Mix => &mut self.magnitudes_db,
            Signal::Left => &mut self.left_db,
            Signal::Right => &mut self.right_db,
            Signal::Side => &mut self.side_db,
        };
        for (m, c) in out.iter_mut().zip(&self.output) {
            let amp = c.norm() * scale;
            *m = 20.0 * amp.max(1e-7).log10();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channels_fold_into_a_pair() {
        assert_eq!(stereo_pair([0.5].into_iter()), (0.5, 0.5));
        assert_eq!(stereo_pair([0.2, -0.4].into_iter()), (0.2, -0.4));
        // 5.1: front L/R stay on their sides, the rest is shared; the pair still
        // averages to the mono mix.
        let frame = [0.6, 0.0, 0.3, 0.3, 0.0, 0.0];
        let (l, r) = stereo_pair(frame.into_iter());
        assert!((l - 0.3).abs() < 1e-6 && (r - 0.1).abs() < 1e-6, "{l} {r}");
        assert!(((l + r) / 2.0 - frame.iter().sum::<f32>() / 6.0).abs() < 1e-6);
    }
}

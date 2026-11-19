//! Everything the user can set, the presets, and the settings file.
//!
//! Settings come in three groups, one per panel:
//!
//! - [`TuningSettings`]: how the bars react to sound.
//! - [`VisualSettings`]: how the bars look.
//! - [`AppearanceSettings`]: background, track info, lyrics and the window.
//!
//! Each panel also keeps its own list of named presets ([`Preset`],
//! [`TuningPreset`], [`AppearancePreset`]). All of it is saved as one JSON file,
//! [`Stored`], at [`config_path`]; [`load`] and [`save`] read and write it.
//!
//! # Compatibility
//!
//! Every struct uses `#[serde(default)]`, so a field missing from an older file
//! gets its default. Renamed fields and values keep their old names as serde
//! aliases. Changes that need more than that bump [`CONFIG_VERSION`] and add a
//! step to [`migrate`]. Values read from a file are always brought back into
//! range (the `clamped` methods), so a hand-edited file can't break the app.
//!
//! # Examples
//!
//! ```
//! use undavi::settings::{Stored, migrate};
//!
//! // A file from an older version: "smoothing" is now called "spread", and
//! // out-of-range values are fixed on load.
//! let old: Stored = serde_json::from_str(r#"{"tuning": {"smoothing": 30, "sensitivity": 250}}"#)?;
//! let settings = migrate(old);
//! assert_eq!(settings.tuning.spread, 30);
//! assert_eq!(settings.tuning.sensitivity, 100);
//! # Ok::<(), serde_json::Error>(())
//! ```

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// How the bars react to sound (the Tuning panel).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TuningSettings {
    /// Overall bar height, 0..=100 (see [`crate::spectrum::sensitivity_gain`]).
    pub sensitivity: u8,
    /// Blends each bar with its neighbors, 0..=100. Saved as "smoothing" by older versions.
    #[serde(alias = "smoothing")]
    pub spread: u8,
    /// How fast bars rise to a louder level (time constant), 5..=100 ms.
    pub rise_ms: u16,
    /// How fast bars fall back (time constant), 50..=800 ms in steps of 10.
    pub fall_ms: u16,
    /// Low end of the frequency range, as a slider position 0..=100 on the
    /// 20 Hz–20 kHz log scale (see [`slider_to_hz`]).
    pub freq_lo: u8,
    /// High end of the frequency range, 0..=100, at least [`FREQ_MIN_GAP`] above `freq_lo`.
    pub freq_hi: u8,
    /// Bass band EQ, -12..=12 dB.
    pub bass_db: i8,
    /// Mids band EQ, -12..=12 dB.
    pub mids_db: i8,
    /// Highs band EQ, -12..=12 dB.
    pub highs_db: i8,
    /// Keep adjusting the gain so quiet and loud passages fill the bars alike.
    pub auto_gain: bool,
    /// How long peak caps hold before falling, 0..=1000 ms (step 50).
    pub peak_hold_ms: u16,
    /// Level mapped to an empty bar, -100..=-20 dB.
    pub floor_db: i8,
    /// Level mapped to a full bar, -80..=0 dB (at least `LEVEL_MIN_SPAN` above the floor).
    pub ceiling_db: i8,
    /// Response curve: 0 = soft (lifts quiet parts), 50 = linear, 100 = punchy.
    pub contrast: u8,
    /// Spectral tilt around 1 kHz, -3..=6 dB per octave (positive lifts the treble).
    pub tilt_db: i8,
    /// FFT size: faster reaction or better low-note resolution.
    pub detail: Detail,
    /// How the frequencies inside one bar are combined.
    pub bar_value: BarValue,
    /// How fast auto-gain follows the music getting louder or quieter.
    pub agc_speed: AdaptSpeed,
    /// How frequencies are spread across the bars.
    pub freq_scale: FreqScale,
    /// Bars and peak caps fall with acceleration instead of easing down.
    pub gravity: bool,
    /// Input below this level counts as silence, -100..=-30 dBFS (-100 = gate off).
    pub gate_db: i8,
    /// What the bars do while it's silent.
    pub when_silent: SilenceMode,
    /// How strong a hit must be to count as a beat (glow and background pulse), 0..=100.
    pub beat_sensitivity: u8,
    /// Hold the visuals back to line up with delayed audio (e.g. Bluetooth), 0..=500 ms.
    pub delay_ms: u16,
}

/// The gate slider's lowest position, which turns the gate off.
pub const GATE_OFF: i8 = -100;
/// Longest visual delay, in ms.
pub const MAX_DELAY_MS: u16 = 500;

/// How fast auto-gain adapts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdaptSpeed {
    /// Recovers over about 8 seconds.
    Slow,
    /// Recovers over about 3 seconds.
    Medium,
    /// Recovers over about a second.
    Fast,
}

impl AdaptSpeed {
    /// Seconds for the gain to recover after a loud passage.
    pub fn release_s(self) -> f32 {
        match self {
            AdaptSpeed::Slow => 8.0,
            AdaptSpeed::Medium => 3.0,
            AdaptSpeed::Fast => 1.0,
        }
    }
}

/// How the frequency range is divided between the bars.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FreqScale {
    /// Equal octaves per bar.
    Log,
    /// The mel scale: close to log, with less room for the extreme highs.
    Mel,
    /// Equal Hz per bar (the bass shares the first few bars).
    Linear,
}

impl FreqScale {
    /// Frequency → position on this scale (any monotonic unit).
    pub fn to_scale(self, hz: f32) -> f32 {
        match self {
            FreqScale::Log => hz.max(1e-3).ln(),
            FreqScale::Mel => 2595.0 * (1.0 + hz / 700.0).log10(),
            FreqScale::Linear => hz,
        }
    }

    /// Inverse of [`FreqScale::to_scale`].
    pub fn to_hz(self, x: f32) -> f32 {
        match self {
            FreqScale::Log => x.exp(),
            FreqScale::Mel => 700.0 * (10f32.powf(x / 2595.0) - 1.0),
            FreqScale::Linear => x,
        }
    }

    /// Band edges for `bars` bars between `lo` and `hi` Hz (`bars + 1` values).
    pub fn edges(self, lo: f32, hi: f32, bars: usize) -> impl Iterator<Item = f32> {
        let (a, b) = (self.to_scale(lo), self.to_scale(hi));
        (0..=bars).map(move |i| self.to_hz(a + (b - a) * i as f32 / bars.max(1) as f32))
    }
}

/// What happens while the input is silent (below the noise gate, or no audio at all).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SilenceMode {
    /// Bars fall to flat.
    Fade,
    /// A slow, low wave keeps moving.
    Wave,
    /// Bars stop where they are and the app stops redrawing (saves battery).
    Freeze,
}

/// FFT size: speed vs. low-frequency precision.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Detail {
    /// 2048 samples (~43 ms at 48 kHz).
    Fast,
    /// 4096 samples (~85 ms).
    Balanced,
    /// 8192 samples (~170 ms).
    Detailed,
}

impl Detail {
    /// The FFT size in samples.
    pub fn fft_size(self) -> usize {
        match self {
            Detail::Fast => 2048,
            Detail::Balanced => 4096,
            Detail::Detailed => 8192,
        }
    }
}

/// How the FFT bins inside one bar's frequency band become its value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BarValue {
    /// The loudest bin: punchy, follows single notes.
    Loudest,
    /// The average power: steadier, less spiky.
    Average,
}

/// Minimum distance between the level floor and ceiling.
pub const LEVEL_MIN_SPAN: i8 = 20;

/// Contrast slider → exponent applied to normalized levels (0.5..=2.0).
pub fn contrast_exponent(contrast: u8) -> f32 {
    2f32.powf((contrast as f32 - 50.0) / 50.0)
}

impl Default for TuningSettings {
    fn default() -> Self {
        Self {
            sensitivity: 72,
            spread: 40,
            rise_ms: 12,
            fall_ms: 180,
            freq_lo: 6,
            freq_hi: 97,
            bass_db: 4,
            mids_db: 0,
            highs_db: -2,
            auto_gain: false,
            peak_hold_ms: 300,
            floor_db: -80,
            ceiling_db: 0,
            contrast: 50,
            tilt_db: 0,
            detail: Detail::Balanced,
            bar_value: BarValue::Loudest,
            agc_speed: AdaptSpeed::Medium,
            freq_scale: FreqScale::Log,
            gravity: false,
            gate_db: -70,
            when_silent: SilenceMode::Fade,
            beat_sensitivity: 50,
            delay_ms: 0,
        }
    }
}

/// Minimum distance between the two frequency-range handles, in steps.
pub const FREQ_MIN_GAP: u8 = 10;

/// Bar corner shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Corners {
    /// Sharp corners.
    Square,
    /// Slightly rounded.
    Rounded,
    /// Fully round ends.
    Pill,
}

/// How the bars are mirrored.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mirror {
    /// Bars grow up from the bottom, bass on the left.
    Off,
    /// Bars grow both ways from a center line.
    UpDown,
    /// Bass in the middle, the spectrum repeated (or split) to both sides.
    LeftRight,
    /// Both of the above.
    Both,
}

impl Mirror {
    /// Bars grow both up and down.
    pub fn up_down(self) -> bool {
        matches!(self, Mirror::UpDown | Mirror::Both)
    }
    /// Bass in the middle with a half on each side.
    pub fn left_right(self) -> bool {
        matches!(self, Mirror::LeftRight | Mirror::Both)
    }
}

/// Which way a multi-color palette runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GradientDir {
    /// From one end of the bars to the other.
    Across,
    /// From the base of each bar to its tip.
    Along,
}

/// Bar colors.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Palette {
    /// One light gray.
    Silver,
    /// The brand gradient, lilac to sky blue. Stored as "blue" by older versions.
    #[serde(rename = "undavi", alias = "blue")]
    Undavi,
    /// Teal, blue and violet.
    Aurora,
    /// Amber, coral and raspberry.
    Ember,
    /// Colors from the album cover (see `album_gradient` / `album_match_accent`).
    Album,
    /// The user's own stops, as RGB (one to three).
    Custom(Vec<[u8; 3]>),
}

impl Palette {
    /// The name shown in the Visuals panel.
    pub fn name(&self) -> &'static str {
        match self {
            Palette::Silver => "Silver",
            Palette::Undavi => "Undavi",
            Palette::Aurora => "Aurora",
            Palette::Ember => "Ember",
            Palette::Album => "From album art",
            Palette::Custom(_) => "Custom",
        }
    }

    /// Fixed stops for this palette. `Album` depends on the cover, so the app
    /// resolves it (`App::bar_stops`); here it falls back to Silver.
    pub fn stops(&self) -> Vec<[u8; 3]> {
        match self {
            Palette::Silver => vec![[0xE4, 0xE4, 0xE7]],
            Palette::Undavi => UNDAVI_GRADIENT.to_vec(),
            Palette::Aurora => vec![[0x34, 0xD3, 0xC4], [0x4B, 0x7B, 0xF5], [0x9B, 0x7B, 0xF7]],
            Palette::Ember => vec![[0xFF, 0xB5, 0x5C], [0xFF, 0x6B, 0x57], [0xD9, 0x3A, 0x6E]],
            Palette::Custom(stops) if !stops.is_empty() => stops.clone(),
            Palette::Custom(_) | Palette::Album => vec![[0xE4, 0xE4, 0xE7]],
        }
    }
}

/// Starting stops when the user first picks the custom swatch.
pub const DEFAULT_CUSTOM: [[u8; 3]; 2] = [[0xFF, 0x7A, 0xC6], [0x7A, 0x9D, 0xF8]];

/// How the bars look (the Visuals panel).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VisualSettings {
    /// Number of bars (or wave points), 16..=96.
    pub bar_count: u8,
    /// Bar (or line) width, 2..=16 px.
    pub bar_width: u8,
    /// Space between bars, 0..=20 px.
    pub spacing: u8,
    /// Length of a full bar, 100..=1200 px in steps of 10 (both halves together when
    /// mirrored up/down). Never more than the window has room for.
    pub max_length: u16,
    /// Bar corner shape.
    pub corners: Corners,
    /// Mirroring.
    pub mirror: Mirror,
    /// Which channel the bars show when both halves show the same spectrum.
    pub channel: Channel,
    /// With Left/right mirroring, give each half its own signal instead of the
    /// same spectrum twice. Older versions saved this as `"stereo": true/false`.
    #[serde(alias = "stereo", deserialize_with = "split_or_bool")]
    pub split: Split,
    /// Draw bars as stacks of segments, like an LED meter.
    pub segments: bool,
    /// Segment height, 2..=16 px.
    pub segment_size: u8,
    /// Gap between segments, 1..=8 px.
    pub segment_gap: u8,
    /// Small caps that mark each bar's recent peak.
    pub peak_caps: bool,
    /// Bar colors.
    pub palette: Palette,
    /// Album palette: a gradient of the cover's colors (or one solid color).
    pub album_gradient: bool,
    /// Album palette with a gradient: build it from the chosen accent instead of
    /// the cover. Only offered when the accent isn't "From album art".
    pub album_match_accent: bool,
    /// Which way a multi-color palette runs.
    pub gradient: GradientDir,
    /// Bars, radial or wave.
    pub style: VisualStyle,
    /// Bars brighten and get a soft halo in time with the music.
    pub glow: bool,
    /// What the glow follows.
    pub glow_source: GlowSource,
    /// 0..=100.
    pub glow_amount: u8,
}

/// The signal behind a single (unsplit) spectrum.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    /// Left and right mixed together.
    Mono,
    /// The left channel only.
    Left,
    /// The right channel only.
    Right,
}

/// What each half shows with Left/right mirroring.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Split {
    /// Both halves show the same spectrum.
    Off,
    /// Left channel on the left, right channel on the right.
    Stereo,
    /// What the channels share (mid) on the left, what differs (side) on the right.
    MidSide,
}

/// Reads `split`, or the `stereo` switch older versions saved in its place.
fn split_or_bool<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Split, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Repr {
        Old(bool),
        New(Split),
    }
    Ok(match Repr::deserialize(d)? {
        Repr::Old(true) => Split::Stereo,
        Repr::Old(false) => Split::Off,
        Repr::New(s) => s,
    })
}

/// How the spectrum is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VisualStyle {
    /// Vertical bars in a row (the original look).
    Bars,
    /// Bars in a ring around the album cover.
    Radial,
    /// A smooth curve through the spectrum.
    Wave,
}

/// What a glow effect (bar glow, background pulse) follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GlowSource {
    /// Follows low-end energy.
    Bass,
    /// Flashes on kick and snare hits.
    Drums,
}

impl Default for VisualSettings {
    fn default() -> Self {
        Self {
            bar_count: 43,
            bar_width: 8,
            spacing: 12,
            max_length: 400,
            corners: Corners::Square,
            mirror: Mirror::UpDown,
            channel: Channel::Mono,
            split: Split::Stereo,
            segments: false,
            segment_size: 6,
            segment_gap: 3,
            peak_caps: false,
            palette: Palette::Undavi,
            album_gradient: true,
            album_match_accent: false,
            gradient: GradientDir::Across,
            style: VisualStyle::Bars,
            glow: false,
            glow_source: GlowSource::Bass,
            glow_amount: 50,
        }
    }
}

impl VisualSettings {
    /// Bring every field back into its documented range (used after loading from disk).
    pub fn clamped(mut self) -> Self {
        self.bar_count = self.bar_count.clamp(16, 96);
        self.bar_width = self.bar_width.clamp(2, 16);
        self.spacing = self.spacing.min(20);
        self.max_length = (self.max_length.clamp(100, 1200) / 10) * 10;
        self.segment_size = self.segment_size.clamp(2, 16);
        self.segment_gap = self.segment_gap.clamp(1, 8);
        self.glow_amount = self.glow_amount.min(100);
        self
    }
}

impl TuningSettings {
    /// Bring every field back into its documented range.
    pub fn clamped(mut self) -> Self {
        self.sensitivity = self.sensitivity.min(100);
        self.spread = self.spread.min(100);
        self.rise_ms = self.rise_ms.clamp(5, 100);
        self.fall_ms = (self.fall_ms.clamp(50, 800) / 10) * 10;
        self.freq_hi = self.freq_hi.clamp(FREQ_MIN_GAP, 100);
        self.freq_lo = self.freq_lo.min(self.freq_hi - FREQ_MIN_GAP);
        self.bass_db = self.bass_db.clamp(-12, 12);
        self.mids_db = self.mids_db.clamp(-12, 12);
        self.highs_db = self.highs_db.clamp(-12, 12);
        self.peak_hold_ms = (self.peak_hold_ms.min(1000) / 50) * 50;
        self.ceiling_db = self.ceiling_db.clamp(-100 + LEVEL_MIN_SPAN, 0);
        self.floor_db = self.floor_db.clamp(-100, self.ceiling_db - LEVEL_MIN_SPAN);
        self.contrast = self.contrast.min(100);
        self.tilt_db = self.tilt_db.clamp(-3, 6);
        self.gate_db = self.gate_db.clamp(GATE_OFF, -30);
        self.beat_sensitivity = self.beat_sensitivity.min(100);
        self.delay_ms = (self.delay_ms.min(MAX_DELAY_MS) / 10) * 10;
        self
    }
}

// ---------------------------------------------------------------------------
// Appearance

/// What fills the window behind the bars.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundMode {
    /// One flat color ([`SolidColor`]).
    Solid,
    /// The album cover, blurred and dimmed.
    AlbumArt,
    /// Soft color blobs (from the cover when there is one).
    Ambient,
}

impl BackgroundMode {
    /// Whether the background is an image that moves and changes with the track.
    pub fn is_dynamic(self) -> bool {
        self != BackgroundMode::Solid
    }
}

/// The solid background colors.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SolidColor {
    /// Near-black gray, #141416.
    Charcoal,
    /// Pure black.
    Black,
    /// Very dark blue, #0F1424.
    Midnight,
    /// Very dark purple, #1C1322.
    Plum,
}

impl SolidColor {
    /// Every color, in swatch order.
    pub const ALL: [SolidColor; 4] = [
        SolidColor::Charcoal,
        SolidColor::Black,
        SolidColor::Midnight,
        SolidColor::Plum,
    ];

    /// The color as RGB.
    pub fn rgb(self) -> [u8; 3] {
        match self {
            SolidColor::Charcoal => [0x14, 0x14, 0x16],
            SolidColor::Black => [0x00, 0x00, 0x00],
            SolidColor::Midnight => [0x0F, 0x14, 0x24],
            SolidColor::Plum => [0x1C, 0x13, 0x22],
        }
    }

    /// The name shown in the Appearance panel.
    pub fn name(self) -> &'static str {
        match self {
            SolidColor::Charcoal => "Charcoal",
            SolidColor::Black => "Black",
            SolidColor::Midnight => "Midnight",
            SolidColor::Plum => "Plum",
        }
    }
}

/// How a dynamic background moves while music plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Motion {
    /// It doesn't.
    Still,
    /// A slow sideways drift.
    Drift,
    /// A slow turn.
    Spiral,
}

/// How the current track is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NowPlayingMode {
    /// Title, artist and album in the header.
    Header,
    /// A large cover and title beside the bars (falls back to Header in narrow windows).
    Spotlight,
    /// One small line in the header.
    Minimal,
    /// Hidden.
    Off,
}

/// The UI's accent color (sliders, switches, active buttons).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Accent {
    /// Picked from the album cover.
    Album,
    /// The lilac end of the Undavi gradient.
    Lilac,
    /// The sky-blue end of the Undavi gradient (the default).
    Sky,
    /// A bright blue, #4B7BF5.
    Blue,
    /// A green-teal, #3DD6B5.
    Mint,
    /// A pink-red, #F2668B.
    Rose,
    /// Off-white, #E4E4E7.
    White,
}

impl Accent {
    /// Every accent, in swatch order.
    pub const ALL: [Accent; 7] = [
        Accent::Album,
        Accent::Lilac,
        Accent::Sky,
        Accent::Blue,
        Accent::Mint,
        Accent::Rose,
        Accent::White,
    ];

    /// The name shown in the Appearance panel.
    pub fn name(self) -> &'static str {
        match self {
            Accent::Album => "From album art",
            Accent::Lilac => "Lilac",
            Accent::Sky => "Sky",
            Accent::Blue => "Blue",
            Accent::Mint => "Mint",
            Accent::Rose => "Rose",
            Accent::White => "White",
        }
    }

    /// Fixed color for this accent (`None` for `Album`, which comes from the cover).
    pub fn rgb(self) -> Option<[u8; 3]> {
        match self {
            Accent::Album => None,
            Accent::Lilac => Some(UNDAVI_GRADIENT[0]),
            Accent::Sky => Some(UNDAVI_GRADIENT[1]),
            Accent::Blue => Some(ACCENT_BLUE),
            Accent::Mint => Some([0x3D, 0xD6, 0xB5]),
            Accent::Rose => Some([0xF2, 0x66, 0x8B]),
            Accent::White => Some([0xE4, 0xE4, 0xE7]),
        }
    }
}

/// The Blue accent, also the fallback when an album accent isn't available yet.
pub const ACCENT_BLUE: [u8; 3] = [0x4B, 0x7B, 0xF5];
/// The brand gradient (logo, wordmark, default bar palette), lilac to sky blue.
pub const UNDAVI_GRADIENT: [[u8; 3]; 2] = [[0xB5, 0x97, 0xF6], [0x96, 0xC6, 0xEA]];

/// Whether icons on an accent-filled button should be dark (the accent is light).
pub fn wants_dark_icon(c: [u8; 3]) -> bool {
    (0.299 * c[0] as f32 + 0.587 * c[1] as f32 + 0.114 * c[2] as f32) / 255.0 > 0.6
}

/// Background, track info, lyrics and window behavior (the Appearance panel).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppearanceSettings {
    /// What fills the background.
    pub bg_mode: BackgroundMode,
    /// The color for [`BackgroundMode::Solid`].
    pub solid_color: SolidColor,
    /// How a dynamic background moves.
    pub motion: Motion,
    /// Background blur, 0..=120 px.
    pub blur_px: u8,
    /// Background darkening, 0..=90 %.
    pub dim_pct: u8,
    /// How the current track is shown.
    pub now_playing: NowPlayingMode,
    /// Show the cover next to the track info.
    pub show_album_art: bool,
    /// Hide the controls when the mouse is still.
    pub auto_hide: bool,
    /// Seconds of stillness before auto-hide, 1..=10.
    pub hide_after_s: u8,
    /// Keep the window above others (not available on Wayland).
    pub always_on_top: bool,
    /// The UI accent color.
    pub accent: Accent,
    /// Synced lyrics above (Header, Minimal, Off) or beside (Spotlight) the bars.
    pub lyrics: bool,
    /// Lyrics text size, 70..=150 %.
    pub lyrics_size: u8,
    /// Shifts lyrics timing, -2000..=2000 ms (positive shows lines earlier).
    pub lyrics_offset_ms: i16,
    /// Glow on lit lyric characters, 0..=100 (0 = off).
    pub lyrics_glow: u8,
    /// Album art / ambient background swells with the beat.
    pub bg_pulse: bool,
    /// What the background pulse follows.
    pub bg_pulse_source: GlowSource,
    /// Background pulse strength, 0..=100.
    pub bg_pulse_amount: u8,
    /// Redraw limit; lower rates save battery.
    pub frame_rate: FrameRate,
}

/// The most frames per second the app draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FrameRate {
    /// 30 fps (saved as `"30"`).
    #[serde(rename = "30")]
    Fps30,
    /// 60 fps (saved as `"60"`).
    #[serde(rename = "60")]
    Fps60,
    /// As fast as the display refreshes.
    Display,
}

impl FrameRate {
    /// Time between frames, or `None` to follow the display.
    pub fn interval(self) -> Option<std::time::Duration> {
        match self {
            FrameRate::Fps30 => Some(std::time::Duration::from_micros(33_333)),
            FrameRate::Fps60 => Some(std::time::Duration::from_micros(16_667)),
            FrameRate::Display => None,
        }
    }
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            bg_mode: BackgroundMode::AlbumArt,
            solid_color: SolidColor::Charcoal,
            motion: Motion::Spiral,
            blur_px: 80,
            dim_pct: 55,
            now_playing: NowPlayingMode::Header,
            show_album_art: true,
            auto_hide: true,
            hide_after_s: 3,
            always_on_top: false,
            accent: Accent::Sky,
            lyrics: true,
            lyrics_size: 100,
            lyrics_offset_ms: 0,
            lyrics_glow: 60,
            bg_pulse: false,
            bg_pulse_source: GlowSource::Bass,
            bg_pulse_amount: 50,
            frame_rate: FrameRate::Display,
        }
    }
}

impl AppearanceSettings {
    /// Bring every field back into its documented range.
    pub fn clamped(mut self) -> Self {
        self.blur_px = self.blur_px.min(120);
        self.dim_pct = self.dim_pct.min(90);
        self.hide_after_s = self.hide_after_s.clamp(1, 10);
        self.lyrics_size = self.lyrics_size.clamp(70, 150);
        self.lyrics_offset_ms = (self.lyrics_offset_ms.clamp(-2000, 2000) / 50) * 50;
        self.lyrics_glow = self.lyrics_glow.min(100);
        self.bg_pulse_amount = self.bg_pulse_amount.min(100);
        self
    }
}

// ---------------------------------------------------------------------------
// Frequency helpers

/// Slider position `v` (0..=100) → frequency in Hz: `20 × 1000^(v/100)`.
pub fn slider_to_hz(v: f32) -> f32 {
    20.0 * 1000f32.powf(v / 100.0)
}

/// Frequency → position on the 20 Hz – 20 kHz log scale (0..=1).
pub fn hz_to_log_pos(f: f32) -> f32 {
    (f / 20.0).max(1e-6).log10() / 3.0
}

/// `NN Hz` below 1 kHz, `N.N kHz` below 10 kHz (dropping a trailing `.0`), else `NN kHz`.
pub fn format_hz(f: f32) -> String {
    if f < 1000.0 {
        format!("{} Hz", f.round() as i32)
    } else if f < 10_000.0 {
        let k = (f / 100.0).round() / 10.0;
        if k.fract() == 0.0 {
            format!("{} kHz", k as i32)
        } else {
            format!("{k:.1} kHz")
        }
    } else {
        format!("{} kHz", (f / 1000.0).round() as i32)
    }
}

/// Format a signed dB value with a typographic minus (`−2 dB`, `+4 dB`, `0 dB`).
pub fn format_db(db: i8) -> String {
    match db {
        0 => "0 dB".into(),
        d if d > 0 => format!("+{d} dB"),
        d => format!("\u{2212}{} dB", -(d as i16)),
    }
}

// ---------------------------------------------------------------------------
// Persistence

/// The settings file format. Bump it when a change needs more than serde defaults
/// and aliases to read older files, and add the step to [`migrate`].
///
/// - 0: files from before the version number existed.
/// - 1: Undavi 0.9, the first release (adds `version`).
pub const CONFIG_VERSION: u32 = 1;

/// Everything in the settings file: the current settings and every preset.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Stored {
    /// Format of the file this was read from (see [`CONFIG_VERSION`]). Files
    /// without one are version 0.
    #[serde(default)]
    pub version: u32,
    /// Current Tuning settings.
    pub tuning: TuningSettings,
    /// Current Visuals settings.
    pub visuals: VisualSettings,
    /// Current Appearance settings.
    pub appearance: AppearanceSettings,
    /// Visuals presets (kept under this name for older settings files).
    pub presets: Vec<Preset>,
    /// Tuning presets.
    pub tuning_presets: Vec<TuningPreset>,
    /// Appearance presets.
    pub appearance_presets: Vec<AppearancePreset>,
    /// The audio source picked last (`None`: System Audio).
    pub source: Option<crate::audio::Source>,
}

impl Default for Stored {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            tuning: TuningSettings::default(),
            visuals: VisualSettings::default(),
            appearance: AppearanceSettings::default(),
            presets: Vec::new(),
            tuning_presets: Vec::new(),
            appearance_presets: Vec::new(),
            source: None,
        }
    }
}

/// Which panel a preset belongs to; each panel keeps its own list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresetKind {
    /// [`Preset`]s.
    Visuals,
    /// [`TuningPreset`]s.
    Tuning,
    /// [`AppearancePreset`]s.
    Appearance,
}

/// A saved set of Visuals settings.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Preset {
    /// The name the user gave it (at most 40 characters).
    pub name: String,
    /// The settings it restores.
    pub visuals: VisualSettings,
    /// Older Visuals presets also stored the tuning. It moves to a Tuning preset
    /// when settings load (see [`migrate_presets`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tuning: Option<TuningSettings>,
}

/// A saved set of Tuning settings.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TuningPreset {
    /// The name the user gave it (at most 40 characters).
    pub name: String,
    /// The settings it restores.
    pub tuning: TuningSettings,
}

/// Everything in Appearance. Loading one keeps the current "Always on top",
/// since that's about the window rather than the look.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppearancePreset {
    /// The name the user gave it (at most 40 characters).
    pub name: String,
    /// The settings it restores.
    pub appearance: AppearanceSettings,
}

/// A saved preset of any kind: just enough for renaming and the shared list UI.
pub trait Named {
    /// The preset's name.
    fn name(&self) -> &str;
    /// The preset's name, for renaming.
    fn name_mut(&mut self) -> &mut String;
}

macro_rules! named {
    ($($t:ty),*) => {$(
        impl Named for $t {
            fn name(&self) -> &str {
                &self.name
            }
            fn name_mut(&mut self) -> &mut String {
                &mut self.name
            }
        }
    )*};
}
named!(Preset, TuningPreset, AppearancePreset);

/// Move the tuning out of older Visuals presets into Tuning presets of the same
/// name, so nothing saved is lost. Identical tunings are only kept once.
pub fn migrate_presets(stored: &mut Stored) {
    for p in &mut stored.presets {
        let Some(tuning) = p.tuning.take() else {
            continue;
        };
        let tuning = tuning.clamped();
        if !stored.tuning_presets.iter().any(|t| t.tuning == tuning) {
            stored.tuning_presets.push(TuningPreset {
                name: p.name.clone(),
                tuning,
            });
        }
    }
}

/// `settings.json` in the platform's config folder: `$XDG_CONFIG_HOME` or
/// `~/.config` on Linux, `%APPDATA%` on Windows, `~/Library/Application Support`
/// on macOS. Presets live in the same file.
pub fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("undavi").join("settings.json"))
}

/// Where settings lived before the app was renamed from Muvijo. Read once if the new
/// file doesn't exist yet; the next save writes the new location.
fn legacy_config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("muvijo").join("settings.json"))
}

/// Bring settings read from a file of any older version up to date, and every
/// value back into range.
pub fn migrate(mut s: Stored) -> Stored {
    // 0 → 1: Visuals presets used to carry tuning; field renames (smoothing →
    // spread, stereo → split) are handled by serde aliases.
    if s.version < 1 {
        migrate_presets(&mut s);
    }
    s.version = CONFIG_VERSION;

    s.tuning = s.tuning.clamped();
    s.visuals = s.visuals.clamped();
    s.appearance = s.appearance.clamped();
    for p in &mut s.presets {
        p.visuals = p.visuals.clone().clamped();
    }
    for p in &mut s.tuning_presets {
        p.tuning = p.tuning.clone().clamped();
    }
    for p in &mut s.appearance_presets {
        p.appearance = p.appearance.clone().clamped();
    }
    s
}

/// Set a file aside (as `<name>.<suffix>`) before it gets overwritten, so a
/// settings file this version can't fully use isn't lost.
fn keep_copy(path: &std::path::Path, suffix: &str) {
    let mut name = path.as_os_str().to_owned();
    name.push(".");
    name.push(suffix);
    let backup = PathBuf::from(name);
    if !backup.exists() {
        match std::fs::copy(path, &backup) {
            Ok(_) => eprintln!(
                "undavi: kept a copy of your settings at {}",
                backup.display()
            ),
            Err(e) => eprintln!("undavi: couldn't back up {}: {e}", path.display()),
        }
    }
}

/// Read the settings file, or the defaults if there is none.
///
/// Never fails: an unreadable file is set aside (as `settings.json.unreadable`)
/// and the defaults are used; a file from a newer Undavi is read as far as
/// possible and also kept a copy of, since the next save drops what this version
/// doesn't know. Settings saved by Muvijo (the old name) are picked up once.
pub fn load() -> Stored {
    let Some(path) = config_path() else {
        return Stored::default();
    };
    let read = |p: &PathBuf| std::fs::read_to_string(p).ok().map(|t| (p.clone(), t));
    let Some((path, text)) = read(&path).or_else(|| legacy_config_path().as_ref().and_then(read))
    else {
        return Stored::default();
    };
    match serde_json::from_str::<Stored>(&text) {
        Ok(s) => {
            if s.version > CONFIG_VERSION {
                // Written by a newer Undavi: use what we understand, but keep the
                // original, since saving drops anything this version doesn't know.
                keep_copy(&path, &format!("v{}", s.version));
            }
            migrate(s)
        }
        Err(e) => {
            eprintln!(
                "undavi: ignoring unreadable settings at {}: {e}",
                path.display()
            );
            keep_copy(&path, "unreadable");
            Stored::default()
        }
    }
}

/// Write `stored` to the settings file, atomically (via a temporary file).
///
/// Errors are reported on stderr and otherwise ignored; settings are saved again
/// on the next change. Does nothing in this crate's own unit tests.
pub fn save(stored: &Stored) {
    // Tests drive the real app; never let them touch the user's settings.
    if cfg!(test) {
        return;
    }
    let Some(path) = config_path() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    match serde_json::to_string_pretty(stored) {
        Ok(json) => {
            // Write to a temp file first so a crash mid-write can't corrupt settings.
            let tmp = path.with_extension("json.tmp");
            if std::fs::write(&tmp, json).is_ok() {
                let _ = std::fs::rename(&tmp, &path);
            }
        }
        Err(e) => eprintln!("undavi: could not serialize settings: {e}"),
    }
}

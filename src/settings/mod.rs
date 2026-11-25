//! Everything the user can set, the presets, and the settings file.
//!
//! Settings come in seven groups, one per section of the settings window
//! ([`SectionId`]):
//!
//! - [`TuningSettings`]: how the bars react to sound.
//! - [`VisualSettings`]: the visualizer shader and its settings.
//! - [`ColorSettings`]: the palette and the accent.
//! - [`BackgroundSettings`]: the background shader, its settings and the effects.
//! - [`NowPlayingSettings`]: how the current track is shown.
//! - [`LyricsSettings`]: the lyrics.
//! - [`GeneralSettings`]: the controls and the window.
//!
//! Each section also keeps its own presets ([`Section`]). All of it is saved as
//! one JSON file, [`Stored`], at [`config_path`]; [`load`] and [`save`] read and
//! write it.
//!
//! # Compatibility
//!
//! Every struct uses `#[serde(default)]`, so a field missing from an older file
//! gets its default. Renamed fields and values keep their old names as serde
//! aliases. Changes that need more than that bump [`CONFIG_VERSION`] and add a
//! step to [`upgrade`]. Values read from a file are always brought back into
//! range (the `clamped` methods), so a hand-edited file can't break the app.
//!
//! # Examples
//!
//! ```
//! use undavi::settings::{Stored, migrate, upgrade};
//!
//! // A file from an older version: "smoothing" is now called "spread", and
//! // out-of-range values are fixed on load.
//! let mut old = serde_json::json!({"tuning": {"smoothing": 30, "sensitivity": 250}});
//! upgrade(&mut old);
//! let settings: Stored = migrate(serde_json::from_value(old)?);
//! assert_eq!(settings.sections.sound.spread, 30);
//! assert_eq!(settings.sections.sound.sensitivity, 100);
//! # Ok::<(), serde_json::Error>(())
//! ```

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

mod controls;
mod presets;
mod versions;

pub use controls::{
    Action, ActionGroup, BarsScroll, Combo, ControlSettings, DoubleClick, MAX_KEYS, Shortcuts,
};
pub use presets::{
    BuiltIn, Fill, Preset, PresetGroup, PresetId, Section, SectionId, SectionPresets,
    SectionSettings, Sections,
};
pub use versions::{upgrade, upgrade_to_v1, upgrade_to_v2, upgrade_to_v3, upgrade_to_v4};

/// How the bars react to sound (the Sound section).
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
    /// How hard gravity pulls, 25..=250 % (100 = a full bar drops in three fall times).
    pub gravity_strength: u8,
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
            gravity_strength: 100,
            gate_db: -70,
            when_silent: SilenceMode::Fade,
            beat_sensitivity: 50,
            delay_ms: 0,
        }
    }
}

/// Minimum distance between the two frequency-range handles, in steps.
pub const FREQ_MIN_GAP: u8 = 10;

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
    /// From a visualizer's `mirror` setting ("Off", "Up/down", "Left/right", "Both").
    pub fn from_label(label: &str) -> Option<Self> {
        Some(match label {
            "Off" => Mirror::Off,
            "Up/down" => Mirror::UpDown,
            "Left/right" => Mirror::LeftRight,
            "Both" => Mirror::Both,
            _ => return None,
        })
    }

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
    /// The name shown in the Colors section.
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

/// A shader's saved setting values by id (the ids its `// @setting` lines
/// declare), as JSON: numbers, `true`/`false`, a choice's label, or a color as
/// `"#rrggbb"`. Anything missing or out of range takes the declared default.
pub type ShaderValues = std::collections::BTreeMap<String, serde_json::Value>;

/// The visualizer (the Visualizer section): which shader draws it, and each
/// visualizer's own settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VisualSettings {
    /// The visualizer: `"bars"`, `"radial"` or `"wave"` (built in), or the name
    /// of a file in the `shaders` folder next to the settings file, without `.wgsl`.
    pub shader: String,
    /// Each visualizer's settings, by shader name.
    pub values: std::collections::BTreeMap<String, ShaderValues>,
}

/// The Colors section: the bar colors every visualizer shares, and the accent.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ColorSettings {
    /// Bar colors.
    pub palette: Palette,
    /// Album palette: a gradient of the cover's colors (or one solid color).
    pub album_gradient: bool,
    /// Album palette with a gradient: build it from the chosen accent instead of
    /// the cover. Only offered when the accent isn't "From album art".
    pub album_match_accent: bool,
    /// Which way a multi-color palette runs.
    pub gradient: GradientDir,
    /// The UI accent color.
    pub accent: Accent,
}

impl Default for ColorSettings {
    fn default() -> Self {
        Self {
            palette: Palette::Undavi,
            album_gradient: true,
            album_match_accent: false,
            gradient: GradientDir::Across,
            accent: Accent::Sky,
        }
    }
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

impl Channel {
    /// From a visualizer's `channel` setting ("Mono", "Left", "Right").
    pub fn from_label(label: &str) -> Option<Self> {
        Some(match label {
            "Mono" => Channel::Mono,
            "Left" => Channel::Left,
            "Right" => Channel::Right,
            _ => return None,
        })
    }
}

impl Split {
    /// From a visualizer's `split` setting ("Off", "Stereo", "Mid/Side").
    pub fn from_label(label: &str) -> Option<Self> {
        Some(match label {
            "Off" => Split::Off,
            "Stereo" => Split::Stereo,
            "Mid/Side" => Split::MidSide,
            _ => return None,
        })
    }
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

impl GlowSource {
    /// From a shader's `glow_source` or `pulse_source` setting ("Bass", "Drums").
    pub fn from_label(label: &str) -> Option<Self> {
        Some(match label {
            "Bass" => GlowSource::Bass,
            "Drums" => GlowSource::Drums,
            _ => return None,
        })
    }
}

impl Default for VisualSettings {
    fn default() -> Self {
        Self {
            shader: "bars".into(),
            values: Default::default(),
        }
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
        self.gravity_strength = (self.gravity_strength.clamp(25, 250) / 5) * 5;
        self
    }
}

// ---------------------------------------------------------------------------
// Now playing, lyrics, colors, background and window

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

    /// The name shown in the Colors section.
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

/// The background (the Background section): which shader draws it, each
/// background's own settings, and the effects over every background.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BackgroundSettings {
    /// `"solid"`, `"album-art"` or `"ambient"` (built in), or the name of a file
    /// in the `shaders` folder, without `.wgsl`.
    pub shader: String,
    /// Each background's settings, by shader name.
    pub values: std::collections::BTreeMap<String, ShaderValues>,
    /// Darkening, 0..=90 % (every background but Solid).
    pub dim_pct: u8,
    /// Color saturation, 0..=200 % (100 = unchanged, 0 = gray).
    pub saturation: u8,
    /// Brightness, 50..=150 %.
    pub brightness: u8,
    /// Contrast, 50..=150 %.
    pub contrast: u8,
    /// How far the colors lean toward the accent, 0..=100 %.
    pub tint_pct: u8,
    /// Darker edges, 0..=100 %.
    pub vignette_pct: u8,
    /// Film grain, 0..=100 %.
    pub grain_pct: u8,
}

impl Default for BackgroundSettings {
    fn default() -> Self {
        Self {
            shader: "album-art".into(),
            values: Default::default(),
            dim_pct: 55,
            saturation: 100,
            brightness: 100,
            contrast: 100,
            tint_pct: 0,
            vignette_pct: 0,
            grain_pct: 0,
        }
    }
}

impl BackgroundSettings {
    /// Whether the background is a picture (anything but Solid), which the
    /// header's text and buttons adapt to.
    pub fn is_dynamic(&self) -> bool {
        self.shader != "solid"
    }

    /// Bring every field back into its documented range. Shader values are
    /// checked against their declarations when used.
    pub fn clamped(mut self) -> Self {
        self.dim_pct = self.dim_pct.min(90);
        self.saturation = self.saturation.min(200);
        self.brightness = self.brightness.clamp(50, 150);
        self.contrast = self.contrast.clamp(50, 150);
        self.tint_pct = self.tint_pct.min(100);
        self.vignette_pct = self.vignette_pct.min(100);
        self.grain_pct = self.grain_pct.min(100);
        self
    }
}

/// The Now playing section.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NowPlayingSettings {
    /// How the current track is shown.
    pub mode: NowPlayingMode,
    /// Show the cover next to the track info.
    pub show_album_art: bool,
}

impl Default for NowPlayingSettings {
    fn default() -> Self {
        Self {
            mode: NowPlayingMode::Header,
            show_album_art: true,
        }
    }
}

/// How the playback bar is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackStyle {
    /// On the background, without a fill.
    #[default]
    Open,
    /// A rounded card below the bars.
    Card,
    /// A strip along the bottom of the window.
    Dock,
}

/// What marks the position on the seek bar.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SeekHandle {
    /// An upright bar.
    #[default]
    Bar,
    /// A round dot.
    Dot,
    /// Nothing.
    None,
}

/// The Playback bar section. What's `None` is up to the style (see
/// [`PlaybackSettings::look`]).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlaybackSettings {
    /// How the bar is drawn.
    pub style: PlaybackStyle,
    /// Everything in one row.
    pub compact: bool,
    /// How wide it gets, 500..=1600 px (a Dock is always as wide as the window).
    pub width: u16,
    /// How solid a Card or Dock is, 0..=100 %.
    pub opacity: u8,
    /// The cover and title in a Card or Dock.
    pub track_info: bool,
    /// The repeat button.
    pub repeat: bool,
    /// The volume button (or slider).
    pub volume: bool,
    /// The full-screen button.
    pub fullscreen: bool,
    /// A filled circle for the play button.
    pub filled_play: Option<bool>,
    /// Height of the curve over the part that's played, from its middle, 0..=14 px
    /// (0 = a straight line).
    pub curve_height: Option<u8>,
    /// Length of one wave of it, 30..=240 px.
    pub curve_length: Option<u16>,
    /// How fast it moves while music plays, 0..=300 % (0 = still).
    pub curve_speed: u16,
    /// How thick the lines are, 2..=10 px.
    pub thickness: Option<u8>,
    /// What marks the position.
    pub handle: SeekHandle,
    /// How bright the part still to play is, 10..=100 %.
    pub remaining: Option<u8>,
    /// The elapsed and remaining time.
    pub times: bool,
}

impl Default for PlaybackSettings {
    fn default() -> Self {
        Self {
            style: PlaybackStyle::Open,
            compact: false,
            width: 1000,
            opacity: 85,
            track_info: true,
            repeat: true,
            volume: true,
            fullscreen: true,
            filled_play: None,
            curve_height: None,
            curve_length: None,
            curve_speed: 100,
            thickness: None,
            handle: SeekHandle::Bar,
            remaining: None,
            times: true,
        }
    }
}

/// [`PlaybackSettings`] with what's up to the style filled in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlaybackLook {
    /// A filled circle for the play button.
    pub filled_play: bool,
    /// Height of the curve from its middle, px.
    pub curve_height: u8,
    /// Length of one wave, px.
    pub curve_length: u16,
    /// Thickness of the lines, px.
    pub thickness: u8,
    /// Brightness of the part still to play, %.
    pub remaining: u8,
    /// Width and height of a [`SeekHandle::Bar`], px.
    pub handle: (f32, f32),
}

impl PlaybackSettings {
    /// What the style (and compact mode) makes of the settings left to it.
    pub fn look(&self) -> PlaybackLook {
        let (filled_play, curve_height, curve_length, thickness, remaining, handle) =
            match (self.style, self.compact) {
                (PlaybackStyle::Open, false) => (false, 6, 100, 7, 100, (8.0, 36.0)),
                (PlaybackStyle::Open, true) => (false, 4, 70, 5, 100, (6.0, 24.0)),
                (PlaybackStyle::Card, _) => (true, 4, 52, 4, 35, (5.0, 20.0)),
                (PlaybackStyle::Dock, _) => (true, 3, 44, 4, 25, (5.0, 18.0)),
            };
        PlaybackLook {
            filled_play: self.filled_play.unwrap_or(filled_play),
            curve_height: self.curve_height.unwrap_or(curve_height),
            curve_length: self.curve_length.unwrap_or(curve_length),
            thickness: self.thickness.unwrap_or(thickness),
            remaining: self.remaining.unwrap_or(remaining),
            handle,
        }
    }

    /// Bring every field back into its documented range.
    pub fn clamped(mut self) -> Self {
        self.width = (self.width.clamp(500, 1600) / 50) * 50;
        self.opacity = self.opacity.min(100);
        self.curve_height = self.curve_height.map(|v| v.min(14));
        self.curve_length = self.curve_length.map(|v| v.clamp(30, 240));
        self.curve_speed = self.curve_speed.min(300);
        self.thickness = self.thickness.map(|v| v.clamp(2, 10));
        self.remaining = self.remaining.map(|v| v.clamp(10, 100));
        self
    }
}

/// The Lyrics section.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LyricsSettings {
    /// Synced lyrics above (Header, Minimal, Off) or beside (Spotlight) the bars.
    pub show: bool,
    /// Text size, 70..=150 %.
    pub size: u8,
    /// Shifts the timing, -2000..=2000 ms (positive shows lines earlier).
    pub offset_ms: i16,
    /// Glow on lit characters, 0..=100 (0 = off).
    pub glow: u8,
    /// Fill each word as it's sung when the lyrics have word timing. Off lights
    /// whole lines at once, even for word-synced lyrics.
    pub word_sync: bool,
}

impl Default for LyricsSettings {
    fn default() -> Self {
        Self {
            show: true,
            size: 100,
            offset_ms: 0,
            glow: 60,
            word_sync: true,
        }
    }
}

impl LyricsSettings {
    /// Bring every field back into its documented range.
    pub fn clamped(mut self) -> Self {
        self.size = self.size.clamp(70, 150);
        self.offset_ms = (self.offset_ms.clamp(-2000, 2000) / 50) * 50;
        self.glow = self.glow.min(100);
        self
    }
}

/// The General section: the controls and the window.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralSettings {
    /// Hide the controls when the mouse is still.
    pub auto_hide: bool,
    /// Seconds of stillness before auto-hide, 1..=10.
    pub hide_after_s: u8,
    /// With auto-hide, only moving the mouse brings the controls back (not keys,
    /// clicks or scrolling).
    pub show_on_move_only: bool,
    /// Keep the window above others (not available on Wayland).
    pub always_on_top: bool,
    /// Redraw limit; lower rates save battery.
    pub frame_rate: FrameRate,
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            auto_hide: true,
            hide_after_s: 3,
            show_on_move_only: false,
            always_on_top: false,
            frame_rate: FrameRate::Display,
        }
    }
}

impl GeneralSettings {
    /// Bring every field back into its documented range.
    pub fn clamped(mut self) -> Self {
        self.hide_after_s = self.hide_after_s.clamp(1, 10);
        self
    }
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
/// and aliases to read older files, and add the step to [`upgrade`].
///
/// - 0: files from before the version number existed.
/// - 1: Undavi 0.9, the first release (adds `version`); see [`upgrade_to_v1`].
/// - 2: visualizers and backgrounds are shaders with their own settings
///   (`visuals.shader` and `values`, `appearance.background` and
///   `background_values`); see [`upgrade_to_v2`].
/// - 3: the Appearance panel split into Background and Interface, each with its
///   own settings (`background`, `interface`) and presets; see [`upgrade_to_v3`].
/// - 4: one `sections` object, with each section's settings, presets and the
///   preset in use together; see [`upgrade_to_v4`].
pub const CONFIG_VERSION: u32 = 4;

/// Everything in the settings file: the current settings and every preset.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Stored {
    /// Format of the file this was read from (see [`CONFIG_VERSION`]). Files
    /// without one are version 0.
    #[serde(default)]
    pub version: u32,
    /// Every section's settings and presets.
    pub sections: Sections,
    /// The audio source picked last (`None`: System Audio).
    pub source: Option<crate::audio::Source>,
    /// Update checks.
    pub updates: UpdateSettings,
    /// Which parts the signal monitor shows.
    pub monitor: MonitorSettings,
    /// The starred settings, in the order they were starred (see the app's
    /// `SettingKey`). Shown in the quick settings.
    pub favorites: Vec<String>,
}

/// The signal monitor's parts (in the Sound section), each shown or hidden.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MonitorSettings {
    /// Input level, the loudest bar and bar heights over time.
    pub signal: bool,
    /// The spectrum with the bars' frequency and level range.
    pub spectrum: bool,
    /// Beat pulses, hits and tempo.
    pub beats: bool,
    /// Frame rate, the audio source and what the player, lyrics and art are doing.
    pub performance: bool,
}

impl Default for MonitorSettings {
    fn default() -> Self {
        Self {
            signal: true,
            spectrum: true,
            beats: true,
            performance: true,
        }
    }
}

/// Whether Undavi looks for new releases, and installs them without asking.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdateSettings {
    /// Ask GitHub for the latest release when Undavi starts.
    pub check: bool,
    /// Install a new release without asking first (only for copies installed
    /// from a release tarball or `install.sh`; package managers handle their own).
    pub auto_install: bool,
}

impl Default for UpdateSettings {
    fn default() -> Self {
        Self {
            check: true,
            auto_install: false,
        }
    }
}

impl Default for Stored {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            sections: Sections::default(),
            source: None,
            updates: UpdateSettings::default(),
            monitor: MonitorSettings::default(),
            favorites: Vec::new(),
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

/// Bring settings read from a file up to date: the current version, and every
/// value back into range. (Older formats are changed before they're read; see
/// [`upgrade`].)
pub fn migrate(mut s: Stored) -> Stored {
    s.version = CONFIG_VERSION;
    s.sections = s.sections.clamped();
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
    let parsed = serde_json::from_str::<serde_json::Value>(&text).and_then(|mut json| {
        upgrade(&mut json);
        serde_json::from_value::<Stored>(json)
    });
    match parsed {
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

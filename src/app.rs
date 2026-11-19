//! Application state and the update loop.

use crate::art::{Art, ArtLoader, Blurred, DEFAULT_AMBIENT, ambient_texture, ramp};
use crate::audio::{Analyzer, Capture, Signal, Source, SourceKind, list_sources};
use crate::focus::{FocusId, Registry, Scope, Step};
use crate::karaoke::{LyricsView, Placement};
use crate::lyrics::{Lyrics, LyricsLoader, Song, Status as LyricsStatus};
use crate::media::{Command, Media, MediaTarget, Track};
use crate::platform::Platform;
use crate::settings::{
    self, ACCENT_BLUE, Accent, AppearancePreset, AppearanceSettings, DEFAULT_CUSTOM, Palette,
    Preset, PresetKind, SilenceMode, SolidColor, Stored, TuningPreset, TuningSettings,
    VisualSettings,
};
use crate::spectrum::{BeatGlow, Gate, Processor, SpectrumFrame};
use crate::theme::*;
use crate::visualizer::{BarMap, bar_signals};
use iced::keyboard::{self, Key, key::Named};
use iced::widget::Canvas;
use iced::widget::image::Handle;
use iced::{Color, Element, Event, Length, Point, Size, Subscription, Task, mouse, window};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How long input may stay silent (no samples at all) before bars fall to zero.
const STALE_AUDIO: Duration = Duration::from_millis(250);
/// How often to try reopening audio while it's down.
const AUDIO_RETRY: Duration = Duration::from_secs(2);
/// After a stream dies, wait this long before the first retry so a restarting server
/// (or a device that's being re-plugged) has a moment to come back.
const AUDIO_RETRY_FIRST: Duration = Duration::from_millis(500);
/// A stream that's still open but has sent nothing for this long while music plays is
/// probably stuck; reopen it quietly.
const AUDIO_STALLED: Duration = Duration::from_secs(5);
/// A lost stream only replaces the bars with a "Reconnecting" note once it has been
/// gone this long, so quick recoveries don't flash a message.
const AUDIO_OUTAGE_NOTICE: Duration = Duration::from_millis(1500);
/// Debounce for writing settings to disk.
const SAVE_DELAY: Duration = Duration::from_secs(1);
pub(crate) const SEEK_STEP: Duration = Duration::from_secs(5);
/// Idle mode: how long everything must be still first, and the slow tick rate.
const IDLE_AFTER: Duration = Duration::from_secs(1);
const IDLE_TICK: Duration = Duration::from_millis(250);
pub(crate) const PANEL_WIDTH: f32 = 340.0;
/// Cross-fade between album backgrounds on track change.
pub(crate) const ART_FADE: Duration = Duration::from_millis(800);
/// Time constant for easing the accent toward a new color (settles in ~400 ms).
const ACCENT_TAU: f32 = 0.13;
const FADE_OUT: f32 = 0.3;
const FADE_IN: f32 = 0.15;
/// Header height and bottom-controls height: the pointer over either keeps controls visible.
const HEADER_ZONE: f32 = 130.0;
const CONTROLS_ZONE: f32 = 170.0;
pub(crate) const SPOTLIGHT_WIDTH: f32 = 240.0;

/// Which side panel is open (at most one).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum Panel {
    #[default]
    None,
    Tuning,
    Visuals,
    Appearance,
    About,
}

/// Why there are no bars right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AudioOutage<'a> {
    /// A working stream went away; retrying until it's back.
    Reconnecting,
    /// The source couldn't be opened (with the error); also retried.
    Failed(&'a str),
    /// Capturing an app that isn't playing anything: its name, or `None` when
    /// following the music player and there is none.
    Waiting(Option<&'a str>),
}

pub struct App {
    pub(crate) tuning: TuningSettings,
    pub(crate) visuals: VisualSettings,
    pub(crate) appearance: AppearanceSettings,
    /// Saved presets, one list per panel.
    pub(crate) presets: Vec<Preset>,
    pub(crate) tuning_presets: Vec<TuningPreset>,
    pub(crate) appearance_presets: Vec<AppearancePreset>,
    pub(crate) panel: Panel,

    pub(crate) host: cpal::Host,
    /// What this platform supports; the view leaves out the rest.
    pub(crate) platform: Platform,
    pub(crate) sources: Vec<Source>,
    pub(crate) source: Source,
    pub(crate) picker_open: bool,
    pub(crate) capture: Option<Capture>,
    pub(crate) audio_error: Option<String>,
    /// When audio went down (the stream died or wouldn't open); `None` while it's working.
    pub(crate) audio_down_since: Option<Instant>,
    /// The outage is a stream that was working and went away (vs. one that never opened).
    pub(crate) reconnecting: bool,
    next_audio_retry: Instant,
    /// Since when an app capture has had no stream to record (the app isn't playing).
    waiting_since: Option<Instant>,
    pub(crate) analyzer: Analyzer,
    pub(crate) processor: Processor,
    pub(crate) beat: BeatGlow,
    /// Separate detector for the background pulse (it has its own Bass/Drums setting).
    bg_beat: BeatGlow,
    /// Eased background pulse, 0..=1.
    pub(crate) bg_pulse: f32,
    pub(crate) last_audio: Option<Instant>,
    /// Noise gate: whether there's sound to show right now.
    pub(crate) gate: Gate,
    /// Readings for the tuning meter (shown while the Tuning panel is open).
    pub(crate) meter: Meter,

    pub(crate) media: Media,
    pub(crate) track: Option<Track>,
    pub(crate) unmuted_volume: Option<f64>,

    pub(crate) last_frame: Option<Instant>,
    pub(crate) now: Instant,
    pub(crate) wave_phase: f32,
    pub(crate) dirty_since: Option<Instant>,
    pub(crate) preset_saved_at: Option<Instant>,
    /// Text in the custom-color hex fields (kept separately so half-typed values survive).
    pub(crate) custom_hex: [String; 3],

    // Appearance
    pub(crate) art_loader: ArtLoader,
    pub(crate) lyrics_loader: LyricsLoader,
    pub(crate) lyrics: Option<Arc<Lyrics>>,
    pub(crate) art: Option<Arc<Art>>,
    pub(crate) blurred: Option<Arc<Blurred>>,
    /// Outgoing album background and when the cross-fade started.
    pub(crate) fading_out: Option<(Arc<Blurred>, Instant)>,
    pub(crate) default_ambient: Handle,
    /// Current (eased) accent, 0..=1 RGB.
    pub(crate) accent: [f32; 3],
    /// Album palette colors, eased toward the current cover's (0..=1 per channel).
    bar_colors: Vec<[f32; 3]>,
    pub(crate) last_activity: Instant,
    pub(crate) cursor: Option<Point>,
    pub(crate) window_size: Size,
    /// Auto-hide: 1 = controls visible, 0 = hidden.
    pub(crate) ui_alpha: f32,

    // Keyboard focus, filled in by the view (see `focus.rs`).
    pub(crate) focus: Option<FocusId>,
    pub(crate) focus_reg: Registry<Message>,
    /// The volume popover next to the volume button.
    pub(crate) volume_open: bool,
    /// Preset being renamed/reordered, and one armed for deletion.
    pub(crate) editing_preset: Option<(PresetKind, usize)>,
    pub(crate) confirm_delete: Option<(PresetKind, usize)>,
    /// The license expanded in the About panel.
    pub(crate) open_license: Option<usize>,

    pub(crate) fullscreen: bool,
    /// Idle mode: nothing playing and nothing animating, so redraw rarely.
    pub(crate) idle: bool,
    idle_since: Option<Instant>,
    /// Background motion clock; only advances while music plays.
    bg_time: f32,
}

/// Levels shown on the tuning meter, eased so they're readable.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Meter {
    /// Input level (RMS of the mix), dBFS; `None` without audio.
    pub input_db: Option<f32>,
    /// Loudest bar before the level range is applied, dB.
    pub loudest_db: Option<f32>,
}

impl Meter {
    /// Jump up at once, fall back over about a third of a second.
    fn follow(current: &mut Option<f32>, reading: Option<f32>, dt: f32) {
        *current = match (*current, reading) {
            (_, None) => None,
            (Some(c), Some(r)) if r < c => Some(c + (r - c) * (1.0 - (-dt / 0.3).exp())),
            (_, Some(r)) => Some(r),
        };
    }
}

/// Pointer and window events used by auto-hide.
#[derive(Debug, Clone, Copy)]
pub enum Pointer {
    Moved(Point),
    Left,
    /// A mouse button went down (also hides the keyboard focus ring).
    Pressed,
    Activity,
    Resized(Size),
}

#[derive(Debug, Clone)]
pub enum Message {
    Frame(Instant),
    Key(keyboard::Event),

    TogglePanel(Panel),
    /// Expand or collapse a license in the About panel.
    ToggleLicense(usize),
    OpenUrl(&'static str),
    TogglePicker,
    ClosePicker,
    PickSource(usize),

    Pointer(Pointer),

    SetTuning(TuningSettings),
    SetVisuals(VisualSettings),
    SetAppearance(AppearanceSettings),
    ResetAppearance,
    PickAccent(usize),
    PickSolid(usize),
    ResetTuning,
    ResetVisuals,
    AutoCalibrate,
    PickSwatch(usize),
    CustomHex(usize, String),
    SavePreset(PresetKind),
    LoadPreset(PresetKind, usize),
    /// Start (Some) or finish (None) editing a preset.
    EditPreset(PresetKind, Option<usize>),
    RenamePreset(PresetKind, usize, String),
    /// Move a preset up (-1) or down (+1).
    MovePreset(PresetKind, usize, isize),
    /// First press arms, second press deletes.
    DeletePreset(PresetKind, usize),

    PlayPause,
    Next,
    Previous,
    ToggleRepeat,
    ToggleMute,
    ToggleVolume,
    SetVolume(f32),
    ToggleFullscreen,
    /// The window's real mode after a toggle (it can also change outside the app).
    FullscreenChanged(bool),
    VolumeScroll(f32),
    Seek(f32),
    /// Seek to a lyric line's start time (seconds, lyric clock).
    SeekLyric(f32),
}

impl App {
    pub fn new() -> (Self, Task<Message>) {
        let stored = settings::load();
        let host = cpal::default_host();
        let sources = list_sources(&host);
        let system = Source::system(&host);
        let custom_hex = custom_hex_strings(&stored.visuals.palette);
        let now = Instant::now();
        let always_on_top = stored.appearance.always_on_top;
        let mut app = Self {
            tuning: stored.tuning,
            visuals: stored.visuals,
            appearance: stored.appearance,
            presets: stored.presets,
            tuning_presets: stored.tuning_presets,
            appearance_presets: stored.appearance_presets,
            panel: Panel::None,
            platform: Platform::detect(),
            host,
            sources,
            source: system,
            picker_open: false,
            capture: None,
            audio_error: None,
            audio_down_since: None,
            reconnecting: false,
            next_audio_retry: now,
            waiting_since: None,
            analyzer: Analyzer::default(),
            processor: Processor::default(),
            beat: BeatGlow::default(),
            bg_beat: BeatGlow::default(),
            bg_pulse: 0.0,
            last_audio: None,
            gate: Gate::default(),
            meter: Meter::default(),
            media: Media::start(),
            track: None,
            unmuted_volume: None,
            last_frame: None,
            now: Instant::now(),
            wave_phase: 0.0,
            dirty_since: None,
            preset_saved_at: None,
            custom_hex,
            art_loader: ArtLoader::start(),
            lyrics_loader: LyricsLoader::start(),
            lyrics: None,
            art: None,
            blurred: None,
            fading_out: None,
            default_ambient: ambient_texture(&DEFAULT_AMBIENT),
            accent: rgb_f(ACCENT_BLUE),
            bar_colors: Vec::new(),
            last_activity: now,
            cursor: None,
            window_size: Size::new(1440.0, 900.0),
            ui_alpha: 1.0,
            focus: None,
            focus_reg: Registry::default(),
            volume_open: false,
            editing_preset: None,
            confirm_delete: None,
            open_license: None,
            fullscreen: false,
            idle: false,
            idle_since: None,
            bg_time: 0.0,
        };
        app.accent = rgb_f(app.target_accent());
        // The source picked last time, if it's still there; System Audio otherwise.
        // (An app that isn't playing is fine: the capture waits for it.)
        let saved = stored
            .source
            .filter(|s| s.kind != SourceKind::SystemOutput)
            .filter(|s| s.kind != SourceKind::Player || app.platform.media);
        if let Some(saved) = saved {
            app.open_source(saved);
            if app.capture.is_none() {
                app.audio_error = None;
                app.audio_down_since = None;
                app.open_source(Source::system(&app.host));
            }
        } else {
            app.open_source(app.source.clone());
        }
        let task = if always_on_top {
            set_on_top(true)
        } else {
            Task::none()
        };
        (app, task)
    }

    pub fn title(&self) -> String {
        match &self.track {
            Some(np) if !np.title.is_empty() => format!("{} — Undavi", np.title),
            _ => "Undavi".into(),
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        // Idle: a slow tick keeps media info and the clock fresh without redrawing at 60 fps.
        let clock = match (self.idle, self.appearance.frame_rate.interval()) {
            (true, _) => iced::time::every(IDLE_TICK).map(Message::Frame),
            // A frame-rate cap: tick on a timer instead of every display refresh.
            (false, Some(interval)) => iced::time::every(interval).map(Message::Frame),
            (false, None) => window::frames().map(Message::Frame),
        };
        Subscription::batch([
            clock,
            keyboard::listen().map(Message::Key),
            iced::event::listen_with(|event, _status, _window| {
                let p = match event {
                    Event::Mouse(mouse::Event::CursorMoved { position }) => {
                        Pointer::Moved(position)
                    }
                    Event::Mouse(mouse::Event::CursorLeft) => Pointer::Left,
                    Event::Mouse(mouse::Event::ButtonPressed(_)) => Pointer::Pressed,
                    Event::Mouse(mouse::Event::WheelScrolled { .. }) => Pointer::Activity,
                    Event::Window(window::Event::Resized(size)) => Pointer::Resized(size),
                    Event::Window(window::Event::Opened { size, .. }) => Pointer::Resized(size),
                    _ => return None,
                };
                Some(Message::Pointer(p))
            }),
        ])
    }

    /// Open `source`, replacing any running stream. On failure, retries keep going
    /// from [`App::keep_audio_alive`].
    fn open_source(&mut self, source: Source) {
        self.capture = None; // stop the old stream first
        self.media.set_target(match source.kind {
            SourceKind::App => MediaTarget::App(source.id.clone()),
            _ => MediaTarget::Any,
        });
        match Capture::start(&self.host, &source) {
            Ok(c) => {
                if self.reconnecting {
                    eprintln!("undavi: reconnected to {}", source.label);
                }
                self.capture = Some(c);
                self.audio_error = None;
                self.audio_down_since = None;
                self.reconnecting = false;
            }
            Err(e) => {
                let message = format!("Couldn't open {}: {e}", source.label);
                // Retries repeat the same failure; log it once.
                if self.audio_error.as_ref() != Some(&message) {
                    eprintln!("undavi: {}: {e}", source.label);
                }
                self.audio_error = Some(message);
                self.audio_down_since.get_or_insert(self.now);
                self.next_audio_retry = self.now + AUDIO_RETRY;
            }
        }
        self.source = source;
    }

    /// Reopen the audio stream when it dies (device unplugged, PipeWire restarted),
    /// stalls, or couldn't be opened in the first place.
    fn keep_audio_alive(&mut self, now: Instant) {
        if self.capture.as_ref().is_some_and(Capture::is_dead) {
            self.capture = None;
            self.reconnecting = true;
            self.audio_down_since = Some(now);
            self.next_audio_retry = now + AUDIO_RETRY_FIRST;
        }
        if now < self.next_audio_retry {
            return;
        }
        let playing = self.track.as_ref().is_some_and(|n| n.playing);
        // App captures are silent whenever the app is; that's not a stall.
        let device = self
            .capture
            .as_ref()
            .is_some_and(|c| c.app_streams().is_none());
        let stalled = device
            && playing
            && self
                .last_audio
                .is_none_or(|t| now.saturating_duration_since(t) > AUDIO_STALLED);
        if stalled {
            // Quiet: no message, and if it fails the normal retries take over.
            self.next_audio_retry = now + AUDIO_STALLED;
            self.open_source(self.source.clone());
            // If it won't reopen, treat it like a lost stream rather than a bad pick.
            self.reconnecting |= self.capture.is_none();
            // The stream may simply have nothing to send (music routed elsewhere);
            // don't count the fresh one as stalled right away.
            self.last_audio = Some(now);
        } else if self.capture.is_none() && self.audio_down_since.is_some() {
            self.open_source(self.source.clone());
        }
    }

    /// Re-list the sources. An app picked earlier stays listed (and selected)
    /// even while it isn't playing.
    fn refresh_sources(&mut self) {
        self.sources = list_sources(&self.host);
        if self.source.kind == SourceKind::App && !self.sources.contains(&self.source) {
            let after = self
                .sources
                .iter()
                .rposition(|s| matches!(s.kind, SourceKind::Player | SourceKind::App))
                .map_or(1, |i| i + 1);
            self.sources
                .insert(after.min(self.sources.len()), self.source.clone());
        }
    }

    /// What to show instead of the bars while audio is down, if anything.
    pub(crate) fn audio_outage(&self) -> Option<AudioOutage<'_>> {
        if self.capture.is_some() {
            let since = self.waiting_since?;
            if self.now.saturating_duration_since(since) < AUDIO_OUTAGE_NOTICE {
                return None;
            }
            return Some(AudioOutage::Waiting(match self.source.kind {
                SourceKind::Player => self
                    .track
                    .as_ref()
                    .map(|n| n.player_name.as_str())
                    .filter(|n| !n.is_empty()),
                _ => Some(self.source.label.as_str()),
            }));
        }
        let since = self.audio_down_since?;
        if self.reconnecting {
            (self.now.saturating_duration_since(since) >= AUDIO_OUTAGE_NOTICE)
                .then_some(AudioOutage::Reconnecting)
        } else {
            self.audio_error.as_deref().map(AudioOutage::Failed)
        }
    }

    fn mark_dirty(&mut self) {
        self.dirty_since.get_or_insert(self.now);
    }

    fn save(&mut self) {
        settings::save(&Stored {
            version: settings::CONFIG_VERSION,
            tuning: self.tuning.clone(),
            visuals: self.visuals.clone(),
            appearance: self.appearance.clone(),
            presets: self.presets.clone(),
            tuning_presets: self.tuning_presets.clone(),
            appearance_presets: self.appearance_presets.clone(),
            // System Audio is the default; only remember other picks.
            source: (self.source.kind != SourceKind::SystemOutput).then(|| self.source.clone()),
        });
        self.dirty_since = None;
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        // Any input wakes the app from idle mode right away.
        if !matches!(message, Message::Frame(_)) {
            self.idle = false;
            self.idle_since = None;
        }
        match message {
            Message::Frame(now) => self.tick(now),
            Message::Key(event) => {
                self.last_activity = self.now.max(Instant::now());
                return self.on_key(event);
            }
            Message::Pointer(p) => match p {
                Pointer::Moved(pos) => {
                    self.cursor = Some(pos);
                    self.last_activity = Instant::now();
                }
                Pointer::Left => self.cursor = None,
                Pointer::Pressed => {
                    self.last_activity = Instant::now();
                    self.focus = None;
                }
                Pointer::Activity => self.last_activity = Instant::now(),
                Pointer::Resized(size) => self.window_size = size,
            },

            Message::TogglePanel(p) => {
                // Presets belong to their panel; leaving it finishes any edit.
                self.finish_preset_edit();
                self.editing_preset = None;
                self.confirm_delete = None;
                self.panel = if self.panel == p { Panel::None } else { p };
            }
            Message::ToggleLicense(i) => {
                self.open_license = (self.open_license != Some(i)).then_some(i);
            }
            Message::OpenUrl(url) => crate::about::open_url(url),
            Message::TogglePicker => {
                if !self.picker_open {
                    // Refresh the list each time so newly plugged devices and newly
                    // playing apps show up.
                    self.refresh_sources();
                }
                self.picker_open = !self.picker_open;
                // Keyboard users land on the current source, and return to the pill.
                if self.focus.is_some() {
                    self.focus = Some(if self.picker_open {
                        let i = self
                            .sources
                            .iter()
                            .position(|s| *s == self.source)
                            .unwrap_or(0);
                        FocusId::Indexed("src", i)
                    } else {
                        FocusId::Named("pill")
                    });
                }
            }
            Message::ClosePicker => {
                self.picker_open = false;
                if self.focus.is_some() {
                    self.focus = Some(FocusId::Named("pill"));
                }
            }
            Message::PickSource(i) => {
                self.picker_open = false;
                if self.focus.is_some() {
                    self.focus = Some(FocusId::Named("pill"));
                }
                if let Some(s) = self.sources.get(i).cloned()
                    && (s != self.source || self.capture.is_none())
                {
                    self.mark_dirty();
                    // A fresh pick, not a reconnect: report its errors right away.
                    self.reconnecting = false;
                    self.audio_down_since = None;
                    self.audio_error = None;
                    self.open_source(s);
                }
            }

            Message::SetTuning(t) => {
                self.tuning = t;
                self.mark_dirty();
            }
            Message::SetVisuals(v) => {
                self.visuals = v;
                self.mark_dirty();
            }
            Message::SetAppearance(a) => {
                let task = (a.always_on_top != self.appearance.always_on_top)
                    .then(|| set_on_top(a.always_on_top));
                self.appearance = a;
                self.mark_dirty();
                if let Some(task) = task {
                    return task;
                }
            }
            Message::ResetAppearance => {
                let was_on_top = self.appearance.always_on_top;
                self.appearance = AppearanceSettings::default();
                self.mark_dirty();
                if was_on_top {
                    return set_on_top(false);
                }
            }
            Message::PickAccent(i) => {
                if let Some(a) = self.accent_choices().get(i) {
                    self.appearance.accent = *a;
                    self.mark_dirty();
                }
            }
            Message::PickSolid(i) => {
                if let Some(c) = SolidColor::ALL.get(i) {
                    self.appearance.solid_color = *c;
                    self.mark_dirty();
                }
            }
            Message::ResetTuning => {
                self.tuning = TuningSettings::default();
                self.processor.cancel_calibration();
                self.mark_dirty();
            }
            Message::ResetVisuals => {
                self.visuals = VisualSettings::default();
                self.custom_hex = custom_hex_strings(&self.visuals.palette);
                self.mark_dirty();
            }
            Message::AutoCalibrate => {
                if self.processor.calibration_remaining().is_some() {
                    self.processor.cancel_calibration();
                } else {
                    self.processor.start_calibration();
                }
            }
            Message::PickSwatch(i) => {
                if let Some(p) = self.palette_choices().get(i) {
                    self.visuals.palette = p.clone();
                    self.mark_dirty();
                }
            }
            Message::CustomHex(i, s) => {
                self.custom_hex[i] = s;
                let stops = parse_custom(&self.custom_hex);
                if !stops.is_empty() {
                    self.visuals.palette = Palette::Custom(stops);
                    self.mark_dirty();
                }
            }
            Message::SavePreset(kind) => {
                let name = next_preset_name(&self.preset_list(kind).names());
                match kind {
                    PresetKind::Visuals => self.presets.push(Preset {
                        name,
                        visuals: self.visuals.clone(),
                        tuning: None,
                    }),
                    PresetKind::Tuning => self.tuning_presets.push(TuningPreset {
                        name,
                        tuning: self.tuning.clone(),
                    }),
                    PresetKind::Appearance => self.appearance_presets.push(AppearancePreset {
                        name,
                        appearance: self.appearance.clone(),
                    }),
                }
                self.preset_saved_at = Some(self.now);
                self.save();
                // Open the new preset for naming, with the cursor in its name field.
                self.editing_preset = Some((kind, self.preset_list(kind).count() - 1));
                self.confirm_delete = None;
                return focus_text_input(PRESET_NAME_INPUT);
            }
            Message::EditPreset(kind, edit) => {
                self.finish_preset_edit();
                let count = self.preset_list(kind).count();
                self.editing_preset = edit.filter(|&i| i < count).map(|i| (kind, i));
                self.confirm_delete = None;
                self.mark_dirty();
                if self.editing_preset.is_some() {
                    return focus_text_input(PRESET_NAME_INPUT);
                }
                if let Some(i) = edit {
                    self.focus = self
                        .focus
                        .as_ref()
                        .map(|_| FocusId::Indexed("preset-edit", i));
                }
            }
            Message::RenamePreset(kind, i, name) => {
                if let Some(slot) = self.preset_list_mut(kind).name_at_mut(i) {
                    *slot = name.chars().take(40).collect();
                    self.mark_dirty();
                }
            }
            Message::MovePreset(kind, i, delta) => {
                let j = i as isize + delta;
                let list = self.preset_list_mut(kind);
                if i < list.count() && (0..list.count() as isize).contains(&j) {
                    let j = j as usize;
                    list.swap_items(i, j);
                    self.editing_preset = Some((kind, j));
                    self.confirm_delete = None;
                    self.mark_dirty();
                }
            }
            Message::DeletePreset(kind, i) => {
                if self.confirm_delete == Some((kind, i)) {
                    self.preset_list_mut(kind).remove_at(i);
                    self.editing_preset = None;
                    self.confirm_delete = None;
                    self.mark_dirty();
                    if self.focus.is_some() {
                        self.focus = Some(FocusId::Named("save-preset"));
                    }
                } else {
                    self.confirm_delete = Some((kind, i));
                }
            }
            Message::LoadPreset(kind, i) => match kind {
                PresetKind::Visuals => {
                    if let Some(p) = self.presets.get(i) {
                        self.visuals = p.visuals.clone();
                        self.custom_hex = custom_hex_strings(&self.visuals.palette);
                        self.mark_dirty();
                    }
                }
                PresetKind::Tuning => {
                    if let Some(p) = self.tuning_presets.get(i) {
                        return self.update(Message::SetTuning(p.tuning.clone()));
                    }
                }
                PresetKind::Appearance => {
                    if let Some(p) = self.appearance_presets.get(i) {
                        return self.update(Message::SetAppearance(self.appearance_from_preset(p)));
                    }
                }
            },

            Message::PlayPause => self.media.send(Command::PlayPause),
            Message::Next => self.media.send(Command::Next),
            Message::Previous => self.media.send(Command::Previous),
            Message::ToggleRepeat => {
                if let Some(t) = &self.track {
                    self.media.send(Command::SetRepeat(!t.repeat));
                }
            }
            Message::ToggleMute => {
                if let Some(vol) = self.track.as_ref().and_then(|n| n.volume) {
                    if vol > 0.0 {
                        self.unmuted_volume = Some(vol);
                        self.media.send(Command::SetVolume(0.0));
                    } else {
                        self.media
                            .send(Command::SetVolume(self.unmuted_volume.unwrap_or(0.5)));
                    }
                }
            }
            Message::ToggleVolume => self.volume_open = !self.volume_open,
            Message::ToggleFullscreen => {
                return window::oldest().and_then(|id| {
                    window::mode(id).then(move |mode| {
                        let full = mode != window::Mode::Fullscreen;
                        let next = if full {
                            window::Mode::Fullscreen
                        } else {
                            window::Mode::Windowed
                        };
                        window::set_mode::<Message>(id, next)
                            .chain(Task::done(Message::FullscreenChanged(full)))
                    })
                });
            }
            Message::FullscreenChanged(full) => self.fullscreen = full,
            Message::SetVolume(v) => {
                if v > 0.0 {
                    self.unmuted_volume = None;
                }
                self.media
                    .send(Command::SetVolume(v.clamp(0.0, 1.0) as f64));
            }
            Message::VolumeScroll(dy) => {
                if let Some(vol) = self.track.as_ref().and_then(|t| t.volume) {
                    let step = if dy > 0.0 { 0.05 } else { -0.05 };
                    self.media
                        .send(Command::SetVolume((vol + step).clamp(0.0, 1.0)));
                }
            }
            Message::Seek(f) => {
                if let Some(len) = self.track.as_ref().and_then(|t| t.length) {
                    self.media.send(Command::SeekTo(len.mul_f32(f)));
                }
            }
            Message::SeekLyric(t) => {
                // Lyric time includes the user's offset; land a hair before the line starts.
                // (The offset is evaluated at the target, so the end-of-song taper matches.)
                let offset = self.track.as_ref().map_or(0.0, |tr| {
                    lyric_offset(self.appearance.lyrics_offset_ms, t, tr.length)
                });
                let pos = t - offset - 0.05;
                self.media
                    .send(Command::SeekTo(Duration::from_secs_f32(pos.max(0.0))));
            }
        }
        // Reflect optimistic media changes right away.
        self.track = self.media.get_track();
        Task::none()
    }

    fn tick(&mut self, now: Instant) {
        let dt = self
            .last_frame
            .map(|t| (now - t).as_secs_f32())
            .unwrap_or(0.0);
        self.last_frame = Some(now);
        self.now = now;

        self.keep_audio_alive(now);
        // "Music player": record whichever app the player is.
        if self.source.kind == SourceKind::Player
            && let Some(capture) = &mut self.capture
        {
            let player = self
                .track
                .as_ref()
                .map(|t| (t.player_bus.as_str(), t.player_name.as_str()));
            capture.follow_player(player);
        }
        let waiting = self.capture.as_ref().and_then(Capture::app_streams) == Some(0);
        self.waiting_since = waiting.then(|| self.waiting_since.unwrap_or(now));
        self.analyzer.set_size(self.tuning.detail.fft_size());
        if let Some(capture) = &mut self.capture
            && self.analyzer.pull(capture)
        {
            self.last_audio = Some(now);
        }
        let sample_rate = self
            .capture
            .as_ref()
            .map(Capture::sample_rate)
            .unwrap_or(48_000) as f32;
        // Visual delay: analyze audio from a moment ago, and keep showing it for that
        // long after the input stops.
        let delay = Duration::from_millis(self.tuning.delay_ms as u64);
        let live = self
            .last_audio
            .is_some_and(|t| now - t < STALE_AUDIO + delay);
        let signals = bar_signals(&self.visuals);
        if live {
            let delay_samples = (delay.as_secs_f32() * sample_rate) as usize;
            self.analyzer.compute(signals, delay_samples);
        }
        let level = live.then(|| self.analyzer.level_db());
        let sound = self.gate.update(level, self.tuning.gate_db, dt);

        let fft_size = self.analyzer.size();
        let spectrum = |magnitudes_db| SpectrumFrame {
            magnitudes_db,
            sample_rate,
            fft_size,
        };
        // The glow and pulse follow the mix; the bars use one spectrum per half when split.
        let frame = sound.then(|| spectrum(self.analyzer.spectrum(Signal::Mix)));
        let bar_frames: Vec<SpectrumFrame<'_>> = if sound {
            signals
                .iter()
                .map(|&s| spectrum(self.analyzer.spectrum(s)))
                .collect()
        } else {
            Vec::new()
        };
        let beat_sensitivity = self.tuning.beat_sensitivity;
        // The tuning meter shows beats too, so detect them while it's open (and let
        // a flash finish fading after Glow or the meter goes away).
        if self.visuals.glow || self.panel == Panel::Tuning || self.beat.pulse() > 0.01 {
            self.beat.update(
                frame.as_ref(),
                self.visuals.glow_source,
                beat_sensitivity,
                dt,
            );
        }
        // Background pulse: quick to swell, slow to settle, so it breathes rather than flickers.
        let a = &self.appearance;
        let target = if a.bg_pulse && a.bg_mode.is_dynamic() {
            self.bg_beat
                .update(frame.as_ref(), a.bg_pulse_source, beat_sensitivity, dt)
                * a.bg_pulse_amount as f32
                / 100.0
        } else {
            0.0
        };
        let tau = if target > self.bg_pulse { 0.06 } else { 0.35 };
        self.bg_pulse += (target - self.bg_pulse) * (1.0 - (-dt / tau).exp());
        let bins = BarMap::for_visuals(&self.visuals).bins(self.visuals.bar_count as usize);
        if let Some(sensitivity) = self.processor.update(&bar_frames, bins, &self.tuning, dt) {
            self.tuning.sensitivity = sensitivity;
            self.mark_dirty();
        }
        Meter::follow(&mut self.meter.input_db, level, dt);
        Meter::follow(&mut self.meter.loudest_db, self.processor.loudest_db(), dt);

        self.track = self.media.get_track();
        if self.track.as_ref().is_some_and(|t| t.playing) {
            // Slow drift so the seek wave reads as "playing".
            self.wave_phase = (self.wave_phase + dt * 2.0) % std::f32::consts::TAU;
        }

        self.update_art(now);
        self.update_lyrics();

        // Background motion pauses with the music (and in idle mode).
        let playing = self.track.as_ref().is_some_and(|t| t.playing);
        if playing || sound {
            self.bg_time += dt;
        }

        // Ease the accent toward its target (album accents change with the track).
        let target = rgb_f(self.target_accent());
        let k = 1.0 - (-dt / ACCENT_TAU).exp();
        for (c, t) in self.accent.iter_mut().zip(target) {
            *c += (t - *c) * k;
        }
        // Album bar colors ease the same way when the track changes; other palettes
        // switch at once, like clicking a swatch.
        let target: Vec<[f32; 3]> = self.target_bar_stops().into_iter().map(rgb_f).collect();
        if self.visuals.palette == Palette::Album && self.bar_colors.len() == target.len() {
            for (c, t) in self.bar_colors.iter_mut().zip(&target) {
                for (a, b) in c.iter_mut().zip(t) {
                    *a += (b - *a) * k;
                }
            }
        } else {
            self.bar_colors = target;
        }

        self.update_auto_hide(now, dt);
        self.update_idle(now, playing || sound);

        if self.dirty_since.is_some_and(|t| now - t >= SAVE_DELAY) {
            self.save();
        }
    }

    fn update_art(&mut self, now: Instant) {
        let art_url = self.track.as_ref().and_then(|t| t.art_url.as_deref());
        self.art_loader.want(art_url, self.appearance.blur_px);
        let (art, blurred) = self.art_loader.snapshot();
        self.art = art;

        let changed_track = match (&self.blurred, &blurred) {
            (Some(old), Some(new)) => old.url != new.url,
            (Some(_), None) => art_url.is_none() || self.art.is_none(),
            _ => false,
        };
        if changed_track && let Some(old) = self.blurred.take() {
            self.fading_out = Some((old, now));
        }
        if blurred.is_some() || self.art.is_none() {
            self.blurred = blurred;
        }
        if self
            .fading_out
            .as_ref()
            .is_some_and(|(_, t)| now - *t >= ART_FADE)
        {
            self.fading_out = None;
        }
    }

    fn update_lyrics(&mut self) {
        let track = self
            .track
            .as_ref()
            .filter(|_| self.appearance.lyrics)
            .and_then(|t| {
                (!t.title.is_empty()).then(|| Song {
                    title: t.title.clone(),
                    artist: t.artist.clone(),
                    album: t.album.clone(),
                    duration_s: t
                        .length
                        .map(|d| d.as_secs_f32().round() as u32)
                        .unwrap_or(0),
                    url: t.url.clone(),
                })
            });
        self.lyrics_loader.want(track);
        self.lyrics = match self.lyrics_loader.status() {
            LyricsStatus::Found(l) => Some(l),
            LyricsStatus::Loading | LyricsStatus::NotFound => None,
        };
    }

    /// The lyrics view for `placement`, if there's something to show there.
    pub(crate) fn lyrics_view(&self, placement: Placement) -> Option<Element<'_, Message>> {
        let lyrics = self.lyrics.as_deref()?;
        let t = self.track.as_ref()?;
        let usable = match (lyrics, placement) {
            (Lyrics::Synced { .. }, _) => true,
            // Unsynced lyrics only make sense in the tall column.
            (Lyrics::Plain { .. }, Placement::Column) => true,
            _ => false,
        };
        if !usable {
            return None;
        }
        let scale = self.appearance.lyrics_size as f32 / 100.0;
        let font_size = match placement {
            // A narrow stage gets slightly smaller lines so they wrap less.
            Placement::Strip => 26.0 * self.layout().lyrics_scale(),
            Placement::Column => 30.0,
        } * scale;
        let position = t.position_at(self.now).as_secs_f32();
        let time = position + lyric_offset(self.appearance.lyrics_offset_ms, position, t.length);
        let progress = t
            .length
            .map(|len| (t.position_at(self.now).as_secs_f32() / len.as_secs_f32()).clamp(0.0, 1.0));
        let view = Canvas::new(LyricsView {
            lyrics,
            time,
            progress,
            placement,
            font_size,
            glow: self.appearance.lyrics_glow as f32 / 100.0,
            on_seek: Message::SeekLyric,
        })
        .width(Length::Fill);
        Some(match placement {
            Placement::Strip => view.height(font_size * 4.6).into(),
            Placement::Column => view.height(Length::Fill).into(),
        })
    }

    pub(crate) fn target_accent(&self) -> [u8; 3] {
        match self.appearance.accent.rgb() {
            Some(c) => c,
            None => self
                .art
                .as_ref()
                .and_then(|a| a.accent)
                .unwrap_or(ACCENT_BLUE),
        }
    }

    /// Accent swatches, in order. "From album art" needs a music player connection.
    pub(crate) fn accent_choices(&self) -> Vec<Accent> {
        Accent::ALL
            .into_iter()
            .filter(|a| self.platform.media || *a != Accent::Album)
            .collect()
    }

    /// Bar palette swatches, in order; the last is the custom palette (with the
    /// colors typed so far). "From album art" needs a music player connection.
    pub(crate) fn palette_choices(&self) -> Vec<Palette> {
        let mut choices = vec![
            Palette::Silver,
            Palette::Undavi,
            Palette::Aurora,
            Palette::Ember,
        ];
        if self.platform.media {
            choices.push(Palette::Album);
        }
        choices.push(Palette::Custom(parse_custom(&self.custom_hex)));
        choices
    }

    pub(crate) fn preset_list(&self, kind: PresetKind) -> &dyn PresetList {
        match kind {
            PresetKind::Visuals => &self.presets,
            PresetKind::Tuning => &self.tuning_presets,
            PresetKind::Appearance => &self.appearance_presets,
        }
    }

    fn preset_list_mut(&mut self, kind: PresetKind) -> &mut dyn PresetList {
        match kind {
            PresetKind::Visuals => &mut self.presets,
            PresetKind::Tuning => &mut self.tuning_presets,
            PresetKind::Appearance => &mut self.appearance_presets,
        }
    }

    /// Settings an Appearance preset loads: everything except "Always on top",
    /// which stays as it is (it's about the window, not the look).
    pub(crate) fn appearance_from_preset(&self, p: &AppearancePreset) -> AppearanceSettings {
        AppearanceSettings {
            always_on_top: self.appearance.always_on_top,
            ..p.appearance.clone()
        }
    }

    /// Close the preset editor's name field: an emptied name gets a fresh default
    /// rather than disappearing.
    fn finish_preset_edit(&mut self) {
        let Some((kind, i)) = self.editing_preset else {
            return;
        };
        let list = self.preset_list_mut(kind);
        let mut others = list.names();
        if i >= others.len() {
            return;
        }
        let trimmed = others.remove(i).trim().to_string();
        let name = if trimmed.is_empty() {
            next_preset_name(&others)
        } else {
            trimmed
        };
        if let Some(slot) = list.name_at_mut(i) {
            *slot = name;
        }
    }

    /// Bar colors the palette asks for right now. Album colors come from the cover:
    /// one color (Solid), the cover's gradient, or a gradient around the chosen accent.
    fn target_bar_stops(&self) -> Vec<[u8; 3]> {
        if self.visuals.palette != Palette::Album {
            return self.visuals.palette.stops();
        }
        let art = self.art.as_ref();
        let base = art
            .and_then(|a| a.accent)
            .unwrap_or_else(|| self.target_accent());
        if !self.visuals.album_gradient {
            return vec![base];
        }
        if self.visuals.album_match_accent
            && let Some(accent) = self.appearance.accent.rgb()
        {
            return ramp(accent).to_vec();
        }
        match art {
            Some(a) => a.gradient.to_vec(),
            None => ramp(base).to_vec(),
        }
    }

    /// Bar colors to draw this frame (eased while Album colors change).
    /// Never empty, so drawing code can always take the first color.
    pub(crate) fn bar_stops(&self) -> Vec<[u8; 3]> {
        let target = self.target_bar_stops();
        if target.is_empty() {
            return Palette::Silver.stops();
        }
        if self.bar_colors.len() != target.len() {
            return target;
        }
        self.bar_colors
            .iter()
            .map(|c| c.map(|v| (v * 255.0).round().clamp(0.0, 255.0) as u8))
            .collect()
    }

    /// Clock for background motion, in seconds.
    pub(crate) fn background_time(&self) -> f32 {
        self.bg_time
    }

    /// Radial style: the center cover turns once every 24 s while music plays.
    pub(crate) fn cover_spin(&self) -> f32 {
        std::f32::consts::TAU * (self.bg_time / 24.0).fract()
    }

    /// Enter idle mode once nothing has moved for a moment; leave it as soon as anything does.
    fn update_idle(&mut self, now: Instant, sound: bool) {
        let settled = |v: &[f32]| v.iter().all(|&x| x < 0.003);
        // Frozen bars don't move, so they don't keep the app awake.
        let frozen = !sound && self.tuning.when_silent == SilenceMode::Freeze;
        let bars_settled =
            frozen || (settled(self.processor.visible()) && settled(self.processor.peaks()));
        let target_accent = rgb_f(self.target_accent());
        let busy = sound
            || !bars_settled
            || self.beat.pulse() > 0.01
            || self.bg_pulse > 0.005
            || (self.ui_alpha > 0.0 && self.ui_alpha < 1.0)
            || self.fading_out.is_some()
            || self.processor.calibration_remaining().is_some()
            || self
                .accent
                .iter()
                .zip(target_accent)
                .any(|(a, b)| (a - b).abs() > 0.002)
            || self
                .bar_colors
                .iter()
                .zip(self.target_bar_stops().into_iter().map(rgb_f))
                .any(|(a, b)| a.iter().zip(b).any(|(x, y)| (x - y).abs() > 0.002))
            || self
                .preset_saved_at
                .is_some_and(|t| now - t < Duration::from_secs(2));
        if busy {
            self.idle_since = None;
            self.idle = false;
        } else {
            let since = *self.idle_since.get_or_insert(now);
            self.idle = now - since >= IDLE_AFTER;
        }
    }

    pub fn accent(&self) -> Color {
        Color::from_rgb(self.accent[0], self.accent[1], self.accent[2])
    }

    fn update_auto_hide(&mut self, now: Instant, dt: f32) {
        let a = &self.appearance;
        let idle = now.saturating_duration_since(self.last_activity)
            >= Duration::from_secs(a.hide_after_s as u64);
        let over_controls = self
            .cursor
            .is_some_and(|p| p.y < HEADER_ZONE || p.y > self.window_size.height - CONTROLS_ZONE);
        let hide = a.auto_hide
            && idle
            && !over_controls
            && self.panel == Panel::None
            && !self.picker_open
            && self.focus.is_none();
        if hide {
            self.ui_alpha = (self.ui_alpha - dt / FADE_OUT).max(0.0);
            if self.ui_alpha <= 0.0 {
                self.volume_open = false;
            }
        } else {
            self.ui_alpha = (self.ui_alpha + dt / FADE_IN).min(1.0);
        }
    }

    pub(crate) fn over_art(&self) -> bool {
        self.appearance.bg_mode.is_dynamic()
    }

    /// Track-info text colors, brighter over dynamic backgrounds.
    pub(crate) fn info_colors(&self) -> (Color, Color) {
        if self.over_art() {
            (TEXT_2_ON_ART, TEXT_3_ON_ART)
        } else {
            (TEXT_2, TEXT_3)
        }
    }

    pub(crate) fn cover_thumb(&self) -> Option<&Handle> {
        self.art
            .as_ref()
            .filter(|_| self.appearance.show_album_art)
            .map(|a| &a.thumb)
    }

    fn on_key(&mut self, event: keyboard::Event) -> Task<Message> {
        let keyboard::Event::KeyPressed { key, modifiers, .. } = event else {
            return Task::none();
        };
        if modifiers.control() || modifiers.alt() || modifiers.logo() {
            return Task::none();
        }
        // Forget focus that points at a control no longer on screen.
        if self
            .focus
            .as_ref()
            .is_some_and(|id| !self.focus_reg.contains(id))
        {
            self.focus = None;
        }

        let step = match key.as_ref() {
            Key::Named(Named::ArrowLeft | Named::ArrowDown) => Some(Step::Less),
            Key::Named(Named::ArrowRight | Named::ArrowUp) => Some(Step::More),
            Key::Named(Named::PageDown) => Some(Step::MuchLess),
            Key::Named(Named::PageUp) => Some(Step::MuchMore),
            Key::Named(Named::Home) => Some(Step::Min),
            Key::Named(Named::End) => Some(Step::Max),
            _ => None,
        };
        if let (Some(id), Some(step)) = (&self.focus, step)
            && let Some(m) = self.focus_reg.step(id, step)
        {
            return self.update(m);
        }

        let message = match key.as_ref() {
            Key::Named(Named::Tab) => return self.move_focus(modifiers.shift()),
            Key::Named(Named::Enter | Named::Space) if self.focus.is_some() => self
                .focus
                .as_ref()
                .and_then(|id| self.focus_reg.activate(id)),
            Key::Named(Named::Space) => Some(Message::PlayPause),
            Key::Named(Named::Escape) => {
                if self.picker_open {
                    Some(Message::ClosePicker)
                } else if let Some((kind, _)) = self.editing_preset {
                    Some(Message::EditPreset(kind, None))
                } else if self.volume_open {
                    Some(Message::ToggleVolume)
                } else if self.panel != Panel::None {
                    Some(Message::TogglePanel(self.panel))
                } else if self.fullscreen {
                    Some(Message::ToggleFullscreen)
                } else {
                    self.focus = None;
                    None
                }
            }
            Key::Named(Named::ArrowLeft | Named::ArrowRight) => {
                let t = self.track.as_ref().filter(|t| t.can_seek);
                t.and_then(|np| {
                    let len = np.length?;
                    let pos = np.position_at(self.now);
                    let target = if key == Key::Named(Named::ArrowLeft) {
                        pos.saturating_sub(SEEK_STEP)
                    } else {
                        (pos + SEEK_STEP).min(len)
                    };
                    Some(Message::Seek(target.as_secs_f32() / len.as_secs_f32()))
                })
            }
            Key::Character("t") => Some(Message::TogglePanel(Panel::Tuning)),
            Key::Character("v") => Some(Message::TogglePanel(Panel::Visuals)),
            Key::Character("a") => Some(Message::TogglePanel(Panel::Appearance)),
            Key::Character("f") | Key::Named(Named::F11) => Some(Message::ToggleFullscreen),
            _ => None,
        };
        match message {
            Some(m) => self.update(m),
            None => Task::none(),
        }
    }

    /// Tab / Shift+Tab. Text inputs get real iced focus; panel controls scroll into view.
    fn move_focus(&mut self, back: bool) -> Task<Message> {
        let trap = self.picker_open.then_some(Scope::Picker);
        let previous = self.focus.take();
        let was_text = previous
            .as_ref()
            .and_then(|id| self.focus_reg.text_input(id));
        self.focus = self.focus_reg.cycle(previous.as_ref(), back, trap);

        let mut tasks = Vec::new();
        if let Some(id) = &self.focus {
            match self.focus_reg.text_input(id) {
                Some(widget) => tasks.push(focus_text_input(widget)),
                None if was_text.is_some() => tasks.push(focus_text_input(NO_TEXT_INPUT)),
                None => {}
            }
            if let Some((i, n, Scope::Panel)) = self.focus_reg.position(id) {
                let y = i as f32 / n.saturating_sub(1).max(1) as f32;
                tasks.push(iced::widget::operation::snap_to(
                    iced::widget::Id::new(PANEL_SCROLL),
                    iced::widget::scrollable::RelativeOffset {
                        x: None,
                        y: Some(y),
                    },
                ));
            }
        }
        Task::batch(tasks)
    }
}

// ---------------------------------------------------------------------------
// Helpers shared with the view

/// Widget ids used for programmatic focus and scrolling.
pub(crate) const PANEL_SCROLL: &str = "panel-scroll";
pub(crate) const PRESET_NAME_INPUT: &str = "preset-name";
/// Focusing an id no widget has blurs every text input.
const NO_TEXT_INPUT: &str = "no-text-input";

fn focus_text_input(id: &'static str) -> Task<Message> {
    iced::widget::operation::focus(iced::widget::Id::new(id))
}

/// "Preset N" with the first N not already taken.
fn next_preset_name(taken: &[String]) -> String {
    (1..)
        .map(|n| format!("Preset {n}"))
        .find(|name| !taken.contains(name))
        .unwrap_or_default()
}

/// The list operations the preset UI needs, for any kind of preset.
pub(crate) trait PresetList {
    fn count(&self) -> usize;
    fn names(&self) -> Vec<String>;
    fn name_at_mut(&mut self, i: usize) -> Option<&mut String>;
    fn swap_items(&mut self, i: usize, j: usize);
    fn remove_at(&mut self, i: usize);
}

impl<T: settings::Named> PresetList for Vec<T> {
    fn count(&self) -> usize {
        self.len()
    }
    fn names(&self) -> Vec<String> {
        self.iter()
            .map(|p| settings::Named::name(p).to_string())
            .collect()
    }
    fn name_at_mut(&mut self, i: usize) -> Option<&mut String> {
        self.get_mut(i).map(settings::Named::name_mut)
    }
    fn swap_items(&mut self, i: usize, j: usize) {
        if i < self.len() && j < self.len() {
            self.swap(i, j);
        }
    }
    fn remove_at(&mut self, i: usize) {
        if i < self.len() {
            self.remove(i);
        }
    }
}

/// The lyrics timing offset in seconds at `position`.
///
/// A negative offset (lyrics later) tapers to zero over the last few seconds of the
/// song and is gone for the final second, so lines sung right at the end still
/// light up at their true times before the track changes.
const END_MARGIN_S: f32 = 1.0;

pub(crate) fn lyric_offset(offset_ms: i16, position: f32, length: Option<Duration>) -> f32 {
    let offset = offset_ms as f32 / 1000.0;
    match length {
        Some(len) if offset < 0.0 => {
            let window = 3.0 * -offset;
            let remaining = (len.as_secs_f32() - position - END_MARGIN_S).max(0.0);
            offset * (remaining / window).min(1.0)
        }
        _ => offset,
    }
}

fn rgb_f(c: [u8; 3]) -> [f32; 3] {
    c.map(|v| v as f32 / 255.0)
}

fn set_on_top(on: bool) -> Task<Message> {
    let level = if on {
        window::Level::AlwaysOnTop
    } else {
        window::Level::Normal
    };
    window::oldest().and_then(move |id| window::set_level(id, level))
}

pub(crate) fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let s = s.trim().trim_start_matches('#');
    let expand = |c: char| c.to_digit(16).map(|d| (d * 17) as u8);
    match s.len() {
        3 => {
            let mut it = s.chars().map(expand);
            Some([it.next()??, it.next()??, it.next()??])
        }
        6 => {
            let v = u32::from_str_radix(s, 16).ok()?;
            Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
        }
        _ => None,
    }
}

fn parse_custom(fields: &[String; 3]) -> Vec<[u8; 3]> {
    let stops: Vec<_> = fields.iter().filter_map(|s| parse_hex(s)).collect();
    if stops.is_empty() {
        DEFAULT_CUSTOM.to_vec()
    } else {
        stops
    }
}

fn custom_hex_strings(palette: &Palette) -> [String; 3] {
    let stops = match palette {
        Palette::Custom(stops) if !stops.is_empty() => stops.clone(),
        _ => DEFAULT_CUSTOM.to_vec(),
    };
    std::array::from_fn(|i| {
        stops
            .get(i)
            .map(|c| format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2]))
            .unwrap_or_default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::FFT_SIZE;
    use crate::settings::{BackgroundMode, Corners, GradientDir, Mirror, NowPlayingMode};

    /// Renders each screen at the 1440×900 reference size (2× scale, like the mockups).
    /// Run with `cargo test snapshots -- --ignored`; PNGs land in `target/snapshots/`.
    #[test]
    #[ignore]
    fn snapshots() {
        use iced_test::simulator::Simulator;

        let render_at = |name: &str, width: f32, height: f32, setup: &dyn Fn(&mut App)| {
            let (mut app, _) = App::new();
            app.window_size = Size::new(width, height);
            app.capture = None; // keep the synthetic spectrum below
            // Render the defaults, not whatever is saved in the user's config.
            app.tuning = TuningSettings::default();
            app.visuals = VisualSettings::default();
            app.appearance = AppearanceSettings::default();
            app.platform = Platform::full();
            app.accent = rgb_f(app.target_accent());
            let now = Instant::now();
            app.now = now;
            app.track = Some(Track {
                title: "Somewhere, slowly".into(),
                artist: "Luna Bay".into(),
                album: "Moon / Tide".into(),
                art_url: None,
                url: None,
                length: Some(Duration::from_secs(200)),
                position: Duration::from_secs(96),
                sampled_at: now,
                playing: true,
                repeat: true,
                volume: Some(0.8),
                can_seek: true,
                player_bus: "spotify".into(),
                player_name: "Spotify".into(),
            });
            setup(&mut app);

            // A plausible music spectrum: loud lows rolling off toward the highs, with jitter.
            let mags: Vec<f32> = (0..=FFT_SIZE / 2)
                .map(|k| {
                    let f = (k as f32 * 48_000.0 / FFT_SIZE as f32).max(20.0);
                    let jitter = ((k as f32 * 12.9898).sin() * 43758.545).fract() * 10.0;
                    -12.0 - 13.0 * (f / 60.0).log2().max(0.0) * 0.9 - jitter
                })
                .collect();
            let frame = SpectrumFrame {
                magnitudes_db: &mags,
                sample_rate: 48_000.0,
                fft_size: FFT_SIZE,
            };
            // In stereo the right channel gets a slightly different spectrum, as panned
            // instruments would give it.
            let right: Vec<f32> = mags
                .iter()
                .enumerate()
                .map(|(k, db)| db - 6.0 * ((k as f32 * 0.05).sin() + 1.0))
                .collect();
            let map = BarMap::for_visuals(&app.visuals);
            let frames = if map == BarMap::Split {
                vec![
                    frame,
                    SpectrumFrame {
                        magnitudes_db: &right,
                        sample_rate: 48_000.0,
                        fft_size: FFT_SIZE,
                    },
                ]
            } else {
                vec![frame]
            };
            let bins = map.bins(app.visuals.bar_count as usize);
            for _ in 0..120 {
                app.processor.update(&frames, bins, &app.tuning, 1.0 / 60.0);
            }

            let settings = iced::Settings {
                fonts: FONTS
                    .iter()
                    .map(|f| std::borrow::Cow::Borrowed(*f))
                    .collect(),
                default_font: INTER,
                ..iced::Settings::default()
            };
            let mut sim = Simulator::with_size(settings, (width, height), app.view());
            let snapshot = sim.snapshot(&iced::Theme::Dark).expect("snapshot");
            let path = format!("{}/target/snapshots/{name}", env!("CARGO_MANIFEST_DIR"));
            snapshot.matches_image(&path).expect("write snapshot");
        };

        let render_sized = |name: &str, height: f32, setup: &dyn Fn(&mut App)| {
            render_at(name, 1440.0, height, setup)
        };
        let render = |name: &str, setup: &dyn Fn(&mut App)| render_sized(name, 900.0, setup);
        // `matches_image` only writes when no file exists, so start from a clean directory.
        let _ = std::fs::remove_dir_all(format!("{}/target/snapshots", env!("CARGO_MANIFEST_DIR")));
        render("screen", &|_| {});
        render("max-length-long", &|app| app.visuals.max_length = 1200);
        render("max-length-short", &|app| {
            app.panel = Panel::Visuals;
            app.visuals.max_length = 150;
        });
        render("max-length-wave", &|app| {
            app.visuals.style = settings::VisualStyle::Wave;
            app.visuals.max_length = 1200;
        });
        render("max-length-radial", &|app| {
            app.visuals.style = settings::VisualStyle::Radial;
            app.visuals.max_length = 120;
        });
        // Narrow windows (the sizes from the scaling bug report).
        render_at("narrow-923-about", 923.0, 1031.0, &|app| {
            app.panel = Panel::About
        });
        render_at("narrow-923-tuning", 923.0, 1031.0, &|app| {
            app.panel = Panel::Tuning
        });
        render_at("narrow-606-about", 606.0, 1031.0, &|app| {
            app.panel = Panel::About
        });
        render_at("narrow-606", 606.0, 1031.0, &|_| {});
        render_at("narrow-480-picker", 480.0, 600.0, &|app| {
            app.picker_open = true
        });
        render_at("narrow-spotlight", 760.0, 900.0, &|app| {
            app.appearance.now_playing = NowPlayingMode::Spotlight;
        });
        render("tuning", &|app| app.panel = Panel::Tuning);
        render("visuals", &|app| app.panel = Panel::Visuals);
        render("visuals-custom", &|app| {
            app.panel = Panel::Visuals;
            app.visuals.segments = true;
            app.visuals.peak_caps = true;
            app.visuals.corners = Corners::Pill;
            app.visuals.mirror = Mirror::Both;
            app.visuals.palette = Palette::Aurora;
            app.visuals.gradient = GradientDir::Along;
        });
        render("picker", &|app| app.picker_open = true);
        render("about", &|app| app.panel = Panel::About);
        render("about-license", &|app| {
            app.panel = Panel::About;
            app.open_license = Some(0);
        });
        render("audio-reconnecting", &|app| {
            app.capture = None;
            app.reconnecting = true;
            app.audio_down_since = Some(app.now - Duration::from_secs(3));
        });
        render("audio-failed", &|app| {
            app.capture = None;
            app.reconnecting = false;
            app.audio_error = Some("Couldn't open USB Headset: audio device not found".into());
            app.audio_down_since = Some(app.now);
        });
        render("focus-tuning", &|app| {
            app.panel = Panel::Tuning;
            app.focus = Some(FocusId::Named("sensitivity"));
        });
        render_sized("focus-visuals-presets", 1900.0, &|app| {
            app.panel = Panel::Visuals;
            app.presets = vec![
                Preset {
                    name: "Default look".into(),
                    visuals: app.visuals.clone(),
                    tuning: None,
                },
                Preset {
                    name: "Night drive".into(),
                    visuals: VisualSettings {
                        mirror: Mirror::Both,
                        ..app.visuals.clone()
                    },
                    tuning: None,
                },
                Preset {
                    name: "Club".into(),
                    visuals: VisualSettings {
                        segments: true,
                        ..app.visuals.clone()
                    },
                    tuning: None,
                },
            ];
            app.editing_preset = Some((PresetKind::Visuals, 1));
            app.confirm_delete = Some((PresetKind::Visuals, 1));
            app.focus = Some(FocusId::Indexed("preset-delete", 1));
        });
        render_sized("tuning-presets", 1900.0, &|app| {
            app.panel = Panel::Tuning;
            app.tuning_presets = vec![
                TuningPreset {
                    name: "Default".into(),
                    tuning: app.tuning.clone(),
                },
                TuningPreset {
                    name: "Bass heavy".into(),
                    tuning: TuningSettings {
                        bass_db: 9,
                        ..app.tuning.clone()
                    },
                },
            ];
        });
        render_sized("appearance-presets", 2200.0, &|app| {
            app.panel = Panel::Appearance;
            app.appearance_presets = vec![AppearancePreset {
                name: "Movie night".into(),
                appearance: AppearanceSettings {
                    dim_pct: 70,
                    ..app.appearance.clone()
                },
            }];
        });
        render("volume-popover", &|app| {
            app.volume_open = true;
            app.focus = Some(FocusId::Named("volume-slider"));
        });
        // Taller windows show the whole (scrolling) panels.
        render_sized("tuning-full", 2500.0, &|app| {
            app.panel = Panel::Tuning;
            app.tuning.auto_gain = true;
            // Live readings for the meter.
            app.gate.set_open(true);
            app.meter = Meter {
                input_db: Some(-31.0),
                loudest_db: Some(-14.0),
            };
            app.beat.set_pulse(0.8);
        });
        render("tuning-silent", &|app| {
            app.panel = Panel::Tuning;
            app.tuning.gate_db = -60;
            app.meter = Meter {
                input_db: Some(-74.0),
                loudest_db: None,
            };
        });
        render_sized("visuals-glow", 1300.0, &|app| {
            app.panel = Panel::Visuals;
            app.visuals.glow = true;
            app.visuals.glow_amount = 100;
            app.visuals.palette = Palette::Aurora;
            app.beat.set_pulse(1.0);
        });

        // A synthetic cover: navy with a peach sun and teal hills.
        let cover = image::RgbImage::from_fn(300, 300, |x, y| {
            let (fx, fy) = (x as f32 / 300.0, y as f32 / 300.0);
            let sun = ((fx - 0.62).powi(2) + (fy - 0.38).powi(2)).sqrt() < 0.2;
            let hill = fy > 0.7 + 0.08 * (fx * 7.0).sin();
            image::Rgb(if sun {
                [0xF2, 0xA6, 0x7F]
            } else if hill {
                [0x2F, 0x6F, 0x8F]
            } else {
                [0x24, 0x28, 0x5C]
            })
        });
        let art = Arc::new(Art::from_image(&cover));
        let blurred = Arc::new(Blurred {
            url: "test".into(),
            blur_px: 80,
            handle: crate::art::blur_cover(&cover, 80),
        });
        let with_art = |app: &mut App| {
            app.art = Some(art.clone());
            app.blurred = Some(blurred.clone());
            app.accent = rgb_f(art.accent.unwrap_or(ACCENT_BLUE));
        };
        // Made-up lyrics; position is 96 s, partway through the word-synced third line.
        let lyrics = Arc::new(Lyrics::Synced {
            entries: crate::lyrics::parse_lrc(
                "[01:20.00]Paper lanterns drifting over the bay\n\
                 [01:24.50]Counting every light until the morning\n\
                 [01:28.80]We were never in a hurry anyway\n\
                 [01:33.40]<01:33.40>Somewhere <01:34.30>slowly <01:35.20>the <01:35.50>tide <01:36.20>is <01:36.60>turning\n\
                 [01:38.20]Hold the quiet like a melody\n\
                 [01:42.60]Let the evening find us where we are\n\
                 [01:47.00]Every wave is coming back to me\n\
                 [01:51.40]Moon above and one unhurried star",
            ),
            source: crate::lyrics::Source::Lrclib,
        });
        let with_lyrics = |app: &mut App| {
            with_art(app);
            app.lyrics = Some(lyrics.clone());
        };
        render("lyrics-header", &|app| with_lyrics(app));
        render("lyrics-spotlight", &|app| {
            with_lyrics(app);
            app.appearance.now_playing = NowPlayingMode::Spotlight;
        });
        render("lyrics-minimal-tuning", &|app| {
            with_lyrics(app);
            app.appearance.now_playing = NowPlayingMode::Minimal;
            app.panel = Panel::Tuning;
        });
        render("style-radial", &|app| {
            with_art(app);
            app.visuals.style = settings::VisualStyle::Radial;
            app.visuals.bar_count = 72;
            app.visuals.bar_width = 6;
            app.visuals.corners = Corners::Pill;
            app.visuals.palette = Palette::Aurora;
        });
        render("style-radial-both", &|app| {
            with_art(app);
            app.panel = Panel::Visuals;
            app.visuals.style = settings::VisualStyle::Radial;
            app.visuals.mirror = Mirror::Both;
            app.visuals.segments = true;
            app.visuals.peak_caps = true;
            app.visuals.palette = Palette::Ember;
            app.visuals.gradient = GradientDir::Along;
        });
        render_sized("album-gradient", 2000.0, &|app| {
            with_art(app);
            app.panel = Panel::Visuals;
            app.visuals.palette = Palette::Album;
            app.visuals.gradient = GradientDir::Along;
            app.appearance.accent = Accent::Mint;
        });
        render_sized("album-solid", 2000.0, &|app| {
            with_art(app);
            app.panel = Panel::Visuals;
            app.visuals.palette = Palette::Album;
            app.visuals.album_gradient = false;
        });
        render("album-match-accent", &|app| {
            with_art(app);
            app.visuals.palette = Palette::Album;
            app.visuals.album_match_accent = true;
            app.appearance.accent = Accent::Rose;
        });
        render("stereo-bars", &|app| {
            with_art(app);
            app.panel = Panel::Visuals;
            app.visuals.mirror = Mirror::LeftRight;
            app.visuals.palette = Palette::Aurora;
        });
        render("stereo-off", &|app| {
            with_art(app);
            app.visuals.mirror = Mirror::LeftRight;
            app.visuals.split = settings::Split::Off;
            app.visuals.palette = Palette::Aurora;
        });
        render("style-wave", &|app| {
            with_art(app);
            app.visuals.style = settings::VisualStyle::Wave;
            app.visuals.bar_count = 64;
            app.visuals.palette = Palette::Aurora;
            app.visuals.glow = true;
            app.visuals.glow_amount = 60;
            app.beat.set_pulse(0.8);
        });
        render("style-wave-off", &|app| {
            with_art(app);
            app.panel = Panel::Visuals;
            app.visuals.style = settings::VisualStyle::Wave;
            app.visuals.mirror = Mirror::Off;
            app.visuals.palette = Palette::Undavi;
            app.visuals.gradient = GradientDir::Along;
        });
        render_sized("appearance-pulse", 1500.0, &|app| {
            with_art(app);
            app.panel = Panel::Appearance;
            app.appearance.bg_pulse = true;
            app.bg_pulse = 1.0;
        });
        render("appearance", &|app| {
            with_art(app);
            app.panel = Panel::Appearance;
        });
        render("appearance-album-accent", &|app| {
            with_art(app);
            app.appearance.accent = Accent::Album;
            app.panel = Panel::Appearance;
            app.appearance.bg_mode = BackgroundMode::Ambient;
        });
        render("spotlight", &|app| {
            with_art(app);
            app.appearance.now_playing = NowPlayingMode::Spotlight;
        });
        render("minimal-solid", &|app| {
            with_art(app);
            app.appearance.now_playing = NowPlayingMode::Minimal;
            app.appearance.bg_mode = BackgroundMode::Solid;
            app.appearance.solid_color = SolidColor::Midnight;
            app.panel = Panel::Appearance;
        });
        render("auto-hidden", &|app| {
            with_art(app);
            app.ui_alpha = 0.0;
        });
    }

    fn press(app: &mut App, key: Key, shift: bool) {
        let _ = app.view(); // rebuilds the focus registry, like a real frame
        let modifiers = if shift {
            keyboard::Modifiers::SHIFT
        } else {
            keyboard::Modifiers::empty()
        };
        let _ = app.update(Message::Key(keyboard::Event::KeyPressed {
            key: key.clone(),
            modified_key: key,
            physical_key: keyboard::key::Physical::Unidentified(
                keyboard::key::NativeCode::Unidentified,
            ),
            location: keyboard::Location::Standard,
            modifiers,
            text: None,
            repeat: false,
        }));
    }

    fn named(k: Named) -> Key {
        Key::Named(k)
    }

    fn test_app() -> App {
        let (mut app, _) = App::new();
        app.tuning = TuningSettings::default();
        app.visuals = VisualSettings::default();
        app.appearance = AppearanceSettings::default();
        app.presets.clear();
        app.tuning_presets.clear();
        app.appearance_presets.clear();
        // The same controls on any machine.
        app.platform = Platform::full();
        // Colors eased from the user's saved settings would keep the app busy.
        app.accent = rgb_f(app.target_accent());
        app.bar_colors.clear();
        app
    }

    #[test]
    fn keyboard_focus_tabs_and_adjusts() {
        let mut app = test_app();
        app.panel = Panel::Tuning;

        // Tab starts at the first control (the logo, which opens About), then the
        // source pill, and wraps both ways.
        press(&mut app, named(Named::Tab), false);
        assert_eq!(app.focus, Some(FocusId::Named("about")));
        press(&mut app, named(Named::Tab), false);
        assert_eq!(app.focus, Some(FocusId::Named("pill")));
        press(&mut app, named(Named::Tab), true);
        press(&mut app, named(Named::Tab), true);
        assert_eq!(
            app.focus,
            Some(FocusId::Named("reset")),
            "Shift+Tab wraps to the last control"
        );
        press(&mut app, named(Named::Tab), false);
        assert_eq!(app.focus, Some(FocusId::Named("about")));

        // Arrows / Page / Home adjust the focused slider.
        app.focus = Some(FocusId::Named("sensitivity"));
        press(&mut app, named(Named::ArrowRight), false);
        assert_eq!(app.tuning.sensitivity, 73);
        press(&mut app, named(Named::PageUp), false);
        assert_eq!(app.tuning.sensitivity, 83);
        press(&mut app, named(Named::Home), false);
        assert_eq!(app.tuning.sensitivity, 0);

        // Range handles move independently and keep their gap.
        app.focus = Some(FocusId::Indexed("freq", 1));
        press(&mut app, named(Named::ArrowDown), false);
        assert_eq!(app.tuning.freq_hi, 96);
        app.focus = Some(FocusId::Indexed("freq", 0));
        press(&mut app, named(Named::End), false);
        assert_eq!(app.tuning.freq_lo, 96 - FREQ_MIN_GAP_TEST);

        // Segmented controls: arrows change the selection.
        app.focus = Some(FocusId::Named("detail"));
        press(&mut app, named(Named::ArrowRight), false);
        assert_eq!(app.tuning.detail, settings::Detail::Detailed);

        // Enter/Space press switches and buttons.
        app.focus = Some(FocusId::Named("auto-gain"));
        press(&mut app, named(Named::Enter), false);
        assert!(app.tuning.auto_gain);
        press(&mut app, named(Named::Space), false);
        assert!(!app.tuning.auto_gain);

        // Escape closes the panel; focus on its controls is dropped next time.
        press(&mut app, named(Named::Escape), false);
        assert_eq!(app.panel, Panel::None);
        press(&mut app, named(Named::Tab), false);
        assert_eq!(app.focus, Some(FocusId::Named("about")));
    }

    const FREQ_MIN_GAP_TEST: u8 = settings::FREQ_MIN_GAP;

    #[test]
    fn picker_traps_focus() {
        let mut app = test_app();
        app.focus = Some(FocusId::Named("pill"));
        press(&mut app, named(Named::Enter), false);
        assert!(app.picker_open);
        assert!(matches!(app.focus, Some(FocusId::Indexed("src", _))));
        for _ in 0..20 {
            press(&mut app, named(Named::Tab), false);
            assert!(
                matches!(app.focus, Some(FocusId::Indexed("src", _))),
                "Tab stays in the picker"
            );
        }
        press(&mut app, named(Named::Escape), false);
        assert!(!app.picker_open);
        assert_eq!(app.focus, Some(FocusId::Named("pill")));
    }

    #[test]
    fn presets_save_rename_move_delete() {
        use PresetKind::Visuals as V;
        let mut app = test_app();
        let _ = app.update(Message::SavePreset(V));
        let _ = app.update(Message::SavePreset(V));
        assert_eq!(app.presets.len(), 2);
        assert_eq!(app.presets[1].name, "Preset 2");
        assert_eq!(
            app.editing_preset,
            Some((V, 1)),
            "a new preset opens for naming"
        );

        let _ = app.update(Message::RenamePreset(V, 1, "  Night drive  ".into()));
        let _ = app.update(Message::EditPreset(V, None));
        assert_eq!(app.presets[1].name, "Night drive");
        assert_eq!(app.editing_preset, None);

        let _ = app.update(Message::EditPreset(V, Some(1)));
        let _ = app.update(Message::MovePreset(V, 1, -1));
        assert_eq!(app.presets[0].name, "Night drive");
        assert_eq!(
            app.editing_preset,
            Some((V, 0)),
            "the editor follows the moved preset"
        );
        let _ = app.update(Message::MovePreset(V, 0, -1)); // already first: no-op
        assert_eq!(app.presets[0].name, "Night drive");

        // Emptied names fall back to a default that isn't already taken.
        let _ = app.update(Message::RenamePreset(V, 0, "   ".into()));
        let _ = app.update(Message::EditPreset(V, None));
        assert_eq!(app.presets[1].name, "Preset 1");
        assert_eq!(app.presets[0].name, "Preset 2");

        // Delete takes two presses.
        let _ = app.update(Message::EditPreset(V, Some(1)));
        let _ = app.update(Message::DeletePreset(V, 1));
        assert_eq!(app.presets.len(), 2);
        let _ = app.update(Message::DeletePreset(V, 1));
        assert_eq!(app.presets.len(), 1);
        assert_eq!(app.editing_preset, None);

        // New default names skip ones in use.
        assert_eq!(app.presets[0].name, "Preset 2");
        assert_eq!(next_preset_name(&app.preset_list(V).names()), "Preset 1");
    }

    #[test]
    fn lost_audio_waits_before_saying_so_and_keeps_retrying() {
        let mut app = test_app();
        let start = app.now;

        // A stream that died: no message for a moment, in case it comes right back.
        app.capture = None;
        app.reconnecting = true;
        app.audio_down_since = Some(start);
        assert_eq!(app.audio_outage(), None);
        app.now = start + AUDIO_OUTAGE_NOTICE;
        assert_eq!(app.audio_outage(), Some(AudioOutage::Reconnecting));

        // A source that can't be opened says so right away, and is tried again later.
        let missing = Source {
            id: "undavi-test:no-such-device".into(),
            label: "Missing".into(),
            kind: SourceKind::Output,
        };
        app.reconnecting = false;
        app.audio_down_since = None;
        app.open_source(missing);
        assert!(matches!(app.audio_outage(), Some(AudioOutage::Failed(_))));
        let first_retry = app.next_audio_retry;
        assert!(first_retry > app.now);
        let _ = app.update(Message::Frame(first_retry));
        assert!(app.next_audio_retry > first_retry, "it tried again");
        assert!(app.audio_down_since.is_some());
    }

    #[test]
    fn about_panel_toggles_one_license_at_a_time() {
        let mut app = test_app();
        let _ = app.update(Message::TogglePanel(Panel::About));
        assert_eq!(app.panel, Panel::About);
        let _ = app.update(Message::ToggleLicense(0));
        let _ = app.update(Message::ToggleLicense(1));
        assert_eq!(app.open_license, Some(1));
        let _ = app.update(Message::ToggleLicense(1));
        assert_eq!(app.open_license, None);
        let _ = app.view(); // builds without panicking
    }

    #[test]
    fn each_panel_keeps_its_own_presets() {
        let mut app = test_app();
        app.tuning.sensitivity = 40;
        let _ = app.update(Message::SavePreset(PresetKind::Tuning));
        app.appearance.dim_pct = 70;
        app.appearance.always_on_top = false;
        let _ = app.update(Message::SavePreset(PresetKind::Appearance));
        assert!(app.presets.is_empty(), "Visuals presets are untouched");
        assert_eq!(app.tuning_presets[0].name, "Preset 1");
        assert_eq!(app.appearance_presets[0].name, "Preset 1");

        // Loading brings back only that panel's settings.
        app.tuning.sensitivity = 90;
        app.appearance.dim_pct = 10;
        app.visuals.bar_count = 20;
        let _ = app.update(Message::LoadPreset(PresetKind::Tuning, 0));
        let _ = app.update(Message::LoadPreset(PresetKind::Appearance, 0));
        assert_eq!(app.tuning.sensitivity, 40);
        assert_eq!(app.appearance.dim_pct, 70);
        assert_eq!(app.visuals.bar_count, 20);

        // "Always on top" belongs to the window and isn't changed by a preset.
        app.appearance.always_on_top = true;
        let _ = app.update(Message::LoadPreset(PresetKind::Appearance, 0));
        assert!(app.appearance.always_on_top);
        assert_eq!(app.appearance.dim_pct, 70);

        // Switching panels finishes an edit in progress.
        let _ = app.update(Message::EditPreset(PresetKind::Tuning, Some(0)));
        let _ = app.update(Message::RenamePreset(PresetKind::Tuning, 0, " ".into()));
        let _ = app.update(Message::TogglePanel(Panel::Visuals));
        assert_eq!(app.editing_preset, None);
        assert_eq!(app.tuning_presets[0].name, "Preset 1");
    }

    #[test]
    fn idle_mode_waits_then_wakes() {
        let mut app = test_app();
        let t0 = Instant::now();
        app.update_idle(t0, false);
        assert!(!app.idle, "needs a moment of stillness first");
        app.update_idle(t0 + Duration::from_millis(1100), false);
        assert!(app.idle);
        // Sound (or music playing) ends idle immediately...
        app.update_idle(t0 + Duration::from_millis(1200), true);
        assert!(!app.idle);
        // ...and so does any input.
        app.update_idle(t0 + Duration::from_millis(1300), false);
        app.update_idle(t0 + Duration::from_millis(2400), false);
        assert!(app.idle);
        let _ = app.update(Message::Pointer(Pointer::Activity));
        assert!(!app.idle);
    }

    #[test]
    fn controls_without_a_media_player_are_left_out() {
        let mut app = test_app();
        app.platform = Platform {
            media: false,
            always_on_top: false,
        };
        app.panel = Panel::Appearance;
        let _ = app.view();
        for gone in ["now-playing", "lyrics", "on-top"] {
            assert!(!app.focus_reg.contains(&FocusId::Named(gone)), "{gone}");
        }
        assert!(app.focus_reg.contains(&FocusId::Named("accent")));
        assert!(!app.accent_choices().contains(&Accent::Album));

        // Swatch indexes follow the shorter list: the last one is still Custom.
        let last = app.palette_choices().len() - 1;
        let _ = app.update(Message::PickSwatch(last));
        assert!(matches!(app.visuals.palette, Palette::Custom(_)));
        let _ = app.update(Message::PickAccent(0));
        assert_eq!(app.appearance.accent, Accent::Lilac);

        // The transport is gone, full screen stays.
        app.panel = Panel::None;
        let _ = app.view();
        assert!(!app.focus_reg.contains(&FocusId::Named("play")));
        assert!(app.focus_reg.contains(&FocusId::Named("fullscreen")));
    }

    #[test]
    fn frozen_bars_let_the_app_rest() {
        let mut app = test_app();
        let loud = vec![-20.0; FFT_SIZE / 2 + 1];
        let frame = [SpectrumFrame {
            magnitudes_db: &loud,
            sample_rate: 48_000.0,
            fft_size: FFT_SIZE,
        }];
        for _ in 0..30 {
            app.processor.update(&frame, 8, &app.tuning, 1.0 / 60.0);
        }
        let t0 = Instant::now();
        // Fading bars keep it drawing; frozen ones don't.
        app.update_idle(t0, false);
        app.update_idle(t0 + Duration::from_millis(1100), false);
        assert!(!app.idle);
        app.tuning.when_silent = SilenceMode::Freeze;
        app.update_idle(t0 + Duration::from_millis(1200), false);
        app.update_idle(t0 + Duration::from_millis(2300), false);
        assert!(app.idle);
    }

    #[test]
    fn escape_leaves_fullscreen_last() {
        let mut app = test_app();
        app.fullscreen = true;
        app.panel = Panel::Visuals;
        press(&mut app, named(Named::Escape), false);
        assert_eq!(
            app.panel,
            Panel::None,
            "panels close before leaving full screen"
        );
        assert!(
            app.fullscreen,
            "the window mode only changes via the ToggleFullscreen task"
        );
    }

    #[test]
    fn lyric_offset_tapers_at_the_end() {
        let len = Some(Duration::from_secs(200));
        // Mid-song: the full offset.
        assert!((lyric_offset(-1400, 100.0, len) + 1.4).abs() < 1e-6);
        // Fades out over 4.2 s, reaching zero a second before the end.
        assert!((lyric_offset(-1400, 200.0 - 1.0 - 2.1, len) + 0.7).abs() < 1e-4);
        assert_eq!(lyric_offset(-1400, 199.0, len), 0.0);
        assert_eq!(lyric_offset(-1400, 200.0, len), 0.0);
        // The lyric clock never runs backwards while it catches up.
        let mut prev = f32::MIN;
        for i in 0..=100 {
            let pos = 190.0 + i as f32 * 0.1;
            let t = pos + lyric_offset(-1400, pos, len);
            assert!(t >= prev);
            prev = t;
        }
        // A last line starting 0.5 s before the end lights up right on time.
        assert_eq!(199.5 + lyric_offset(-1400, 199.5, len), 199.5);
        // Positive offsets and unknown lengths are unchanged.
        assert_eq!(lyric_offset(500, 199.9, len), 0.5);
        assert_eq!(lyric_offset(-1400, 199.9, None), -1.4);
    }

    #[test]
    fn hex_parsing() {
        assert_eq!(parse_hex("#4B7BF5"), Some([0x4B, 0x7B, 0xF5]));
        assert_eq!(parse_hex("fff"), Some([255, 255, 255]));
        assert_eq!(parse_hex("#12"), None);
        assert_eq!(parse_hex("zzzzzz"), None);
    }

    #[test]
    fn custom_round_trip() {
        let p = Palette::Custom(vec![[1, 2, 3], [255, 0, 16]]);
        let fields = custom_hex_strings(&p);
        assert_eq!(fields[0], "#010203");
        assert_eq!(fields[2], "");
        assert_eq!(parse_custom(&fields), vec![[1, 2, 3], [255, 0, 16]]);
    }
}

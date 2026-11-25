//! Application state and the update loop.

use crate::art::{Art, ArtLoader, DEFAULT_AMBIENT, ramp};
use crate::audio::{Analyzer, Capture, Signal, Source, SourceKind, list_sources};
use crate::background::BgEngine;
use crate::focus::{FocusId, Registry, Scope, Step};
use crate::karaoke::{LyricsView, Placement};
use crate::lyrics::{Lyrics, LyricsLoader, Song, Status as LyricsStatus};
use crate::media::{Command, Media, MediaTarget, Track};
use crate::monitor::{Monitor, Reading};
use crate::platform::{Platform, ReducedMotion};
use crate::settings::{
    self as config, ACCENT_BLUE, Accent, Action, BackgroundSettings, BarsScroll, ColorSettings,
    Combo, ControlSettings, DEFAULT_CUSTOM, DoubleClick, GeneralSettings, LyricsSettings,
    MonitorSettings, NowPlayingSettings, Palette, PlaybackSettings, SectionId, Sections,
    SilenceMode, Stored, TuningSettings, UpdateSettings,
};
use crate::shaders;
use crate::shaders::settings::Resolved;
use crate::spectrum::{BeatGlow, Gate, Processor, SpectrumFrame};
use crate::theme::*;
use crate::update::{self, Install, Release};
use crate::visualizer::{BarMap, VisEngine, bar_signals};
use iced::keyboard::{self, Key, key::Named};
use iced::widget::Canvas;
use iced::widget::image::Handle;
use iced::{Color, Element, Event, Length, Point, Size, Subscription, Task, mouse, window};
use settings::{SettingsMessage, SettingsWindow};
use std::cell::RefCell;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub(crate) mod settings;

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
/// How long the refresh icon turns for.
const REFRESH_TURN: Duration = Duration::from_millis(700);
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

/// A setting on the page shown: its label, its control, and its id if it can be
/// starred.
pub(crate) struct RowFocus {
    pub label: String,
    pub focus: FocusId,
    pub favorite: Option<String>,
}

pub struct App {
    /// Every setting and preset, by section.
    pub(crate) sections: Sections,
    /// The settings window.
    pub(crate) settings: SettingsWindow,
    /// The About panel, left of the stage, under the wordmark.
    pub(crate) about_open: bool,

    pub(crate) host: cpal::Host,
    /// What this platform supports; the view leaves out the rest.
    pub(crate) platform: Platform,
    /// The system's reduced-motion setting.
    pub(crate) reduced_motion: ReducedMotion,
    pub(crate) sources: Vec<Source>,
    pub(crate) source: Source,
    pub(crate) picker_open: bool,
    /// The quick settings, and the settings starred for them (saved).
    pub(crate) quick_open: bool,
    pub(crate) favorites: Vec<String>,
    /// When the song's info was last refreshed by hand.
    pub(crate) refreshed: Option<Instant>,
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
    pub(crate) bg_beat: BeatGlow,
    /// Eased background pulse, 0..=1.
    pub(crate) bg_pulse: f32,
    pub(crate) last_audio: Option<Instant>,
    /// Noise gate: whether there's sound to show right now.
    pub(crate) gate: Gate,
    /// Readings for the level meter (in the Sound section of the settings window).
    pub(crate) meter: Meter,

    pub(crate) media: Media,
    pub(crate) track: Option<Track>,
    pub(crate) unmuted_volume: Option<f64>,

    pub(crate) last_frame: Option<Instant>,
    pub(crate) now: Instant,
    /// When the app started (the clock of animations that don't follow the music).
    pub(crate) started: Instant,
    pub(crate) wave_phase: f32,
    pub(crate) dirty_since: Option<Instant>,
    /// Text in the custom-color hex fields (kept separately so half-typed values survive).
    pub(crate) custom_hex: [String; 3],

    // Background and interface
    pub(crate) art_loader: ArtLoader,
    pub(crate) lyrics_loader: LyricsLoader,
    pub(crate) lyrics: Option<Arc<Lyrics>>,
    pub(crate) art: Option<Arc<Art>>,
    /// Outgoing cover and when the background's cross-fade from it started.
    pub(crate) fading_out: Option<(Arc<Art>, Instant)>,
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
    /// The controls of the settings shown, by their labels (also filled in by the view).
    pub(crate) row_focus: RefCell<Vec<RowFocus>>,
    /// The volume popover next to the volume button.
    pub(crate) volume_open: bool,
    /// The license expanded in the About panel.
    pub(crate) open_license: Option<usize>,

    /// Update checks (saved).
    pub(crate) updates: UpdateSettings,
    /// How this copy was installed; known once the startup check has run.
    pub(crate) install: Option<Install>,
    /// The update pop-up and any install in progress.
    pub(crate) update: UpdateState,

    /// The signal monitor (in the settings window's Sound section): whether it's on,
    /// which of its parts show (saved) and its history.
    pub(crate) monitor_open: bool,
    pub(crate) monitor_sections: MonitorSettings,
    pub(crate) monitor: Monitor,

    /// Every visualizer and background shader (built in, and the shaders folder),
    /// and the music as they see it.
    pub(crate) shaders: shaders::Library,
    pub(crate) shader_audio: shaders::Audio,
    /// The visualizer and background in use: their shaders and settings, kept
    /// current after every message (see [`App::refresh_shaders`]).
    pub(crate) vis: Active,
    pub(crate) bg: Active,
    pub(crate) vis_engine: VisEngine,
    pub(crate) bg_engine: BgEngine,

    pub(crate) fullscreen: bool,
    /// Idle mode: nothing playing and nothing animating, so redraw rarely.
    pub(crate) idle: bool,
    idle_since: Option<Instant>,
    /// Background motion clock; only advances while music plays.
    bg_time: f32,
    /// The same, scaled by the background's speed setting (drives the background).
    motion_time: f32,
}

/// A visualizer or background in use: its name, shader and settings.
#[derive(Clone, Debug)]
pub(crate) struct Active {
    pub name: String,
    pub prepared: shaders::Prepared,
    pub resolved: Resolved,
    /// Why the picked shader isn't the one showing (it doesn't compile, or is gone).
    pub problem: Option<String>,
}

impl Active {
    fn builtin(name: &str, kind: shaders::Kind) -> Self {
        let prepared = shaders::Library::new(None)
            .prepared(name, kind)
            .expect("built-in shaders compile (tested)")
            .clone();
        let resolved = prepared.decls.resolve(None, &[]);
        Self {
            name: name.into(),
            prepared,
            resolved,
            problem: None,
        }
    }
}

/// Where an update stands. Everything but `None` and a hidden `Installing` shows
/// the update pop-up.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum UpdateState {
    None,
    /// A newer release, with the pop-up's two checkboxes.
    Offer {
        release: Release,
        auto: bool,
        dont_ask: bool,
    },
    /// Downloading and installing. Automatic updates run hidden.
    Installing {
        release: Release,
        shown: bool,
    },
    Installed {
        version: String,
    },
    Failed {
        error: String,
        page: String,
    },
}

impl UpdateState {
    pub(crate) fn shown(&self) -> bool {
        !matches!(
            self,
            UpdateState::None | UpdateState::Installing { shown: false, .. }
        )
    }
}

/// Run `f` on its own thread and resolve with its result.
fn background<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> impl std::future::Future<Output = Result<T, String>> {
    let (tx, rx) = iced::futures::channel::oneshot::channel();
    let spawned = std::thread::Builder::new()
        .name("undavi-update".into())
        .spawn(move || {
            let _ = tx.send(f());
        });
    async move {
        spawned.map_err(|e| e.to_string())?;
        rx.await
            .map_err(|_| "the update thread stopped".to_string())?
    }
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

    /// Open or close the About panel.
    ToggleAbout,
    /// The settings window.
    Settings(SettingsMessage),
    /// Expand or collapse a license in the About panel.
    ToggleLicense(usize),
    OpenUrl(&'static str),
    TogglePicker,
    ClosePicker,
    PickSource(usize),
    /// Open or close the quick settings.
    ToggleQuick,
    /// Put a setting in the quick settings, or take it out (by its id).
    ToggleFavorite(String),
    /// Read the song's info, art and lyrics again.
    Refresh,

    Pointer(Pointer),

    SetTuning(TuningSettings),
    SetColors(ColorSettings),
    SetBackground(BackgroundSettings),
    SetNowPlaying(NowPlayingSettings),
    SetPlayback(PlaybackSettings),
    SetLyrics(LyricsSettings),
    SetControls(ControlSettings),
    SetGeneral(GeneralSettings),
    PickAccent(usize),
    AutoCalibrate,
    PickSwatch(usize),
    CustomHex(usize, String),

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
    /// Scrolling over the bars, and double-clicking them.
    BarsScroll(f32),
    BarsDoubleClick,
    Seek(f32),
    /// Seek to a lyric line's start time (seconds, lyric clock).
    SeekLyric(f32),

    /// The startup update check finished.
    UpdateChecked(Result<update::Found, String>),
    /// The pop-up's "Update automatically" and "Don't ask again" checkboxes.
    ToggleUpdateAuto,
    ToggleUpdateDontAsk,
    /// Install the offered release.
    UpdateNow,
    /// Close the update pop-up ("Not now", "OK", "Later", "Close"; also hides an
    /// install in progress).
    CloseUpdate,
    /// Open the offered (or failed) release's page.
    OpenReleasePage,
    UpdateInstalled(Result<String, String>),
    /// Start the newly installed version and quit.
    RestartUpdated,
    SetUpdates(UpdateSettings),

    /// Turn the signal monitor on or off.
    ToggleMonitor,
    SetMonitor(MonitorSettings),

    /// Use this custom shader (by its index in the library): for the background,
    /// or as the visualizer.
    PickBackground(String),
    PickVisualizer(String),
    /// Change a setting of the visualizer (`background: false`) or background in
    /// use: its id and new value.
    SetShaderValue {
        background: bool,
        id: String,
        value: serde_json::Value,
    },
    /// Put the settings of the visualizer (or background) in use back to its
    /// defaults.
    ResetShaderValues(bool),
    /// Copy a built-in shader into the shaders folder, to start a new one from it.
    CopyShader(String),
    /// Open the shaders folder (creating it, with examples, the first time).
    OpenShaderFolder,
}

impl App {
    pub fn new() -> (Self, Task<Message>) {
        let stored = config::load();
        let host = cpal::default_host();
        let sources = list_sources(&host);
        let system = Source::system(&host);
        let custom_hex = custom_hex_strings(&stored.sections.colors.palette);
        let now = Instant::now();
        let always_on_top = stored.sections.general.always_on_top;
        let mut app = Self {
            sections: stored.sections,
            settings: SettingsWindow::default(),
            about_open: false,
            platform: Platform::detect(),
            reduced_motion: ReducedMotion::default(),
            host,
            sources,
            source: system,
            picker_open: false,
            quick_open: false,
            favorites: stored.favorites,
            refreshed: None,
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
            now,
            started: now,
            wave_phase: 0.0,
            dirty_since: None,
            custom_hex,
            art_loader: ArtLoader::start(),
            lyrics_loader: LyricsLoader::start(),
            lyrics: None,
            art: None,
            fading_out: None,
            accent: rgb_f(ACCENT_BLUE),
            bar_colors: Vec::new(),
            last_activity: now,
            cursor: None,
            window_size: Size::new(1440.0, 900.0),
            ui_alpha: 1.0,
            focus: None,
            focus_reg: Registry::default(),
            row_focus: RefCell::default(),
            volume_open: false,
            open_license: None,
            updates: stored.updates,
            install: None,
            update: UpdateState::None,
            monitor_open: false,
            monitor_sections: stored.monitor,
            monitor: Monitor::default(),
            // Tests never read the user's shaders folder.
            shaders: if cfg!(test) {
                shaders::Library::new(None)
            } else {
                shaders::Library::user()
            },
            shader_audio: shaders::Audio::default(),
            vis: Active::builtin(shaders::BARS, shaders::Kind::Visualizer),
            bg: Active::builtin(shaders::ALBUM_ART, shaders::Kind::Scene),
            vis_engine: VisEngine::default(),
            bg_engine: BgEngine::read(
                &Active::builtin(shaders::ALBUM_ART, shaders::Kind::Scene).resolved,
            ),
            fullscreen: false,
            idle: false,
            idle_since: None,
            bg_time: 0.0,
            motion_time: 0.0,
        };
        app.accent = rgb_f(app.target_accent());
        app.settle_presets();
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
        // Tests drive the real app; never let them go online.
        let check = if app.platform.updates && app.updates.check && !cfg!(test) {
            let unstable = app.updates.unstable;
            Task::perform(
                background(move || update::check(unstable)),
                Message::UpdateChecked,
            )
        } else {
            Task::none()
        };
        app.refresh_shaders();
        (app, Task::batch([task, check]))
    }

    /// Handle the update pop-up and the install behind it.
    fn on_update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::UpdateChecked(Err(e)) => eprintln!("undavi: update check failed: {e}"),
            Message::UpdateChecked(Ok(found)) => {
                let auto = self.updates.auto_install && matches!(found.install, Install::Prefix(_));
                self.install = Some(found.install);
                if let Some(release) = found.release {
                    if auto && release.archive.is_some() {
                        return self.start_install(release, false);
                    }
                    self.update = UpdateState::Offer {
                        release,
                        auto: false,
                        dont_ask: false,
                    };
                }
            }
            Message::ToggleUpdateAuto => {
                if let UpdateState::Offer { auto, .. } = &mut self.update {
                    *auto = !*auto;
                }
            }
            Message::ToggleUpdateDontAsk => {
                if let UpdateState::Offer { dont_ask, .. } = &mut self.update {
                    *dont_ask = !*dont_ask;
                }
            }
            Message::UpdateNow => {
                if let UpdateState::Offer { release, .. } = &self.update {
                    let release = release.clone();
                    self.apply_update_choices();
                    return self.start_install(release, true);
                }
            }
            Message::CloseUpdate => {
                self.apply_update_choices();
                self.update = match std::mem::replace(&mut self.update, UpdateState::None) {
                    // The install keeps going; its result shows when it's done.
                    UpdateState::Installing { release, .. } => UpdateState::Installing {
                        release,
                        shown: false,
                    },
                    _ => UpdateState::None,
                };
                self.focus = None;
            }
            Message::OpenReleasePage => match &self.update {
                UpdateState::Offer { release, .. } => crate::about::open_url(&release.page),
                UpdateState::Failed { page, .. } => crate::about::open_url(page),
                _ => {}
            },
            Message::UpdateInstalled(result) => {
                let page = match &self.update {
                    UpdateState::Installing { release, .. } => release.page.clone(),
                    _ => String::new(),
                };
                self.update = match result {
                    Ok(version) => UpdateState::Installed { version },
                    Err(error) => {
                        eprintln!("undavi: update failed: {error}");
                        UpdateState::Failed { error, page }
                    }
                };
            }
            Message::RestartUpdated => {
                if let Some(Install::Prefix(prefix)) = self.install.clone() {
                    if self.dirty_since.is_some() {
                        self.save();
                    }
                    match update::relaunch(&prefix) {
                        Ok(()) => return iced::exit(),
                        Err(e) => {
                            self.update = UpdateState::Failed {
                                error: format!("couldn't restart: {e}"),
                                page: String::new(),
                            }
                        }
                    }
                }
            }
            Message::SetUpdates(u) => {
                self.updates = u;
                self.mark_dirty();
            }
            _ => {}
        }
        Task::none()
    }

    /// Save the pop-up's checkboxes. "Update automatically" also means not asking;
    /// "Don't ask again" on its own stops the checks.
    fn apply_update_choices(&mut self) {
        let UpdateState::Offer { auto, dont_ask, .. } = self.update else {
            return;
        };
        let choose = move |u: &mut UpdateSettings| {
            if auto {
                u.auto_install = true;
            } else if dont_ask {
                u.check = false;
            }
        };
        if auto || dont_ask {
            choose(&mut self.updates);
            self.settings.keep_updates(choose);
            self.mark_dirty();
        }
    }

    fn start_install(&mut self, release: Release, shown: bool) -> Task<Message> {
        let Some(Install::Prefix(prefix)) = self.install.clone() else {
            return Task::none();
        };
        self.update = UpdateState::Installing {
            release: release.clone(),
            shown,
        };
        self.focus = None;
        if cfg!(test) {
            return Task::none();
        }
        Task::perform(
            background(move || update::install(&release, &prefix).map(|()| release.version)),
            Message::UpdateInstalled,
        )
    }

    pub fn title(&self) -> String {
        match &self.track {
            Some(np) if !np.title.is_empty() => format!("{} — Undavi", np.title),
            _ => "Undavi".into(),
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        // Idle: a slow tick keeps media info and the clock fresh without redrawing at 60 fps.
        let clock = match (self.idle, self.sections.general.frame_rate.interval()) {
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
        config::save(&Stored {
            version: config::CONFIG_VERSION,
            sections: self.sections.clone(),
            updates: self.updates.clone(),
            monitor: self.monitor_sections.clone(),
            favorites: self.favorites.clone(),
            // System Audio is the default; only remember other picks.
            source: (self.source.kind != SourceKind::SystemOutput).then(|| self.source.clone()),
        });
        self.dirty_since = None;
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        // Frames refresh them themselves when the shaders folder changes.
        let frame = matches!(message, Message::Frame(_));
        let task = self.handle(message);
        if !frame {
            self.refresh_shaders();
            self.forget_redos();
        }
        task
    }

    /// The visualizer and background in use, from the settings and the shaders
    /// folder: a picked shader that's gone or doesn't compile falls back to Bars
    /// (or Album art), with the reason in `problem`. A background whose animation
    /// is a custom warp is built with it.
    pub(crate) fn refresh_shaders(&mut self) {
        let warps = self.shaders.warps();
        let pick = |lib: &shaders::Library, name: &str, kind, fallback: &str| match lib
            .prepared(name, kind)
        {
            Some(p) => (name.to_string(), p.clone(), None),
            None => {
                let problem = match lib.get(name).map(|e| &e.state) {
                    Some(Err(e)) => e.clone(),
                    _ => format!("There's no shader called \"{name}\" any more."),
                };
                let p = lib
                    .prepared(fallback, kind)
                    .expect("built-ins compile")
                    .clone();
                (fallback.to_string(), p, Some(problem))
            }
        };

        let (name, prepared, problem) = pick(
            &self.shaders,
            &self.sections.visualizer.shader,
            shaders::Kind::Visualizer,
            shaders::BARS,
        );
        let resolved = prepared
            .decls
            .resolve(self.sections.visualizer.values.get(&name), &warps);
        self.vis_engine = VisEngine::read(&resolved);
        self.vis = Active {
            name,
            prepared,
            resolved,
            problem,
        };

        let (name, mut prepared, mut problem) = pick(
            &self.shaders,
            &self.sections.background.shader,
            shaders::Kind::Scene,
            shaders::ALBUM_ART,
        );
        let saved = self.sections.background.values.get(&name);
        let mut resolved = prepared.decls.resolve(saved, &warps);
        if resolved.toggle("animated") == Some(true)
            && let Some(warp) = resolved.choice("animation").map(str::to_string)
            && warps.contains(&warp)
        {
            match self.shaders.with_warp(&name, &warp) {
                Some(Ok(p)) => {
                    prepared = p.clone();
                    resolved = prepared.decls.resolve(saved, &warps);
                }
                Some(Err(e)) => problem = Some(e.clone()),
                None => {}
            }
        }
        self.bg_engine = BgEngine::read(&resolved);
        self.bg = Active {
            name,
            prepared,
            resolved,
            problem,
        };
    }

    fn handle(&mut self, message: Message) -> Task<Message> {
        // Any input wakes the app from idle mode right away.
        if !matches!(message, Message::Frame(_)) {
            self.idle = false;
            self.idle_since = None;
        }
        match message {
            Message::Frame(now) => self.tick(now),
            Message::Key(event) => {
                if !self.move_only() {
                    self.last_activity = self.now.max(Instant::now());
                }
                return self.on_key(event);
            }
            Message::Pointer(p) => match p {
                Pointer::Moved(pos) => {
                    self.cursor = Some(pos);
                    self.last_activity = Instant::now();
                }
                Pointer::Left => self.cursor = None,
                Pointer::Pressed => {
                    if !self.move_only() {
                        self.last_activity = Instant::now();
                    }
                    self.focus = None;
                    self.settings.capturing = None;
                }
                Pointer::Activity => {
                    if !self.move_only() {
                        self.last_activity = Instant::now();
                    }
                }
                Pointer::Resized(size) => self.window_size = size,
            },

            Message::ToggleAbout => self.about_open = !self.about_open,
            Message::Settings(m) => return self.on_settings(m),
            Message::ToggleLicense(i) => {
                self.open_license = (self.open_license != Some(i)).then_some(i);
            }
            Message::OpenUrl(url) => crate::about::open_url(url),
            Message::UpdateChecked(_)
            | Message::ToggleUpdateAuto
            | Message::ToggleUpdateDontAsk
            | Message::UpdateNow
            | Message::CloseUpdate
            | Message::OpenReleasePage
            | Message::UpdateInstalled(_)
            | Message::RestartUpdated
            | Message::SetUpdates(_) => return self.on_update(message),
            Message::ToggleMonitor => {
                self.monitor_open = !self.monitor_open;
                if !self.monitor_open {
                    self.monitor.clear();
                }
            }
            Message::PickBackground(name) => {
                self.sections.background.shader = name;
                self.mark_dirty();
            }
            Message::PickVisualizer(name) => {
                self.sections.visualizer.shader = name;
                self.mark_dirty();
            }
            Message::SetShaderValue {
                background,
                id,
                value,
            } => {
                let (values, name) = if background {
                    (&mut self.sections.background.values, &self.bg.name)
                } else {
                    (&mut self.sections.visualizer.values, &self.vis.name)
                };
                values.entry(name.clone()).or_default().insert(id, value);
                self.mark_dirty();
            }
            Message::ResetShaderValues(background) => {
                if background {
                    self.sections.background.values.remove(&self.bg.name);
                } else {
                    self.sections.visualizer.values.remove(&self.vis.name);
                }
                self.mark_dirty();
            }
            Message::CopyShader(name) => match shaders::copy_builtin(&name) {
                Ok(copy) => {
                    let _ = self.shaders.refresh(self.now + Duration::from_secs(60));
                    eprintln!("undavi: copied {name} to {}", copy.display());
                    if let Some(dir) = copy.parent() {
                        crate::about::open_url(&dir.to_string_lossy());
                    }
                }
                Err(e) => eprintln!("undavi: couldn't copy {name}: {e}"),
            },
            Message::OpenShaderFolder => match shaders::prepare_folder() {
                Ok(dir) => {
                    // Pick up the examples right away.
                    let _ = self.shaders.refresh(self.now + Duration::from_secs(60));
                    crate::about::open_url(&dir.to_string_lossy());
                }
                Err(e) => eprintln!("undavi: couldn't open the shaders folder: {e}"),
            },
            Message::SetMonitor(m) => {
                self.monitor_sections = m;
                self.mark_dirty();
            }
            Message::ToggleQuick => {
                self.quick_open = !self.quick_open;
                if self.quick_open {
                    self.picker_open = false;
                    self.volume_open = false;
                }
                // Keyboard users go in, and return to the button.
                self.focus = self
                    .focus
                    .take()
                    .and_then(|_| (!self.quick_open).then_some(FocusId::Named("quick")));
            }
            Message::ToggleFavorite(id) => {
                match self.favorites.iter().position(|f| *f == id) {
                    Some(i) => {
                        self.favorites.remove(i);
                    }
                    None => self.favorites.push(id),
                }
                self.mark_dirty();
            }
            Message::Refresh => {
                self.media.refresh();
                self.art_loader.reload();
                self.lyrics_loader.retry();
                self.refreshed = Some(Instant::now());
            }
            Message::TogglePicker => {
                self.quick_open = false;
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
                self.sections.sound.settings = t;
                self.mark_dirty();
            }
            Message::SetColors(c) => {
                self.sections.colors.settings = c;
                self.mark_dirty();
            }
            Message::SetBackground(b) => {
                self.sections.background.settings = b;
                self.mark_dirty();
            }
            Message::SetNowPlaying(n) => {
                self.sections.now_playing.settings = n;
                self.mark_dirty();
            }
            Message::SetPlayback(p) => self.set_playback(p),
            Message::SetLyrics(l) => {
                self.sections.lyrics.settings = l;
                self.mark_dirty();
            }
            Message::SetControls(c) => {
                self.sections.controls.settings = c;
                self.mark_dirty();
            }
            Message::SetGeneral(g) => {
                let task = (g.always_on_top != self.sections.general.always_on_top)
                    .then(|| set_on_top(g.always_on_top));
                self.sections.general.settings = g;
                self.mark_dirty();
                if let Some(task) = task {
                    return task;
                }
            }
            Message::PickAccent(i) => {
                if let Some(a) = self.accent_choices().get(i) {
                    self.sections.colors.accent = *a;
                    self.mark_dirty();
                }
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
                    self.sections.colors.palette = p.clone();
                    self.mark_dirty();
                }
            }
            Message::CustomHex(i, s) => {
                self.custom_hex[i] = s;
                let stops = parse_custom(&self.custom_hex);
                if !stops.is_empty() {
                    self.sections.colors.palette = Palette::Custom(stops);
                    self.mark_dirty();
                }
            }
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
            Message::VolumeScroll(dy) => self.step_volume(dy > 0.0),
            Message::BarsScroll(dy) => match self.sections.controls.scroll {
                BarsScroll::Nothing => {}
                BarsScroll::Volume => self.step_volume(dy > 0.0),
                BarsScroll::Seek => return self.step_seek(dy > 0.0),
            },
            Message::BarsDoubleClick => match self.sections.controls.double_click {
                DoubleClick::Fullscreen => return self.update(Message::ToggleFullscreen),
                DoubleClick::PlayPause => return self.update(Message::PlayPause),
                DoubleClick::Nothing => {}
            },
            Message::Seek(f) => {
                if let Some(len) = self.track.as_ref().and_then(|t| t.length) {
                    self.media.send(Command::SeekTo(len.mul_f32(f)));
                }
            }
            Message::SeekLyric(t) => {
                // Lyric time includes the user's offset; land a hair before the line starts.
                // (The offset is evaluated at the target, so the end-of-song taper matches.)
                let offset = self.track.as_ref().map_or(0.0, |tr| {
                    lyric_offset(self.sections.lyrics.offset_ms, t, tr.length)
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
        let started = Instant::now();
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
        self.analyzer
            .set_size(self.sections.sound.detail.fft_size());
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
        let delay = Duration::from_millis(self.sections.sound.delay_ms as u64);
        let live = self
            .last_audio
            .is_some_and(|t| now - t < STALE_AUDIO + delay);
        let signals = bar_signals(&self.vis_engine);
        if live {
            let delay_samples = (delay.as_secs_f32() * sample_rate) as usize;
            self.analyzer.compute(signals, delay_samples);
        }
        let level = live.then(|| self.analyzer.level_db());
        let sound = self.gate.update(level, self.sections.sound.gate_db, dt);

        let fft_size = self.analyzer.size();
        let spectrum = |magnitudes_db| SpectrumFrame {
            magnitudes_db,
            sample_rate,
            fft_size,
        };
        // The glow and pulse follow the mix; the bars use one spectrum per half when split.
        let frame = sound.then(|| spectrum(self.analyzer.spectrum(Signal::Mix)));
        // Shaders get 16 bands of the mix (taken now, while `frame` is here).
        let shader_bands = frame.as_ref().map(|f| {
            shaders::bands(
                f.magnitudes_db,
                f.sample_rate,
                f.fft_size,
                self.sections.sound.floor_db as f32,
                self.sections.sound.ceiling_db as f32,
            )
        });
        let bar_frames: Vec<SpectrumFrame<'_>> = if sound {
            signals
                .iter()
                .map(|&s| spectrum(self.analyzer.spectrum(s)))
                .collect()
        } else {
            Vec::new()
        };
        let beat_sensitivity = self.sections.sound.beat_sensitivity;
        // The level meter shows beats too, and a flash has to finish fading after
        // Glow goes away.
        // Every shader can follow the beat (`beat()`), so it's always detected.
        self.beat.update(
            frame.as_ref(),
            self.vis_engine.glow_source,
            beat_sensitivity,
            dt,
        );
        // Background pulse: quick to swell, slow to settle, so it breathes rather than flickers.
        let e = self.bg_engine;
        let target = if e.pulse {
            self.bg_beat
                .update(frame.as_ref(), e.pulse_source, beat_sensitivity, dt)
                * e.pulse_amount
        } else {
            0.0
        };
        let tau = if target > self.bg_pulse { 0.06 } else { 0.35 };
        self.bg_pulse += (target - self.bg_pulse) * (1.0 - (-dt / tau).exp());
        let bins = BarMap::for_engine(&self.vis_engine).bins(self.vis_engine.bar_count);
        if let Some(sensitivity) =
            self.processor
                .update(&bar_frames, bins, &self.sections.sound, dt)
        {
            self.sections.sound.sensitivity = sensitivity;
            self.mark_dirty();
        }
        Meter::follow(&mut self.meter.input_db, level, dt);
        Meter::follow(&mut self.meter.loudest_db, self.processor.loudest_db(), dt);
        // Custom shaders: watch the folder while they could be picked, and feed
        // the music to the one in use.
        // Watch the shaders folder (once a second) for new or edited shaders.
        if self.shaders.refresh(now) {
            self.refresh_shaders();
        }
        let loudness = level.map_or(0.0, |db| (db + 60.0) / 60.0);
        self.shader_audio
            .follow(shader_bands.as_ref(), self.beat.pulse(), loudness, dt);
        // The history is kept for as long as the settings are open, whatever they show.
        if self.monitor_open && self.settings.open {
            let visible = self.processor.visible();
            self.monitor.record(Reading {
                now,
                dt,
                input_db: level,
                loudest_db: self.processor.loudest_db(),
                sound,
                pulse: self.beat.pulse(),
                bg_pulse: self.bg_pulse,
                visible,
                per_channel: visible.len() / signals.len().max(1),
                spectrum: live
                    .then(|| (self.analyzer.spectrum(Signal::Mix), sample_rate, fft_size)),
            });
        }

        self.track = self.media.get_track();
        if self.track.as_ref().is_some_and(|t| t.playing) {
            // Slow drift so the seek wave reads as "playing".
            let speed = f32::from(self.sections.playback.curve_speed) / 100.0;
            self.wave_phase = (self.wave_phase + dt * 2.0 * speed) % std::f32::consts::TAU;
        }

        self.update_art(now);
        self.update_lyrics();

        // Background motion pauses with the music (and in idle mode).
        let playing = self.track.as_ref().is_some_and(|t| t.playing);
        if playing || sound {
            self.bg_time += dt;
            self.motion_time += dt * self.bg_engine.speed;
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
        if self.sections.colors.palette == Palette::Album && self.bar_colors.len() == target.len() {
            for (c, t) in self.bar_colors.iter_mut().zip(&target) {
                for (a, b) in c.iter_mut().zip(t) {
                    *a += (b - *a) * k;
                }
            }
        } else {
            self.bar_colors = target;
        }

        self.update_auto_hide(now, dt);
        // The monitor's graphs keep moving, so it keeps the app awake while it shows; so
        // does the settings window's "Live" light.
        let monitor = self.monitor_open
            && self.settings.shown()
            && self.settings.section() == Some(SectionId::Sound);
        let light =
            self.settings.shown() && self.settings.show_preview && !self.reduced_motion.on();
        let found = self.fade_found(now);
        let turning = self.refresh_turn().is_some();
        self.update_idle(
            now,
            playing || sound || monitor || light || found || turning,
        );

        if self.dirty_since.is_some_and(|t| now - t >= SAVE_DELAY) {
            self.save();
        }
        self.monitor.update_cost = started.elapsed();
    }

    fn update_art(&mut self, now: Instant) {
        let art = self.track.as_ref().and_then(|t| t.art.as_ref());
        self.art_loader.want(art);
        let art = self.art_loader.snapshot();
        // A different cover (or none any more): cross-fade the background from the old one.
        let changed = match (&self.art, &art) {
            (Some(old), Some(new)) => old.backdrop.id != new.backdrop.id,
            (Some(_), None) => true,
            _ => false,
        };
        if changed && let Some(old) = self.art.take() {
            self.fading_out = Some((old, now));
        }
        self.art = art;
        if self
            .fading_out
            .as_ref()
            .is_some_and(|(_, t)| now - *t >= ART_FADE)
        {
            self.fading_out = None;
        }
    }

    /// The Ambient background's colors: from the cover, or the defaults.
    pub(crate) fn ambient_colors(&self) -> [[u8; 3]; 4] {
        self.art
            .as_ref()
            .map_or(DEFAULT_AMBIENT, |a| a.ambient_colors)
    }

    fn update_lyrics(&mut self) {
        let track = self
            .track
            .as_ref()
            .filter(|_| self.sections.lyrics.show)
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
    /// `strip_scale` is the layout's text scale for the strip.
    pub(crate) fn lyrics_view(
        &self,
        placement: Placement,
        strip_scale: f32,
    ) -> Option<Element<'_, Message>> {
        let (view, font_size) = self.lyrics_canvas(placement, strip_scale)?;
        Some(match placement {
            Placement::Strip => view.height(font_size * 4.6).into(),
            Placement::Column => view.height(Length::Fill).into(),
        })
    }

    /// The same, before it gets its height, with its font size.
    pub(crate) fn lyrics_canvas(
        &self,
        placement: Placement,
        strip_scale: f32,
    ) -> Option<(Canvas<LyricsView<'_, Message>, Message>, f32)> {
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
        let scale = self.sections.lyrics.size as f32 / 100.0;
        let font_size = match placement {
            // A narrow stage gets slightly smaller lines so they wrap less.
            Placement::Strip => 26.0 * strip_scale,
            Placement::Column => 30.0,
        } * scale;
        let position = t.position_at(self.now).as_secs_f32();
        let time = position + lyric_offset(self.sections.lyrics.offset_ms, position, t.length);
        let progress = t
            .length
            .map(|len| (t.position_at(self.now).as_secs_f32() / len.as_secs_f32()).clamp(0.0, 1.0));
        let view = Canvas::new(LyricsView {
            lyrics,
            time,
            progress,
            placement,
            font_size,
            glow: self.sections.lyrics.glow as f32 / 100.0,
            word_sync: self.sections.lyrics.word_sync,
            on_seek: Message::SeekLyric,
        })
        .width(Length::Fill);
        Some((view, font_size))
    }

    pub(crate) fn target_accent(&self) -> [u8; 3] {
        match self.sections.colors.accent.rgb() {
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

    /// Bar colors the palette asks for right now. Album colors come from the cover:
    /// one color (Solid), the cover's gradient, or a gradient around the chosen accent.
    fn target_bar_stops(&self) -> Vec<[u8; 3]> {
        if self.sections.colors.palette != Palette::Album {
            return self.sections.colors.palette.stops();
        }
        let art = self.art.as_ref();
        let base = art
            .and_then(|a| a.accent)
            .unwrap_or_else(|| self.target_accent());
        if !self.sections.colors.album_gradient {
            return vec![base];
        }
        if self.sections.colors.album_match_accent
            && let Some(accent) = self.sections.colors.accent.rgb()
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
        self.motion_time
    }

    /// Radial style: the center cover turns once every 24 s while music plays.
    /// Seconds of music played (the clock custom visualizers get).
    pub(crate) fn cover_time(&self) -> f32 {
        self.bg_time
    }

    /// Enter idle mode once nothing has moved for a moment; leave it as soon as anything does.
    fn update_idle(&mut self, now: Instant, sound: bool) {
        let settled = |v: &[f32]| v.iter().all(|&x| x < 0.003);
        // Frozen bars don't move, so they don't keep the app awake.
        let frozen = !sound && self.sections.sound.when_silent == SilenceMode::Freeze;
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
                .any(|(a, b)| a.iter().zip(b).any(|(x, y)| (x - y).abs() > 0.002));
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

    /// Auto-hide's "only when the mouse moves": other input leaves the controls hidden.
    fn move_only(&self) -> bool {
        self.sections.general.auto_hide && self.sections.general.show_on_move_only
    }

    fn update_auto_hide(&mut self, now: Instant, dt: f32) {
        let a = &self.sections.general;
        let idle = now.saturating_duration_since(self.last_activity)
            >= Duration::from_secs(a.hide_after_s as u64);
        let over_controls = self
            .cursor
            .is_some_and(|p| p.y < HEADER_ZONE || p.y > self.window_size.height - CONTROLS_ZONE);
        let hide = a.auto_hide
            && idle
            && !over_controls
            && !self.about_open
            && !self.settings.shown()
            && !self.picker_open
            && !self.quick_open
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
        self.sections.background.is_dynamic()
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
            .filter(|_| self.sections.now_playing.show_album_art)
            .map(|a| &a.thumb)
    }

    fn on_key(&mut self, event: keyboard::Event) -> Task<Message> {
        let keyboard::Event::KeyPressed { key, modifiers, .. } = event else {
            return Task::none();
        };
        if modifiers.logo() {
            return Task::none();
        }
        if self.settings.capturing.is_some() {
            self.capture_key(&key, modifiers);
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
        let shortcut =
            combo(&key, modifiers).and_then(|c| self.sections.controls.shortcuts.action(&c));
        // With Ctrl or Alt there are only shortcuts.
        if modifiers.control() || modifiers.alt() {
            return match shortcut {
                Some(action) => self.act(action),
                None => Task::none(),
            };
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
            Key::Named(Named::Escape) => {
                if self.update.shown() {
                    Some(Message::CloseUpdate)
                } else if self.picker_open {
                    Some(Message::ClosePicker)
                } else if self.quick_open {
                    Some(Message::ToggleQuick)
                } else if self.settings.shown() {
                    // Like ×, once whatever is open inside is closed.
                    Some(Message::Settings(if self.settings.editing.is_some() {
                        SettingsMessage::Edit(None)
                    } else if self.settings.menu {
                        SettingsMessage::ToggleMenu
                    } else {
                        SettingsMessage::Done
                    }))
                } else if self.settings.open {
                    Some(SettingsMessage::EndPeek.into())
                } else if self.volume_open {
                    Some(Message::ToggleVolume)
                } else if self.about_open {
                    Some(Message::ToggleAbout)
                } else if self.fullscreen {
                    Some(Message::ToggleFullscreen)
                } else {
                    self.focus = None;
                    None
                }
            }
            _ => {
                return match shortcut {
                    Some(action) => self.act(action),
                    None => Task::none(),
                };
            }
        };
        match message {
            Some(m) => self.update(m),
            None => Task::none(),
        }
    }

    /// Do what a shortcut is for.
    fn act(&mut self, action: Action) -> Task<Message> {
        if action.needs_media() && !self.platform.media {
            return Task::none();
        }
        let open = |id| Message::from(SettingsMessage::Open(id));
        let message = match action {
            Action::PlayPause => Message::PlayPause,
            Action::Previous => Message::Previous,
            Action::Next => Message::Next,
            Action::SeekBack => return self.step_seek(false),
            Action::SeekForward => return self.step_seek(true),
            Action::VolumeUp | Action::VolumeDown => {
                self.step_volume(action == Action::VolumeUp);
                return Task::none();
            }
            Action::Mute => Message::ToggleMute,
            Action::Repeat => Message::ToggleRepeat,
            Action::Fullscreen => Message::ToggleFullscreen,
            Action::Settings => SettingsMessage::Toggle.into(),
            Action::Quick => Message::ToggleQuick,
            Action::Source => Message::TogglePicker,
            Action::About => Message::ToggleAbout,
            Action::Refresh => Message::Refresh,
            Action::Sound => open(SectionId::Sound),
            Action::Visualizer => open(SectionId::Visualizer),
            Action::Background => open(SectionId::Background),
            Action::General => open(SectionId::General),
            Action::Favorite => match self.focused_favorite() {
                Some(id) => Message::ToggleFavorite(id),
                None => return Task::none(),
            },
        };
        // What opens on the screen is behind the settings while they're open.
        let behind = matches!(action, Action::Quick | Action::Source | Action::About);
        if behind && self.settings.shown() {
            return Task::none();
        }
        self.update(message)
    }

    /// The starred id of the setting that has the keyboard focus.
    fn focused_favorite(&self) -> Option<String> {
        let focus = self.focus.as_ref()?;
        let rows = self.row_focus.borrow();
        let row = rows.iter().find(|r| r.focus == *focus)?;
        row.favorite.clone()
    }

    /// How far one step of seeking goes.
    pub(crate) fn seek_step(&self) -> Duration {
        Duration::from_secs(self.sections.controls.seek_step_s.into())
    }

    fn step_seek(&mut self, forward: bool) -> Task<Message> {
        let step = self.seek_step();
        let Some((t, len)) = self
            .track
            .as_ref()
            .filter(|t| t.can_seek)
            .and_then(|t| Some((t, t.length?)))
        else {
            return Task::none();
        };
        let pos = t.position_at(self.now);
        let target = if forward {
            (pos + step).min(len)
        } else {
            pos.saturating_sub(step)
        };
        self.update(Message::Seek(target.as_secs_f32() / len.as_secs_f32()))
    }

    fn step_volume(&mut self, up: bool) {
        if let Some(vol) = self.track.as_ref().and_then(|t| t.volume) {
            let step = f64::from(self.sections.controls.volume_step) / 100.0;
            let step = if up { step } else { -step };
            if up {
                self.unmuted_volume = None;
            }
            self.media
                .send(Command::SetVolume((vol + step).clamp(0.0, 1.0)));
        }
    }

    /// How far round the refresh icon is, for a moment after it was pressed (0..=1).
    pub(crate) fn refresh_turn(&self) -> Option<f32> {
        let t = self.now.saturating_duration_since(self.refreshed?);
        let turn = t.as_secs_f32() / REFRESH_TURN.as_secs_f32();
        (turn < 1.0 && !self.reduced_motion.on()).then_some(turn)
    }

    /// The key for the shortcut being changed.
    fn capture_key(&mut self, key: &Key, modifiers: keyboard::Modifiers) {
        let Some((action, slot)) = self.settings.capturing else {
            return;
        };
        let bare = modifiers.is_empty();
        let binding = match key.as_ref() {
            Key::Named(Named::Escape) if bare => {
                self.settings.capturing = None;
                return;
            }
            Key::Named(Named::Backspace) if bare => None,
            _ => match combo(key, modifiers) {
                Some(c) => Some(c),
                // A key that's only held down, or one that can't be a shortcut.
                None => return,
            },
        };
        self.settings.capturing = None;
        let controls = &mut self.sections.controls;
        controls.shortcuts.bind(action, slot, binding);
        self.mark_dirty();
    }

    /// Tab / Shift+Tab. Text inputs get real iced focus; controls in a scrolling list
    /// scroll into view.
    fn move_focus(&mut self, back: bool) -> Task<Message> {
        let trap: Option<&[Scope]> = if self.update.shown() {
            Some(&[Scope::Dialog])
        } else if self.picker_open {
            Some(&[Scope::Picker])
        } else if self.quick_open && !self.settings.shown() {
            Some(&[Scope::Quick])
        } else if self.settings.shown() && self.settings.menu {
            Some(&[Scope::Menu])
        } else if self.settings.shown() {
            Some(&Scope::SETTINGS)
        } else {
            None
        };
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
            let scroll = match self.focus_reg.position(id) {
                Some((i, n, Scope::Panel)) => Some((PANEL_SCROLL, i, n)),
                Some((i, n, Scope::Content)) => Some((settings::CONTENT_SCROLL, i, n)),
                Some((i, n, Scope::Tree)) => Some((settings::TREE_SCROLL, i, n)),
                Some((i, n, Scope::Menu)) => Some((settings::MENU_SCROLL, i, n)),
                _ => None,
            };
            if let Some((scroll, i, n)) = scroll {
                let y = i as f32 / n.saturating_sub(1).max(1) as f32;
                tasks.push(iced::widget::operation::snap_to(
                    iced::widget::Id::new(scroll),
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
/// Focusing an id no widget has blurs every text input.
const NO_TEXT_INPUT: &str = "no-text-input";

fn focus_text_input(id: &'static str) -> Task<Message> {
    iced::widget::operation::focus(iced::widget::Id::new(id))
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

/// The key that was pressed, as a shortcut. `None` for keys that can't be one.
fn combo(key: &Key, modifiers: keyboard::Modifiers) -> Option<Combo> {
    let key = match key.as_ref() {
        Key::Character(c) => c.to_lowercase(),
        Key::Named(
            Named::Shift
            | Named::Control
            | Named::Alt
            | Named::AltGraph
            | Named::Super
            | Named::Meta
            | Named::Hyper
            | Named::Fn
            | Named::FnLock
            | Named::CapsLock
            | Named::NumLock
            | Named::ScrollLock,
        )
        | Key::Unidentified => return None,
        Key::Named(named) => format!("{named:?}"),
    };
    let mut out = Combo::parse(&key)?;
    out.ctrl = modifiers.control();
    out.alt = modifiers.alt();
    out.shift = modifiers.shift();
    Some(out)
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
    use crate::settings::{GradientDir, NowPlayingMode, PresetId, SectionPresets, VisualSettings};
    use crate::settings::{PlaybackStyle, SeekHandle};
    use settings::{Editing, Goal, Page, SettingKey, Shape};

    /// Open the settings window at a section.
    fn open(app: &mut App, id: SectionId) {
        let _ = app.update(SettingsMessage::Open(id).into());
    }

    fn send(app: &mut App, m: SettingsMessage) {
        let _ = app.update(m.into());
    }

    /// Set a setting of the visualizer in use (as its control would).
    fn vis_set(app: &mut App, id: &str, value: impl Into<serde_json::Value>) {
        let name = app.sections.visualizer.shader.clone();
        app.sections
            .visualizer
            .values
            .entry(name)
            .or_default()
            .insert(id.into(), value.into());
    }

    /// Set a setting of the background in use.
    fn bg_set(app: &mut App, id: &str, value: impl Into<serde_json::Value>) {
        let name = app.sections.background.shader.clone();
        app.sections
            .background
            .values
            .entry(name)
            .or_default()
            .insert(id.into(), value.into());
    }

    /// Settings with the app's own fonts: what fits on a line mustn't depend on the
    /// fonts this machine has.
    fn own_fonts() -> iced::Settings {
        iced::Settings {
            fonts: FONTS
                .iter()
                .map(|f| std::borrow::Cow::Borrowed(*f))
                .collect(),
            default_font: INTER,
            ..iced::Settings::default()
        }
    }

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
            app.sections = Sections::default();
            app.settle_presets();
            app.updates = UpdateSettings::default();
            app.monitor_sections = MonitorSettings::default();
            app.platform = Platform::full();
            app.accent = rgb_f(app.target_accent());
            let now = Instant::now();
            app.now = now;
            app.track = Some(Track {
                sampled_at: now,
                ..test_track()
            });
            setup(&mut app);
            app.refresh_shaders();

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
            let map = BarMap::for_engine(&app.vis_engine);
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
            let bins = map.bins(app.vis_engine.bar_count);
            for _ in 0..120 {
                app.processor
                    .update(&frames, bins, &app.sections.sound, 1.0 / 60.0);
            }

            let mut sim = Simulator::with_size(own_fonts(), (width, height), app.view());
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
        render("max-length-long", &|app| vis_set(app, "max_length", 1200));
        render("max-length-short", &|app| vis_set(app, "max_length", 150));
        render("max-length-wave", &|app| {
            app.sections.visualizer.shader = shaders::WAVE.into();
            vis_set(app, "max_length", 1200);
        });
        render("max-length-radial", &|app| {
            app.sections.visualizer.shader = shaders::RADIAL.into();
            vis_set(app, "max_length", 120);
        });
        // Narrow windows (the sizes from the scaling bug report).
        render_at("narrow-923-about", 923.0, 1031.0, &|app| {
            app.about_open = true
        });
        render_at("narrow-923-settings", 923.0, 1031.0, &|app| {
            open(app, SectionId::Sound)
        });
        render_at("narrow-923-settings-no-preview", 923.0, 1031.0, &|app| {
            open(app, SectionId::Sound);
            send(app, SettingsMessage::TogglePreview);
        });
        render_at("narrow-606-settings", 606.0, 1031.0, &|app| {
            open(app, SectionId::Visualizer)
        });
        render_at("narrow-480-settings", 480.0, 520.0, &|app| {
            open(app, SectionId::NowPlaying)
        });
        render_at("narrow-606-about", 606.0, 1031.0, &|app| {
            app.about_open = true
        });
        render_at("narrow-606", 606.0, 1031.0, &|_| {});
        render_at("narrow-480-picker", 480.0, 600.0, &|app| {
            app.picker_open = true
        });
        let offer = |app: &mut App| {
            app.install = Some(Install::Prefix("/usr".into()));
            app.update = UpdateState::Offer {
                release: test_release(),
                auto: true,
                dont_ask: false,
            };
        };
        render("update-offer", &offer);
        render_at("narrow-480-update", 480.0, 600.0, &offer);
        let unstable = |app: &mut App| {
            app.install = Some(Install::Prefix("/usr".into()));
            app.update = UpdateState::Offer {
                release: Release {
                    version: "1.0.0-alpha".into(),
                    unstable: true,
                    ..test_release()
                },
                auto: false,
                dont_ask: false,
            };
        };
        render("update-unstable", &unstable);
        render_at("narrow-480-update-unstable", 480.0, 600.0, &unstable);
        render("update-package", &|app| {
            app.install = Some(Install::Package);
            app.update = UpdateState::Offer {
                release: test_release(),
                auto: false,
                dont_ask: false,
            };
        });
        render("update-installed", &|app| {
            app.update = UpdateState::Installed {
                version: "0.10.0".into(),
            };
        });
        render("update-failed", &|app| {
            app.update = UpdateState::Failed {
                error: "the download doesn't match its checksum".into(),
                page: "https://example.invalid".into(),
            };
        });
        render_at("narrow-spotlight", 760.0, 900.0, &|app| {
            app.sections.now_playing.mode = NowPlayingMode::Spotlight;
        });
        // The settings window: each section at its top, and all of the longer ones.
        for id in SectionId::ALL {
            let name = id.name().to_lowercase().replace(' ', "-");
            render(&format!("settings-{name}"), &|app| open(app, id));
        }
        // (The window's height is capped, so these are scrolled rather than tall.)
        render("settings-ultrawide", &|app| {
            open(app, SectionId::Visualizer);
            send(app, SettingsMessage::Shape(Shape::Ultrawide));
        });
        render("settings-narrow-shape", &|app| {
            open(app, SectionId::Visualizer);
            send(app, SettingsMessage::Shape(Shape::Narrow));
        });
        render("settings-collapsed-preview", &|app| {
            open(app, SectionId::Visualizer);
            send(app, SettingsMessage::TogglePreview);
        });
        render("settings-peek", &|app| {
            open(app, SectionId::Visualizer);
            send(app, SettingsMessage::Peek);
        });
        render("settings-search", &|app| {
            open(app, SectionId::Visualizer);
            send(app, SettingsMessage::Search("gl".into()));
        });
        render_at("settings-big-screen", 2560.0, 1440.0, &|app| {
            open(app, SectionId::Visualizer)
        });
        // The preset menu in each of its states.
        render("settings-presets-plain", &|app| {
            open(app, SectionId::Visualizer);
            send(app, SettingsMessage::ToggleMenu);
        });
        render("settings-presets-built-in-edited", &|app| {
            open(app, SectionId::Visualizer);
            vis_set(app, "bar_count", 20);
            send(app, SettingsMessage::ToggleMenu);
        });
        let saved = |app: &mut App| {
            open(app, SectionId::Visualizer);
            send(
                app,
                SettingsMessage::LoadPreset(
                    SectionId::Visualizer,
                    PresetId::BuiltIn("Radial".into()),
                ),
            );
            send(app, SettingsMessage::SaveNewPreset);
            send(
                app,
                SettingsMessage::Rename(
                    Editing::Preset(SectionId::Visualizer, 0),
                    "My radial".into(),
                ),
            );
            send(app, SettingsMessage::Edit(None));
        };
        render("settings-presets-yours-edited", &|app| {
            saved(app);
            vis_set(app, "peak_caps", true);
            app.focus = None;
            send(app, SettingsMessage::ToggleMenu);
        });
        render("settings-presets-rename", &|app| {
            saved(app);
            send(app, SettingsMessage::SaveNewPreset);
            send(
                app,
                SettingsMessage::Delete(Editing::Preset(SectionId::Visualizer, 1)),
            );
            app.settings.menu = true;
            app.focus = Some(FocusId::Indexed("preset-delete", 1));
        });
        render_at("narrow-480-presets", 480.0, 520.0, &|app| {
            saved(app);
            vis_set(app, "peak_caps", true);
            send(app, SettingsMessage::ToggleMenu);
        });
        // The signal monitor, with a few seconds of history.
        let monitored = |app: &mut App| {
            open(app, SectionId::Sound);
            app.monitor_open = true;
            app.monitor_sections = MonitorSettings::default();
            app.sections.sound.gate_db = -70;
            let start = app.now;
            for i in 0..240 {
                let t = i as f32 / 60.0;
                let pulse = if (t % 0.5) < 0.12 {
                    1.0 - (t % 0.5) * 4.0
                } else {
                    0.0
                };
                let visible: Vec<f32> = (0..43).map(|k| 0.9 - k as f32 * 0.018).collect();
                let spectrum: Vec<f32> = (0..=FFT_SIZE / 2)
                    .map(|k| -20.0 - 0.012 * k as f32 - 8.0 * (k as f32 * 0.37).sin().abs())
                    .collect();
                app.monitor.record(crate::monitor::Reading {
                    now: start + Duration::from_secs_f32(t),
                    dt: 1.0 / 60.0,
                    input_db: Some(-28.0 + 6.0 * (t * 3.0).sin()),
                    loudest_db: Some(-14.0 + 5.0 * (t * 2.0).sin()),
                    sound: true,
                    pulse,
                    bg_pulse: pulse * 0.6,
                    visible: &visible,
                    per_channel: visible.len(),
                    spectrum: Some((&spectrum, 48_000.0, FFT_SIZE)),
                });
            }
        };
        render("settings-monitor", &monitored);
        // Each part on its own (the window isn't tall enough for all of them).
        let only = |part: fn(&mut MonitorSettings)| {
            move |app: &mut App| {
                monitored(app);
                app.monitor_sections = MonitorSettings {
                    signal: false,
                    spectrum: false,
                    beats: false,
                    performance: false,
                };
                part(&mut app.monitor_sections);
            }
        };
        render("settings-monitor-signal", &only(|s| s.signal = true));
        render("settings-monitor-spectrum", &only(|s| s.spectrum = true));
        render("settings-monitor-beats", &only(|s| s.beats = true));
        render("settings-monitor-system", &only(|s| s.performance = true));
        render_at("narrow-480-monitor", 480.0, 900.0, &monitored);
        // Up/down halves join into one continuous bar (rounded only at the tips).
        render("mirror-up-down-pill", &|app| {
            vis_set(app, "mirror", "Up/down");
            vis_set(app, "corners", "Pill");
        });
        render("mirror-radial-rounded", &|app| {
            app.sections.visualizer.shader = shaders::RADIAL.into();
            vis_set(app, "mirror", "Up/down");
            vis_set(app, "corners", "Rounded");
        });
        // The playback bar's styles, with a cover.
        let playback = |style: PlaybackStyle, compact: bool| {
            move |app: &mut App| {
                app.art = Some(test_cover());
                app.sections.playback.style = style;
                app.sections.playback.compact = compact;
            }
        };
        for (name, style) in [
            ("open", PlaybackStyle::Open),
            ("card", PlaybackStyle::Card),
            ("dock", PlaybackStyle::Dock),
        ] {
            render(&format!("playback-{name}"), &playback(style, false));
            render(&format!("playback-{name}-compact"), &playback(style, true));
            render_at(
                &format!("narrow-480-playback-{name}"),
                480.0,
                700.0,
                &playback(style, false),
            );
            render_at(
                &format!("narrow-480-playback-{name}-compact"),
                480.0,
                700.0,
                &playback(style, true),
            );
            for width in [900.0, 660.0, 480.0] {
                let volume = format!("playback-{name}-volume-{width}");
                render_at(&volume, width, 620.0, &|app| {
                    playback(style, false)(app);
                    app.volume_open = true;
                });
            }
        }
        render("playback-custom", &|app| {
            playback(PlaybackStyle::Card, false)(app);
            let p = &mut app.sections.playback.settings;
            (p.curve_height, p.curve_length) = (Some(12), Some(160));
            (p.thickness, p.remaining) = (Some(8), Some(80));
            (p.handle, p.times, p.opacity) = (SeekHandle::Dot, false, 40);
        });
        render("settings-playback", &|app| {
            playback(PlaybackStyle::Card, false)(app);
            open(app, SectionId::Playback);
        });
        render("settings-playback-only", &|app| {
            playback(PlaybackStyle::Card, false)(app);
            open(app, SectionId::Playback);
            send(app, SettingsMessage::ToggleOnly);
        });
        // Changed settings with their undo buttons, and one that was undone.
        render("settings-undo", &|app| {
            open(app, SectionId::Sound);
            let t = &mut app.sections.sound.settings;
            (t.sensitivity, t.spread, t.gravity) = (20, 80, true);
            let spread = SettingKey::Fields(SectionId::Sound, &["spread"]);
            send(app, SettingsMessage::Undo(spread));
            send(
                app,
                SettingsMessage::Fold(SectionId::Sound.into(), "Levels".into()),
            );
        });
        render("settings-folded", &|app| {
            open(app, SectionId::Sound);
            for g in ["Levels", "Response", "Frequency", "Bands"] {
                send(
                    app,
                    SettingsMessage::Fold(SectionId::Sound.into(), g.into()),
                );
            }
        });
        // A setting right after it was searched for.
        render("settings-found", &|app| {
            open(app, SectionId::Visualizer);
            send(app, SettingsMessage::Search("bar w".into()));
            send(app, SettingsMessage::SearchGo);
            app.now = Instant::now();
            app.focus = Some(FocusId::Indexed("vis-setting", 1));
        });
        // The beat, under what follows it.
        render("settings-glow-beat", &|app| {
            open(app, SectionId::Visualizer);
            vis_set(app, "glow", true);
            app.beat.set_pulse(0.8);
            for g in ["Style", "Bars", "Mirror", "Details"] {
                send(
                    app,
                    SettingsMessage::Fold(SectionId::Visualizer.into(), g.into()),
                );
            }
        });
        render("settings-pulse-beat", &|app| {
            open(app, SectionId::Background);
            bg_set(app, "pulse", true);
            app.bg_beat.set_pulse(0.4);
            for g in ["Style", "Layout", "Motion", "Effects"] {
                send(
                    app,
                    SettingsMessage::Fold(SectionId::Background.into(), g.into()),
                );
            }
        });
        // The quick settings, with and without any.
        let starred = |app: &mut App| {
            app.art = Some(test_cover());
            app.quick_open = true;
            app.favorites = [
                "visualizer.shader",
                "sound.sensitivity",
                "sound.spread",
                "background:pulse",
                "lyrics.show",
                "visualizer:glow",
                "general.auto_hide",
                "colors.accent",
            ]
            .map(String::from)
            .to_vec();
        };
        render("quick", &starred);
        render("quick-empty", &|app| app.quick_open = true);
        render("quick-more", &|app| {
            starred(app);
            let more = [
                "playback.style",
                "now_playing.mode",
                "sound.floor_db",
                "sound.calibrate",
                "colors.palette",
                "lyrics.size",
            ];
            app.favorites.extend(more.map(String::from));
            app.sections.colors.accent = Accent::White;
            app.focus = Some(FocusId::Named("sensitivity"));
        });
        render("quick-rose", &|app| {
            starred(app);
            app.sections.colors.accent = Accent::Rose;
            app.accent = rgb_f(app.target_accent());
        });
        render_at("narrow-480-quick", 480.0, 520.0, &starred);
        // Settings with stars, some of them lit, and the whole tree expanded.
        render("settings-stars", &|app| {
            starred(app);
            app.quick_open = false;
            open(app, SectionId::Sound);
            send(app, SettingsMessage::ExpandAll(true));
        });
        // Shortcuts, one of them waiting for its key.
        render("settings-controls-waiting", &|app| {
            open(app, SectionId::Controls);
            send(app, SettingsMessage::Capture(Action::Next, 0));
            let c = &mut app.sections.controls.shortcuts;
            c.give(Action::Mute, vec![Combo::parse("ctrl+shift+m").unwrap()]);
        });
        render_at("narrow-480-controls", 480.0, 800.0, &|app| {
            open(app, SectionId::Controls);
        });
        // Every section's presets, and a group of them.
        let grouped = |app: &mut App| {
            open(app, SectionId::Colors);
            send(app, SettingsMessage::SaveNewPreset);
            let what = Editing::Preset(SectionId::Colors, 0);
            send(app, SettingsMessage::Rename(what, "Mine".into()));
            send(app, SettingsMessage::Page(Page::Presets));
            for name in ["Night drive", "Focus"] {
                send(app, SettingsMessage::NewGroup);
                let i = app.sections.groups.len() - 1;
                send(app, SettingsMessage::Rename(Editing::Group(i), name.into()));
            }
            send(app, SettingsMessage::SetInGroup(1, SectionId::Sound, None));
        };
        render("settings-presets-page", &|app| {
            grouped(app);
            send(app, SettingsMessage::Edit(None));
        });
        render("settings-presets-group", &|app| {
            grouped(app);
            send(app, SettingsMessage::Choose(SectionId::Colors));
        });
        render_at("narrow-480-presets-page", 480.0, 900.0, &|app| {
            grouped(app);
            send(app, SettingsMessage::Choose(SectionId::Colors));
        });
        // Custom visualizers, from the examples.
        let examples = std::env::temp_dir().join(format!("undavi-vis-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&examples);
        for (name, code) in shaders::EXAMPLES {
            let _ = std::fs::write(examples.join(name), code);
        }
        let custom_vis = |name: &'static str| {
            let examples = examples.clone();
            move |app: &mut App| {
                app.shaders = shaders::Library::new(Some(examples.clone()));
                app.shaders.refresh(app.now);
                app.sections.visualizer.shader = name.into();
            }
        };
        render("vis-custom-led-meter", &custom_vis("led-meter"));
        render("vis-custom-orbit", &|app| {
            custom_vis("orbit")(app);
            open(app, SectionId::Visualizer);
        });
        render("settings-visualizer-custom", &|app| {
            open(app, SectionId::Visualizer);
            vis_set(app, "segments", true);
            vis_set(app, "peak_caps", true);
            vis_set(app, "corners", "Pill");
            vis_set(app, "mirror", "Both");
            app.sections.colors.palette = Palette::Aurora;
            app.sections.colors.gradient = GradientDir::Along;
        });
        render("picker", &|app| app.picker_open = true);
        render("about", &|app| app.about_open = true);
        render("about-license", &|app| {
            app.about_open = true;
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
        // Keyboard focus in the settings window.
        for (name, section, id) in [
            (
                "focus-slider",
                SectionId::Sound,
                FocusId::Named("sensitivity"),
            ),
            (
                "focus-tree",
                SectionId::Sound,
                FocusId::Indexed("section", 3),
            ),
            (
                "focus-cards",
                SectionId::Visualizer,
                FocusId::Named("vis-shader"),
            ),
            (
                "focus-shape",
                SectionId::Visualizer,
                FocusId::Named("window-shape"),
            ),
            ("focus-only", SectionId::Visualizer, FocusId::Named("only")),
            ("focus-done", SectionId::Visualizer, FocusId::Named("done")),
        ] {
            render(name, &|app| {
                open(app, section);
                app.focus = Some(id.clone());
            });
        }
        render("volume-popover", &|app| {
            app.volume_open = true;
            app.focus = Some(FocusId::Named("volume-slider"));
        });
        render("settings-sound-live", &|app| {
            open(app, SectionId::Sound);
            app.sections.sound.auto_gain = true;
            // Live readings for the meter.
            app.gate.set_open(true);
            app.meter = Meter {
                input_db: Some(-31.0),
                loudest_db: Some(-14.0),
            };
            app.beat.set_pulse(0.8);
        });
        render("settings-sound-silent", &|app| {
            open(app, SectionId::Sound);
            app.sections.sound.gate_db = -60;
            app.meter = Meter {
                input_db: Some(-74.0),
                loudest_db: None,
            };
        });
        render("visuals-glow", &|app| {
            vis_set(app, "glow", true);
            vis_set(app, "glow_amount", 100);
            app.sections.colors.palette = Palette::Aurora;
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
        let with_art = |app: &mut App| {
            app.art = Some(art.clone());
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
            app.sections.now_playing.mode = NowPlayingMode::Spotlight;
        });
        render("lyrics-minimal", &|app| {
            with_lyrics(app);
            app.sections.now_playing.mode = NowPlayingMode::Minimal;
        });
        // The settings window over a real screen, whole and with only the part each
        // section affects in the preview.
        let full = |app: &mut App, id: SectionId| {
            with_lyrics(app);
            app.track = app.track.take().map(|t| Track {
                title: "Somewhere, slowly".into(),
                ..t
            });
            open(app, id);
        };
        render("settings", &|app| full(app, SectionId::Visualizer));
        for id in SectionId::ALL {
            let name = id.name().to_lowercase().replace(' ', "-");
            render(&format!("settings-only-{name}"), &|app| {
                full(app, id);
                send(app, SettingsMessage::ToggleOnly);
            });
        }
        render("settings-only-spotlight", &|app| {
            full(app, SectionId::NowPlaying);
            app.sections.now_playing.mode = NowPlayingMode::Spotlight;
            send(app, SettingsMessage::ToggleOnly);
        });
        render("settings-only-lyrics-spotlight", &|app| {
            full(app, SectionId::Lyrics);
            app.sections.now_playing.mode = NowPlayingMode::Spotlight;
            send(app, SettingsMessage::ToggleOnly);
        });
        render("settings-only-lyrics-off", &|app| {
            full(app, SectionId::Lyrics);
            app.sections.lyrics.show = false;
            send(app, SettingsMessage::ToggleOnly);
        });
        render("settings-spotlight", &|app| {
            full(app, SectionId::NowPlaying);
            app.sections.now_playing.mode = NowPlayingMode::Spotlight;
        });
        render("settings-solid", &|app| {
            full(app, SectionId::Background);
            app.sections.background.shader = shaders::SOLID.into();
        });
        render_at("settings-ultrawide-full", 1920.0, 1080.0, &|app| {
            full(app, SectionId::Background);
            send(app, SettingsMessage::Shape(Shape::Ultrawide));
        });
        render("style-radial", &|app| {
            with_art(app);
            app.sections.visualizer.shader = shaders::RADIAL.into();
            vis_set(app, "bar_count", 72);
            vis_set(app, "bar_width", 6);
            vis_set(app, "corners", "Pill");
            app.sections.colors.palette = Palette::Aurora;
        });
        render("style-radial-both", &|app| {
            with_art(app);
            app.sections.visualizer.shader = shaders::RADIAL.into();
            vis_set(app, "mirror", "Both");
            vis_set(app, "segments", true);
            vis_set(app, "peak_caps", true);
            app.sections.colors.palette = Palette::Ember;
            app.sections.colors.gradient = GradientDir::Along;
        });
        render("album-gradient", &|app| {
            with_art(app);
            open(app, SectionId::Colors);
            app.sections.colors.palette = Palette::Album;
            app.sections.colors.gradient = GradientDir::Along;
            app.sections.colors.accent = Accent::Mint;
        });
        render("album-solid", &|app| {
            with_art(app);
            open(app, SectionId::Colors);
            app.sections.colors.palette = Palette::Album;
            app.sections.colors.album_gradient = false;
        });
        render("album-match-accent", &|app| {
            with_art(app);
            app.sections.colors.palette = Palette::Album;
            app.sections.colors.album_match_accent = true;
            app.sections.colors.accent = Accent::Rose;
        });
        render("stereo-bars", &|app| {
            with_art(app);
            vis_set(app, "mirror", "Left/right");
            app.sections.colors.palette = Palette::Aurora;
        });
        render("stereo-off", &|app| {
            with_art(app);
            vis_set(app, "mirror", "Left/right");
            vis_set(app, "split", "Off");
            app.sections.colors.palette = Palette::Aurora;
        });
        render("style-wave", &|app| {
            with_art(app);
            app.sections.visualizer.shader = shaders::WAVE.into();
            vis_set(app, "bar_count", 64);
            app.sections.colors.palette = Palette::Aurora;
            vis_set(app, "glow", true);
            vis_set(app, "glow_amount", 60);
            app.beat.set_pulse(0.8);
        });
        render("style-wave-off", &|app| {
            with_art(app);
            app.sections.visualizer.shader = shaders::WAVE.into();
            vis_set(app, "mirror", "Off");
            app.sections.colors.palette = Palette::Undavi;
            app.sections.colors.gradient = GradientDir::Along;
        });
        render("background-pulse", &|app| {
            with_art(app);
            open(app, SectionId::Background);
            bg_set(app, "pulse", true);
            app.bg_pulse = 1.0;
        });
        render("custom-palette", &|app| {
            with_art(app);
            open(app, SectionId::Colors);
            app.sections.colors.palette = Palette::Custom(DEFAULT_CUSTOM.to_vec());
            app.custom_hex[1] = "#12".into();
        });
        // Album art background options.
        render("bg-single", &|app| {
            with_art(app);
            bg_set(app, "arrangement", "Single");
        });
        render("bg-tiles", &|app| {
            with_art(app);
            bg_set(app, "arrangement", "Tiles");
            bg_set(app, "blur", 20);
        });
        render("bg-animated-liquid", &|app| {
            with_art(app);
            bg_set(app, "arrangement", "Single");
            bg_set(app, "blur", 20);
            bg_set(app, "animated", true);
            app.motion_time = 12.0;
        });
        render("bg-animated-warp", &|app| {
            with_art(app);
            bg_set(app, "arrangement", "Single");
            bg_set(app, "blur", 20);
            bg_set(app, "animated", true);
            bg_set(app, "animation", "Warp");
            app.motion_time = 6.0;
        });
        render("bg-animated-spiral", &|app| {
            with_art(app);
            bg_set(app, "arrangement", "Single");
            bg_set(app, "blur", 20);
            bg_set(app, "animated", true);
            bg_set(app, "animation", "Spiral");
            app.motion_time = 10.0;
        });
        render("bg-custom-picker", &|app| {
            with_art(app);
            app.shaders = shaders::Library::new(Some(examples.clone()));
            app.shaders.refresh(app.now);
            app.sections.background.shader = "plasma".into();
            open(app, SectionId::Background);
        });
        render("bg-effects", &|app| {
            with_art(app);
            app.sections.background.saturation = 60;
            app.sections.background.contrast = 130;
            app.sections.background.tint_pct = 60;
            app.sections.background.vignette_pct = 80;
            app.sections.background.grain_pct = 60;
        });
        render("album-accent", &|app| {
            with_art(app);
            app.sections.colors.accent = Accent::Album;
            open(app, SectionId::Colors);
            app.sections.background.shader = shaders::AMBIENT.into();
        });
        render("spotlight", &|app| {
            with_art(app);
            app.sections.now_playing.mode = NowPlayingMode::Spotlight;
        });
        render("minimal-solid", &|app| {
            with_art(app);
            app.sections.now_playing.mode = NowPlayingMode::Minimal;
            app.sections.background.shader = shaders::SOLID.into();
            bg_set(app, "color", "#0f1424");
        });
        render("auto-hidden", &|app| {
            with_art(app);
            app.ui_alpha = 0.0;
        });
    }

    #[test]
    fn custom_shaders_are_picked_and_errors_fall_back() {
        let dir = std::env::temp_dir().join(format!("undavi-app-shaders-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("good.wgsl"),
            "fn warp(q: vec2<f32>, t: f32) -> vec2<f32> { return q * (1.0 + 0.1 * bass()); }",
        )
        .unwrap();
        std::fs::write(
            dir.join("broken.wgsl"),
            "fn scene(uv: vec2<f32>, t: f32) -> vec3<f32> { return uv; }",
        )
        .unwrap();

        let mut app = test_app();
        app.shaders = shaders::Library::new(Some(dir.clone()));
        app.sections.background.shader = shaders::AMBIENT.into();
        bg_set(&mut app, "animated", true);
        open(&mut app, SectionId::Background);
        app.tick(app.now + Duration::from_millis(16));
        let names: Vec<&str> = app
            .shaders
            .entries
            .iter()
            .filter(|e| !e.builtin)
            .map(|e| e.name.as_str())
            .collect();
        assert_eq!(names, ["broken", "good"]);
        // The warp is one of Ambient's animations, not a background of its own;
        // the broken scene is listed, with its error. (The cards are one control:
        // End goes to the last one.)
        let _ = app.view();
        let last = app.focus_reg.step(&FocusId::Named("bg-shader"), Step::Max);
        assert!(matches!(last, Some(Message::PickBackground(n)) if n == "broken"));
        let animation = app
            .bg
            .resolved
            .decls
            .items
            .iter()
            .find(|d| d.id == "animation")
            .unwrap();
        assert!(
            animation
                .options(&app.bg.resolved.warps)
                .contains(&"good".to_string())
        );

        // The good one: Ambient built with it as the animation.
        let builtin = app.bg.prepared.compiled.id;
        let _ = app.update(Message::SetShaderValue {
            background: true,
            id: "animation".into(),
            value: "good".into(),
        });
        assert_eq!(app.bg.name, shaders::AMBIENT);
        assert!(app.bg.problem.is_none());
        assert_ne!(app.bg.prepared.compiled.id, builtin);

        // The broken one: its error is shown, and Album art stands in (the pick
        // is kept, for when the file is fixed).
        let _ = app.update(Message::PickBackground("broken".into()));
        assert_eq!(app.sections.background.shader, "broken");
        assert_eq!(app.bg.name, shaders::ALBUM_ART);
        assert!(app.bg.problem.is_some());
        let _ = app.view();

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn move_only_ignores_keys_and_clicks() {
        let mut app = test_app();
        app.sections.general.auto_hide = true;
        app.sections.general.show_on_move_only = true;
        let before = app.last_activity;
        press(&mut app, Key::Character("v".into()), false);
        let _ = app.update(Message::Pointer(Pointer::Pressed));
        let _ = app.update(Message::Pointer(Pointer::Activity));
        assert_eq!(
            app.last_activity, before,
            "keys, clicks and scrolls don't count"
        );
        let _ = app.update(Message::Pointer(Pointer::Moved(Point::new(5.0, 5.0))));
        assert!(app.last_activity > before, "moving the mouse does");
    }

    fn press(app: &mut App, key: Key, shift: bool) {
        let modifiers = if shift {
            keyboard::Modifiers::SHIFT
        } else {
            keyboard::Modifiers::empty()
        };
        press_with(app, key, modifiers);
    }

    fn press_with(app: &mut App, key: Key, modifiers: keyboard::Modifiers) {
        let _ = app.view(); // rebuilds the focus registry, like a real frame
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

    fn test_release() -> Release {
        Release {
            version: "0.10.0".into(),
            unstable: false,
            page: "https://example.invalid/releases/v0.10.0".into(),
            archive: Some((
                "https://example.invalid/undavi.tar.gz".into(),
                "https://example.invalid/undavi.tar.gz.sha256".into(),
            )),
        }
    }

    #[test]
    fn the_signal_monitor_shows_under_its_switch() {
        let mut app = test_app();
        let shown = |app: &App| {
            let _ = app.view();
            app.focus_reg.contains(&FocusId::Named("monitor-signal"))
        };
        // There's no shortcut or button for it on the main screen.
        press(&mut app, Key::Character("d".into()), false);
        assert!(!app.monitor_open);
        assert!(!app.focus_reg.contains(&FocusId::Named("open-monitor")));

        // On: its parts are in the Sound section, right after the switch.
        open(&mut app, SectionId::Sound);
        assert!(!shown(&app));
        let _ = app.update(Message::ToggleMonitor);
        assert!(shown(&app));
        let next = app
            .focus_reg
            .cycle(Some(&FocusId::Named("monitor")), false, None);
        assert_eq!(next, Some(FocusId::Named("monitor-signal")));
        let start = app.now;
        app.tick(start + Duration::from_millis(16));
        app.tick(start + Duration::from_millis(32));
        assert_eq!(app.monitor.samples().len(), 2);

        // Hiding a part takes it (and nothing else) away.
        let mut sections = app.monitor_sections.clone();
        sections.spectrum = false;
        let _ = app.update(Message::SetMonitor(sections));
        assert!(!app.monitor_sections.spectrum);
        assert!(shown(&app));

        // Only there: other sections don't have it, but its history goes on.
        send(&mut app, SettingsMessage::Page(SectionId::Colors.into()));
        assert!(!shown(&app));
        app.tick(start + Duration::from_millis(48));
        assert_eq!(app.monitor.samples().len(), 3);

        // Closing the settings drops the history and stops recording; it's still on
        // the next time.
        send(&mut app, SettingsMessage::Done);
        app.tick(start + Duration::from_millis(64));
        assert!(app.monitor.samples().is_empty());
        open(&mut app, SectionId::Sound);
        assert!(shown(&app));
        let _ = app.update(Message::ToggleMonitor);
        assert!(!shown(&app));
        assert!(app.monitor.samples().is_empty());
    }

    #[test]
    fn update_offer_saves_the_checkbox_choices() {
        let found = |install: Install| {
            Message::UpdateChecked(Ok(update::Found {
                install,
                release: Some(test_release()),
            }))
        };

        // "Don't ask again" on its own turns the checks off.
        let mut app = test_app();
        let _ = app.update(found(Install::Prefix("/usr".into())));
        assert!(app.update.shown());
        let _ = app.view(); // builds without panicking
        let _ = app.update(Message::ToggleUpdateDontAsk);
        let _ = app.update(Message::CloseUpdate);
        assert_eq!(app.update, UpdateState::None);
        assert!(!app.updates.check);
        assert!(!app.updates.auto_install);

        // "Update automatically" keeps checking, and installs without asking.
        let mut app = test_app();
        let _ = app.update(found(Install::Prefix("/usr".into())));
        let _ = app.update(Message::ToggleUpdateAuto);
        let _ = app.update(Message::ToggleUpdateDontAsk);
        let _ = app.update(Message::UpdateNow);
        assert!(matches!(
            app.update,
            UpdateState::Installing { shown: true, .. }
        ));
        assert!(app.updates.check);
        assert!(app.updates.auto_install);
        // Hiding it leaves the install running, and its result shows up.
        let _ = app.update(Message::CloseUpdate);
        assert!(matches!(
            app.update,
            UpdateState::Installing { shown: false, .. }
        ));
        let _ = app.update(Message::UpdateInstalled(Ok("0.10.0".into())));
        assert!(app.update.shown());

        // With auto-install on, the next found release installs without the pop-up.
        let _ = app.update(Message::CloseUpdate);
        let _ = app.update(found(Install::Prefix("/usr".into())));
        assert!(matches!(
            app.update,
            UpdateState::Installing { shown: false, .. }
        ));

        // Package installs are only told about it, even with auto-install on.
        let mut app = test_app();
        app.updates.auto_install = true;
        let _ = app.update(found(Install::Package));
        assert!(matches!(app.update, UpdateState::Offer { .. }));
        let _ = app.update(Message::UpdateNow);
        assert!(matches!(app.update, UpdateState::Offer { .. }));
        let _ = app.view();
    }

    /// A made-up cover: a moon over the sea.
    fn test_cover() -> Arc<Art> {
        let img = image::RgbImage::from_fn(256, 256, |x, y| {
            let (dx, dy) = (x as f32 - 150.0, y as f32 - 70.0);
            if dx.hypot(dy) < 34.0 {
                image::Rgb([250, 240, 214])
            } else if y > 150 {
                image::Rgb([24, (70 + y / 6) as u8, 110])
            } else {
                image::Rgb([(50 + y / 2) as u8, 50, (120 + y / 4) as u8])
            }
        });
        Arc::new(Art::from_image(&img))
    }

    /// A song that's playing.
    fn test_track() -> Track {
        Track {
            title: "Somewhere, slowly".into(),
            artist: "Luna Bay".into(),
            album: "Moon / Tide".into(),
            art: None,
            url: None,
            length: Some(Duration::from_secs(200)),
            position: Duration::from_secs(96),
            sampled_at: Instant::now(),
            playing: true,
            repeat: true,
            volume: Some(0.8),
            can_seek: true,
            player_bus: "spotify".into(),
            player_name: "Spotify".into(),
        }
    }

    fn test_app() -> App {
        let (mut app, _) = App::new();
        app.sections = Sections::default();
        app.settle_presets();
        app.updates = UpdateSettings::default();
        app.monitor_sections = MonitorSettings::default();
        // The same controls on any machine.
        app.platform = Platform::full();
        // And the bars, even on one without sound: no "No audio input" in their place.
        app.audio_error = None;
        app.audio_down_since = None;
        // Colors eased from the user's saved settings would keep the app busy.
        app.accent = rgb_f(app.target_accent());
        app.bar_colors.clear();
        app.refresh_shaders();
        app
    }

    #[test]
    fn keyboard_focus_tabs_and_adjusts() {
        let mut app = test_app();

        // Tab starts at the first control (the logo, which opens About), then the
        // source pill and the gear, and wraps.
        for expected in ["about", "refresh", "pill", "quick", "open-settings"] {
            press(&mut app, named(Named::Tab), false);
            assert_eq!(app.focus, Some(FocusId::Named(expected)));
        }
        for _ in 0..5 {
            press(&mut app, named(Named::Tab), true);
        }
        assert_eq!(
            app.focus,
            Some(FocusId::Named("fullscreen")),
            "Shift+Tab wraps to the last control"
        );

        // In the settings window, Tab stays inside it, in reading order: the tree,
        // the section's header, its settings and footer, then the preview.
        open(&mut app, SectionId::Sound);
        press(&mut app, named(Named::Tab), false);
        assert_eq!(app.focus, Some(FocusId::Named("expand-all")));
        let mut seen = Vec::new();
        for _ in 0..200 {
            press(&mut app, named(Named::Tab), false);
            if app.focus == Some(FocusId::Named("expand-all")) {
                break;
            }
            seen.push(app.focus.clone().unwrap());
        }
        let at = |id: FocusId| {
            seen.iter()
                .position(|s| *s == id)
                .unwrap_or_else(|| panic!("{id:?} isn't a Tab stop"))
        };
        let order = [
            at(FocusId::Indexed("section", 0)),
            at(FocusId::Indexed("group", 0)),
            at(FocusId::Indexed("section", 1)),
            at(FocusId::Named("preset-menu")),
            at(FocusId::Named("sensitivity")),
            at(FocusId::Named("delay")),
            at(FocusId::Named("reset")),
            at(FocusId::Named("done")),
            at(FocusId::Named("hide-preview")),
            at(FocusId::Named("close-settings")),
            at(FocusId::Named("window-shape")),
            at(FocusId::Named("only")),
            at(FocusId::Named("peek")),
        ];
        assert!(order.is_sorted(), "{order:?}");
        assert!(!seen.contains(&FocusId::Named("about")), "only the window");

        // Arrows / Page / Home adjust the focused slider.
        app.focus = Some(FocusId::Named("sensitivity"));
        press(&mut app, named(Named::ArrowRight), false);
        assert_eq!(app.sections.sound.sensitivity, 73);
        press(&mut app, named(Named::PageUp), false);
        assert_eq!(app.sections.sound.sensitivity, 83);
        press(&mut app, named(Named::Home), false);
        assert_eq!(app.sections.sound.sensitivity, 0);

        // Range handles move independently and keep their gap.
        app.focus = Some(FocusId::Indexed("freq", 1));
        press(&mut app, named(Named::ArrowDown), false);
        assert_eq!(app.sections.sound.freq_hi, 96);
        app.focus = Some(FocusId::Indexed("freq", 0));
        press(&mut app, named(Named::End), false);
        assert_eq!(app.sections.sound.freq_lo, 96 - FREQ_MIN_GAP_TEST);

        // Segmented controls: arrows change the selection.
        app.focus = Some(FocusId::Named("detail"));
        press(&mut app, named(Named::ArrowRight), false);
        assert_eq!(app.sections.sound.detail, config::Detail::Detailed);

        // Enter/Space press switches and buttons.
        app.focus = Some(FocusId::Named("auto-gain"));
        press(&mut app, named(Named::Enter), false);
        assert!(app.sections.sound.auto_gain);
        press(&mut app, named(Named::Space), false);
        assert!(!app.sections.sound.auto_gain);

        // Picture cards too, and the preview's window shape.
        send(
            &mut app,
            SettingsMessage::Page(SectionId::Visualizer.into()),
        );
        app.focus = Some(FocusId::Named("vis-shader"));
        press(&mut app, named(Named::ArrowRight), false);
        assert_eq!(app.vis.name, shaders::RADIAL);
        app.focus = Some(FocusId::Named("window-shape"));
        press(&mut app, named(Named::ArrowRight), false);
        assert_eq!(app.settings.shape, Shape::Ultrawide);

        // Escape closes the window; keyboard users are back on the gear.
        press(&mut app, named(Named::Escape), false);
        assert!(!app.settings.open);
        assert_eq!(app.focus, Some(FocusId::Named("open-settings")));
    }

    const FREQ_MIN_GAP_TEST: u8 = config::FREQ_MIN_GAP;

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
        use SettingsMessage as M;
        let mut app = test_app();
        open(&mut app, SectionId::Colors);
        let names = |app: &App| -> Vec<String> {
            let s = &app.sections.colors;
            s.yours().iter().map(|n| (*n).to_string()).collect()
        };
        send(&mut app, M::ToggleMenu);
        send(&mut app, M::SaveNewPreset);
        assert_eq!(
            app.settings.editing,
            Some(Editing::Preset(SectionId::Colors, 0)),
            "it opens for naming"
        );
        assert!(app.settings.menu);
        send(&mut app, M::SaveNewPreset);
        assert_eq!(names(&app), ["Preset 1", "Preset 2"]);
        assert_eq!(
            app.settings.editing,
            Some(Editing::Preset(SectionId::Colors, 1))
        );

        send(
            &mut app,
            M::Rename(
                Editing::Preset(SectionId::Colors, 1),
                "  Night drive  ".into(),
            ),
        );
        send(&mut app, M::Edit(None));
        assert_eq!(names(&app)[1], "Night drive");
        assert_eq!(app.settings.editing, None);
        assert_eq!(app.sections.colors.selected_name(), "Night drive");

        send(
            &mut app,
            M::Edit(Some(Editing::Preset(SectionId::Colors, 1))),
        );
        send(&mut app, M::Move(Editing::Preset(SectionId::Colors, 1), -1));
        assert_eq!(names(&app), ["Night drive", "Preset 1"]);
        assert_eq!(
            app.settings.editing,
            Some(Editing::Preset(SectionId::Colors, 0)),
            "the editor follows it"
        );
        assert_eq!(app.sections.colors.selected_name(), "Night drive");
        send(&mut app, M::Move(Editing::Preset(SectionId::Colors, 0), -1)); // already first: no-op
        assert_eq!(names(&app)[0], "Night drive");

        // Emptied names fall back to a default that isn't already taken, also when
        // the menu closes.
        send(
            &mut app,
            M::Rename(Editing::Preset(SectionId::Colors, 0), "   ".into()),
        );
        send(&mut app, M::Page(SectionId::Sound.into()));
        assert_eq!(names(&app), ["Preset 2", "Preset 1"]);
        assert!(!app.settings.menu, "another section closes the menu");
        assert_eq!(app.settings.editing, None);

        // Delete takes two presses.
        send(&mut app, M::Page(SectionId::Colors.into()));
        send(&mut app, M::ToggleMenu);
        send(
            &mut app,
            M::Edit(Some(Editing::Preset(SectionId::Colors, 0))),
        );
        send(&mut app, M::Delete(Editing::Preset(SectionId::Colors, 0)));
        assert_eq!(names(&app).len(), 2);
        send(&mut app, M::Delete(Editing::Preset(SectionId::Colors, 0)));
        assert_eq!(names(&app), ["Preset 1"]);
        assert_eq!(app.settings.editing, None);

        // Esc: the rename, then the menu, then the window.
        send(
            &mut app,
            M::Edit(Some(Editing::Preset(SectionId::Colors, 0))),
        );
        press(&mut app, named(Named::Escape), false);
        assert!(app.settings.menu && app.settings.editing.is_none());
        press(&mut app, named(Named::Escape), false);
        assert!(app.settings.open && !app.settings.menu);
        press(&mut app, named(Named::Escape), false);
        assert!(!app.settings.open);
    }

    #[test]
    fn the_preset_menu_saves_and_discards() {
        use SettingsMessage as M;
        let mut app = test_app();
        open(&mut app, SectionId::Sound);
        assert!(!app.section_edited(SectionId::Sound));
        send(&mut app, M::ToggleMenu);
        let _ = app.view();
        let has = |app: &App, id| app.focus_reg.contains(&FocusId::Named(id));
        assert!(has(&app, "save-new-preset"));
        assert!(!has(&app, "save-preset") && !has(&app, "discard-preset"));
        // Tab stays in the menu.
        app.focus = None;
        for _ in 0..20 {
            press(&mut app, named(Named::Tab), false);
            let id = app.focus.clone().unwrap();
            assert_eq!(app.focus_reg.position(&id).unwrap().2, Scope::Menu);
        }

        // An edited built-in: save as new or discard.
        app.sections.sound.sensitivity = 10;
        let _ = app.view();
        assert!(app.section_edited(SectionId::Sound));
        assert!(has(&app, "save-new-preset") && has(&app, "discard-preset"));
        assert!(!has(&app, "save-preset"), "built-ins can't be overwritten");
        send(&mut app, M::SaveNewPreset);
        send(&mut app, M::Edit(None));
        assert!(!app.section_edited(SectionId::Sound));

        // An edited preset of the user's: save the changes, or discard them.
        app.sections.sound.sensitivity = 20;
        let _ = app.view();
        assert!(has(&app, "save-preset") && has(&app, "discard-preset"));
        send(&mut app, M::SavePreset);
        assert!(!app.settings.menu, "picking something closes the menu");
        assert_eq!(app.sections.sound.presets[0].settings.sensitivity, 20);
        app.sections.sound.sensitivity = 30;
        send(&mut app, M::DiscardChanges);
        assert_eq!(app.sections.sound.sensitivity, 20);

        // Reset only touches the section shown.
        app.sections.background.dim_pct = 80;
        app.sections.sound.sensitivity = 1;
        send(&mut app, M::Reset);
        assert_eq!(app.sections.sound.settings, TuningSettings::default());
        assert_eq!(app.sections.sound.selected_name(), "Balanced");
        assert_eq!(app.sections.background.dim_pct, 80);
        assert_eq!(app.sections.sound.presets.len(), 1, "presets are kept");
    }

    #[test]
    fn done_keeps_changes_and_cancel_puts_everything_back() {
        use SettingsMessage as M;
        let mut app = test_app();
        open(&mut app, SectionId::Sound);
        app.sections.sound.sensitivity = 10;
        send(&mut app, M::Done);
        assert!(!app.settings.open);
        assert_eq!(app.sections.sound.sensitivity, 10);

        // Every section goes back, not only the one shown.
        let before = app.sections.clone();
        open(&mut app, SectionId::Colors);
        let last = app.palette_choices().len() - 1;
        let _ = app.update(Message::PickSwatch(last));
        let _ = app.update(Message::CustomHex(0, "#123456".into()));
        send(&mut app, M::Page(SectionId::Visualizer.into()));
        send(
            &mut app,
            M::LoadPreset(SectionId::Visualizer, PresetId::BuiltIn("Radial".into())),
        );
        send(&mut app, M::SaveNewPreset);
        assert_eq!(app.vis.name, shaders::RADIAL);
        send(&mut app, M::Cancel);
        assert!(!app.settings.open);
        assert_eq!(app.sections, before);
        assert_eq!(app.vis.name, shaders::BARS, "the screen too");
        assert_eq!(app.custom_hex, custom_hex_strings(&Palette::Undavi));

        // Opening it again starts from there.
        open(&mut app, SectionId::Sound);
        app.sections.sound.sensitivity = 99;
        send(&mut app, M::Cancel);
        assert_eq!(app.sections.sound.sensitivity, 10);
    }

    #[test]
    fn update_switches_are_in_the_general_section() {
        use SettingsMessage as M;
        let mut app = test_app();
        open(&mut app, SectionId::General);
        let has = |app: &App, id| {
            let _ = app.view();
            app.focus_reg.contains(&FocusId::Named(id))
        };
        assert!(has(&app, "check-updates") && has(&app, "auto-update"));
        app.focus = Some(FocusId::Named("auto-update"));
        press(&mut app, named(Named::Enter), false);
        assert!(app.updates.auto_install);
        // Unstable versions are only for those who ask, and not for packages.
        assert!(has(&app, "unstable-updates") && !app.updates.unstable);
        app.focus = Some(FocusId::Named("unstable-updates"));
        press(&mut app, named(Named::Enter), false);
        assert!(app.updates.unstable);
        app.install = Some(Install::Package);
        assert!(!has(&app, "unstable-updates"));
        app.install = None;
        app.focus = Some(FocusId::Named("check-updates"));
        press(&mut app, named(Named::Enter), false);
        assert!(!app.updates.check);
        assert!(!has(&app, "auto-update"), "nothing to install unchecked");
        assert!(!has(&app, "unstable-updates"));

        // They aren't part of the section's presets…
        assert!(!app.section_edited(SectionId::General));
        send(
            &mut app,
            M::LoadPreset(
                SectionId::General,
                PresetId::BuiltIn("Battery saver".into()),
            ),
        );
        assert!(!app.updates.check);
        // …but Reset and Cancel cover what the window shows.
        send(&mut app, M::Reset);
        assert_eq!(app.updates, UpdateSettings::default());
        app.updates.check = false;
        send(&mut app, M::Cancel);
        assert_eq!(app.updates, UpdateSettings::default());

        // Another section's Reset leaves them.
        app.updates.auto_install = true;
        open(&mut app, SectionId::Sound);
        send(&mut app, M::Reset);
        assert!(app.updates.auto_install);

        // A choice made in the update pop-up meanwhile isn't the window's to undo.
        app.update = UpdateState::Offer {
            release: test_release(),
            auto: false,
            dont_ask: true,
        };
        let _ = app.update(Message::CloseUpdate);
        app.updates.auto_install = false;
        send(&mut app, M::Cancel);
        assert!(!app.updates.check, "kept");
        assert!(app.updates.auto_install, "put back");

        // A package manager's copy is only told where updates come from.
        app.updates = UpdateSettings::default();
        app.install = Some(Install::Package);
        open(&mut app, SectionId::General);
        assert!(has(&app, "check-updates") && !has(&app, "auto-update"));
    }

    #[test]
    fn the_tree_selects_expands_and_searches() {
        use SettingsMessage as M;
        let mut app = test_app();
        send(&mut app, M::Toggle);
        assert!(app.settings.shown());
        assert_eq!(app.settings.section(), Some(SectionId::Sound));
        let group = |id: SectionId, i: usize| FocusId::Indexed("group", id as usize * 64 + i);
        let _ = app.view();
        assert!(app.focus_reg.contains(&group(SectionId::Sound, 0)));
        assert!(!app.focus_reg.contains(&group(SectionId::Colors, 0)));

        // Another section: selected and expanded, and the first one stays expanded.
        send(&mut app, M::Page(SectionId::Colors.into()));
        let _ = app.view();
        assert_eq!(app.settings.section(), Some(SectionId::Colors));
        assert!(app.focus_reg.contains(&group(SectionId::Colors, 0)));
        assert!(app.focus_reg.contains(&group(SectionId::Sound, 0)));
        // The selected one again: collapsed, and expanded.
        send(&mut app, M::Page(SectionId::Colors.into()));
        assert!(!app.settings.expanded(SectionId::Colors.into()));
        assert_eq!(app.settings.section(), Some(SectionId::Colors));
        send(&mut app, M::Page(SectionId::Colors.into()));
        assert!(app.settings.expanded(SectionId::Colors.into()));
        // A group of another section selects that section.
        send(&mut app, M::Group(SectionId::Background.into(), 1));
        assert_eq!(app.settings.section(), Some(SectionId::Background));
        assert_eq!(app.settings.group, 1);

        // Searching finds groups by the labels in them, and expands them.
        send(&mut app, M::Search("VIGN".into()));
        let _ = app.view();
        let effects = app.outline(SectionId::Background.into()).len() - 1;
        assert!(
            app.focus_reg
                .contains(&group(SectionId::Background, effects))
        );
        assert!(!app.focus_reg.contains(&group(SectionId::Background, 0)));
        assert!(
            !app.focus_reg
                .contains(&FocusId::Indexed("section", SectionId::Sound as usize))
        );
        // A section's own name finds all of it.
        send(&mut app, M::Search("sound".into()));
        let _ = app.view();
        assert!(app.focus_reg.contains(&group(SectionId::Sound, 8)));
        send(&mut app, M::Search("nothing like it".into()));
        let _ = app.view();

        // The shortcuts open it at their section, and close it there.
        send(&mut app, M::Done);
        press(&mut app, Key::Character("b".into()), false);
        assert_eq!(app.settings.section(), Some(SectionId::Background));
        press(&mut app, Key::Character("v".into()), false);
        assert_eq!(app.settings.section(), Some(SectionId::Visualizer));
        assert!(app.settings.shown());
        press(&mut app, Key::Character("v".into()), false);
        assert!(!app.settings.open);
        press(&mut app, Key::Character("s".into()), false);
        assert_eq!(
            app.settings.section(),
            Some(SectionId::Visualizer),
            "where it was"
        );
    }

    #[test]
    fn the_outline_matches_the_content() {
        let mut app = test_app();
        app.sections.sound.gravity = true;
        app.sections.sound.auto_gain = true;
        vis_set(&mut app, "glow", true);
        for mode in [NowPlayingMode::Header, NowPlayingMode::Off] {
            app.sections.now_playing.mode = mode;
            app.sections.lyrics.show = mode == NowPlayingMode::Header;
            app.refresh_shaders();
            for page in app.pages() {
                let outline = app.outline(page);
                let titles: Vec<String> = outline.iter().map(|g| g.title.clone()).collect();
                app.row_focus.borrow_mut().clear();
                assert_eq!(app.group_titles(page), titles, "{}", page.name());
                // Everything a search finds is a row with a control.
                let rows = app.row_focus.borrow();
                for l in outline.iter().flat_map(|g| &g.labels) {
                    assert!(
                        rows.iter().any(|r| r.label == l.row),
                        "{}: {} ({})",
                        page.name(),
                        l.text,
                        l.row
                    );
                }
            }
        }
    }

    #[test]
    fn the_arrow_expands_and_collapses_without_going_there() {
        use SettingsMessage as M;
        let mut app = test_app();
        open(&mut app, SectionId::Sound);
        let colors = Page::Section(SectionId::Colors);
        send(&mut app, M::Expand(colors, true));
        assert!(app.settings.expanded(colors));
        assert_eq!(app.settings.section(), Some(SectionId::Sound));
        send(&mut app, M::Expand(colors, false));
        assert!(!app.settings.expanded(colors));
        // The arrow keys do it too.
        app.focus = Some(FocusId::Indexed("section", colors.index()));
        let _ = app.view();
        press(&mut app, named(Named::ArrowRight), false);
        assert!(app.settings.expanded(colors));
        press(&mut app, named(Named::ArrowLeft), false);
        assert!(!app.settings.expanded(colors));
        assert_eq!(app.settings.section(), Some(SectionId::Sound));
    }

    #[test]
    fn groups_fold_away() {
        use SettingsMessage as M;
        let mut app = test_app();
        open(&mut app, SectionId::Sound);
        let sound = Page::Section(SectionId::Sound);
        let has = |app: &App, id| {
            let _ = app.view();
            app.focus_reg.contains(&FocusId::Named(id))
        };
        assert!(has(&app, "sensitivity") && has(&app, "contrast"));
        send(&mut app, M::Fold(sound, "Response".into()));
        assert!(
            !has(&app, "sensitivity"),
            "not a Tab stop while folded away"
        );
        assert!(has(&app, "contrast"), "the others are");
        assert!(
            app.focus_reg.contains(&FocusId::Indexed("fold", 1)),
            "its heading still is"
        );
        // Going to it in the tree unfolds it.
        send(&mut app, M::Group(sound, 1));
        assert!(has(&app, "sensitivity"));
        // The heading does both, by keyboard too.
        app.focus = Some(FocusId::Indexed("fold", 1));
        press(&mut app, named(Named::Enter), false);
        assert!(!has(&app, "sensitivity"));
        press(&mut app, named(Named::Enter), false);
        assert!(has(&app, "sensitivity"));
    }

    #[test]
    fn a_changed_setting_is_undone_and_redone() {
        use SettingsMessage as M;
        let mut app = test_app();
        app.sections.sound.sensitivity = 40;
        open(&mut app, SectionId::Sound);
        let key = SettingKey::Fields(SectionId::Sound, &["sensitivity"]);
        // The button's message, as the view has it.
        let button = |app: &App| {
            let _ = app.view();
            app.focus_reg
                .activate(&FocusId::Indexed("undo", 0))
                .map(|m| format!("{m:?}"))
        };
        assert_eq!(button(&app), None, "nothing to undo");
        let mut t = app.sections.sound.settings.clone();
        t.sensitivity = 70;
        let _ = app.update(Message::SetTuning(t.clone()));
        assert!(button(&app).is_some_and(|m| m.contains("Undo")));
        send(&mut app, M::Undo(key.clone()));
        assert_eq!(app.sections.sound.sensitivity, 40);
        assert!(button(&app).is_some_and(|m| m.contains("Redo")));
        send(&mut app, M::Redo(key.clone()));
        assert_eq!(app.sections.sound.sensitivity, 70);
        assert!(button(&app).is_some_and(|m| m.contains("Undo")));
        // Changed again after it was undone: there's nothing to redo any more.
        send(&mut app, M::Undo(key.clone()));
        t.sensitivity = 55;
        let _ = app.update(Message::SetTuning(t.clone()));
        t.sensitivity = 40;
        let _ = app.update(Message::SetTuning(t));
        assert_eq!(button(&app), None);

        // A preset is a new start.
        send(
            &mut app,
            M::LoadPreset(SectionId::Sound, PresetId::BuiltIn("Punchy".into())),
        );
        assert_eq!(app.sections.sound.contrast, 68);
        assert_eq!(
            app.setting_change(&SettingKey::Fields(SectionId::Sound, &["contrast"])),
            None
        );
        // Reset isn't: everything it changed can be brought back.
        send(&mut app, M::Reset);
        let contrast = SettingKey::Fields(SectionId::Sound, &["contrast"]);
        send(&mut app, M::Undo(contrast));
        assert_eq!(app.sections.sound.contrast, 68);
        // Two settings on one row go together.
        let mut t = app.sections.sound.settings.clone();
        (t.floor_db, t.ceiling_db) = (-70, -10);
        let _ = app.update(Message::SetTuning(t));
        let range = SettingKey::Fields(SectionId::Sound, &["floor_db", "ceiling_db"]);
        send(&mut app, M::Undo(range));
        let d = TuningSettings::default();
        assert_eq!(
            (app.sections.sound.floor_db, app.sections.sound.ceiling_db),
            (d.floor_db, d.ceiling_db)
        );
        // Opened again, there's nothing from before.
        send(&mut app, M::Done);
        open(&mut app, SectionId::Sound);
        assert_eq!(app.setting_change(&key), None);
    }

    #[test]
    fn every_kind_of_setting_can_be_undone() {
        use SettingsMessage as M;
        let mut app = test_app();
        open(&mut app, SectionId::Visualizer);
        // What a shader declares, back to a default that was never saved.
        let _ = app.update(Message::SetShaderValue {
            background: false,
            id: "bar_count".into(),
            value: 20.into(),
        });
        assert_eq!(app.vis_engine.bar_count, 20);
        let count = SettingKey::Shader {
            background: false,
            shader: shaders::BARS.into(),
            id: "bar_count".into(),
        };
        assert!(app.setting_change(&count).is_some());
        send(&mut app, M::Undo(count.clone()));
        assert_eq!(
            app.vis_engine.bar_count,
            VisEngine::default().bar_count.min(43)
        );
        assert!(!app.section_edited(SectionId::Visualizer));
        send(&mut app, M::Redo(count));
        assert_eq!(app.vis_engine.bar_count, 20);
        // Which shader.
        let _ = app.update(Message::PickVisualizer(shaders::WAVE.into()));
        send(
            &mut app,
            M::Undo(SettingKey::Fields(SectionId::Visualizer, &["shader"])),
        );
        assert_eq!(app.vis.name, shaders::BARS);
        // A palette, with its text fields.
        let last = app.palette_choices().len() - 1;
        let _ = app.update(Message::PickSwatch(last));
        let _ = app.update(Message::CustomHex(0, "#123456".into()));
        send(
            &mut app,
            M::Undo(SettingKey::Fields(SectionId::Colors, &["palette"])),
        );
        assert_eq!(app.sections.colors.palette, Palette::Undavi);
        assert_eq!(app.custom_hex, custom_hex_strings(&Palette::Undavi));
        // One that was up to the style.
        let mut p = app.sections.playback.settings.clone();
        p.curve_height = Some(12);
        let _ = app.update(Message::SetPlayback(p));
        send(
            &mut app,
            M::Undo(SettingKey::Fields(SectionId::Playback, &["curve_height"])),
        );
        assert_eq!(app.sections.playback.curve_height, None);
        // An update switch, which isn't part of any section.
        let u = UpdateSettings {
            check: false,
            ..app.updates.clone()
        };
        let _ = app.update(Message::SetUpdates(u));
        send(&mut app, M::Undo(SettingKey::Updates("check")));
        assert!(app.updates.check);
        send(&mut app, M::Redo(SettingKey::Updates("check")));
        assert!(!app.updates.check);
    }

    #[test]
    fn enter_goes_to_the_setting_searched_for() {
        use SettingsMessage as M;
        let mut app = test_app();
        app.sections.background.settings.grain_pct = 10;
        open(&mut app, SectionId::Sound);
        let effects = app.outline(SectionId::Background.into()).len() - 1;
        send(
            &mut app,
            M::Fold(SectionId::Background.into(), "Effects".into()),
        );
        send(&mut app, M::Search("vign".into()));
        send(&mut app, M::SearchGo);
        assert_eq!(app.settings.section(), Some(SectionId::Background));
        assert_eq!(app.settings.group, effects);
        assert!(
            !app.settings.folded(SectionId::Background.into(), "Effects"),
            "it's unfolded to show it"
        );
        assert_eq!(
            app.settings.found.as_ref().map(|(l, _)| l.as_str()),
            Some("Vignette")
        );
        // Once it's laid out, its control has the keyboard.
        let _ = app.view();
        let goal = Goal::Setting(effects, "Vignette".into());
        send(&mut app, M::Located(goal, Default::default()));
        assert_eq!(app.focus, Some(FocusId::Named("vignette")));
        press(&mut app, named(Named::ArrowRight), false);
        assert_eq!(app.sections.background.vignette_pct, 5);

        // Another name for a setting, and the best match rather than the first.
        for (query, page, label) in [
            ("auto-cal", SectionId::Sound, "Set the sensitivity for me"),
            ("spotlight", SectionId::NowPlaying, "Track info"),
            ("curve l", SectionId::Playback, "Curve length"),
            ("frame", SectionId::General, "Frame rate"),
        ] {
            send(&mut app, M::Search(query.into()));
            send(&mut app, M::SearchGo);
            assert_eq!(app.settings.section(), Some(page), "{query}");
            let found = app.settings.found.as_ref().map(|(l, _)| l.as_str());
            assert_eq!(found, Some(label), "{query}");
        }
        // A heading, and a preset by its name.
        send(&mut app, M::Search("dynamics".into()));
        send(&mut app, M::SearchGo);
        assert_eq!(app.settings.section(), Some(SectionId::Sound));
        assert_eq!(app.settings.group, 2);
        send(&mut app, M::Search("ember".into()));
        send(&mut app, M::SearchGo);
        assert_eq!(app.settings.page, Page::Presets);
        // Nothing to go to.
        send(&mut app, M::Search("nothing like it".into()));
        send(&mut app, M::SearchGo);
        send(&mut app, M::Search(String::new()));
        send(&mut app, M::SearchGo);
        assert_eq!(app.settings.page, Page::Presets);
    }

    #[test]
    fn the_presets_page_edits_every_sections_presets() {
        use SettingsMessage as M;
        let mut app = test_app();
        open(&mut app, SectionId::Colors);
        send(&mut app, M::SaveNewPreset);
        send(&mut app, M::Edit(None));
        send(&mut app, M::Page(SectionId::Lyrics.into()));
        send(&mut app, M::SaveNewPreset);
        send(&mut app, M::Edit(None));
        send(&mut app, M::Page(Page::Presets));
        let _ = app.view();
        // Both, with a pencil, and the ones that come with Undavi without.
        for id in [SectionId::Colors, SectionId::Lyrics] {
            let i = settings::preset_index(id, 0);
            assert!(app.focus_reg.contains(&FocusId::Indexed("preset", i)));
            assert!(app.focus_reg.contains(&FocusId::Indexed("preset-edit", i)));
            assert!(app.focus_reg.contains(&FocusId::Indexed("built-in", i)));
        }
        // There's no menu, and nothing to reset.
        let has = |id| app.focus_reg.contains(&FocusId::Named(id));
        assert!(!has("preset-menu") && !has("reset") && has("done"));
        send(&mut app, M::ToggleMenu);
        assert!(!app.settings.menu);

        // Loading one changes its section, not the page.
        send(
            &mut app,
            M::LoadPreset(SectionId::Colors, PresetId::BuiltIn("Ember".into())),
        );
        assert_eq!(app.sections.colors.palette, Palette::Ember);
        assert_eq!(app.settings.page, Page::Presets);
        // Renamed and deleted from here.
        let what = Editing::Preset(SectionId::Lyrics, 0);
        send(&mut app, M::Edit(Some(what)));
        send(&mut app, M::Rename(what, "Big words".into()));
        send(&mut app, M::Edit(None));
        assert_eq!(app.sections.lyrics.yours(), ["Big words"]);
        send(&mut app, M::Edit(Some(what)));
        send(&mut app, M::Delete(what));
        assert_eq!(app.sections.lyrics.yours().len(), 1, "asks first");
        send(&mut app, M::Delete(what));
        assert!(app.sections.lyrics.yours().is_empty());
        assert_eq!(app.sections.colors.yours().len(), 1);
    }

    #[test]
    fn preset_groups_load_several_presets_at_once() {
        use SettingsMessage as M;
        let mut app = test_app();
        open(&mut app, SectionId::Colors);
        send(
            &mut app,
            M::LoadPreset(SectionId::Colors, PresetId::BuiltIn("Aurora".into())),
        );
        for _ in 0..2 {
            send(&mut app, M::SaveNewPreset);
        }
        send(&mut app, M::Page(Page::Presets));
        send(&mut app, M::NewGroup);
        assert_eq!(app.settings.editing, Some(Editing::Group(0)));
        let group = |app: &App| app.sections.groups[0].clone();
        assert_eq!(group(&app).name, "Group 1");
        assert_eq!(group(&app).presets.len(), app.section_list().len());
        assert_eq!(
            group(&app).presets[&SectionId::Colors],
            PresetId::Yours(1),
            "it starts with what's in use"
        );
        // Choose another preset for a section, and leave one as it is.
        send(&mut app, M::Choose(SectionId::Visualizer));
        assert_eq!(app.settings.choosing, Some(SectionId::Visualizer));
        let _ = app.view();
        assert!(app.focus_reg.contains(&FocusId::Indexed("group-option", 0)));
        let wave = PresetId::BuiltIn("Wave".into());
        send(
            &mut app,
            M::SetInGroup(0, SectionId::Visualizer, Some(wave)),
        );
        assert_eq!(app.settings.choosing, None);
        send(&mut app, M::SetInGroup(0, SectionId::Sound, None));
        send(
            &mut app,
            M::Rename(Editing::Group(0), "  Night drive ".into()),
        );
        send(&mut app, M::Edit(None));
        assert_eq!(group(&app).name, "Night drive");

        // It loads them all, and only them.
        let fill = |app: &App| {
            app.sections
                .group_in_use(0, &|shader, saved| app.fill_values(shader, saved))
        };
        assert!(!fill(&app), "the visualizer isn't Wave yet");
        app.sections.sound.sensitivity = 11;
        app.sections.colors.palette = Palette::Silver;
        send(&mut app, M::LoadGroup(0));
        assert_eq!(app.vis.name, shaders::WAVE);
        assert_eq!(app.sections.colors.palette, Palette::Aurora);
        assert_eq!(app.sections.sound.sensitivity, 11, "left as it is");
        assert!(fill(&app));
        app.sections.colors.palette = Palette::Silver;
        assert!(!fill(&app), "changed since");

        // It follows its presets as they move, and lets go of deleted ones.
        let colors = |i| Editing::Preset(SectionId::Colors, i);
        send(&mut app, M::Move(colors(1), -1));
        assert_eq!(group(&app).presets[&SectionId::Colors], PresetId::Yours(0));
        for _ in 0..2 {
            send(&mut app, M::Delete(colors(0)));
        }
        assert!(!group(&app).presets.contains_key(&SectionId::Colors));

        // Groups move and delete like presets, and Cancel puts them back.
        send(&mut app, M::NewGroup);
        send(&mut app, M::Move(Editing::Group(1), -1));
        assert_eq!(app.sections.groups[1].name, "Night drive");
        for _ in 0..2 {
            send(&mut app, M::Delete(Editing::Group(0)));
        }
        assert_eq!(app.sections.groups.len(), 1);
        send(&mut app, M::Cancel);
        assert!(app.sections.groups.is_empty());
    }

    #[test]
    fn shortcuts_can_be_changed() {
        use SettingsMessage as M;
        let mut app = test_app();
        app.track = Some(test_track());
        let key = |s: &str| Key::Character(s.into());
        let saved = |app: &App, a| app.sections.controls.shortcuts.saved(a);
        open(&mut app, SectionId::Controls);

        // The next key is the shortcut, rather than doing what it's for.
        send(&mut app, M::Capture(Action::Quick, 0));
        press(&mut app, key("b"), false);
        assert_eq!(app.settings.capturing, None);
        assert_eq!(app.settings.section(), Some(SectionId::Controls));
        assert_eq!(saved(&app, Action::Quick), ["b"]);
        assert!(saved(&app, Action::Background).is_empty(), "it was taken");
        // With something held down, and keys that can't be one.
        send(&mut app, M::Capture(Action::Mute, 0));
        for held in [Named::Control, Named::Shift] {
            press_with(&mut app, named(held), keyboard::Modifiers::CTRL);
        }
        press(&mut app, named(Named::Tab), false);
        press(&mut app, named(Named::Enter), false);
        assert_eq!(
            app.settings.capturing,
            Some((Action::Mute, 0)),
            "still waiting"
        );
        press_with(
            &mut app,
            key("m"),
            keyboard::Modifiers::CTRL | keyboard::Modifiers::SHIFT,
        );
        assert_eq!(saved(&app, Action::Mute), ["ctrl+shift+m"]);
        // A second one, taken away again.
        send(&mut app, M::Capture(Action::Mute, 1));
        press(&mut app, named(Named::F5), false);
        assert_eq!(saved(&app, Action::Mute), ["ctrl+shift+m", "F5"]);
        send(&mut app, M::Capture(Action::Mute, 0));
        press(&mut app, named(Named::Backspace), false);
        assert_eq!(saved(&app, Action::Mute), ["F5"]);
        // Esc leaves it, and the window open.
        send(&mut app, M::Capture(Action::Mute, 0));
        press(&mut app, named(Named::Escape), false);
        assert!(app.settings.shown() && app.settings.capturing.is_none());
        // So does a click somewhere.
        send(&mut app, M::Capture(Action::Mute, 0));
        let _ = app.update(Message::Pointer(Pointer::Pressed));
        assert_eq!(app.settings.capturing, None);

        // Undo is for one of them.
        send(&mut app, M::Undo(SettingKey::Shortcut(Action::Background)));
        assert_eq!(saved(&app, Action::Background), ["b"]);
        assert!(
            saved(&app, Action::Quick).is_empty(),
            "a key is for one thing"
        );
        assert_eq!(saved(&app, Action::Mute), ["F5"], "the others stay");
        // A preset brings them all back.
        send(
            &mut app,
            M::LoadPreset(SectionId::Controls, PresetId::BuiltIn("Standard".into())),
        );
        assert_eq!(app.sections.controls.settings, ControlSettings::default());
    }

    #[test]
    fn shortcuts_do_what_they_are_for() {
        let mut app = test_app();
        let key = |s: &str| Key::Character(s.into());
        press(&mut app, key("q"), false);
        assert!(app.quick_open);
        press(&mut app, key("q"), false);
        assert!(!app.quick_open);
        press(&mut app, key("r"), false);
        assert!(app.refreshed.is_some());
        press(&mut app, key("s"), true);
        assert!(!app.settings.open, "only the exact keys");
        press(&mut app, key("S"), false);
        assert!(app.settings.open, "with Caps Lock too");
        // What's behind the settings stays there.
        press(&mut app, key("q"), false);
        assert!(!app.quick_open && app.settings.open);
        press(&mut app, key("s"), false);
        // Changed ones.
        let parse = |s| Combo::parse(s).unwrap();
        let shortcuts = &mut app.sections.controls.shortcuts;
        shortcuts.give(Action::About, vec![parse("ctrl+a")]);
        shortcuts.give(Action::Settings, vec![parse("alt+F2")]);
        press(&mut app, key("s"), false);
        assert!(!app.settings.open);
        press_with(&mut app, key("a"), keyboard::Modifiers::CTRL);
        assert!(app.about_open);
        press_with(&mut app, named(Named::F2), keyboard::Modifiers::ALT);
        assert!(app.settings.open);
        // The compositor's own keys are left alone.
        press_with(&mut app, key("a"), keyboard::Modifiers::LOGO);
        // A focused control's keys come first.
        app.focus = Some(FocusId::Named("done"));
        press(&mut app, named(Named::Space), false);
        assert!(!app.settings.open);
    }

    #[test]
    fn the_mouse_does_what_the_controls_say() {
        use crate::settings::{BarsScroll, DoubleClick};
        let mut app = test_app();
        app.platform.media = true;
        assert_eq!(app.seek_step(), Duration::from_secs(5));
        app.sections.controls.seek_step_s = 12;
        assert_eq!(app.seek_step(), Duration::from_secs(12));
        for (scroll, expected) in [
            (BarsScroll::Nothing, 0),
            (BarsScroll::Volume, 1),
            (BarsScroll::Seek, 1),
        ] {
            app.sections.controls.scroll = scroll;
            app.track = Some(test_track());
            app.window_size = Size::new(1440.0, 900.0);
            let wheel = [
                (Mouse::Wheel(700.0, 400.0, 60.0), "BarsScroll(60.0)"),
                (Mouse::Wheel(700.0, 60.0, 60.0), ""),
            ];
            let wheel = &wheel[usize::from(expected == 0)..];
            mouse(&app, wheel);
        }
        // (Without a player none of this does anything, but it mustn't fail either.)
        for double in [
            DoubleClick::Fullscreen,
            DoubleClick::PlayPause,
            DoubleClick::Nothing,
        ] {
            app.sections.controls.double_click = double;
            app.track = None;
            let _ = app.update(Message::BarsScroll(1.0));
            let _ = app.update(Message::VolumeScroll(-1.0));
        }
    }

    #[test]
    fn starred_settings_are_in_the_quick_settings() {
        use SettingsMessage as M;
        let mut app = test_app();
        let star = |app: &mut App, id: &str| {
            let _ = app.update(Message::ToggleFavorite(id.into()));
        };
        let has = |app: &App, id: FocusId| {
            let _ = app.view();
            app.focus_reg.count(&id)
        };
        let _ = app.update(Message::ToggleQuick);
        assert!(app.quick_open);
        assert_eq!(has(&app, FocusId::Named("all-settings")), 1);
        assert_eq!(has(&app, FocusId::Named("sensitivity")), 0);
        for id in [
            "sound.sensitivity",
            "sound.floor_db",
            "lyrics.show",
            "colors.accent",
            "visualizer.shader",
            "visualizer:glow",
            "background:pulse",
            "playback.style",
            "updates.check",
            "sound.calibrate",
        ] {
            star(&mut app, id);
        }
        for id in [
            FocusId::Named("sensitivity"),
            FocusId::Indexed("level", 1),
            FocusId::Named("lyrics"),
            FocusId::Named("accent"),
            FocusId::Named("vis-shader"),
            FocusId::Named("playback-style"),
            FocusId::Named("check-updates"),
            FocusId::Named("calibrate"),
        ] {
            assert_eq!(has(&app, id.clone()), 1, "{id:?}");
        }
        // Tab goes through them in the order they're in: choices, then sliders, …
        let _ = app.view();
        let place = |id| app.focus_reg.position(&id).unwrap().0;
        assert!(place(FocusId::Named("vis-shader")) < place(FocusId::Named("sensitivity")));
        assert!(place(FocusId::Named("sensitivity")) < place(FocusId::Named("lyrics")));
        assert!(place(FocusId::Named("lyrics")) < place(FocusId::Named("accent")));
        assert!(place(FocusId::Named("accent")) < place(FocusId::Named("all-settings")));
        // Nothing else of their sections is.
        for id in ["spread", "vis-shader-copy", "monitor", "fold", "on-top"] {
            assert_eq!(has(&app, FocusId::Named(id)), 0, "{id}");
        }
        // They work, and Tab stays in the panel.
        app.focus = Some(FocusId::Named("sensitivity"));
        press(&mut app, named(Named::Home), false);
        assert_eq!(app.sections.sound.sensitivity, 0);
        for _ in 0..20 {
            press(&mut app, named(Named::Tab), false);
            let id = app.focus.clone().unwrap();
            assert_eq!(app.focus_reg.position(&id).unwrap().2, Scope::Quick);
        }
        // A shader's setting is there whichever shader is in use, if it has it.
        let glow = |app: &App| {
            let r = &app.vis.resolved.decls.items;
            let i = r.iter().position(|d| d.id == "glow").unwrap();
            has(app, FocusId::Indexed("vis-setting", i))
        };
        assert_eq!(glow(&app), 1);
        let _ = app.update(Message::PickVisualizer(shaders::WAVE.into()));
        assert_eq!(glow(&app), 1);
        // Unstarred from the keyboard, in the panel and in the settings.
        app.focus = Some(FocusId::Named("sensitivity"));
        press_with(
            &mut app,
            Key::Character("d".into()),
            keyboard::Modifiers::CTRL,
        );
        assert_eq!(has(&app, FocusId::Named("sensitivity")), 0);
        // Esc closes it, and so does opening the settings from it.
        press(&mut app, named(Named::Escape), false);
        assert!(!app.quick_open);
        let _ = app.update(Message::ToggleQuick);
        send(&mut app, M::Toggle);
        assert!(app.settings.shown() && !app.quick_open);
        app.focus = Some(FocusId::Named("spread"));
        press_with(
            &mut app,
            Key::Character("d".into()),
            keyboard::Modifiers::CTRL,
        );
        assert_eq!(
            app.favorites.last().map(String::as_str),
            Some("sound.spread")
        );
        // Cancel is about the settings, not the stars.
        send(&mut app, M::Cancel);
        assert!(app.favorites.contains(&"sound.spread".to_string()));
        // Ones that don't exist (any more) are left out.
        star(&mut app, "sound.gone");
        star(&mut app, "nonsense");
        let _ = app.update(Message::ToggleQuick);
        let _ = app.view();
    }

    #[test]
    fn the_whole_tree_expands_and_collapses() {
        use SettingsMessage as M;
        let mut app = test_app();
        open(&mut app, SectionId::Sound);
        let button = |app: &App| {
            let _ = app.view();
            app.focus_reg
                .activate(&FocusId::Named("expand-all"))
                .map(|m| format!("{m:?}"))
        };
        assert!(button(&app).is_some_and(|m| m.contains("ExpandAll(true)")));
        send(&mut app, M::ExpandAll(true));
        assert!(app.pages().iter().all(|p| app.settings.expanded(*p)));
        assert!(button(&app).is_some_and(|m| m.contains("ExpandAll(false)")));
        send(&mut app, M::ExpandAll(false));
        assert!(!app.pages().iter().any(|p| app.settings.expanded(*p)));
        assert_eq!(app.settings.section(), Some(SectionId::Sound));
        send(&mut app, M::Search("glow".into()));
        assert_eq!(button(&app), None, "a search expands everything");
    }

    #[test]
    fn the_playback_bar_shows_what_its_settings_say() {
        let mut app = test_app();
        app.track = Some(test_track());
        let has = |app: &App, id| {
            let _ = app.view();
            app.focus_reg.count(&FocusId::Named(id))
        };
        for style in [
            PlaybackStyle::Open,
            PlaybackStyle::Card,
            PlaybackStyle::Dock,
        ] {
            for compact in [false, true] {
                for width in [480.0, 700.0, 1000.0, 1600.0] {
                    app.window_size.width = width;
                    app.sections.playback.settings = PlaybackSettings {
                        style,
                        compact,
                        ..PlaybackSettings::default()
                    };
                    for id in ["seek", "previous", "play", "next", "repeat", "volume"] {
                        assert_eq!(has(&app, id), 1, "{style:?} {compact} {width}: {id}");
                    }
                    assert_eq!(has(&app, "fullscreen"), 1);
                    let p = &mut app.sections.playback.settings;
                    (p.repeat, p.volume, p.fullscreen) = (false, false, false);
                    for id in ["repeat", "volume", "fullscreen", "volume-slider"] {
                        assert_eq!(has(&app, id), 0, "{style:?} {compact} {width}: {id}");
                    }
                    assert_eq!(has(&app, "play"), 1);
                }
            }
        }
        // What's left to the style changes with it; what isn't, doesn't.
        let mut p = PlaybackSettings::default();
        assert!(!p.look().filled_play);
        p.style = PlaybackStyle::Card;
        assert!(p.look().filled_play && p.look().thickness == 4);
        p.thickness = Some(9);
        p.style = PlaybackStyle::Dock;
        assert_eq!(p.look().thickness, 9);
        p.curve_height = Some(200);
        p.width = 9000;
        p.handle = SeekHandle::Dot;
        let p = p.clamped();
        assert_eq!((p.look().curve_height, p.width), (14, 1600));
    }

    #[test]
    fn peeking_hides_the_window_and_esc_brings_it_back() {
        use SettingsMessage as M;
        let mut app = test_app();
        open(&mut app, SectionId::Visualizer);
        send(&mut app, M::Shape(Shape::Narrow));
        send(&mut app, M::ToggleOnly);
        send(&mut app, M::ToggleMenu);
        send(&mut app, M::Scrolled(120.0));
        app.focus = Some(FocusId::Named("peek"));
        send(&mut app, M::Peek);
        assert!(app.settings.open && !app.settings.shown());
        assert!(!app.settings.menu, "a peek closes the menu");
        let _ = app.view();
        assert_eq!(app.focus, Some(FocusId::Named("end-peek")));
        assert!(app.focus_reg.contains(&FocusId::Named("end-peek")));
        assert!(!app.focus_reg.contains(&FocusId::Named("done")));
        press(&mut app, named(Named::Escape), false);
        assert!(app.settings.shown());
        assert_eq!(app.settings.section(), Some(SectionId::Visualizer));
        assert_eq!(app.settings.shape, Shape::Narrow);
        assert!(app.settings.only);
        assert_eq!(app.settings.scroll, 120.0);
        // Closing during a peek would be a surprise; Cancel still works after it.
        send(&mut app, M::Cancel);
        assert!(!app.settings.open);
    }

    /// Something done with the mouse.
    #[derive(Clone, Copy)]
    enum Mouse {
        /// A click on a piece of text.
        On(&'static str),
        /// A click at a point.
        At(f32, f32),
        /// A turn of the wheel at a point.
        Wheel(f32, f32, f32),
    }

    /// Do all of `steps` to the app as it is, and check that each sends one message
    /// containing its text (empty: nothing at all).
    fn mouse(app: &App, steps: &[(Mouse, &str)]) {
        use iced_test::simulator::{Simulator, click};
        let size = (app.window_size.width, app.window_size.height);
        let mut sim = Simulator::with_size(own_fonts(), size, app.view());
        for (step, _) in steps {
            match *step {
                Mouse::On(label) => {
                    sim.click(label)
                        .unwrap_or_else(|e| panic!("{label} isn't on screen: {e}"));
                }
                Mouse::At(x, y) => {
                    sim.point_at(Point::new(x, y));
                    let _ = sim.simulate(click());
                }
                Mouse::Wheel(x, y, dy) => {
                    sim.point_at(Point::new(x, y));
                    let delta = mouse::ScrollDelta::Pixels { x: 0.0, y: dy };
                    let _ = sim.simulate([Event::Mouse(mouse::Event::WheelScrolled { delta })]);
                }
            }
        }
        let sent: Vec<String> = sim.into_messages().map(|m| format!("{m:?}")).collect();
        let expected: Vec<&str> = steps
            .iter()
            .map(|(_, e)| *e)
            .filter(|e| !e.is_empty())
            .collect();
        assert!(
            sent.len() == expected.len() && sent.iter().zip(&expected).all(|(s, e)| s.contains(e)),
            "expected {expected:#?}\nsent {sent:#?}"
        );
    }

    #[test]
    fn the_mouse_reaches_the_settings_window_and_nothing_behind_it() {
        use Mouse::{At, On, Wheel};
        use SettingsMessage as M;

        // The main screen: the gear, and the wordmark with the About panel under it.
        let mut app = test_app();
        app.window_size = Size::new(1440.0, 900.0);
        let _ = app.update(Message::ToggleAbout);
        mouse(
            &app,
            &[
                (At(1384.0, 48.0), "Settings(Toggle)"),
                (At(90.0, 48.0), "ToggleAbout"),
                (On("Show license"), "ToggleLicense(0)"),
            ],
        );

        open(&mut app, SectionId::Sound);
        mouse(
            &app,
            &[
                // The tree, the content and its footer.
                (On("Colors"), "Page(Section(Colors))"),
                (On("Timing"), "Group(Section(Sound), 8)"),
                // The tree's arrows, of the section shown and of another one.
                (At(69.0, 166.0), "Expand(Section(Sound), false)"),
                (At(69.0, 622.0), "Expand(Section(Visualizer), true)"),
                (At(120.0, 622.0), "Page(Section(Visualizer))"),
                (On("Auto-calibrate"), "AutoCalibrate"),
                (On("Done"), "Settings(Done)"),
                (On("Cancel"), "Settings(Cancel)"),
                (On("Reset Sound to defaults"), "Settings(Reset)"),
                // The preview: its switches, × and the button that hides it.
                (On("Ultrawide"), "Shape(Ultrawide)"),
                (On("Only the visualizer"), "ToggleOnly"),
                (On("Peek at full size"), "Settings(Peek)"),
                (At(1361.0, 67.0), "Settings(Done)"),
                (At(958.0, 67.0), "TogglePreview"),
                // The picture of the app and the dimmed app around the window (its
                // gear and play button are there) don't react.
                (At(1160.0, 450.0), ""),
                (At(1384.0, 20.0), ""),
                (At(720.0, 886.0), ""),
                (Wheel(1160.0, 450.0, -100.0), ""),
                (Wheel(600.0, 500.0, -200.0), "Scrolled("),
            ],
        );

        // Cards, segments and sliders.
        send(&mut app, M::Page(SectionId::Visualizer.into()));
        mouse(
            &app,
            &[
                (On("Radial"), "PickVisualizer(\"radial\")"),
                // A group's heading.
                (At(320.0, 128.0), "Fold(Section(Visualizer), \"Style\")"),
                (On("Pill"), "id: \"corners\", value: String(\"Pill\")"),
                (At(700.0, 437.0), "id: \"bar_count\""),
                (On("Classic"), "ToggleMenu"),
            ],
        );

        // The preset menu: its items and buttons, and clicks beside it close it (the
        // settings under it don't react, the tree does).
        send(&mut app, M::ToggleMenu);
        mouse(
            &app,
            &[
                (On("Dense"), "LoadPreset(Visualizer, BuiltIn(\"Dense\"))"),
                (On("Save current as new preset"), "SaveNewPreset"),
                (At(400.0, 700.0), "ToggleMenu"),
                (At(420.0, 260.0), "ToggleMenu"),
                (On("Response"), "Group(Section(Sound), 1)"),
            ],
        );
        vis_set(&mut app, "bar_count", 20);
        mouse(
            &app,
            &[
                (On("Save as new preset"), "SaveNewPreset"),
                (On("Discard changes"), "DiscardChanges"),
            ],
        );

        // A peek: the app works again, with the way back on top.
        send(&mut app, M::Peek);
        mouse(
            &app,
            &[
                (On("Back to settings"), "EndPeek"),
                (At(90.0, 48.0), "ToggleAbout"),
            ],
        );
    }

    #[test]
    fn the_preview_never_takes_focus() {
        let mut app = test_app();
        open(&mut app, SectionId::Visualizer);
        let _ = app.view();
        // The app's own header and controls, not the preview's copies of them.
        for id in ["about", "pill", "open-settings", "fullscreen"] {
            assert_eq!(app.focus_reg.count(&FocusId::Named(id)), 1, "{id}");
        }
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
        let _ = app.update(Message::ToggleAbout);
        assert!(app.about_open);
        // It's left of the bars, so it comes before the controls under them.
        let _ = app.view();
        let next = |app: &App, id| app.focus_reg.cycle(Some(&FocusId::Named(id)), false, None);
        assert_eq!(
            next(&app, "open-settings"),
            Some(FocusId::Named("close-panel"))
        );
        assert!(!app.focus_reg.contains(&FocusId::Named("check-updates")));
        let _ = app.update(Message::ToggleLicense(0));
        let _ = app.update(Message::ToggleLicense(1));
        assert_eq!(app.open_license, Some(1));
        let _ = app.update(Message::ToggleLicense(1));
        assert_eq!(app.open_license, None);
        let _ = app.view(); // builds without panicking
    }

    #[test]
    fn presets_bring_back_the_shader_and_its_settings() {
        use SettingsMessage as M;
        let mut app = test_app();
        open(&mut app, SectionId::Visualizer);
        let _ = app.update(Message::PickVisualizer(shaders::RADIAL.into()));
        let set = |id: &str, value: serde_json::Value| Message::SetShaderValue {
            background: false,
            id: id.into(),
            value,
        };
        let _ = app.update(set("bar_count", 72.into()));
        send(&mut app, M::SaveNewPreset);
        send(&mut app, M::Page(SectionId::Background.into()));
        let _ = app.update(Message::PickBackground(shaders::SOLID.into()));
        send(&mut app, M::SaveNewPreset);

        // Change both, then load the presets back.
        let _ = app.update(set("bar_count", 20.into()));
        let _ = app.update(Message::PickVisualizer(shaders::WAVE.into()));
        assert_eq!(app.vis_engine.bar_count, 43, "Wave has its own settings");
        let _ = app.update(Message::PickBackground(shaders::AMBIENT.into()));
        assert!(app.section_edited(SectionId::Visualizer));
        assert!(app.section_edited(SectionId::Background));
        send(
            &mut app,
            M::LoadPreset(SectionId::Background, PresetId::Yours(0)),
        );
        assert_eq!(app.bg.name, shaders::SOLID);
        assert_eq!(app.vis.name, shaders::WAVE, "only that section");
        send(&mut app, M::Page(SectionId::Visualizer.into()));
        send(
            &mut app,
            M::LoadPreset(SectionId::Visualizer, PresetId::Yours(0)),
        );
        assert_eq!(app.vis.name, shaders::RADIAL);
        assert_eq!(app.vis_engine.bar_count, 72);
        assert!(!app.bg_engine.dim, "Solid opts out of the Dim");

        // A setting at its default is the same as one that was never touched.
        assert!(!app.section_edited(SectionId::Visualizer));
        let _ = app.update(set("bar_width", 9.into()));
        assert!(app.section_edited(SectionId::Visualizer));
        let _ = app.update(set("bar_width", 8.into()));
        assert!(!app.section_edited(SectionId::Visualizer));

        // Resetting a shader's settings leaves the others alone.
        let _ = app.update(Message::PickVisualizer(shaders::WAVE.into()));
        let _ = app.update(set("bar_width", 4.into()));
        let _ = app.update(Message::PickVisualizer(shaders::RADIAL.into()));
        let _ = app.update(Message::ResetShaderValues(false));
        assert_eq!(app.vis_engine.bar_count, 43);
        assert!(app.sections.visualizer.values.contains_key(shaders::WAVE));
        assert_eq!(VisualSettings::default().shader, shaders::BARS);
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
            updates: false,
        };
        for id in SectionId::ALL {
            open(&mut app, id);
            let _ = app.view();
            for gone in ["now-playing", "lyrics", "on-top"] {
                assert!(!app.focus_reg.contains(&FocusId::Named(gone)), "{gone}");
            }
        }
        assert!(!app.section_list().contains(&SectionId::Lyrics));
        open(&mut app, SectionId::General);
        let _ = app.view();
        assert!(!app.focus_reg.contains(&FocusId::Named("check-updates")));
        open(&mut app, SectionId::Colors);
        let _ = app.view();
        assert!(app.focus_reg.contains(&FocusId::Named("accent")));
        assert!(!app.accent_choices().contains(&Accent::Album));

        // Swatch indexes follow the shorter list: the last one is still Custom.
        let last = app.palette_choices().len() - 1;
        let _ = app.update(Message::PickSwatch(last));
        assert!(matches!(app.sections.colors.palette, Palette::Custom(_)));
        let _ = app.update(Message::PickAccent(0));
        assert_eq!(app.sections.colors.accent, Accent::Lilac);

        // The transport is gone, full screen stays.
        send(&mut app, SettingsMessage::Done);
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
            app.processor
                .update(&frame, 8, &app.sections.sound, 1.0 / 60.0);
        }
        let t0 = Instant::now();
        // Fading bars keep it drawing; frozen ones don't.
        app.update_idle(t0, false);
        app.update_idle(t0 + Duration::from_millis(1100), false);
        assert!(!app.idle);
        app.sections.sound.when_silent = SilenceMode::Freeze;
        app.update_idle(t0 + Duration::from_millis(1200), false);
        app.update_idle(t0 + Duration::from_millis(2300), false);
        assert!(app.idle);
    }

    #[test]
    fn escape_leaves_fullscreen_last() {
        let mut app = test_app();
        app.fullscreen = true;
        open(&mut app, SectionId::Visualizer);
        press(&mut app, named(Named::Escape), false);
        assert!(!app.settings.open, "settings close before full screen does");
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

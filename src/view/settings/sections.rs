//! What's in each section: its groups of settings, and the outline of them the
//! navigation tree shows and searches.

use super::pictures::Picture;
use super::rows::{Card, Group, Rows, seg};
use super::signed;
use crate::app::settings::{Change, Page, SettingKey};
use crate::app::{App, Message, parse_hex};
use crate::art::ramp;
use crate::controls::SwatchFill;
use crate::focus::FocusId;
use crate::lyrics::{Entry as LyricEntry, Lyrics, Source as LyricsSource, Status as LyricsStatus};
use crate::settings::{
    ACCENT_BLUE, Accent, Action, ActionGroup, AdaptSpeed, BackgroundSettings, BarValue, BarsScroll,
    ColorSettings, ControlSettings, Detail, DoubleClick, FREQ_MIN_GAP, FrameRate, FreqScale,
    GATE_OFF, GeneralSettings, GradientDir, LEVEL_MIN_SPAN, LyricsSettings, MAX_DELAY_MS,
    NowPlayingMode, NowPlayingSettings, Palette, PlaybackSettings, PlaybackStyle, SectionId,
    SeekHandle, SilenceMode, TuningSettings, UpdateSettings, format_db, format_hz, slider_to_hz,
};
use crate::shaders::settings::{Type, Value};
use crate::shaders::{self, Kind as ShaderKind};
use crate::theme::*;
use crate::update::Install;
use crate::view::ellipsize;
use crate::visualizer::is_solid;
use iced::widget::{container, row, text};
use iced::{Element, Length};

/// How to write custom shaders.
const SHADER_GUIDE: &str = "https://github.com/karsoncaliff/undavi/blob/main/docs/shaders.md";

/// The line under a page's title.
pub fn description(page: Page) -> &'static str {
    let id = match page {
        Page::Section(id) => id,
        Page::Presets => return "Every section's presets, and groups of them",
    };
    match id {
        SectionId::Sound => "How the bars react to sound",
        SectionId::Visualizer => "How the music is drawn",
        SectionId::Colors => "The bars' palette and the accent",
        SectionId::Background => "The picture behind the bars",
        SectionId::NowPlaying => "How the current track is shown",
        SectionId::Playback => "The seek bar and the buttons around it",
        SectionId::Lyrics => "Synced lyrics with the music",
        SectionId::Controls => "The keyboard and the mouse",
        SectionId::General => "Auto-hide, the window and updates",
    }
}

/// Something to search for: what it's called, and the label of its row.
#[derive(Clone, Debug, PartialEq)]
pub struct Label {
    pub text: String,
    pub row: String,
}

/// A group in the navigation tree: its heading, and the labels in it to search.
#[derive(Clone, Debug, PartialEq)]
pub struct Outline {
    pub title: String,
    pub labels: Vec<Label>,
}

impl Outline {
    /// Labels are rows' own, or "another name -> a row's label".
    pub fn new(title: &str, labels: &[&str]) -> Self {
        let mut out = Self {
            title: title.into(),
            labels: Vec::new(),
        };
        for l in labels {
            match l.split_once(" -> ") {
                Some((text, row)) => out.push(text, row),
                None => out.push(l, l),
            }
        }
        out
    }

    pub fn push(&mut self, text: &str, row: &str) {
        self.labels.push(Label {
            text: text.into(),
            row: row.into(),
        });
    }

    /// Whether its heading or any label contains `query` (lowercase).
    pub fn matches(&self, query: &str) -> bool {
        std::iter::once(&self.title)
            .chain(self.labels.iter().map(|l| &l.text))
            .any(|s| s.to_lowercase().contains(query))
    }
}

impl App {
    /// A page's groups, in order, as [`App::page_groups`] builds them.
    pub(crate) fn outline(&self, page: Page) -> Vec<Outline> {
        let o = Outline::new;
        let id = match page {
            Page::Section(id) => id,
            Page::Presets => return self.presets_outline(),
        };
        match id {
            SectionId::Sound => vec![
                o(
                    "Levels",
                    &[
                        "Meter -> Signal monitor",
                        "Input -> Signal monitor",
                        "Signal monitor",
                    ],
                ),
                o(
                    "Response",
                    &[
                        "Sensitivity",
                        "Auto-calibrate -> Set the sensitivity for me",
                        "Set the sensitivity for me",
                        "Spread",
                        "Rise time",
                        "Fall time",
                        "Peak hold",
                        "Gravity",
                        "Gravity strength",
                    ],
                ),
                o("Dynamics", &["Level range", "Contrast"]),
                o(
                    "Frequency",
                    &["Range", "Scale", "Tilt", "Detail", "Bar value"],
                ),
                o("Bands", &["Bass", "Mids", "Highs"]),
                o("Auto-gain", &["Auto-gain", "Adapt speed"]),
                o("Silence", &["Noise gate", "When it's silent"]),
                o("Beats", &["Beat sensitivity"]),
                o("Timing", &["Visual delay"]),
            ],
            SectionId::Visualizer => {
                let mut out = vec![o(
                    "Style",
                    &[
                        "Visualizer style",
                        "Shader -> Visualizer style",
                        "Open folder -> Visualizer style",
                    ],
                )];
                out.extend(self.shader_outline(false));
                out
            }
            SectionId::Colors => vec![
                o("Palette", &["Bars", "Gradient", "Custom -> Bars"]),
                o("Accent", &["Color"]),
            ],
            SectionId::Background => {
                let mut out = vec![o(
                    "Style",
                    &[
                        "Background style",
                        "Shader -> Background style",
                        "Open folder -> Background style",
                    ],
                )];
                out.extend(self.shader_outline(true));
                out.push(o(
                    "Effects",
                    &[
                        "Dim",
                        "Saturation",
                        "Brightness",
                        "Contrast",
                        "Accent tint",
                        "Vignette",
                        "Grain",
                    ],
                ));
                out
            }
            SectionId::NowPlaying => {
                let mut out = vec![o(
                    "Layout",
                    &[
                        "Track info",
                        "Header -> Track info",
                        "Spotlight -> Track info",
                        "Minimal -> Track info",
                        "Off -> Track info",
                    ],
                )];
                if self.sections.now_playing.mode != NowPlayingMode::Off {
                    out.push(o("Cover", &["Show album art"]));
                }
                out
            }
            SectionId::Lyrics => {
                let mut out = vec![o("Lyrics", &["Show lyrics"])];
                if self.sections.lyrics.show {
                    out.push(o("Text", &["Text size", "Glow", "Word by word"]));
                    out.push(o(
                        "Timing",
                        &["Timing", "Earlier -> Timing", "Later -> Timing"],
                    ));
                }
                out
            }
            SectionId::Playback => self.playback_outline(),
            SectionId::Controls => self.controls_outline(),
            SectionId::General => {
                let mut out = vec![
                    o(
                        "Auto-hide",
                        &[
                            "Auto-hide controls",
                            "Hide after",
                            "Only when the mouse moves",
                        ],
                    ),
                    o("Display", &["Frame rate", "Always on top"]),
                ];
                if self.platform.updates {
                    out.push(o(
                        "Updates",
                        &["Check for updates", "Install automatically"],
                    ));
                }
                out
            }
        }
    }

    /// The groups a shader's own settings make.
    fn shader_outline(&self, background: bool) -> Vec<Outline> {
        let r = if background {
            &self.bg.resolved
        } else {
            &self.vis.resolved
        };
        let mut out: Vec<Outline> = Vec::new();
        for (i, d) in r.decls.items.iter().enumerate() {
            if !r.visible(i) {
                continue;
            }
            let title = d.section.as_deref().unwrap_or("Settings");
            if out.last().is_none_or(|g| g.title != title) {
                out.push(Outline::new(title, &[]));
            }
            if let Some(g) = out.last_mut() {
                g.push(&d.label, &d.label);
            }
        }
        out
    }

    /// The headings of the groups a page has, as built.
    #[cfg(test)]
    pub(crate) fn group_titles(&self, page: Page) -> Vec<String> {
        let rows = Rows::new(self, page, 560.0);
        self.page_groups(page, &rows)
            .into_iter()
            .map(|g| g.title)
            .collect()
    }

    /// A page's settings. Only the controls that do something right now are built, so
    /// hidden ones aren't Tab stops.
    pub(in crate::view) fn page_groups<'a>(
        &'a self,
        page: Page,
        rows: &Rows<'a>,
    ) -> Vec<Group<'a>> {
        let id = match page {
            Page::Section(id) => id,
            Page::Presets => return self.presets_groups(rows),
        };
        match id {
            SectionId::Sound => self.sound_groups(rows),
            SectionId::Visualizer => {
                let mut out = vec![self.style_group(rows, false)];
                out.extend(self.shader_groups(rows, false));
                out
            }
            SectionId::Colors => self.color_groups(rows),
            SectionId::Background => {
                let mut out = vec![self.style_group(rows, true)];
                out.extend(self.shader_groups(rows, true));
                out.push(self.effects_group(rows));
                out
            }
            SectionId::NowPlaying => self.now_playing_groups(rows),
            SectionId::Playback => self.playback_groups(rows),
            SectionId::Lyrics => self.lyrics_groups(rows),
            SectionId::Controls => self.controls_groups(rows),
            SectionId::General => self.general_groups(rows),
        }
    }

    fn sound_groups<'a>(&'a self, rows: &Rows<'a>) -> Vec<Group<'a>> {
        let t: &TuningSettings = &self.sections.sound;
        let set = |f: fn(&mut TuningSettings, f32)| {
            let base = t.clone();
            move |v: f32| {
                let mut next = base.clone();
                f(&mut next, v);
                Message::SetTuning(next)
            }
        };
        let with = |f: &dyn Fn(&mut TuningSettings)| {
            let mut next = t.clone();
            f(&mut next);
            Message::SetTuning(next)
        };

        let mut levels = rows
            .group("Levels")
            .push(rows.plain(self.level_meter()))
            .push(rows.switch(
                "monitor",
                "Signal monitor",
                Some("What the bars are working with: levels, spectrum, beats and frame rate"),
                self.monitor_open,
                Message::ToggleMonitor,
            ));
        if self.monitor_open {
            levels.rows.extend(self.monitor_rows(rows));
        }

        let calibrate = match self.processor.calibration_remaining() {
            Some(s) => format!("Listening\u{2026} {}s", s.ceil() as u32),
            None => "Auto-calibrate".into(),
        };
        let response = rows
            .group("Response")
            .push(rows.on(&["sensitivity"]).slider(
                "sensitivity",
                "Sensitivity",
                None,
                format!("{}%", t.sensitivity),
                t.sensitivity as f32,
                (0.0, 100.0, 1.0),
                set(|t, v| t.sensitivity = v as u8),
            ))
            .push(rows.on_star("sound.calibrate").action(
                "calibrate",
                "Set the sensitivity for me",
                "Listens for a few seconds; play something typical",
                calibrate,
                Message::AutoCalibrate,
            ))
            .push(rows.on(&["spread"]).slider(
                "spread",
                "Spread",
                Some("Blends neighboring bars into a smoother, wave-like shape"),
                format!("{}%", t.spread),
                t.spread as f32,
                (0.0, 100.0, 1.0),
                set(|t, v| t.spread = v as u8),
            ))
            .push(rows.on(&["rise_ms"]).slider(
                "rise",
                "Rise time",
                None,
                format!("{} ms", t.rise_ms),
                t.rise_ms as f32,
                (5.0, 100.0, 1.0),
                set(|t, v| t.rise_ms = v as u16),
            ))
            .push(rows.on(&["fall_ms"]).slider(
                "fall",
                "Fall time",
                None,
                format!("{} ms", t.fall_ms),
                t.fall_ms as f32,
                (50.0, 800.0, 10.0),
                set(|t, v| t.fall_ms = v as u16),
            ))
            .push(rows.on(&["peak_hold_ms"]).slider(
                "peak-hold",
                "Peak hold",
                None,
                format!("{} ms", t.peak_hold_ms),
                t.peak_hold_ms as f32,
                (0.0, 1000.0, 50.0),
                set(|t, v| t.peak_hold_ms = v as u16),
            ))
            .push(rows.on(&["gravity"]).switch(
                "gravity",
                "Gravity",
                Some("Bars and peak caps fall faster and faster, like real weights"),
                t.gravity,
                with(&|n| n.gravity = !n.gravity),
            ))
            .push_maybe(t.gravity.then(|| {
                rows.on(&["gravity_strength"]).slider(
                    "gravity-strength",
                    "Gravity strength",
                    None,
                    format!("{}%", t.gravity_strength),
                    t.gravity_strength as f32,
                    (25.0, 250.0, 5.0),
                    set(|t, v| t.gravity_strength = v as u8),
                )
            }));

        // Dynamics: which levels count as empty and full, and the curve between them.
        let base = t.clone();
        let dynamics = rows
            .group("Dynamics")
            .push(rows.on(&["floor_db", "ceiling_db"]).range(
                "level",
                "Level range",
                None,
                format!(
                    "{} \u{2013} {} dB",
                    signed(t.floor_db),
                    signed(t.ceiling_db)
                ),
                ((t.floor_db as f32) + 100.0, (t.ceiling_db as f32) + 100.0),
                100.0,
                LEVEL_MIN_SPAN as f32,
                move |(lo, hi)| {
                    Message::SetTuning(TuningSettings {
                        floor_db: (lo - 100.0) as i8,
                        ceiling_db: (hi - 100.0) as i8,
                        ..base.clone()
                    })
                },
            ))
            .push(rows.on(&["contrast"]).slider(
                "contrast",
                "Contrast",
                Some("Low lifts quiet sounds; high makes loud hits stand out"),
                contrast_label(t.contrast),
                t.contrast as f32,
                (0.0, 100.0, 1.0),
                set(|t, v| t.contrast = v as u8),
            ));

        let base = t.clone();
        let frequency = rows
            .group("Frequency")
            .push(rows.on(&["freq_lo", "freq_hi"]).range(
                "freq",
                "Range",
                None,
                format!(
                    "{} \u{2013} {}",
                    format_hz(slider_to_hz(t.freq_lo as f32)),
                    format_hz(slider_to_hz(t.freq_hi as f32))
                ),
                (t.freq_lo as f32, t.freq_hi as f32),
                100.0,
                FREQ_MIN_GAP as f32,
                move |(lo, hi)| {
                    Message::SetTuning(TuningSettings {
                        freq_lo: lo as u8,
                        freq_hi: hi as u8,
                        ..base.clone()
                    })
                },
            ))
            .push(
                rows.on(&["freq_scale"]).segmented(
                    "freq-scale",
                    "Scale",
                    Some(match t.freq_scale {
                        FreqScale::Log => "Every octave gets the same room",
                        FreqScale::Mel => {
                            "Follows how we hear pitch; less room for the extreme highs"
                        }
                        FreqScale::Linear => {
                            "Every bar covers the same Hz, so the bass shares a few bars"
                        }
                    }),
                    [
                        (FreqScale::Log, "Log"),
                        (FreqScale::Mel, "Mel"),
                        (FreqScale::Linear, "Linear"),
                    ]
                    .into_iter()
                    .map(|(f, label)| seg(label, t.freq_scale == f, with(&|n| n.freq_scale = f)))
                    .collect(),
                    true,
                ),
            )
            .push(rows.on(&["tilt_db"]).slider(
                "tilt",
                "Tilt",
                None,
                format!("{}/oct", format_db(t.tilt_db)),
                t.tilt_db as f32,
                (-3.0, 6.0, 1.0),
                set(|t, v| t.tilt_db = v as i8),
            ))
            .push(
                rows.on(&["detail"]).segmented(
                    "detail",
                    "Detail",
                    Some("Detailed resolves low notes better but reacts a little later"),
                    [
                        (Detail::Fast, "Fast"),
                        (Detail::Balanced, "Balanced"),
                        (Detail::Detailed, "Detailed"),
                    ]
                    .into_iter()
                    .map(|(d, label)| seg(label, t.detail == d, with(&|n| n.detail = d)))
                    .collect(),
                    true,
                ),
            )
            .push(
                rows.on(&["bar_value"]).segmented(
                    "bar-value",
                    "Bar value",
                    Some("How the frequencies inside each bar are combined"),
                    [
                        (BarValue::Loudest, "Loudest"),
                        (BarValue::Average, "Average"),
                    ]
                    .into_iter()
                    .map(|(b, label)| seg(label, t.bar_value == b, with(&|n| n.bar_value = b)))
                    .collect(),
                    true,
                ),
            );

        let band = |id: &'static str,
                    fields: &'static [&'static str],
                    name: &str,
                    db: i8,
                    f: fn(&mut TuningSettings, f32)| {
            rows.on(fields).slider(
                id,
                name,
                None,
                format_db(db),
                db as f32,
                (-12.0, 12.0, 1.0),
                set(f),
            )
        };
        let bands = rows
            .group("Bands")
            .push(band("bass", &["bass_db"], "Bass", t.bass_db, |t, v| {
                t.bass_db = v as i8;
            }))
            .push(band("mids", &["mids_db"], "Mids", t.mids_db, |t, v| {
                t.mids_db = v as i8;
            }))
            .push(band("highs", &["highs_db"], "Highs", t.highs_db, |t, v| {
                t.highs_db = v as i8;
            }));

        let auto_gain = rows
            .group("Auto-gain")
            .push(rows.on(&["auto_gain"]).switch(
                "auto-gain",
                "Auto-gain",
                Some("Keeps adjusting so quiet parts aren't flat and loud ones don't max out"),
                t.auto_gain,
                with(&|n| n.auto_gain = !n.auto_gain),
            ))
            .push_maybe(t.auto_gain.then(|| {
                rows.on(&["agc_speed"]).segmented(
                    "agc-speed",
                    "Adapt speed",
                    Some("How quickly it catches up when the music gets quieter"),
                    [
                        (AdaptSpeed::Slow, "Slow"),
                        (AdaptSpeed::Medium, "Medium"),
                        (AdaptSpeed::Fast, "Fast"),
                    ]
                    .into_iter()
                    .map(|(a, label)| seg(label, t.agc_speed == a, with(&|n| n.agc_speed = a)))
                    .collect(),
                    true,
                )
            }));

        let silence = rows
            .group("Silence")
            .push(rows.on(&["gate_db"]).slider(
                "gate",
                "Noise gate",
                Some("Anything quieter counts as silence, so hiss doesn't make the bars twitch"),
                if t.gate_db <= GATE_OFF {
                    "Off".into()
                } else {
                    format!("{} dB", signed(t.gate_db))
                },
                t.gate_db as f32,
                (GATE_OFF as f32, -30.0, 1.0),
                set(|t, v| t.gate_db = v as i8),
            ))
            .push(
                rows.on(&["when_silent"]).segmented(
                    "when-silent",
                    "When it's silent",
                    Some(match t.when_silent {
                        SilenceMode::Fade => "The bars fall flat",
                        SilenceMode::Wave => "A slow wave keeps moving until the music is back",
                        SilenceMode::Freeze => {
                            "The bars stop where they are and drawing pauses, which saves battery"
                        }
                    }),
                    [
                        (SilenceMode::Fade, "Fade out"),
                        (SilenceMode::Wave, "Idle wave"),
                        (SilenceMode::Freeze, "Freeze"),
                    ]
                    .into_iter()
                    .map(|(m, label)| seg(label, t.when_silent == m, with(&|n| n.when_silent = m)))
                    .collect(),
                    true,
                ),
            );

        let beats = rows
            .group("Beats")
            .push(rows.on(&["beat_sensitivity"]).slider(
                "beat-sensitivity",
                "Beat sensitivity",
                Some(
                    "How strong a hit must be to set off Glow and the background pulse. Raise \
                 it for quiet music, lower it for heavy EDM",
                ),
                format!("{}%", t.beat_sensitivity),
                t.beat_sensitivity as f32,
                (0.0, 100.0, 1.0),
                set(|t, v| t.beat_sensitivity = v as u8),
            ));

        let timing = rows.group("Timing").push(rows.on(&["delay_ms"]).slider(
            "delay",
            "Visual delay",
            Some("Holds the bars back to match audio that plays late, like Bluetooth headphones"),
            if t.delay_ms == 0 {
                "Off".into()
            } else {
                format!("{} ms", t.delay_ms)
            },
            t.delay_ms as f32,
            (0.0, MAX_DELAY_MS as f32, 10.0),
            set(|t, v| t.delay_ms = v as u16),
        ));

        vec![
            levels, response, dynamics, frequency, bands, auto_gain, silence, beats, timing,
        ]
    }

    /// Which visualizer (or background): the built-ins and the shaders in the
    /// folder, why the picked one isn't showing (if it isn't), and the way to the
    /// folder and the guide.
    fn style_group<'a>(&'a self, rows: &Rows<'a>, background: bool) -> Group<'a> {
        let active = if background { &self.bg } else { &self.vis };
        let picked = if background {
            &self.sections.background.shader
        } else {
            &self.sections.visualizer.shader
        };
        let pick = |name: &str| {
            if background {
                Message::PickBackground(name.to_string())
            } else {
                Message::PickVisualizer(name.to_string())
            }
        };
        let cards = self
            .shaders
            .entries
            .iter()
            .filter(|e| e.fits(background))
            // Warps are picked as a background's animation, not on their own
            // (unless they're broken: then they show here with their error).
            .filter(|e| !(e.kind == Some(ShaderKind::Warp) && e.state.is_ok()))
            // Album art needs a music player connection for its covers.
            .filter(|e| self.platform.media || e.name != shaders::ALBUM_ART || *picked == e.name)
            .map(|e| Card {
                picture: match e.name.as_str() {
                    shaders::BARS => Picture::Bars,
                    shaders::RADIAL => Picture::Radial,
                    shaders::WAVE => Picture::Wave,
                    shaders::SOLID => Picture::Solid,
                    shaders::ALBUM_ART => Picture::AlbumArt,
                    shaders::AMBIENT => Picture::Ambient,
                    _ => Picture::Shader,
                },
                label: ellipsize(&e.label, 16),
                error: e.state.is_err(),
                selected: *picked == e.name,
                msg: pick(&e.name),
            })
            .collect();
        let ids = if background {
            [
                "bg-shader",
                "bg-shader-copy",
                "bg-shader-folder",
                "bg-shader-guide",
                "bg-shader-reset",
            ]
        } else {
            [
                "vis-shader",
                "vis-shader-copy",
                "vis-shader-folder",
                "vis-shader-guide",
                "vis-shader-reset",
            ]
        };
        let mut group = rows.group("Style").push(rows.on(&["shader"]).cards(
            ids[0],
            if background {
                "Background style"
            } else {
                "Visualizer style"
            },
            cards,
        ));
        if let Some(problem) = &active.problem {
            group = group.push(
                rows.plain(
                    container(
                        text(problem.as_str())
                            .size(11)
                            .font(iced::Font::MONOSPACE)
                            .color(DANGER),
                    )
                    .padding(10)
                    .width(Length::Fill)
                    .style(dark_trough(false)),
                ),
            );
        }
        let button = |id: &'static str, label: &str, msg: Message| {
            rows.ctx
                .text_button(FocusId::Named(id), label.into(), msg, Length::Shrink)
        };
        // A built-in is a good start for your own: copy it into the folder.
        let copy =
            (active.problem.is_none() && shaders::builtin(&active.name).is_some()).then(|| {
                button(
                    ids[1],
                    "Make a copy",
                    Message::CopyShader(active.name.clone()),
                )
            });
        let reset = (!active.resolved.decls.items.is_empty()).then(|| {
            button(
                ids[4],
                "Reset these settings",
                Message::ResetShaderValues(background),
            )
        });
        let actions = row![
            button(ids[2], "Open folder", Message::OpenShaderFolder),
            button(
                ids[3],
                "Shader guide \u{2197}",
                Message::OpenUrl(SHADER_GUIDE)
            ),
        ]
        .extend(copy)
        .extend(reset)
        .spacing(2);
        group.push(
            container(actions.wrap())
                .padding([6, 4])
                .width(Length::Fill),
        )
    }

    /// The settings the visualizer (or background) in use declares, one group per
    /// `@section`, with only the controls whose condition holds.
    fn shader_groups<'a>(&'a self, rows: &Rows<'a>, background: bool) -> Vec<Group<'a>> {
        let r = if background {
            &self.bg.resolved
        } else {
            &self.vis.resolved
        };
        let id = if background {
            "bg-setting"
        } else {
            "vis-setting"
        };
        let set = move |key: &str, value: serde_json::Value| Message::SetShaderValue {
            background,
            id: key.to_string(),
            value,
        };
        let with_unit = |shown: String, unit: &str| {
            if unit.is_empty() || unit == "%" {
                format!("{shown}{unit}")
            } else {
                format!("{shown} {unit}")
            }
        };

        let active = if background { &self.bg } else { &self.vis };
        let base = self.declared_base(background);
        // Where the beat shows: under the switch for what follows it.
        let (beat_switch, pulse) = if background {
            ("pulse", self.bg_beat.pulse())
        } else {
            ("glow", self.beat.pulse())
        };
        let mut groups: Vec<Group<'a>> = Vec::new();
        for (i, decl) in r.decls.items.iter().enumerate() {
            if !r.visible(i) {
                continue;
            }
            let title = decl.section.as_deref().unwrap_or("Settings");
            if groups.last().is_none_or(|g| g.title != title) {
                groups.push(rows.group(title));
            }
            let setting = SettingKey::Shader {
                background,
                shader: active.name.clone(),
                id: decl.id.clone(),
            };
            let differs = base
                .as_ref()
                .and_then(|b| b.values.get(i))
                .is_some_and(|v| *v != r.values[i]);
            let rows = rows.on_key(setting, differs);
            let help = decl.help.as_deref();
            let unit = decl.unit.as_deref().unwrap_or_default();
            let key = decl.id.clone();
            let focus = FocusId::Indexed(id, i);
            let control: Element<'a, Message> = match (&decl.ty, &r.values[i]) {
                (Type::Int { min, max, step }, Value::Int(v)) => rows.slider(
                    focus,
                    &decl.label,
                    help,
                    with_unit(v.to_string(), unit),
                    *v as f32,
                    (*min as f32, *max as f32, (*step).max(1) as f32),
                    move |x| set(&key, (x.round() as i32).into()),
                ),
                (Type::Float { min, max, step }, Value::Float(v)) => {
                    let decimals = if *step >= 1.0 {
                        0
                    } else if *step >= 0.1 {
                        1
                    } else {
                        2
                    };
                    rows.slider(
                        focus,
                        &decl.label,
                        help,
                        with_unit(format!("{v:.decimals$}"), unit),
                        *v,
                        (*min, *max, *step),
                        move |x| {
                            set(
                                &key,
                                serde_json::Number::from_f64(f64::from(x))
                                    .map_or(serde_json::Value::Null, serde_json::Value::Number),
                            )
                        },
                    )
                }
                (Type::Toggle, Value::Toggle(on)) => {
                    rows.switch(focus, &decl.label, help, *on, set(&key, (!on).into()))
                }
                (Type::Choice { .. }, Value::Choice(current)) => rows.segmented(
                    focus,
                    &decl.label,
                    help,
                    decl.options(&r.warps)
                        .into_iter()
                        .map(|o| {
                            let msg = set(&key, o.clone().into());
                            let selected = o == *current;
                            seg(o, selected, msg)
                        })
                        .collect(),
                    true,
                ),
                (Type::Color { swatches }, Value::Color(current)) => {
                    let mut colors = swatches.clone();
                    if !colors.contains(current) {
                        colors.push(*current);
                    }
                    let selected = colors.iter().position(|c| c == current).unwrap_or(0);
                    let fills = colors.iter().map(|c| SwatchFill::Stops(vec![*c])).collect();
                    rows.swatches(
                        focus,
                        &decl.label,
                        help,
                        format!("#{:02x}{:02x}{:02x}", current[0], current[1], current[2]),
                        fills,
                        selected,
                        true,
                        move |n| {
                            let [r, g, b] = colors.get(n).copied().unwrap_or_default();
                            set(&key, format!("#{r:02x}{g:02x}{b:02x}").into())
                        },
                    )
                }
                // A value always matches its declaration's type (`resolve`).
                _ => continue,
            };
            if let Some(g) = groups.last_mut() {
                g.rows.push(control);
                if decl.id == beat_switch && r.values[i] == Value::Toggle(true) {
                    g.rows.push(rows.plain(self.beat_meter(pulse)));
                }
            }
        }
        groups
    }

    fn color_groups<'a>(&'a self, rows: &Rows<'a>) -> Vec<Group<'a>> {
        let c: &ColorSettings = &self.sections.colors;
        let with = |f: &dyn Fn(&mut ColorSettings)| {
            let mut next = c.clone();
            f(&mut next);
            Message::SetColors(next)
        };

        let choices = self.palette_choices();
        let selected = choices
            .iter()
            .position(|p| std::mem::discriminant(p) == std::mem::discriminant(&c.palette))
            .unwrap_or(0);
        let fills = choices
            .iter()
            .map(|p| match (p, &self.art) {
                (Palette::Album, Some(art)) => SwatchFill::Image(art.swatch.clone()),
                (Palette::Album, None) => SwatchFill::Stops(ramp(self.target_accent()).to_vec()),
                (Palette::Custom(_), _) => match &c.palette {
                    Palette::Custom(stops) => SwatchFill::Stops(stops.clone()),
                    _ => SwatchFill::Add,
                },
                (p, _) => SwatchFill::Stops(p.stops()),
            })
            .collect();
        let mut palette = rows.group("Palette").push(rows.on(&["palette"]).swatches(
            "palette",
            "Bars",
            None,
            c.palette.name().into(),
            fills,
            selected,
            false,
            Message::PickSwatch,
        ));
        if matches!(c.palette, Palette::Custom(_)) {
            palette = palette.push(self.custom_editor(rows));
        }
        if c.palette == Palette::Album {
            palette = palette.push(rows.on(&["album_gradient"]).segmented(
                "album-fill",
                "Colors",
                None,
                vec![
                    seg(
                        "Solid",
                        !c.album_gradient,
                        with(&|n| n.album_gradient = false),
                    ),
                    seg(
                        "Gradient",
                        c.album_gradient,
                        with(&|n| n.album_gradient = true),
                    ),
                ],
                true,
            ));
            // Only meaningful when the accent is a fixed color, not the cover's.
            if c.album_gradient && c.accent != Accent::Album {
                palette = palette.push(rows.on(&["album_match_accent"]).switch(
                    "album-match-accent",
                    "Match accent color",
                    Some("Build the gradient from your accent instead of the cover"),
                    c.album_match_accent,
                    with(&|n| n.album_match_accent = !n.album_match_accent),
                ));
            }
        }
        let palette = palette.push(
            rows.on(&["gradient"]).segmented(
                "gradient",
                "Gradient",
                None,
                // Same two settings; the labels describe them for the built-ins.
                match self.vis.name.as_str() {
                    shaders::RADIAL => [
                        (GradientDir::Across, "Around the ring"),
                        (GradientDir::Along, "Inside out"),
                    ],
                    shaders::WAVE => [
                        (GradientDir::Across, "Left to right"),
                        (GradientDir::Along, "Base to peaks"),
                    ],
                    _ => [
                        (GradientDir::Across, "Across bars"),
                        (GradientDir::Along, "Along each bar"),
                    ],
                }
                .into_iter()
                .map(|(g, label)| seg(label, c.gradient == g, with(&|n| n.gradient = g)))
                .collect(),
                !is_solid(&self.bar_stops()),
            ),
        );

        let accents = self.accent_choices();
        let fills = accents
            .iter()
            .map(|acc| match (acc.rgb(), &self.art) {
                (Some(c), _) => SwatchFill::Stops(vec![c]),
                (None, Some(art)) => SwatchFill::Image(art.swatch.clone()),
                (None, None) => SwatchFill::Stops(vec![ACCENT_BLUE]),
            })
            .collect();
        let selected = accents.iter().position(|x| *x == c.accent).unwrap_or(0);
        let accent = rows.group("Accent").push(rows.on(&["accent"]).swatches(
            "accent",
            "Color",
            Some("Sliders, switches, active buttons and the seek bar"),
            c.accent.name().into(),
            fills,
            selected,
            false,
            Message::PickAccent,
        ));
        vec![palette, accent]
    }

    /// The custom palette's hex fields.
    fn custom_editor<'a>(&'a self, rows: &Rows<'a>) -> Element<'a, Message> {
        const IDS: [&str; 3] = ["hex-0", "hex-1", "hex-2"];
        let field = |i: usize, placeholder: &'static str| {
            let value = &self.custom_hex[i];
            let valid = value.trim().is_empty() || parse_hex(value).is_some();
            rows.field(
                FocusId::Indexed("hex", i),
                IDS[i],
                placeholder,
                value,
                valid,
                move |s| Message::CustomHex(i, s),
            )
        };
        rows.plain(
            iced::widget::column![
                row![field(0, "#start"), field(1, "#middle"), field(2, "#end")].spacing(8),
                text("Hex colors, left to right. Leave a field empty to skip it.")
                    .size(12)
                    .color(TEXT_3),
            ]
            .spacing(8),
        )
    }

    /// The effects every background gets.
    fn effects_group<'a>(&'a self, rows: &Rows<'a>) -> Group<'a> {
        let b: &BackgroundSettings = &self.sections.background;
        let set = |f: fn(&mut BackgroundSettings, f32)| {
            let base = b.clone();
            move |x: f32| {
                let mut next = base.clone();
                f(&mut next, x);
                Message::SetBackground(next)
            }
        };
        let off_or = |v: u8| {
            if v == 0 {
                "Off".to_string()
            } else {
                format!("{v}%")
            }
        };
        let effects = rows.group("Effects");
        // A background can opt out of the Dim (`@no-dim`, like Solid).
        let dim = self.bg_engine.dim.then(|| {
            rows.on(&["dim_pct"]).slider(
                "dim",
                "Dim",
                None,
                format!("{}%", b.dim_pct),
                b.dim_pct as f32,
                (0.0, 90.0, 1.0),
                set(|b, x| b.dim_pct = x as u8),
            )
        });
        effects
            .push_maybe(dim)
            .push(rows.on(&["saturation"]).slider(
                "bg-saturation",
                "Saturation",
                None,
                format!("{}%", b.saturation),
                b.saturation as f32,
                (0.0, 200.0, 5.0),
                set(|b, x| b.saturation = x as u8),
            ))
            .push(rows.on(&["brightness"]).slider(
                "bg-brightness",
                "Brightness",
                None,
                format!("{}%", b.brightness),
                b.brightness as f32,
                (50.0, 150.0, 5.0),
                set(|b, x| b.brightness = x as u8),
            ))
            .push(rows.on(&["contrast"]).slider(
                "bg-contrast",
                "Contrast",
                None,
                format!("{}%", b.contrast),
                b.contrast as f32,
                (50.0, 150.0, 5.0),
                set(|b, x| b.contrast = x as u8),
            ))
            .push(rows.on(&["tint_pct"]).slider(
                "bg-tint",
                "Accent tint",
                Some("Leans the background's colors toward the accent"),
                off_or(b.tint_pct),
                b.tint_pct as f32,
                (0.0, 100.0, 5.0),
                set(|b, x| b.tint_pct = x as u8),
            ))
            .push(rows.on(&["vignette_pct"]).slider(
                "vignette",
                "Vignette",
                None,
                off_or(b.vignette_pct),
                b.vignette_pct as f32,
                (0.0, 100.0, 5.0),
                set(|b, x| b.vignette_pct = x as u8),
            ))
            .push(rows.on(&["grain_pct"]).slider(
                "grain",
                "Grain",
                None,
                off_or(b.grain_pct),
                b.grain_pct as f32,
                (0.0, 100.0, 5.0),
                set(|b, x| b.grain_pct = x as u8),
            ))
    }

    fn now_playing_groups<'a>(&'a self, rows: &Rows<'a>) -> Vec<Group<'a>> {
        let n: &NowPlayingSettings = &self.sections.now_playing;
        let with = |f: &dyn Fn(&mut NowPlayingSettings)| {
            let mut next = n.clone();
            f(&mut next);
            Message::SetNowPlaying(next)
        };
        let layout = rows.group("Layout").push(
            rows.on(&["mode"]).cards(
                "now-playing",
                "Track info",
                [
                    (NowPlayingMode::Header, "Header", Picture::Header),
                    (NowPlayingMode::Spotlight, "Spotlight", Picture::Spotlight),
                    (NowPlayingMode::Minimal, "Minimal", Picture::Minimal),
                    (NowPlayingMode::Off, "Off", Picture::Nothing),
                ]
                .into_iter()
                .map(|(m, label, picture)| Card {
                    picture,
                    label: label.into(),
                    error: false,
                    selected: n.mode == m,
                    msg: with(&|n| n.mode = m),
                })
                .collect(),
            ),
        );
        let mut out = vec![layout];
        if n.mode != NowPlayingMode::Off {
            out.push(
                rows.group("Cover")
                    .push(rows.on(&["show_album_art"]).switch(
                        "show-art",
                        "Show album art",
                        Some("Next to the track info"),
                        n.show_album_art,
                        with(&|n| n.show_album_art = !n.show_album_art),
                    )),
            );
        }
        out
    }

    /// What's in the Playback bar section ([`App::playback_groups`]).
    fn playback_outline(&self) -> Vec<Outline> {
        let p = &self.sections.playback;
        let o = Outline::new;
        let mut layout = o("Layout", &["Style", "Compact"]);
        let mut buttons = o(
            "Buttons",
            &[
                "Filled play button",
                "Repeat button",
                "Volume",
                "Full-screen button",
            ],
        );
        if p.style != PlaybackStyle::Dock {
            layout.push("Width", "Width");
        }
        if p.style != PlaybackStyle::Open {
            layout.push("Opacity", "Opacity");
            buttons.title = "Contents".into();
            buttons.push("Track info", "Track info");
        }
        let mut seek = o(
            "Seek bar",
            &[
                "Curve height",
                "Line thickness",
                "Handle",
                "Remaining line",
                "Times",
            ],
        );
        if p.look().curve_height > 0 {
            seek.push("Curve length", "Curve length");
            seek.push("Curve speed", "Curve speed");
        }
        vec![layout, buttons, seek]
    }

    fn playback_groups<'a>(&'a self, rows: &Rows<'a>) -> Vec<Group<'a>> {
        let p: &PlaybackSettings = &self.sections.playback;
        let look = p.look();
        let with = |f: &dyn Fn(&mut PlaybackSettings)| {
            let mut next = p.clone();
            f(&mut next);
            Message::SetPlayback(next)
        };
        let set = |f: fn(&mut PlaybackSettings, f32)| {
            let base = p.clone();
            move |x: f32| {
                let mut next = base.clone();
                f(&mut next, x);
                Message::SetPlayback(next)
            }
        };
        let boxed = p.style != PlaybackStyle::Open;

        let mut layout = rows
            .group("Layout")
            .push(
                rows.on(&["style"]).segmented(
                    "playback-style",
                    "Style",
                    Some(match p.style {
                        PlaybackStyle::Open => "On the background, without a fill",
                        PlaybackStyle::Card => "A rounded card below the bars",
                        PlaybackStyle::Dock => "A strip along the bottom of the window",
                    }),
                    [
                        (PlaybackStyle::Open, "Open"),
                        (PlaybackStyle::Card, "Card"),
                        (PlaybackStyle::Dock, "Dock"),
                    ]
                    .into_iter()
                    .map(|(v, label)| seg(label, p.style == v, with(&|n| n.style = v)))
                    .collect(),
                    true,
                ),
            )
            .push(rows.on(&["compact"]).switch(
                "playback-compact",
                "Compact",
                Some("Everything in one row"),
                p.compact,
                with(&|n| n.compact = !n.compact),
            ));
        if p.style != PlaybackStyle::Dock {
            layout = layout.push(rows.on(&["width"]).slider(
                "playback-width",
                "Width",
                Some("How wide it gets, if there's room"),
                format!("{} px", p.width),
                p.width as f32,
                (500.0, 1600.0, 50.0),
                set(|n, x| n.width = x as u16),
            ));
        }
        if boxed {
            layout = layout.push(rows.on(&["opacity"]).slider(
                "playback-opacity",
                "Opacity",
                Some("How solid it is over the background"),
                format!("{}%", p.opacity),
                p.opacity as f32,
                (0.0, 100.0, 5.0),
                set(|n, x| n.opacity = x as u8),
            ));
        }

        let mut contents = rows.group(if boxed { "Contents" } else { "Buttons" });
        if boxed {
            contents = contents.push(rows.on(&["track_info"]).switch(
                "playback-info",
                "Track info",
                Some("The cover, title and artist. Now playing has its own, at the top"),
                p.track_info,
                with(&|n| n.track_info = !n.track_info),
            ));
        }
        let contents = contents
            .push(rows.on(&["filled_play"]).switch(
                "playback-filled",
                "Filled play button",
                None,
                look.filled_play,
                with(&|n| n.filled_play = Some(!look.filled_play)),
            ))
            .push(rows.on(&["repeat"]).switch(
                "playback-repeat",
                "Repeat button",
                None,
                p.repeat,
                with(&|n| n.repeat = !n.repeat),
            ))
            .push(rows.on(&["volume"]).switch(
                "playback-volume",
                "Volume",
                None,
                p.volume,
                with(&|n| n.volume = !n.volume),
            ))
            .push(rows.on(&["fullscreen"]).switch(
                "playback-fullscreen",
                "Full-screen button",
                Some("F and F11 work either way"),
                p.fullscreen,
                with(&|n| n.fullscreen = !n.fullscreen),
            ));

        let seek = rows
            .group("Seek bar")
            .push(rows.on(&["curve_height"]).slider(
                "curve-height",
                "Curve height",
                Some("Of the part that's played. None is a straight line"),
                match look.curve_height {
                    0 => "None".to_string(),
                    v => format!("{v} px"),
                },
                look.curve_height as f32,
                (0.0, 14.0, 1.0),
                set(|n, x| n.curve_height = Some(x as u8)),
            ))
            .push_maybe((look.curve_height > 0).then(|| {
                rows.on(&["curve_length"]).slider(
                    "curve-length",
                    "Curve length",
                    Some("From one wave to the next"),
                    format!("{} px", look.curve_length),
                    look.curve_length as f32,
                    (30.0, 240.0, 2.0),
                    set(|n, x| n.curve_length = Some(x as u16)),
                )
            }))
            .push_maybe((look.curve_height > 0).then(|| {
                rows.on(&["curve_speed"]).slider(
                    "curve-speed",
                    "Curve speed",
                    Some("How fast it moves while music plays"),
                    match p.curve_speed {
                        0 => "Still".to_string(),
                        v => format!("{v}%"),
                    },
                    p.curve_speed as f32,
                    (0.0, 300.0, 10.0),
                    set(|n, x| n.curve_speed = x as u16),
                )
            }))
            .push(rows.on(&["thickness"]).slider(
                "line-thickness",
                "Line thickness",
                None,
                format!("{} px", look.thickness),
                look.thickness as f32,
                (2.0, 10.0, 1.0),
                set(|n, x| n.thickness = Some(x as u8)),
            ))
            .push(
                rows.on(&["handle"]).segmented(
                    "seek-handle",
                    "Handle",
                    Some("What marks the position"),
                    [
                        (SeekHandle::Bar, "Bar"),
                        (SeekHandle::Dot, "Dot"),
                        (SeekHandle::None, "None"),
                    ]
                    .into_iter()
                    .map(|(v, label)| seg(label, p.handle == v, with(&|n| n.handle = v)))
                    .collect(),
                    true,
                ),
            )
            .push(rows.on(&["remaining"]).slider(
                "remaining-line",
                "Remaining line",
                Some("How bright the part still to play is"),
                format!("{}%", look.remaining),
                look.remaining as f32,
                (10.0, 100.0, 5.0),
                set(|n, x| n.remaining = Some(x as u8)),
            ))
            .push(rows.on(&["times"]).switch(
                "seek-times",
                "Times",
                Some("Elapsed and remaining, if there's room"),
                p.times,
                with(&|n| n.times = !n.times),
            ));
        vec![layout, contents, seek]
    }

    fn lyrics_groups<'a>(&'a self, rows: &Rows<'a>) -> Vec<Group<'a>> {
        let l: &LyricsSettings = &self.sections.lyrics;
        let with = |f: &dyn Fn(&mut LyricsSettings)| {
            let mut next = l.clone();
            f(&mut next);
            Message::SetLyrics(next)
        };
        let set = |f: fn(&mut LyricsSettings, f32)| {
            let base = l.clone();
            move |x: f32| {
                let mut next = base.clone();
                f(&mut next, x);
                Message::SetLyrics(next)
            }
        };
        let show = rows.group("Lyrics").push(rows.on(&["show"]).switch(
            "lyrics",
            "Show lyrics",
            Some("Synced lyrics from LRCLIB, or a .lrc file next to the song"),
            l.show,
            with(&|n| n.show = !n.show),
        ));
        if !l.show {
            return vec![show];
        }
        let show = show.push(rows.note(self.lyrics_status_text()));
        let text = rows
            .group("Text")
            .push(rows.on(&["size"]).slider(
                "lyrics-size",
                "Text size",
                None,
                format!("{}%", l.size),
                l.size as f32,
                (70.0, 150.0, 5.0),
                set(|l, x| l.size = x as u8),
            ))
            .push(rows.on(&["glow"]).slider(
                "lyrics-glow",
                "Glow",
                None,
                if l.glow == 0 {
                    "Off".to_string()
                } else {
                    format!("{}%", l.glow)
                },
                l.glow as f32,
                (0.0, 100.0, 5.0),
                set(|l, x| l.glow = x as u8),
            ))
            .push(rows.on(&["word_sync"]).switch(
                "lyrics-word-sync",
                "Word by word",
                Some("Fill each word as it's sung, when the lyrics have word timing"),
                l.word_sync,
                with(&|n| n.word_sync = !n.word_sync),
            ));
        let timing = rows.group("Timing").push(rows.on(&["offset_ms"]).slider(
            "lyrics-timing",
            "Timing",
            None,
            match l.offset_ms {
                0 => "In sync".to_string(),
                ms if ms > 0 => format!("{:.2} s earlier", ms as f32 / 1000.0),
                ms => format!("{:.2} s later", -ms as f32 / 1000.0),
            },
            l.offset_ms as f32,
            (-2000.0, 2000.0, 50.0),
            set(|l, x| l.offset_ms = x as i16),
        ));
        vec![show, text, timing]
    }

    /// Where this song's lyrics came from, or why there are none.
    pub(super) fn lyrics_status_text(&self) -> String {
        if self.track.is_none() {
            return "Nothing playing.".into();
        }
        match self.lyrics_loader.status() {
            LyricsStatus::Loading => "Looking for lyrics\u{2026}".into(),
            LyricsStatus::NotFound => "No lyrics found for this song.".into(),
            LyricsStatus::Found(l) => match &*l {
                Lyrics::Instrumental => "This song is instrumental.".into(),
                Lyrics::Plain { .. } => {
                    "Only unsynced lyrics found; they show in Spotlight.".into()
                }
                Lyrics::Synced { entries, source } => {
                    let words = entries
                        .iter()
                        .any(|e| matches!(e, LyricEntry::Line(l) if l.word_synced));
                    let from = match source {
                        LyricsSource::File => "a .lrc file",
                        LyricsSource::Lrclib => "LRCLIB",
                    };
                    if words {
                        format!("Word-synced lyrics from {from}.")
                    } else {
                        format!("Line-synced lyrics from {from}.")
                    }
                }
            },
        }
    }

    /// The shortcuts there are, in their groups.
    fn shortcut_groups(&self) -> Vec<(&'static str, Vec<Action>)> {
        [
            ("Playback keys", ActionGroup::Playback),
            ("Window keys", ActionGroup::Window),
            ("Settings keys", ActionGroup::Settings),
        ]
        .into_iter()
        .map(|(title, group)| {
            let actions = Action::ALL
                .into_iter()
                .filter(|a| a.group() == group && (self.platform.media || !a.needs_media()))
                .collect();
            (title, actions)
        })
        .filter(|(_, actions): &(_, Vec<Action>)| !actions.is_empty())
        .collect()
    }

    /// What's in the Controls section ([`App::controls_groups`]).
    fn controls_outline(&self) -> Vec<Outline> {
        let mut out = Vec::new();
        for (title, actions) in self.shortcut_groups() {
            let names: Vec<&str> = actions.iter().map(|a| a.name()).collect();
            out.push(Outline::new(title, &names));
        }
        if self.platform.media {
            out.push(Outline::new("Steps", &["Seek step", "Volume step"]));
        }
        let mut mouse = Outline::new("Mouse", &["Double-click the bars"]);
        if self.platform.media {
            mouse.push("Scroll over the bars", "Scroll over the bars");
        }
        out.push(mouse);
        out
    }

    fn controls_groups<'a>(&'a self, rows: &Rows<'a>) -> Vec<Group<'a>> {
        let c: &ControlSettings = &self.sections.controls;
        let media = self.platform.media;
        let with = |f: &dyn Fn(&mut ControlSettings)| {
            let mut next = c.clone();
            f(&mut next);
            Message::SetControls(next)
        };
        let set = |f: fn(&mut ControlSettings, f32)| {
            let base = c.clone();
            move |x: f32| {
                let mut next = base.clone();
                f(&mut next, x);
                Message::SetControls(next)
            }
        };
        let mut out = Vec::new();
        for (title, actions) in self.shortcut_groups() {
            let mut group = rows.group(title);
            for action in actions {
                group = group.push(rows.shortcut(action));
            }
            out.push(group);
        }
        if let Some(keys) = out.pop() {
            out.push(keys.push(rows.note(
                "Tab, Enter and Esc always do the same, and so do the arrow keys on a control \
                 that has the focus."
                    .into(),
            )));
        }
        if media {
            out.push(
                rows.group("Steps")
                    .push(rows.on(&["seek_step_s"]).slider(
                        "seek-step",
                        "Seek step",
                        Some("How far seeking with the keys (or the wheel) goes"),
                        format!("{} s", c.seek_step_s),
                        c.seek_step_s as f32,
                        (1.0, 60.0, 1.0),
                        set(|n, x| n.seek_step_s = x as u8),
                    ))
                    .push(rows.on(&["volume_step"]).slider(
                        "volume-step",
                        "Volume step",
                        Some("The same for the volume"),
                        format!("{}%", c.volume_step),
                        c.volume_step as f32,
                        (1.0, 25.0, 1.0),
                        set(|n, x| n.volume_step = x as u8),
                    )),
            );
        }
        let double = [
            (DoubleClick::Fullscreen, "Full screen"),
            (DoubleClick::PlayPause, "Play or pause"),
            (DoubleClick::Nothing, "Nothing"),
        ];
        let mut mouse = rows.group("Mouse").push(
            rows.on(&["double_click"]).segmented(
                "double-click",
                "Double-click the bars",
                None,
                double
                    .into_iter()
                    .filter(|(v, _)| media || *v != DoubleClick::PlayPause)
                    .map(|(v, label)| {
                        seg(label, c.double_click == v, with(&|n| n.double_click = v))
                    })
                    .collect(),
                true,
            ),
        );
        if media {
            mouse = mouse.push(
                rows.on(&["scroll"]).segmented(
                    "bars-scroll",
                    "Scroll over the bars",
                    None,
                    [
                        (BarsScroll::Nothing, "Nothing"),
                        (BarsScroll::Volume, "Volume"),
                        (BarsScroll::Seek, "Seek"),
                    ]
                    .into_iter()
                    .map(|(v, label)| seg(label, c.scroll == v, with(&|n| n.scroll = v)))
                    .collect(),
                    true,
                ),
            );
        }
        out.push(mouse);
        out
    }

    fn general_groups<'a>(&'a self, rows: &Rows<'a>) -> Vec<Group<'a>> {
        let g: &GeneralSettings = &self.sections.general;
        let with = |f: &dyn Fn(&mut GeneralSettings)| {
            let mut next = g.clone();
            f(&mut next);
            Message::SetGeneral(next)
        };
        let mut controls = rows.group("Auto-hide").push(rows.on(&["auto_hide"]).switch(
            "auto-hide",
            "Auto-hide controls",
            Some("Hides menus when the mouse is still"),
            g.auto_hide,
            with(&|n| n.auto_hide = !n.auto_hide),
        ));
        if g.auto_hide {
            let base = g.clone();
            controls = controls
                .push(rows.on(&["hide_after_s"]).slider(
                    "hide-after",
                    "Hide after",
                    None,
                    format!("{} s", g.hide_after_s),
                    g.hide_after_s as f32,
                    (1.0, 10.0, 1.0),
                    move |x| {
                        Message::SetGeneral(GeneralSettings {
                            hide_after_s: x as u8,
                            ..base.clone()
                        })
                    },
                ))
                .push(rows.on(&["show_on_move_only"]).switch(
                    "show-on-move",
                    "Only when the mouse moves",
                    Some("Keys, clicks and scrolling don't bring the controls back"),
                    g.show_on_move_only,
                    with(&|n| n.show_on_move_only = !n.show_on_move_only),
                ));
        }
        let display = rows.group("Display");
        let on_top = self.platform.always_on_top.then(|| {
            rows.on(&["always_on_top"]).switch(
                "on-top",
                "Always on top",
                None,
                g.always_on_top,
                with(&|n| n.always_on_top = !n.always_on_top),
            )
        });
        let display = display
            .push(
                rows.on(&["frame_rate"]).segmented(
                    "frame-rate",
                    "Frame rate",
                    Some("Lower rates use less power. Display matches your screen's refresh rate"),
                    [
                        (FrameRate::Fps30, "30 fps"),
                        (FrameRate::Fps60, "60 fps"),
                        (FrameRate::Display, "Display"),
                    ]
                    .into_iter()
                    .map(|(f, label)| seg(label, g.frame_rate == f, with(&|n| n.frame_rate = f)))
                    .collect(),
                    true,
                ),
            )
            .push_maybe(on_top);
        let mut out = vec![controls, display];
        if self.platform.updates {
            out.push(self.updates_group(rows));
        }
        out
    }

    fn updates_group<'a>(&'a self, rows: &Rows<'a>) -> Group<'a> {
        let u = &self.updates;
        let updates = |field: &'static str| {
            let key = SettingKey::Updates(field);
            let differs = self.setting_change(&key) == Some(Change::Undo);
            rows.on_key(key, differs)
        };
        let with = |f: &dyn Fn(&mut UpdateSettings)| {
            let mut next = u.clone();
            f(&mut next);
            Message::SetUpdates(next)
        };
        let group = rows.group("Updates").push(updates("check").switch(
            "check-updates",
            "Check for updates",
            Some("Looks for a new release on GitHub when Undavi starts"),
            u.check,
            with(&|n| n.check = !n.check),
        ));
        if !u.check {
            group
        } else if self.install == Some(Install::Package) {
            group.push(
                rows.note("Installed with your package manager, which handles updates.".into()),
            )
        } else {
            group.push(updates("auto_install").switch(
                "auto-update",
                "Install automatically",
                Some("Without asking first; you may still need to enter your password"),
                u.auto_install,
                with(&|n| n.auto_install = !n.auto_install),
            ))
        }
    }
}

fn contrast_label(c: u8) -> String {
    match c {
        0..=35 => format!("Soft \u{00B7} {c}"),
        65..=100 => format!("Punchy \u{00B7} {c}"),
        _ => format!("Linear \u{00B7} {c}"),
    }
}

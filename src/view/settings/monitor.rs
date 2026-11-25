//! The signal monitor, under its switch in the Sound section: what the bars are
//! working with, in parts that can be shown or hidden. (`crate::monitor` gathers the
//! numbers and draws the graphs.)

use super::rows::Rows;
use super::signed;
use crate::app::{App, AudioOutage, Message, lyric_offset};
use crate::icons;
use crate::monitor::{self, BeatGraph, LevelGraph, SpectrumGraph};
use crate::settings::{
    FrameRate, FreqScale, GATE_OFF, GlowSource, MonitorSettings, format_hz, slider_to_hz,
};
use crate::theme::*;
use crate::visualizer::format_time;
use iced::widget::{Canvas, Column, column, container, row, text};
use iced::{Color, Element, Length};

/// Four switches in a row need this much room; with less, they go in two rows.
const FOUR_ACROSS: f32 = 440.0;

impl App {
    /// The monitor's rows: which parts to show, then the parts.
    pub(super) fn monitor_rows<'a>(&'a self, rows: &Rows<'a>) -> Vec<Element<'a, Message>> {
        let s = &self.monitor_sections;
        let mut out = vec![rows.plain(self.monitor_switches(rows))];
        if s.signal {
            out.push(rows.plain(self.monitor_signal()));
        }
        if s.spectrum {
            out.push(rows.plain(self.monitor_spectrum()));
        }
        if s.beats {
            out.push(rows.plain(self.monitor_beats()));
        }
        if s.performance {
            out.push(rows.plain(self.monitor_system()));
        }
        out
    }

    fn monitor_switches<'a>(&'a self, rows: &Rows<'a>) -> Element<'a, Message> {
        let s = &self.monitor_sections;
        let chip = |id, glyph, label, on, f: &dyn Fn(&mut MonitorSettings)| {
            let mut next = s.clone();
            f(&mut next);
            rows.ctx
                .chip(id, glyph, label, on, Message::SetMonitor(next))
        };
        let signal = chip(
            "monitor-signal",
            icons::ACTIVITY,
            "Signal",
            s.signal,
            &|n| n.signal = !n.signal,
        );
        let spectrum = chip(
            "monitor-spectrum",
            icons::BARS,
            "Spectrum",
            s.spectrum,
            &|n| n.spectrum = !n.spectrum,
        );
        let beats = chip("monitor-beats", icons::ZAP, "Beats", s.beats, &|n| {
            n.beats = !n.beats;
        });
        let system = chip(
            "monitor-system",
            icons::GAUGE,
            "System",
            s.performance,
            &|n| n.performance = !n.performance,
        );
        if rows.width() >= FOUR_ACROSS {
            row![signal, spectrum, beats, system].spacing(8).into()
        } else {
            column![
                row![signal, spectrum].spacing(8),
                row![beats, system].spacing(8)
            ]
            .spacing(8)
            .into()
        }
    }

    fn monitor_signal(&self) -> Element<'_, Message> {
        let accent = self.accent();
        let t = &self.sections.sound;
        let m = &self.monitor;
        let gate = if t.gate_db <= GATE_OFF {
            "Off".to_string()
        } else if self.gate.is_open() {
            format!("Open (at {} dB)", signed(t.gate_db))
        } else {
            format!("Closed (at {} dB)", signed(t.gate_db))
        };
        let auto_gain = if t.auto_gain {
            format!("\u{00D7}{:.2}", self.processor.gain())
        } else {
            "Off".into()
        };
        let (full, mean) = m
            .heights()
            .map_or(("\u{2013}".into(), "\u{2013}".into()), |(f, a)| {
                (percent(f), percent(a))
            });
        let col = part("Signal")
            .push(graph(
                LevelGraph {
                    monitor: m,
                    tuning: t,
                    accent,
                },
                110.0,
            ))
            .push(legend(vec![
                ("\u{2501} Input", TEXT_2),
                ("\u{2501} Loudest bar", accent),
                ("\u{25AD} Level range", TEXT_3),
                ("\u{254C} Noise gate", TEXT),
            ]))
            .push(stat("Input", decibels(self.meter.input_db)))
            .push(stat("Loudest bar", decibels(self.meter.loudest_db)))
            .push(stat(
                "Level range",
                format!("{} to {} dB", signed(t.floor_db), signed(t.ceiling_db)),
            ))
            .push(stat("Noise gate", gate))
            .push(stat("Auto-gain", auto_gain))
            .push(stat("Bars at full height", full))
            .push(stat("Average bar height", mean));
        m.level_tips(t)
            .into_iter()
            .fold(col, |col, tip| col.push(tip_text(tip)))
            .into()
    }

    fn monitor_spectrum(&self) -> Element<'_, Message> {
        let accent = self.accent();
        let t = &self.sections.sound;
        let m = &self.monitor;
        let lo = slider_to_hz(t.freq_lo as f32);
        let hi = slider_to_hz(t.freq_hi as f32);
        let fft = if m.fft_size > 0 {
            format!(
                "{} \u{00B7} {:.1} Hz per bin \u{00B7} {:.0} ms",
                m.fft_size,
                m.sample_rate / m.fft_size as f32,
                m.fft_size as f32 / m.sample_rate * 1000.0
            )
        } else {
            "\u{2013}".into()
        };
        let scale = match t.freq_scale {
            FreqScale::Log => "log",
            FreqScale::Mel => "mel",
            FreqScale::Linear => "linear",
        };
        part("Spectrum")
            .push(graph(
                SpectrumGraph {
                    monitor: m,
                    tuning: t,
                    accent,
                },
                140.0,
            ))
            .push(legend(vec![
                ("\u{2501} Spectrum (with Tilt)", TEXT),
                ("\u{25AE} Bars", accent),
                ("\u{25AD} Frequency and level range", TEXT_3),
            ]))
            .push(stat("FFT", fft))
            .push(stat(
                "Bars cover",
                format!("{} \u{2013} {} ({scale})", format_hz(lo), format_hz(hi)),
            ))
            .push(stat("Tilt", format!("{} dB per octave", signed(t.tilt_db))))
            .into()
    }

    fn monitor_beats(&self) -> Element<'_, Message> {
        let accent = self.accent();
        let t = &self.sections.sound;
        let m = &self.monitor;
        let show_bg = self.bg_engine.pulse && self.sections.background.is_dynamic();
        let detector = match self.vis_engine.glow_source {
            GlowSource::Bass => "bass",
            GlowSource::Drums => "drums",
        };
        let mut legend_items = vec![("\u{2501} Glow pulse", accent)];
        if show_bg {
            legend_items.push(("\u{2501} Background pulse", TEXT_2));
        }
        legend_items.push(("\u{25CF} Hit", accent));
        legend_items.push(("\u{254C} Hit level", TEXT_3));
        let col = part("Beats")
            .push(graph(
                BeatGraph {
                    monitor: m,
                    show_bg,
                    accent,
                },
                72.0,
            ))
            .push(legend(legend_items))
            .push(stat(
                "Glow",
                if self.vis_engine.glow {
                    format!("On, follows the {detector}")
                } else {
                    format!("Off (shown here for the {detector})")
                },
            ))
            .push(stat("Beat sensitivity", format!("{}%", t.beat_sensitivity)))
            .push(stat(
                "Hits",
                format!("{} in {} s", m.hits(), monitor::HISTORY.as_secs()),
            ))
            .push(stat(
                "Tempo",
                m.bpm()
                    .map_or("\u{2013}".into(), |b| format!("about {} BPM", b.round())),
            ));
        m.beat_tips()
            .into_iter()
            .fold(col, |col, tip| col.push(tip_text(tip)))
            .into()
    }

    fn monitor_system(&self) -> Element<'_, Message> {
        let t = &self.sections.sound;
        let m = &self.monitor;
        let fps = m.fps().map_or("\u{2013}".into(), |f| format!("{f:.0} fps"));
        let cap = match self.sections.general.frame_rate {
            FrameRate::Fps30 => "capped at 30",
            FrameRate::Fps60 => "capped at 60",
            FrameRate::Display => "follows the display",
        };
        let frame_time = m.frame_times().map_or("\u{2013}".into(), |(avg, max)| {
            format!("{avg:.1} ms average, {max:.1} ms longest")
        });
        let audio = match self.audio_outage() {
            None if self.capture.is_some() => "Capturing".to_string(),
            None => "Starting".into(),
            Some(AudioOutage::Reconnecting) => "Reconnecting".into(),
            Some(AudioOutage::Failed(e)) => e.to_string(),
            Some(AudioOutage::Waiting(_)) => "Waiting for the app to play".into(),
        };
        let rate = self.capture.as_ref().map_or("\u{2013}".into(), |c| {
            format!("{:.1} kHz", c.sample_rate() as f32 / 1000.0)
        });
        let mut col = part("System")
            .push(stat("Frame rate", format!("{fps}, {cap}")))
            .push(stat("Frame time", frame_time))
            .push(stat(
                "Update",
                format!("{:.2} ms per frame", m.update_cost.as_secs_f32() * 1000.0),
            ))
            .push(stat("Source", self.source.label.clone()))
            .push(stat("Audio", audio))
            .push(stat("Sample rate", rate))
            .push(stat(
                "Visual delay",
                if t.delay_ms == 0 {
                    "Off".into()
                } else {
                    format!("{} ms", t.delay_ms)
                },
            ));
        if self.platform.media {
            let player = match &self.track {
                Some(tr) => format!(
                    "{}, {}",
                    if tr.player_name.is_empty() {
                        "Unknown"
                    } else {
                        &tr.player_name
                    },
                    if tr.playing { "playing" } else { "paused" }
                ),
                None => "None".into(),
            };
            let lyric_time = self.track.as_ref().map_or("\u{2013}".into(), |tr| {
                let pos = tr.position_at(self.now).as_secs_f32();
                let off = lyric_offset(self.sections.lyrics.offset_ms, pos, tr.length);
                format!(
                    "{} (offset {:+.2} s)",
                    format_time(pos + off),
                    self.sections.lyrics.offset_ms as f32 / 1000.0
                )
            });
            col = col
                .push(stat("Player", player))
                .push(stat("Lyrics", self.lyrics_status_text()))
                .push(stat("Lyrics time", lyric_time))
                .push(stat(
                    "Album art",
                    if self.art.is_some() { "Loaded" } else { "None" }.into(),
                ));
        }
        col.into()
    }
}

/// One part of the monitor, starting with its heading.
fn part<'a>(title: &'a str) -> Column<'a, Message> {
    column![text(title).font(INTER_SEMIBOLD).size(14).color(TEXT)].spacing(6)
}

/// A graph, in a sunken trough like the section's other controls.
fn graph<'a, P>(program: P, height: f32) -> Element<'a, Message>
where
    P: iced::widget::canvas::Program<Message> + 'a,
{
    container(Canvas::new(program).width(Length::Fill).height(height))
        .padding(10)
        .width(Length::Fill)
        .style(dark_trough(false))
        .into()
}

/// One reading: name on the left, value on the right.
fn stat<'a>(label: &'a str, value: String) -> Element<'a, Message> {
    row![
        text(label).size(13).color(TEXT_2).width(Length::Fill),
        text(value).size(13).color(TEXT),
    ]
    .spacing(12)
    .into()
}

/// What the lines in a graph are, in their colors.
fn legend<'a>(items: Vec<(&'a str, Color)>) -> Element<'a, Message> {
    let mut r = row![].spacing(12);
    for (label, color) in items {
        r = r.push(text(label).size(12).color(color));
    }
    r.wrap().vertical_spacing(4).into()
}

/// A suggestion under a part, styled like help text.
fn tip_text<'a>(tip: &'a str) -> Element<'a, Message> {
    text(tip).size(12).color(TEXT_3).into()
}

fn percent(x: f32) -> String {
    format!("{}%", (x * 100.0).round() as i32)
}

/// `-42 dB`, or a dash without a reading.
fn decibels(db: Option<f32>) -> String {
    match db {
        Some(db) => format!("{} dB", signed(db.round().clamp(-120.0, 20.0) as i8)),
        None => "\u{2013}".into(),
    }
}

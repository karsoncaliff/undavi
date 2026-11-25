//! The playback bar: the seek bar and the buttons around it, in each of its styles.

use super::{Ctx, cover_image, scrolled, tip};
use crate::app::{App, Message};
use crate::controls::{compact_slider, fading_slider};
use crate::focus::{FocusId, Step};
use crate::icons::{self, icon};
use crate::media::Track;
use crate::settings::{PlaybackLook, PlaybackSettings, PlaybackStyle, SeekHandle};
use crate::theme::*;
use crate::visualizer::{Seek, SeekLook};
use iced::widget::text::Wrapping;
use iced::widget::{
    Canvas, Space, button, center, column, container, mouse_area, row, text, tooltip,
};
use iced::{Alignment, Color, Element, Length, Padding};
use std::rc::Rc;
use std::time::Duration;

/// Space inside a Card or Dock.
const PAD: f32 = 16.0;
/// A Dock's seek bar doesn't get wider than this.
const DOCK_SEEK: f32 = 620.0;
/// Below these widths the track info is left out.
const INFO_ROOM: f32 = 640.0;
const INFO_ROOM_COMPACT: f32 = 820.0;
/// Below this, a Dock has everything in the middle, with a volume button rather than
/// a slider.
const DOCK_ROOM: f32 = 900.0;
/// Below these, the volume popover is a short one, takes the full-screen button's
/// place (in Open), and moves the buttons in the middle over.
const POPOVER_ROOM: f32 = 820.0;
const POPOVER_ROOM_OPEN: f32 = 940.0;
const POPOVER_ALONE: f32 = 710.0;
const POPOVER_MOVES: f32 = 600.0;

/// Sizes of the buttons: the icon, and the button around it.
#[derive(Clone, Copy)]
struct Sizes {
    play: (f32, f32),
    /// Previous and next.
    skip: (f32, f32),
    /// Repeat, volume and full screen.
    side: (f32, f32),
    fullscreen: (f32, f32),
}

/// Builds the bar's parts.
struct Bar<'a, 'b> {
    app: &'a App,
    ctx: &'b Ctx<'a>,
    settings: &'a PlaybackSettings,
    look: PlaybackLook,
    sizes: Sizes,
    /// What's playing, if there's a connection to the player.
    track: Option<&'a Track>,
    /// How much of it shows (auto-hide).
    alpha: f32,
    /// The room it has.
    width: f32,
}

impl App {
    /// The playback bar with `width` to fill.
    pub(super) fn playback_bar<'a>(&'a self, ctx: &Ctx<'a>, width: f32) -> Element<'a, Message> {
        let settings = &self.sections.playback.settings;
        let bar = Bar {
            app: self,
            ctx,
            settings,
            look: settings.look(),
            sizes: sizes(settings),
            track: self.track.as_ref().filter(|_| self.platform.media),
            alpha: self.ui_alpha,
            width: match settings.style {
                PlaybackStyle::Dock => width,
                _ => width.min(f32::from(settings.width)),
            },
        };
        // Without a music player connection there's nothing to play, seek or turn
        // up; only the full-screen button is left.
        if !self.platform.media {
            return container(bar.fullscreen())
                .align_right(Length::Fill)
                .max_width(1000)
                .into();
        }
        match (settings.style, settings.compact) {
            (PlaybackStyle::Open, false) => bar.open(),
            (PlaybackStyle::Open, true) => bar.one_row(false),
            (PlaybackStyle::Card, false) => bar.card(),
            (PlaybackStyle::Dock, false) => bar.dock(),
            (_, true) => bar.one_row(true),
        }
    }

    /// The same on its own, for the settings' preview.
    pub(super) fn playback_part(&self, width: f32) -> Element<'_, Message> {
        let ctx = Ctx::new(self, crate::focus::Scope::Main);
        ctx.still.set(true);
        container(self.playback_bar(&ctx, width))
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into()
    }

    /// Whether it's a strip along the bottom of the window, rather than below the bars.
    pub(super) fn docked(&self) -> bool {
        self.platform.media && self.sections.playback.style == PlaybackStyle::Dock
    }

    /// How tall it is below the bars, for a panel that floats over them.
    pub(super) fn playback_height(&self) -> f32 {
        let s = &self.sections.playback.settings;
        let sizes = sizes(s);
        match s.style {
            _ if !self.platform.media => sizes.fullscreen.1,
            PlaybackStyle::Dock => 0.0,
            PlaybackStyle::Open if s.compact => sizes.play.1.max(seek_height(s)),
            PlaybackStyle::Open => seek_height(s) + 20.0 + sizes.play.1,
            PlaybackStyle::Card if s.compact => 2.0 * 8.0 + sizes.play.1.max(seek_height(s)),
            PlaybackStyle::Card => 2.0 * 12.0 + sizes.play.1 + 6.0 + seek_height(s),
        }
    }
}

fn sizes(s: &PlaybackSettings) -> Sizes {
    let filled = s.look().filled_play;
    let play = |plain: (f32, f32), circle: (f32, f32)| if filled { circle } else { plain };
    match (s.style, s.compact) {
        (PlaybackStyle::Open, false) => Sizes {
            play: play((46.0, 56.0), (32.0, 56.0)),
            skip: (28.0, 48.0),
            side: (44.0, 56.0),
            fullscreen: (26.0, 48.0),
        },
        (PlaybackStyle::Open, true) => Sizes {
            play: play((36.0, 44.0), (24.0, 44.0)),
            skip: (24.0, 40.0),
            side: (24.0, 40.0),
            fullscreen: (22.0, 40.0),
        },
        (_, false) => Sizes {
            play: play((38.0, 46.0), (26.0, 46.0)),
            skip: (26.0, 44.0),
            side: (22.0, 40.0),
            fullscreen: (22.0, 40.0),
        },
        (_, true) => Sizes {
            play: play((32.0, 40.0), (22.0, 40.0)),
            skip: (24.0, 38.0),
            side: (20.0, 36.0),
            fullscreen: (20.0, 36.0),
        },
    }
}

/// Tall enough for the curve, the line and the handle.
fn seek_height(s: &PlaybackSettings) -> f32 {
    let look = s.look();
    let curve = 2.0 * f32::from(look.curve_height) + f32::from(look.thickness) + 2.0;
    let handle = match s.handle {
        SeekHandle::None => 0.0,
        _ => look.handle.1 + 4.0,
    };
    curve.max(handle).max(24.0)
}

impl<'a> Bar<'a, '_> {
    // -----------------------------------------------------------------------
    // The styles

    /// The seek bar, with the buttons under it.
    fn open(&self) -> Element<'a, Message> {
        let transport = row![
            container(row![].extend(self.repeat())).width(self.left()),
            self.controls(12.0),
            container(self.right(false)).align_right(Length::Fill),
        ]
        .align_y(Alignment::Center);
        column![self.seek(), transport]
            .spacing(20)
            .max_width(self.width)
            .into()
    }

    /// The track info and the buttons, with the seek bar under them.
    fn card(&self) -> Element<'a, Message> {
        let info = self.info(42.0, self.width >= INFO_ROOM);
        let top = row![
            container(row![].extend(info)).width(self.left()).clip(true),
            self.controls(4.0),
            container(self.right(true)).align_right(Length::Fill),
        ]
        .spacing(12)
        .align_y(Alignment::Center);
        let content = column![top, self.seek()].spacing(6);
        self.boxed(content.into(), 12.0)
    }

    /// The track info, the buttons over the seek bar, and the volume.
    fn dock(&self) -> Element<'a, Message> {
        let roomy = self.width >= DOCK_ROOM;
        let mut buttons = row![].spacing(8).align_y(Alignment::Center);
        if roomy {
            buttons = buttons
                .extend(self.repeat())
                .push(self.controls(4.0))
                .extend(self.settings.fullscreen.then(|| self.fullscreen()));
        } else if self.popover_open() {
            buttons = buttons
                .push(self.controls(4.0))
                .extend(self.popover())
                .extend(self.volume());
        } else {
            buttons = buttons
                .extend(self.repeat())
                .push(self.controls(4.0))
                .extend(self.volume())
                .extend(self.settings.fullscreen.then(|| self.fullscreen()));
        }
        let middle = column![buttons, self.seek()]
            .align_x(Alignment::Center)
            .max_width(DOCK_SEEK);
        if !roomy {
            return self.boxed(container(middle).center_x(Length::Fill).into(), 10.0);
        }
        // Two equal sides keep the middle centered.
        let side = Length::FillPortion(3);
        let r = row![
            container(row![].extend(self.info(52.0, true)))
                .width(side)
                .clip(true),
            container(middle).center_x(Length::FillPortion(6)),
            container(row![].extend(self.volume_slider())).align_right(side),
        ]
        .spacing(20)
        .align_y(Alignment::Center);
        self.boxed(r.into(), 10.0)
    }

    /// Everything in one row.
    fn one_row(&self, boxed: bool) -> Element<'a, Message> {
        let dock = self.settings.style == PlaybackStyle::Dock && self.width >= DOCK_ROOM;
        let room = if boxed { INFO_ROOM_COMPACT } else { f32::MAX };
        let mut r = row![].spacing(14).align_y(Alignment::Center);
        if let Some(info) = self.info(if dock { 40.0 } else { 36.0 }, self.width >= room) {
            r = r.push(container(info).max_width(240).clip(true));
        }
        r = r.push(self.controls(2.0)).push(self.seek());
        r = if dock {
            let sides = row![]
                .extend(self.repeat())
                .extend(self.settings.fullscreen.then(|| self.fullscreen()))
                .align_y(Alignment::Center);
            r.push(sides).extend(self.volume_slider())
        } else {
            r.push(self.right(true))
        };
        if boxed {
            self.boxed(r.into(), 8.0)
        } else {
            container(r).max_width(self.width).into()
        }
    }

    /// The fill behind a Card or Dock.
    fn boxed(&self, content: Element<'a, Message>, pad: f32) -> Element<'a, Message> {
        let opacity = f32::from(self.settings.opacity) / 100.0;
        let alpha = self.alpha;
        match self.settings.style {
            PlaybackStyle::Dock => {
                let line = container(Space::new().height(1))
                    .width(Length::Fill)
                    .style(fill(with_alpha(OUTLINE, opacity * alpha)));
                let body = container(content)
                    .padding([pad, 22.0_f32.min(self.width / 30.0)])
                    .width(Length::Fill)
                    .style(fill(with_alpha(DOCK, opacity * alpha)));
                column![line, body].into()
            }
            _ => container(content)
                .padding([pad, PAD])
                .width(Length::Fill)
                .max_width(self.width)
                .style(playback_card(opacity, alpha))
                .into(),
        }
    }

    // -----------------------------------------------------------------------
    // The parts

    /// The cover, title and artist, in a Card or Dock with room for them.
    fn info(&self, cover: f32, room: bool) -> Option<Element<'a, Message>> {
        if !self.settings.track_info || self.settings.style == PlaybackStyle::Open || !room {
            return None;
        }
        let alpha = self.alpha;
        let line = |s: String, font, size: f32, color: Color| {
            text(s)
                .font(font)
                .size(size)
                .color(with_alpha(color, alpha))
                .wrapping(Wrapping::None)
        };
        let Some(t) = self.track else {
            return Some(line("Nothing playing".into(), INTER, 13.0, TEXT_3).into());
        };
        let mut lines = column![line(t.title.clone(), INTER_SEMIBOLD, 14.0, TEXT)].spacing(2);
        if !t.artist.is_empty() {
            lines = lines.push(line(t.artist.clone(), INTER, 13.0, TEXT_2));
        }
        let art = self
            .app
            .art
            .as_ref()
            .map(|a| cover_image(&a.thumb, cover, 8.0, 10.0, 4.0, 0.3 * alpha, alpha));
        Some(
            row![]
                .extend(art)
                .push(lines)
                .spacing(12)
                .align_y(Alignment::Center)
                .into(),
        )
    }

    /// The seek bar.
    fn seek(&self) -> Element<'a, Message> {
        let seekable = self
            .track
            .filter(|n| n.can_seek)
            .and_then(|n| Some((n.position_at(self.app.now), n.length?)));
        let step = self.app.seek_step();
        let focused = match seekable {
            Some((pos, len)) => self.ctx.focus(
                FocusId::Named("seek"),
                None,
                Some(Rc::new(move |s: Step| {
                    let big = step * 3;
                    let target = match s {
                        Step::Less => pos.saturating_sub(step),
                        Step::More => pos + step,
                        Step::MuchLess => pos.saturating_sub(big),
                        Step::MuchMore => pos + big,
                        Step::Min => Duration::ZERO,
                        Step::Max => len,
                    }
                    .min(len);
                    Some(Message::Seek(target.as_secs_f32() / len.as_secs_f32()))
                })),
            ),
            None => false,
        };
        Canvas::new(Seek {
            time: seekable.map(|(pos, len)| (pos.as_secs_f32(), len.as_secs_f32())),
            phase: self.app.wave_phase,
            accent: self.ctx.accent,
            alpha: self.alpha,
            focused,
            look: SeekLook {
                amplitude: self.look.curve_height.into(),
                wavelength: self.look.curve_length.into(),
                stroke: self.look.thickness.into(),
                handle: self.settings.handle,
                handle_size: self.look.handle,
                remaining: f32::from(self.look.remaining) / 100.0,
                times: self.settings.times,
            },
            on_seek: Message::Seek,
        })
        .width(Length::Fill)
        .height(seek_height(self.settings))
        .into()
    }

    /// Previous, play and next.
    fn controls(&self, spacing: f32) -> Element<'a, Message> {
        let on = self.track.is_some();
        let playing = self.track.is_some_and(|n| n.playing);
        let color = if on { TEXT } else { ICON_OFF };
        row![
            self.button(
                "previous",
                icons::PREV,
                "Previous track",
                self.sizes.skip,
                color,
                on.then_some(Message::Previous)
            ),
            self.play(playing, on),
            self.button(
                "next",
                icons::NEXT,
                "Next track",
                self.sizes.skip,
                color,
                on.then_some(Message::Next)
            ),
        ]
        .spacing(spacing)
        .align_y(Alignment::Center)
        .into()
    }

    fn play(&self, playing: bool, on: bool) -> Element<'a, Message> {
        let (glyph, name) = if playing {
            (icons::PAUSE, "Pause")
        } else {
            (icons::PLAY, "Play")
        };
        let msg = on.then_some(Message::PlayPause);
        if !self.look.filled_play {
            let color = if on { TEXT } else { ICON_OFF };
            return self.button("play", glyph, name, self.sizes.play, color, msg);
        }
        let (size, around) = self.sizes.play;
        let focused = self.ctx.button_stop(FocusId::Named("play"), msg.as_ref());
        tip(
            button(center(icon(glyph, size, with_alpha(BG, self.alpha))))
                .width(around)
                .height(around)
                .padding(0)
                .style(ringed(focused, play_circle(around / 2.0, self.alpha)))
                .on_press_maybe(msg),
            name,
            tooltip::Position::Top,
        )
    }

    fn repeat(&self) -> Option<Element<'a, Message>> {
        let on = self.track.is_some_and(|n| n.repeat);
        self.settings.repeat.then(|| {
            self.button(
                "repeat",
                icons::REPEAT,
                if on { "Repeat: on" } else { "Repeat: off" },
                self.sizes.side,
                if on { TEXT } else { ICON_OFF },
                self.track.is_some().then_some(Message::ToggleRepeat),
            )
        })
    }

    fn fullscreen(&self) -> Element<'a, Message> {
        let full = self.app.fullscreen;
        self.button(
            "fullscreen",
            if full {
                icons::EXIT_FULLSCREEN
            } else {
                icons::FULLSCREEN
            },
            if full {
                "Exit full screen (F11)"
            } else {
                "Full screen (F11)"
            },
            self.sizes.fullscreen,
            TEXT,
            Some(Message::ToggleFullscreen),
        )
    }

    /// The width of what's left of the buttons in the middle: as much as right of them,
    /// unless the volume popover needs the room.
    fn left(&self) -> Length {
        if self.popover_open() && self.width < POPOVER_MOVES {
            Length::Shrink
        } else {
            Length::Fill
        }
    }

    /// The buttons on the right: with `all`, repeat too, and the volume before full
    /// screen.
    fn right(&self, all: bool) -> Element<'a, Message> {
        let mut r = row![].align_y(Alignment::Center);
        let open = self.popover_open();
        if all {
            // The popover takes the other buttons' place.
            if !open {
                r = r.extend(self.repeat());
            }
            r = r.extend(self.popover()).extend(self.volume());
            if !open {
                r = r.extend(self.settings.fullscreen.then(|| self.fullscreen()));
            }
        } else {
            let fullscreen = self.settings.fullscreen && (!open || self.width >= POPOVER_ALONE);
            r = r
                .spacing(8)
                .extend(fullscreen.then(|| self.fullscreen()))
                .extend(self.popover())
                .extend(self.volume());
        }
        r.into()
    }

    /// The volume button, which scrolls the volume and opens the popover.
    fn volume(&self) -> Option<Element<'a, Message>> {
        if !self.settings.volume {
            return None;
        }
        let volume = self.track.and_then(|n| n.volume);
        let glyph = if volume == Some(0.0) {
            icons::VOLUME_MUTED
        } else {
            icons::VOLUME
        };
        let color = if volume.is_some() { TEXT } else { ICON_OFF };
        let msg = volume.is_some().then_some(Message::ToggleVolume);
        let b = self.button("volume", glyph, "Volume", self.sizes.side, color, msg);
        Some(scrolls_volume(b))
    }

    fn popover_open(&self) -> bool {
        self.app.volume_open
            && self.settings.volume
            && self.alpha > 0.0
            && !self.ctx.still.get()
            && self.track.is_some_and(|n| n.volume.is_some())
    }

    fn popover(&self) -> Option<Element<'a, Message>> {
        let volume = self.track.and_then(|n| n.volume)?;
        self.popover_open()
            .then(|| self.volume_popover(volume as f32))
    }

    /// A Dock's volume, where there's room for a slider.
    fn volume_slider(&self) -> Option<Element<'a, Message>> {
        if !self.settings.volume {
            return None;
        }
        let volume = self.track.and_then(|n| n.volume).map(|v| v as f32);
        let level = volume.unwrap_or(0.0);
        let mute = self.button(
            "volume",
            if volume == Some(0.0) {
                icons::VOLUME_MUTED
            } else {
                icons::VOLUME
            },
            if volume == Some(0.0) {
                "Unmute"
            } else {
                "Mute"
            },
            self.sizes.side,
            if volume.is_some() { TEXT_2 } else { ICON_OFF },
            volume.is_some().then_some(Message::ToggleMute),
        );
        let range = (0.0, 1.0, 0.01);
        let track: Element<'a, Message> = if volume.is_some() {
            let focused = self.ctx.focus(
                FocusId::Named("volume-slider"),
                None,
                Some(Rc::new(move |s: Step| {
                    Some(Message::SetVolume(s.apply(level, range)))
                })),
            );
            fading_slider(level, range, TEXT, self.alpha, focused, Message::SetVolume)
                .width(150)
                .into()
        } else {
            // Nothing to turn up.
            Space::new().width(150).into()
        };
        let both = row![mute, track].align_y(Alignment::Center);
        Some(scrolls_volume(both.into()))
    }

    /// Mute toggle, volume slider and percentage, opened from the volume button.
    fn volume_popover(&self, volume: f32) -> Element<'a, Message> {
        let ctx = self.ctx;
        let open = self.settings.style == PlaybackStyle::Open;
        let short = self.width
            < if open {
                POPOVER_ROOM_OPEN
            } else {
                POPOVER_ROOM
            };
        let muted = volume <= 0.0;
        let mute_msg = Message::ToggleMute;
        let mute_focused = ctx.focus(FocusId::Named("mute"), Some(mute_msg.clone()), None);
        let mute = tip(
            button(center(icon(
                if muted {
                    icons::VOLUME_MUTED
                } else {
                    icons::VOLUME
                },
                20.0,
                TEXT,
            )))
            .width(40)
            .height(40)
            .padding(0)
            .style(ringed(mute_focused, ghost_button(20.0, 1.0)))
            .on_press(mute_msg),
            if muted { "Unmute" } else { "Mute" },
            tooltip::Position::Top,
        );
        let range = (0.0, 1.0, 0.01);
        let slider_focused = ctx.focus(
            FocusId::Named("volume-slider"),
            None,
            Some(Rc::new(move |s: Step| {
                Some(Message::SetVolume(s.apply(volume, range)))
            })),
        );
        let level = compact_slider(
            volume,
            range,
            ctx.accent,
            slider_focused,
            Message::SetVolume,
        )
        .width(if short { 90 } else { 150 });
        let percent = (!short).then(|| {
            text(format!("{}%", (volume * 100.0).round() as u32))
                .size(13)
                .color(TEXT_2)
                .width(40)
                .align_x(iced::alignment::Horizontal::Right)
                .into()
        });
        let height = if self.settings.compact || self.settings.style != PlaybackStyle::Open {
            44.0
        } else {
            52.0
        };
        container(
            row![mute, level]
                .extend(percent)
                .spacing(6)
                .align_y(Alignment::Center),
        )
        .padding(Padding {
            top: 0.0,
            right: if short { 8.0 } else { 16.0 },
            bottom: 0.0,
            left: 6.0,
        })
        .height(height)
        .center_y(height)
        .style(popover)
        .into()
    }

    fn button(
        &self,
        id: &'static str,
        glyph: &'static str,
        name: &'static str,
        (size, around): (f32, f32),
        color: Color,
        msg: Option<Message>,
    ) -> Element<'a, Message> {
        let alpha = self.alpha;
        let focused = self.ctx.button_stop(FocusId::Named(id), msg.as_ref());
        tip(
            button(center(icon(glyph, size, with_alpha(color, alpha))))
                .width(around)
                .height(around)
                .padding(0)
                .style(ringed(focused, ghost_button(around / 2.0, alpha)))
                .on_press_maybe(msg),
            name,
            tooltip::Position::Top,
        )
    }
}

/// Scrolling over it changes the volume.
fn scrolls_volume<'a>(content: Element<'a, Message>) -> Element<'a, Message> {
    mouse_area(content)
        .on_scroll(|delta| Message::VolumeScroll(scrolled(delta)))
        .into()
}

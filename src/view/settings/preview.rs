//! The settings window's right column: the live preview. It's the real screen,
//! drawn small (`scaled.rs`), or only the part of it the section affects.

use crate::app::settings::{Part, SettingsMessage, Shape};
use crate::app::{App, AudioOutage, Message};
use crate::backdrop::Place;
use crate::controls::switch;
use crate::focus::{FocusId, Scope, Step};
use crate::icons::{self, icon};
use crate::karaoke::Placement;
use crate::layout::SettingsLayout;
use crate::settings::NowPlayingMode;
use crate::theme::*;
use crate::view::scaled::{Fit, Frame as ScaledFrame, scaled};
use crate::view::{Ctx, tip};
use iced::mouse::Cursor;
use iced::widget::canvas::{self, Frame, Geometry, Path};
use iced::widget::text::Wrapping;
use iced::widget::{
    Canvas, Space, button, center, column, container, mouse_area, row, text, tooltip,
};
use iced::{
    Alignment, Color, Element, Length, Padding, Point, Radians, Rectangle, Renderer, Theme, Vector,
};
use std::f32::consts::{FRAC_PI_2, TAU};
use std::rc::Rc;

/// Space around everything in the column.
const PAD: f32 = 16.0;
/// Below this, "Peek at full size" is only an icon.
const ROOMY: f32 = 400.0;
/// Seconds between the Live light's pulses.
const PULSE_S: f32 = 1.8;
/// The size the visualizer is laid out at when it's shown on its own: narrower
/// than the stage, so it fills the preview.
const VISUALIZER_WIDTH: f32 = 900.0;
const BACKGROUND_WIDTH: f32 = 900.0;
const LYRICS_WIDTH: f32 = 520.0;
const PLAYBACK_WIDTH: f32 = 760.0;

impl App {
    pub(super) fn settings_preview(&self, width: f32) -> Element<'_, Message> {
        let ctx = Ctx::new(self, Scope::Preview);
        let part = self.settings.focused_part();

        let header = row![
            ctx.plain_icon_button(
                FocusId::Named("hide-preview"),
                icons::HIDE_COLUMN,
                "Hide preview",
                Some(SettingsMessage::TogglePreview.into()),
                tooltip::Position::Bottom,
            ),
            text("Preview").font(INTER_SEMIBOLD).size(16).color(TEXT),
            self.live_light(),
            Space::new().width(Length::Fill),
            self.close_settings_button(&ctx),
        ]
        .spacing(10)
        .align_y(Alignment::Center);

        let mut col = column![header].spacing(14);
        if part.is_none() {
            col = col.push(self.shape_switch(&ctx));
        }
        let (view, caption): (Element<'_, Message>, String) = match part {
            Some(part) => {
                let (view, caption) = self.preview_part(part);
                let empty = || Space::new().width(Length::Fill).height(Length::Fill).into();
                (view.unwrap_or_else(empty), caption)
            }
            None => {
                let shape = self.settings.shape;
                let view = scaled(self.still_screen(shape.size()), Fit::Whole(shape.size())).frame(
                    ScaledFrame {
                        radius: 10.0,
                        corners: SIDE,
                        outline: OUTLINE,
                    },
                );
                (view.into(), shape.caption().into())
            }
        };
        let view = container(view).width(Length::Fill).height(Length::Fill);
        let caption = text(caption).size(13).color(TEXT_2);
        let bottom = self.preview_bottom(&ctx, width);
        container(col.push(view).push(caption).push(bottom))
            .width(width)
            .height(Length::Fill)
            .padding(PAD)
            .style(settings_side(false))
            .into()
    }

    /// The part of the screen a section affects, filling the preview, and what to
    /// say under it. Nothing to show: why.
    fn preview_part(&self, part: Part) -> (Option<Element<'_, Message>>, String) {
        let layout = self.preview_screen(Shape::Window.size()).layout;
        match part {
            Part::Visualizer => {
                let caption = match self.audio_outage() {
                    Some(AudioOutage::Waiting(_)) => "The visualizer, waiting for sound.",
                    Some(_) => "The visualizer, without any audio.",
                    None => "The visualizer, on its own.",
                };
                let view = scaled(self.visualizer(Place::Preview), Fit::Fill(VISUALIZER_WIDTH));
                (Some(view.into()), caption.into())
            }
            Part::Background => {
                let view = scaled(self.background(Place::Preview), Fit::Fill(BACKGROUND_WIDTH))
                    .frame(ScaledFrame {
                        radius: 10.0,
                        corners: SIDE,
                        outline: Color::TRANSPARENT,
                    });
                (Some(view.into()), "The background, on its own.".into())
            }
            Part::TrackInfo => {
                let mode = self.sections.now_playing.mode;
                if mode == NowPlayingMode::Off {
                    return (None, "Track info is off.".into());
                }
                if self.track.is_none() {
                    return (None, "Nothing is playing, so there's no track info.".into());
                }
                let (info, max_scale) = match mode {
                    NowPlayingMode::Spotlight => (self.spotlight_info(), 2.0),
                    _ => (Some(self.header_info(&layout)), 1.8),
                };
                let view = info.map(|info| {
                    scaled(
                        info,
                        Fit::Natural {
                            max_width: 440.0,
                            max_scale,
                        },
                    )
                    .into()
                });
                (view, "The track info, on its own.".into())
            }
            Part::Playback => {
                let bar = scaled(
                    self.playback_part(PLAYBACK_WIDTH),
                    Fit::Fill(PLAYBACK_WIDTH),
                );
                (Some(bar.into()), "The playback bar, on its own.".into())
            }
            Part::Lyrics => {
                if !self.sections.lyrics.show {
                    return (None, "Lyrics are off.".into());
                }
                let placement = match layout.now_playing {
                    NowPlayingMode::Spotlight => Placement::Column,
                    _ => Placement::Strip,
                };
                // The lines fill the preview, rather than the strip's few.
                let view = self.lyrics_canvas(placement, 1.0).map(|(lyrics, _)| {
                    scaled(lyrics.height(Length::Fill), Fit::Fill(LYRICS_WIDTH)).into()
                });
                let caption = if view.is_some() {
                    "The lyrics, on their own.".into()
                } else {
                    self.lyrics_status_text()
                };
                (view, caption)
            }
        }
    }

    /// The collapsed preview.
    pub(super) fn preview_strip(&self, layout: &SettingsLayout) -> Element<'_, Message> {
        let ctx = Ctx::new(self, Scope::Preview);
        let close = self.close_settings_button(&ctx);
        // Without room to show it, there's still Peek.
        let show = if layout.preview_fits {
            ctx.plain_icon_button(
                FocusId::Named("show-preview"),
                icons::SHOW_COLUMN,
                "Show preview",
                Some(SettingsMessage::TogglePreview.into()),
                tooltip::Position::Left,
            )
        } else {
            ctx.plain_icon_button(
                FocusId::Named("peek"),
                icons::EYE,
                "Peek at full size",
                Some(SettingsMessage::Peek.into()),
                tooltip::Position::Left,
            )
        };
        let label = Canvas::new(Vertical("Preview")).width(44).height(80);
        container(
            column![close, show, label]
                .spacing(8)
                .align_x(Alignment::Center),
        )
        .width(layout.strip)
        .height(Length::Fill)
        .padding(Padding {
            top: PAD,
            right: 0.0,
            bottom: PAD,
            left: (layout.strip - 44.0) / 2.0,
        })
        .style(settings_side(false))
        .into()
    }

    fn close_settings_button<'a>(&'a self, ctx: &Ctx<'a>) -> Element<'a, Message> {
        ctx.plain_icon_button(
            FocusId::Named("close-settings"),
            icons::CLOSE,
            "Close settings (Esc)",
            Some(SettingsMessage::Done.into()),
            tooltip::Position::Bottom,
        )
    }

    /// A pulsing dot and "Live".
    fn live_light(&self) -> Element<'_, Message> {
        let pulse = if self.reduced_motion.on() {
            1.0
        } else {
            let t = self.clock() / PULSE_S;
            0.65 + 0.35 * (t * TAU).cos()
        };
        row![
            Canvas::new(Light {
                color: self.accent(),
                pulse,
            })
            .width(8)
            .height(8),
            text("Live").font(INTER_MEDIUM).size(13).color(TEXT_2),
        ]
        .spacing(7)
        .align_y(Alignment::Center)
        .into()
    }

    fn shape_switch<'a>(&'a self, ctx: &Ctx<'a>) -> Element<'a, Message> {
        let current = self.settings.shape;
        let index = Shape::ALL.iter().position(|s| *s == current).unwrap_or(0);
        let focused = ctx.focus(
            FocusId::Named("window-shape"),
            None,
            Some(Rc::new(move |s: Step| {
                let i = s.apply_index(index, Shape::ALL.len());
                Some(SettingsMessage::Shape(Shape::ALL[i]).into())
            })),
        );
        let segments = Shape::ALL.into_iter().fold(row![].spacing(2), |r, shape| {
            r.push(
                button(center(
                    text(shape.name())
                        .font(INTER_MEDIUM)
                        .size(13)
                        .wrapping(Wrapping::None),
                ))
                .width(Length::Fill)
                .height(44)
                .padding(0)
                .style(dark_segment(shape == current))
                .on_press(SettingsMessage::Shape(shape).into()),
            )
        });
        column![
            text("Window shape")
                .font(INTER_MEDIUM)
                .size(13)
                .color(TEXT_2),
            container(segments)
                .padding(2)
                .width(Length::Fill)
                .style(dark_trough(focused)),
        ]
        .spacing(8)
        .into()
    }

    /// The focus switch (for sections that only affect part of the screen) and Peek.
    fn preview_bottom<'a>(&'a self, ctx: &Ctx<'a>, width: f32) -> Element<'a, Message> {
        let mut r = row![].spacing(12).align_y(Alignment::Center);
        if let Some(part) = Part::of(self.settings.page) {
            let msg = Message::from(SettingsMessage::ToggleOnly);
            let focused = ctx.focus(FocusId::Named("only"), Some(msg.clone()), None);
            r = r.push(switch(self.settings.only, ctx.accent, focused, msg.clone()));
            r = r.push(
                mouse_area(
                    container(
                        text(part.label())
                            .font(INTER_MEDIUM)
                            .size(14)
                            .color(TEXT)
                            .wrapping(Wrapping::None),
                    )
                    .center_y(44),
                )
                .interaction(iced::mouse::Interaction::Pointer)
                .on_press(msg),
            );
        }
        r = r.push(Space::new().width(Length::Fill));
        let msg = Message::from(SettingsMessage::Peek);
        let peek: Element<'a, Message> = if width >= ROOMY {
            let focused = ctx.focus(FocusId::Named("peek"), Some(msg.clone()), None);
            button(
                container(
                    row![
                        icon(icons::EYE, 16.0, TEXT),
                        text("Peek at full size")
                            .font(INTER_MEDIUM)
                            .size(14)
                            .wrapping(Wrapping::None),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                )
                .center_y(Length::Fill),
            )
            .height(44)
            .padding([0, 14])
            .style(ringed(focused, raised_button(RAISED)))
            .on_press(msg)
            .into()
        } else {
            ctx.icon_button_with(
                FocusId::Named("peek"),
                icons::EYE,
                "Peek at full size",
                Some(msg),
                tooltip::Position::Top,
            )
        };
        r.push(peek).into()
    }

    /// The floating button over the app while peeking.
    pub(in crate::view) fn back_to_settings(&self) -> Element<'_, Message> {
        let ctx = Ctx::new(self, Scope::Main);
        let msg = Message::from(SettingsMessage::EndPeek);
        let focused = ctx.focus(FocusId::Named("end-peek"), Some(msg.clone()), None);
        let back = button(
            container(
                row![
                    icon(icons::ARROW_LEFT, 18.0, TEXT),
                    text("Back to settings")
                        .font(INTER_MEDIUM)
                        .size(14)
                        .wrapping(Wrapping::None),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            )
            .center_y(Length::Fill),
        )
        .height(44)
        .padding([0, 18])
        .style(ringed(focused, floating_button))
        .on_press(msg);
        // Below the header, so it doesn't cover the track info.
        container(tip(back, "Esc", tooltip::Position::Bottom))
            .center_x(Length::Fill)
            .padding(Padding::default().top(104))
            .into()
    }

    /// Seconds since some moment, for animations that don't follow the music.
    fn clock(&self) -> f32 {
        self.now
            .saturating_duration_since(self.started)
            .as_secs_f32()
    }
}

/// The Live light.
struct Light {
    color: Color,
    /// 0..=1.
    pulse: f32,
}

impl<M> canvas::Program<M> for Light {
    type State = ();

    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let c = Point::new(bounds.width / 2.0, bounds.height / 2.0);
        frame.fill(
            &Path::circle(c, bounds.width / 2.0),
            with_alpha(self.color, self.pulse),
        );
        vec![frame.into_geometry()]
    }
}

/// Text written downwards.
struct Vertical(&'static str);

impl<M> canvas::Program<M> for Vertical {
    type State = ();

    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        frame.translate(Vector::new(bounds.width / 2.0, 8.0));
        frame.rotate(Radians(FRAC_PI_2));
        frame.fill_text(canvas::Text {
            content: self.0.into(),
            position: Point::ORIGIN,
            color: TEXT_2,
            size: 13.0.into(),
            font: INTER_MEDIUM,
            align_x: iced::widget::text::Alignment::Left,
            align_y: iced::alignment::Vertical::Center,
            ..canvas::Text::default()
        });
        vec![frame.into_geometry()]
    }
}

//! Layout and widgets: the header, stage, About panel, picker and update pop-up. The
//! settings window is in `settings/`.
//!
//! Every focusable control is built through [`Ctx`], which registers it with the
//! keyboard-focus model (`focus.rs`) while drawing it, so Tab order always
//! matches what's on screen.

use crate::about;
use crate::app::settings::SettingsMessage;
use crate::app::{ART_FADE, App, AudioOutage, Message, PANEL_SCROLL, SPOTLIGHT_WIDTH, UpdateState};
use crate::audio::SourceKind;
use crate::backdrop::Place;
use crate::background::Background;
use crate::focus::{Entry, FocusId, Scope, Stepper};
use crate::icons::{self, icon};
use crate::karaoke::Placement;
use crate::layout::{HEADER_SPACING, INFO_MAX, Layout, REFRESH};
use crate::settings::{BarsScroll, GradientDir, NowPlayingMode};
use crate::theme::{self, *};
use crate::update::Install;
use crate::visualizer::Visualizer;
use iced::widget::image::Handle;
use iced::widget::text::Wrapping;
use iced::widget::{
    Shader, Space, button, center, column, container, image, mouse_area, opaque, row, scrollable,
    stack, svg, text, tooltip,
};
use iced::{Alignment, Color, Element, Length, Padding, Radians, Size, mouse};
use std::cell::Cell;
use std::f32::consts::TAU;

mod playback;
mod quick;
mod scaled;
mod settings;

/// Height of the wordmark in the header (including its one-unit padding, so the
/// letters themselves are 30 px tall).
const LOGO_HEIGHT: f32 = 36.0;

// ---------------------------------------------------------------------------
// Focus-aware widget helpers

/// Builds focusable controls for one area of the UI and registers them for Tab.
struct Ctx<'a> {
    app: &'a App,
    scope: Scope,
    accent: Color,
    /// Only a picture (the mini preview), or folded away: nothing in it takes focus.
    still: Cell<bool>,
}

impl<'a> Ctx<'a> {
    fn new(app: &'a App, scope: Scope) -> Self {
        Self {
            app,
            scope,
            accent: app.accent(),
            still: Cell::new(false),
        }
    }

    /// For the screen: the app itself, or a picture of it.
    fn for_screen(app: &'a App, screen: &Screen) -> Self {
        Self {
            still: Cell::new(screen.place == Place::Preview),
            ..Self::new(app, Scope::Main)
        }
    }

    /// Register a focus stop; returns whether it's the focused one.
    fn focus(
        &self,
        id: FocusId,
        activate: Option<Message>,
        step: Option<Stepper<Message>>,
    ) -> bool {
        if self.still.get() {
            return false;
        }
        let focused = self.app.focus.as_ref() == Some(&id);
        self.app.focus_reg.push(Entry {
            id,
            scope: self.scope,
            activate,
            step,
            text_input: None,
        });
        focused
    }

    fn text_input_stop(&self, id: FocusId, widget: &'static str) {
        if self.still.get() {
            return;
        }
        self.app.focus_reg.push(Entry {
            id,
            scope: self.scope,
            activate: None,
            step: None,
            text_input: Some(widget),
        });
    }

    /// A focusable button stop (Enter/Space sends `msg`).
    fn button_stop(&self, id: FocusId, msg: Option<&Message>) -> bool {
        match msg {
            Some(m) => self.focus(id, Some(m.clone()), None),
            None => false,
        }
    }

    fn chip(
        &self,
        id: &'static str,
        glyph: &'static str,
        label: &'static str,
        on: bool,
        msg: Message,
    ) -> Element<'a, Message> {
        let focused = self.focus(FocusId::Named(id), Some(msg.clone()), None);
        button(center(
            row![
                icon(glyph, 20.0, TEXT),
                text(label).font(INTER_MEDIUM).size(14)
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        ))
        .width(Length::Fill)
        .height(44)
        .padding(0)
        .style(ringed(focused, toggle_chip(on)))
        .on_press(msg)
        .into()
    }

    /// A text button (footer "Reset", preset actions…).
    fn text_button(
        &self,
        id: FocusId,
        label: String,
        msg: Message,
        width: Length,
    ) -> Element<'a, Message> {
        let focused = self.focus(id, Some(msg.clone()), None);
        button(center(text(label).font(INTER_MEDIUM).size(14)))
            .width(width)
            .height(44)
            .padding([0, 12])
            .style(ringed(focused, text_button))
            .on_press(msg)
            .into()
    }

    /// A square icon button with a tooltip naming it.
    fn icon_button(
        &self,
        id: FocusId,
        glyph: &'static str,
        name: &'a str,
        msg: Option<Message>,
    ) -> Element<'a, Message> {
        self.icon_button_with(id, glyph, name, msg, tooltip::Position::Top)
    }

    /// The same, with the tooltip on another side.
    fn icon_button_with(
        &self,
        id: FocusId,
        glyph: &'static str,
        name: &'a str,
        msg: Option<Message>,
        position: tooltip::Position,
    ) -> Element<'a, Message> {
        let focused = self.button_stop(id, msg.as_ref());
        let color = if msg.is_some() {
            TEXT
        } else {
            with_alpha(TEXT, 0.3)
        };
        tip(
            button(center(icon(glyph, 20.0, color)))
                .width(44)
                .height(44)
                .padding(0)
                .style(ringed(focused, control_button(false, 12.0)))
                .on_press_maybe(msg),
            name,
            position,
        )
    }

    /// The same without a fill, where it would be too much.
    fn plain_icon_button(
        &self,
        id: FocusId,
        glyph: &'static str,
        name: &'a str,
        msg: Option<Message>,
        position: tooltip::Position,
    ) -> Element<'a, Message> {
        let focused = self.button_stop(id, msg.as_ref());
        tip(
            button(center(icon(glyph, 20.0, TEXT)))
                .width(44)
                .height(44)
                .padding(0)
                .style(ringed(focused, ghost_button(12.0, 1.0)))
                .on_press_maybe(msg),
            name,
            position,
        )
    }

    /// Accent-filled button for a pop-up's main action.
    fn primary_button(&self, id: FocusId, label: String, msg: Message) -> Element<'a, Message> {
        let focused = self.focus(id, Some(msg.clone()), None);
        button(center(text(label).font(INTER_MEDIUM).size(14)))
            .width(Length::Fixed(112.0))
            .height(44)
            .padding(0)
            .style(ringed(focused, accent_button(self.accent)))
            .on_press(msg)
            .into()
    }

    /// A checkbox with its label; Enter/Space toggles it like a click.
    fn checkbox(
        &self,
        id: &'static str,
        label: &'a str,
        on: bool,
        msg: Message,
    ) -> Element<'a, Message> {
        let focused = self.focus(FocusId::Named(id), Some(msg.clone()), None);
        let accent = self.accent;
        let mark: Element<'a, Message> = if on {
            icon(icons::CHECK, 16.0, on_accent(accent)).into()
        } else {
            Space::new().into()
        };
        let mark = container(center(mark))
            .width(20)
            .height(20)
            .style(move |_| check_box(on, accent));
        button(
            row![mark, text(label).size(14).color(TEXT)]
                .spacing(10)
                .align_y(Alignment::Center),
        )
        .padding([6, 8])
        .style(ringed(focused, ghost_button(10.0, 1.0)))
        .on_press(msg)
        .into()
    }
}

/// Wraps `content` in a tooltip showing the control's name.
fn tip<'a>(
    content: impl Into<Element<'a, Message>>,
    name: &'a str,
    position: tooltip::Position,
) -> Element<'a, Message> {
    tooltip(
        content,
        container(text(name).size(12).color(TEXT))
            .padding([6, 10])
            .style(theme::tooltip),
        position,
    )
    .gap(6)
    .into()
}

// ---------------------------------------------------------------------------
// Layout

/// A screen to draw: the app itself, or the mini preview of it in the settings
/// window.
struct Screen {
    place: Place,
    layout: Layout,
}

impl App {
    /// The app's screen, at the current window size.
    fn screen(&self) -> Screen {
        Screen {
            place: Place::Screen,
            layout: Layout::new(
                self.window_size,
                self.sections.now_playing.mode,
                &self.pill_label(),
                self.about_open,
            ),
        }
    }

    /// The screen as it would be in a window of `size`, with nothing open in it.
    fn preview_screen(&self, size: Size) -> Screen {
        Screen {
            place: Place::Preview,
            layout: Layout::new(
                size,
                self.sections.now_playing.mode,
                &self.pill_label(),
                false,
            ),
        }
    }

    fn pill_label(&self) -> String {
        if self.source.kind == SourceKind::SystemOutput {
            self.source.label.clone()
        } else {
            ellipsize(&self.source.label, 22)
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        // The focus registry is rebuilt on every view, in Tab order.
        self.focus_reg.clear();
        self.row_focus.borrow_mut().clear();
        let screen = self.screen();
        let layout = screen.layout;

        // Built in reading order, for Tab: the About panel is left of the stage.
        let header = self.header(&screen);
        let panel = self.about_open.then(|| self.about_panel(&layout));
        let stage = self.stage(&screen);
        // Room a floating panel leaves for the playback bar below it.
        let below = if self.docked() {
            0.0
        } else {
            self.playback_height() + 24.0
        };
        let body: Element<_> = match panel {
            // Not enough room beside the bars: the panel floats over them.
            // It gets a solid backing (panels are see-through, which is fine over the
            // background but not over bars), stops above the seek bar and transport so
            // those stay usable, and is opaque so clicks don't reach the bars.
            Some(panel) if layout.panel_overlay => stack![
                stage,
                container(opaque(container(panel).style(panel_backing)))
                    .align_left(Length::Fill)
                    .height(Length::Fill)
                    .padding(Padding::default().bottom(below))
            ]
            .into(),
            Some(panel) => row![panel, stage].spacing(24).into(),
            None => stage,
        };

        let content = column![header, self.framed(body, &layout)]
            .extend(self.docked().then(|| self.dock(&screen)));
        let mut root = stack![self.background(Place::Screen), content];
        if self.picker_open {
            root = root.push(self.source_picker(&layout));
        }
        if self.quick_open && !self.settings.shown() {
            root = root.push(self.quick_settings(&screen));
        }
        if self.settings.shown() {
            root = root.push(self.settings_window());
        } else if self.settings.open {
            root = root.push(self.back_to_settings());
        }
        if self.update.shown() {
            root = root.push(self.update_dialog());
        }
        let root = mouse_area(container(root).width(Length::Fill).height(Length::Fill));
        if self.ui_alpha <= 0.0 {
            // Hide the cursor along with the controls.
            root.interaction(mouse::Interaction::Hidden).into()
        } else {
            root.into()
        }
    }

    /// The whole screen with nothing open in it, for the mini preview.
    fn still_screen(&self, size: Size) -> Element<'_, Message> {
        let screen = self.preview_screen(size);
        let content = column![
            self.header(&screen),
            self.framed(self.stage(&screen), &screen.layout)
        ]
        .extend(self.docked().then(|| self.dock(&screen)));
        stack![self.background(Place::Preview), content].into()
    }

    /// The playback bar along the bottom of the window.
    fn dock(&self, screen: &Screen) -> Element<'_, Message> {
        let ctx = Ctx::for_screen(self, screen);
        self.playback_bar(&ctx, screen.layout.width)
    }

    /// The area below the header, with its margins.
    fn framed<'a>(
        &self,
        body: impl Into<Element<'a, Message>>,
        layout: &Layout,
    ) -> Element<'a, Message> {
        container(body)
            .height(Length::Fill)
            .padding(Padding {
                top: 0.0,
                right: layout.right_margin,
                bottom: if self.docked() { 16.0 } else { 24.0 },
                left: layout.gutter,
            })
            .into()
    }

    fn background(&self, place: Place) -> Element<'_, Message> {
        // While the cover changes, the old one fades out over the new.
        let fading = self.fading_out.as_ref().map(|(old, start)| {
            let t =
                self.now.saturating_duration_since(*start).as_secs_f32() / ART_FADE.as_secs_f32();
            (Some(&old.backdrop), t.clamp(0.0, 1.0))
        });
        Shader::new(Background {
            place,
            settings: &self.sections.background,
            shader: &self.bg.prepared,
            resolved: &self.bg.resolved,
            engine: self.bg_engine,
            time: self.background_time(),
            cover: self.art.as_ref().map(|a| &a.backdrop),
            fading,
            pulse: self.bg_pulse,
            ambient: self.ambient_colors(),
            tint: self.target_accent(),
            audio: self.shader_audio,
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn header(&self, screen: &Screen) -> Element<'_, Message> {
        let ctx = Ctx::for_screen(self, screen);
        let layout = screen.layout;
        // The logo opens the About panel.
        let about = Message::ToggleAbout;
        let about_focused = ctx.focus(FocusId::Named("about"), Some(about.clone()), None);
        let logo = tip(
            button(
                svg(about::TITLE.clone())
                    .height(LOGO_HEIGHT)
                    .width(LOGO_HEIGHT * about::TITLE_ASPECT),
            )
            .padding([5, 5])
            .style(ringed(about_focused, ghost_button(12.0, self.ui_alpha)))
            .on_press(about),
            "About Undavi",
            tooltip::Position::Bottom,
        );

        // Reads the song's info again.
        let refresh = self.platform.media.then(|| {
            let msg = Message::Refresh;
            let focused = ctx.focus(FocusId::Named("refresh"), Some(msg.clone()), None);
            let mut glyph = icon(
                icons::REFRESH,
                20.0,
                with_alpha(self.info_colors().0, self.ui_alpha),
            );
            if let Some(turn) = self.refresh_turn() {
                glyph = glyph.rotation(Radians(turn * TAU));
            }
            tip(
                button(center(glyph))
                    .width(REFRESH)
                    .height(REFRESH)
                    .padding(0)
                    .style(ringed(focused, ghost_button(REFRESH / 2.0, self.ui_alpha)))
                    .on_press(msg),
                "Refresh track info",
                tooltip::Position::Bottom,
            )
        });
        let logo = row![logo].extend(refresh).align_y(Alignment::Center);

        let info = self.header_info(&layout);
        let accent = ctx.accent;
        let alpha = self.ui_alpha;

        let pill_label = self.pill_label();
        let pill_fg = with_alpha(
            if self.picker_open {
                on_accent(accent)
            } else {
                TEXT
            },
            alpha,
        );
        let pill_focused = ctx.focus(FocusId::Named("pill"), Some(Message::TogglePicker), None);
        let pill_style = ringed(
            pill_focused,
            header_button(self.picker_open, accent, 24.0, alpha),
        );
        // Narrow windows: just the icon, with the source in a tooltip.
        let pill: Element<_> = if layout.compact_pill {
            tip(
                button(center(icon(icons::MONITOR, 22.0, pill_fg)))
                    .width(48)
                    .height(48)
                    .padding(0)
                    .style(pill_style)
                    .on_press(Message::TogglePicker),
                &self.source.label,
                tooltip::Position::Bottom,
            )
        } else {
            button(
                container(
                    row![
                        icon(icons::MONITOR, 22.0, pill_fg),
                        text(pill_label)
                            .size(16)
                            .color(pill_fg)
                            .wrapping(Wrapping::None)
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center),
                )
                .center_y(Length::Fill),
            )
            .padding([0, 20])
            .height(48)
            .style(pill_style)
            .on_press(Message::TogglePicker)
            .into()
        };

        let toggle_quick = Message::ToggleQuick;
        let quick_focused = ctx.focus(FocusId::Named("quick"), Some(toggle_quick.clone()), None);
        let quick_open = self.quick_open && !ctx.still.get();
        let quick_fg = if quick_open { on_accent(accent) } else { TEXT };
        let quick = tip(
            button(center(icon(
                icons::QUICK,
                24.0,
                with_alpha(quick_fg, alpha),
            )))
            .width(48)
            .height(48)
            .padding(0)
            .style(ringed(
                quick_focused,
                header_button(quick_open, accent, 12.0, alpha),
            ))
            .on_press(toggle_quick),
            "Quick settings",
            tooltip::Position::Bottom,
        );

        let open_settings = Message::from(SettingsMessage::Toggle);
        let settings_focused = ctx.focus(
            FocusId::Named("open-settings"),
            Some(open_settings.clone()),
            None,
        );
        let settings_open = self.settings.open;
        let settings_fg = if settings_open {
            on_accent(accent)
        } else {
            TEXT
        };
        let settings = tip(
            button(center(icon(
                icons::SETTINGS,
                24.0,
                with_alpha(settings_fg, alpha),
            )))
            .width(48)
            .height(48)
            .padding(0)
            .style(ringed(
                settings_focused,
                header_button(settings_open, accent, 12.0, alpha),
            ))
            .on_press(open_settings),
            "Settings",
            tooltip::Position::Bottom,
        );

        let actions = row![pill, quick, settings]
            .spacing(HEADER_SPACING)
            .align_y(Alignment::Center);

        // With track info, two equal sides keep it centered; without, the logo
        // only takes its own width so the buttons get the rest.
        let bar = match layout.info_width {
            Some(width) => row![
                container(logo).width(Length::Fill),
                container(info).max_width(width).clip(true),
            ],
            None => row![logo],
        };
        bar.push(container(actions).align_right(Length::Fill))
            .align_y(Alignment::Center)
            .padding(Padding {
                top: 24.0,
                right: layout.gutter,
                bottom: 18.0,
                left: layout.gutter,
            })
            .into()
    }

    /// Track info for the header, per the Now playing mode.
    fn header_info(&self, layout: &Layout) -> Element<'_, Message> {
        let mode = layout.now_playing;
        let (text_2, text_3) = self.info_colors();
        let Some(t) = &self.track else {
            return match mode {
                NowPlayingMode::Header | NowPlayingMode::Minimal => {
                    text("Nothing playing").size(14).color(text_3).into()
                }
                _ => Space::new().into(),
            };
        };
        let cover = self.cover_thumb();
        match mode {
            NowPlayingMode::Header => {
                let align = if cover.is_some() {
                    Alignment::Start
                } else {
                    Alignment::Center
                };
                let mut col = column![
                    text(t.title.clone())
                        .font(INTER_SEMIBOLD)
                        .size(20)
                        .color(TEXT)
                        .wrapping(Wrapping::None)
                ]
                .align_x(align)
                .spacing(4);
                if !t.artist.is_empty() {
                    col = col.push(
                        text(t.artist.clone())
                            .size(14)
                            .color(text_2)
                            .wrapping(Wrapping::None),
                    );
                }
                if !t.album.is_empty() {
                    col = col.push(
                        text(tracked(&t.album))
                            .font(INTER_SEMIBOLD)
                            .size(10)
                            .color(text_3)
                            .wrapping(Wrapping::None),
                    );
                }
                let content: Element<_> = match cover {
                    Some(h) => row![cover_image(h, 56.0, 8.0, 16.0, 6.0, 0.35, 1.0), col]
                        .spacing(14)
                        .align_y(Alignment::Center)
                        .into(),
                    None => col.into(),
                };
                container(content).max_width(INFO_MAX).clip(true).into()
            }
            NowPlayingMode::Minimal => {
                let mut r = row![].spacing(10).align_y(Alignment::Center);
                if let Some(h) = cover {
                    r = r.push(cover_image(h, 28.0, 6.0, 8.0, 3.0, 0.3, 1.0));
                }
                r = r.push(
                    text(t.title.clone())
                        .font(INTER_SEMIBOLD)
                        .size(15)
                        .color(TEXT)
                        .wrapping(Wrapping::None),
                );
                if !t.artist.is_empty() {
                    r = r.push(
                        text(t.artist.clone())
                            .size(15)
                            .color(text_2)
                            .wrapping(Wrapping::None),
                    );
                }
                container(r).max_width(INFO_MAX).clip(true).into()
            }
            NowPlayingMode::Spotlight | NowPlayingMode::Off => Space::new().into(),
        }
    }

    /// The Spotlight column beside the bars.
    fn spotlight(&self, layout: &Layout) -> Option<Element<'_, Message>> {
        if layout.now_playing != NowPlayingMode::Spotlight {
            return None;
        }
        Some(
            container(self.spotlight_info()?)
                .center_y(Length::Fill)
                .into(),
        )
    }

    /// The cover and track info in it.
    fn spotlight_info(&self) -> Option<Element<'_, Message>> {
        let t = self.track.as_ref()?;
        let (text_2, text_3) = self.info_colors();
        let mut col = column![].spacing(6).width(SPOTLIGHT_WIDTH);
        if let Some(art) = self
            .art
            .as_ref()
            .filter(|_| self.sections.now_playing.show_album_art)
        {
            col = col
                .push(cover_image(
                    &art.cover,
                    SPOTLIGHT_WIDTH,
                    16.0,
                    60.0,
                    24.0,
                    0.45,
                    1.0,
                ))
                .push(Space::new().height(14));
        }
        col = col.push(
            text(t.title.clone())
                .font(INTER_SEMIBOLD)
                .size(26)
                .color(TEXT),
        );
        if !t.artist.is_empty() {
            col = col.push(text(t.artist.clone()).size(16).color(text_2));
        }
        if !t.album.is_empty() {
            col = col.push(
                text(tracked(&t.album))
                    .font(INTER_SEMIBOLD)
                    .size(11)
                    .color(text_3),
            );
        }
        Some(col.into())
    }

    /// What's drawn behind the bars' place: the visualizer.
    fn visualizer(&self, place: Place) -> Shader<Message, Visualizer<'_>> {
        Shader::new(Visualizer {
            place,
            values: self.processor.visible(),
            peaks: self.processor.peaks(),
            engine: self.vis_engine,
            shader: &self.vis.prepared,
            settings: &self.vis.resolved,
            glow: if self.vis_engine.glow {
                self.beat.pulse() * self.vis_engine.glow_amount
            } else {
                0.0
            },
            cover: self.art.as_ref().map(|a| &a.backdrop),
            stops: self.bar_stops(),
            along: self.sections.colors.gradient == GradientDir::Along,
            audio: self.shader_audio,
            colors: self.ambient_colors(),
            accent: self.target_accent(),
            time: self.cover_time(),
        })
        .width(Length::Fill)
        .height(Length::Fill)
    }

    fn stage(&self, screen: &Screen) -> Element<'_, Message> {
        let ctx = Ctx::for_screen(self, screen);
        let layout = screen.layout;
        let outage = |title: String, detail: String, hint: &'static str| -> Element<'_, Message> {
            center(
                column![
                    text(title).font(INTER_SEMIBOLD).size(18).color(TEXT),
                    text(detail).size(14).color(TEXT_2),
                    text(hint).size(12).color(TEXT_3),
                ]
                .spacing(8)
                .align_x(Alignment::Center),
            )
            .into()
        };
        let bars: Element<_> = match self.audio_outage() {
            Some(AudioOutage::Reconnecting) => outage(
                format!("Reconnecting to {}…", self.source.label),
                "The audio stream stopped. Undavi picks it up again as soon as it's back.".into(),
                "Or pick another source from the System Audio menu.",
            ),
            Some(AudioOutage::Waiting(app)) => outage(
                match app {
                    Some(app) => format!("Waiting for {app}\u{2026}"),
                    None => "Waiting for a music player\u{2026}".into(),
                },
                "The bars start moving as soon as it plays something.".into(),
                "Or pick another source from the menu at the top.",
            ),
            Some(AudioOutage::Failed(err)) => outage(
                "No audio input".into(),
                err.to_string(),
                "Trying again every few seconds. You can also pick another source from the System Audio menu.",
            ),
            _ => {
                let mut bars = mouse_area(self.visualizer(screen.place))
                    .on_double_click(Message::BarsDoubleClick);
                if self.sections.controls.scroll != BarsScroll::Nothing {
                    bars = bars.on_scroll(|delta| Message::BarsScroll(scrolled(delta)));
                }
                bars.into()
            }
        };
        // Lyrics sit beside the bars in Spotlight and above them otherwise.
        let strip_scale = layout.lyrics_scale();
        let bars: Element<_> = match self.spotlight(&layout) {
            Some(spot) => match self.lyrics_view(Placement::Column, strip_scale) {
                Some(lyrics) => row![
                    spot,
                    container(lyrics)
                        .width(Length::FillPortion(5))
                        .height(Length::Fill),
                    container(bars)
                        .width(Length::FillPortion(6))
                        .height(Length::Fill),
                ]
                .spacing(48)
                .height(Length::Fill)
                .into(),
                None => row![spot, bars].spacing(48).height(Length::Fill).into(),
            },
            None => match self.lyrics_view(Placement::Strip, strip_scale) {
                Some(lyrics) => column![lyrics, bars].spacing(8).height(Length::Fill).into(),
                None => bars,
            },
        };

        if self.docked() {
            // The playback bar is along the bottom of the window (see `view`).
            return bars;
        }
        let bottom = self.playback_bar(&ctx, layout.stage_width);
        column![bars, container(bottom).center_x(Length::Fill)]
            .spacing(24)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn source_picker(&self, layout: &Layout) -> Element<'_, Message> {
        let ctx = Ctx::new(self, Scope::Picker);
        let mut list = column![].spacing(2);
        let mut last_heading = None;
        for (i, s) in self.sources.iter().enumerate() {
            // "Music player" needs a player connection.
            if s.kind == SourceKind::Player && !self.platform.media {
                continue;
            }
            let heading = match s.kind {
                SourceKind::SystemOutput => None,
                SourceKind::Player | SourceKind::App => Some("Apps"),
                SourceKind::Output => Some("Outputs"),
                SourceKind::Input => Some("Inputs"),
            };
            if let Some(heading) = heading.filter(|h| last_heading != Some(*h)) {
                list = list.push(
                    container(
                        text(tracked(heading))
                            .font(INTER_SEMIBOLD)
                            .size(11)
                            .color(TEXT_3),
                    )
                    .padding(Padding {
                        top: 12.0,
                        right: 12.0,
                        bottom: 6.0,
                        left: 12.0,
                    }),
                );
            }
            last_heading = heading.or(last_heading);
            let selected = *s == self.source;
            let focused = ctx.focus(
                FocusId::Indexed("src", i),
                Some(Message::PickSource(i)),
                None,
            );
            let label = text(ellipsize(&s.label, 32))
                .size(14)
                .wrapping(Wrapping::None);
            let content: Element<_> = if s.kind == SourceKind::Player {
                column![
                    label,
                    text("Whichever app is playing the music")
                        .size(12)
                        .color(TEXT_3)
                ]
                .spacing(2)
                .into()
            } else {
                label.into()
            };
            list = list.push(
                button(content)
                    .width(Length::Fill)
                    .padding([10, 12])
                    .style(ringed(focused, menu_item(selected)))
                    .on_press(Message::PickSource(i)),
            );
        }
        let width = 300f32.min(self.window_size.width - 2.0 * layout.gutter);
        let menu = container(scrollable(list).height(Length::Shrink))
            .padding(8)
            .width(width)
            .max_height(420)
            .style(menu);

        // Line up with the pill's right edge, unless that would push the menu off
        // the left.
        let right = layout
            .under_pill()
            .min(self.window_size.width - layout.gutter - width)
            .max(layout.gutter);
        let placed = container(opaque(menu))
            .align_right(Length::Fill)
            .padding(Padding {
                top: 24.0 + 48.0 + 8.0,
                right,
                bottom: 0.0,
                left: 0.0,
            });

        stack![
            mouse_area(Space::new().width(Length::Fill).height(Length::Fill))
                .on_press(Message::ClosePicker),
            placed,
        ]
        .into()
    }

    // -----------------------------------------------------------------------
    // Panels

    fn panel_scroll<'a>(&self, content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
        scrollable(content)
            .id(iced::widget::Id::new(PANEL_SCROLL))
            .direction(scrollable::Direction::Vertical(
                scrollable::Scrollbar::new()
                    .width(4)
                    .scroller_width(4)
                    .spacing(8),
            ))
            .height(Length::Fill)
            .style(scrollbar(self.accent()))
            .into()
    }

    fn about_panel(&self, layout: &Layout) -> Element<'_, Message> {
        let ctx = Ctx::new(self, Scope::Panel);
        let header = row![
            text("About")
                .font(INTER_SEMIBOLD)
                .size(18)
                .color(TEXT)
                .width(Length::Fill),
            ctx.icon_button(
                FocusId::Named("close-panel"),
                icons::CLOSE,
                "Close about",
                Some(Message::ToggleAbout),
            ),
        ]
        .align_y(Alignment::Center);

        let title = column![
            image(about::ICON.clone()).width(96).height(96),
            svg(about::TITLE.clone())
                .height(LOGO_HEIGHT)
                .width(LOGO_HEIGHT * about::TITLE_ASPECT),
            text(format!("Version {}", about::VERSION))
                .size(13)
                .color(TEXT_2),
        ]
        .spacing(14)
        .align_x(Alignment::Center)
        .width(Length::Fill);

        let link = |id: FocusId, (label, url): (&'static str, &'static str)| {
            ctx.text_button(
                id,
                format!("{label} \u{2197}"),
                Message::OpenUrl(url),
                Length::Shrink,
            )
        };
        let entry = |title: &'static str, detail: &'static str| {
            column![
                text(title).font(INTER_MEDIUM).size(14).color(TEXT),
                text(detail).size(13).color(TEXT_2),
            ]
            .spacing(2)
        };

        let credits =
            about::CREDITS
                .iter()
                .enumerate()
                .fold(column![].spacing(12), |col, (i, c)| {
                    let mut item = entry(c.title, c.detail);
                    if let Some(l) = c.link {
                        item = item.push(link(FocusId::Indexed("credit-link", i), l));
                    }
                    col.push(item)
                });

        let licenses =
            about::licenses()
                .into_iter()
                .enumerate()
                .fold(column![].spacing(12), |col, (i, l)| {
                    let open = self.open_license == Some(i);
                    let mut actions = row![ctx.text_button(
                        FocusId::Indexed("license", i),
                        if open { "Hide license" } else { "Show license" }.into(),
                        Message::ToggleLicense(i),
                        Length::Shrink,
                    )];
                    if let Some(l) = l.link {
                        actions = actions.push(link(FocusId::Indexed("license-link", i), l));
                    }
                    let mut item = entry(l.name, l.summary).push(actions.spacing(4));
                    if open {
                        item = item.push(
                            container(text(l.text).size(11).color(TEXT_2))
                                .padding(12)
                                .width(Length::Fill)
                                .style(trough),
                        );
                    }
                    col.push(item)
                });

        let content = column![
            title,
            section("Credits", credits.into()),
            section("Licenses", licenses.into()),
        ]
        .spacing(24);

        container(column![header, self.panel_scroll(content)].spacing(16))
            .width(layout.panel_width)
            .height(Length::Fill)
            .padding(20)
            .style(panel_translucent)
            .into()
    }

    /// The pop-up for a new release, and for the install behind it.
    fn update_dialog(&self) -> Element<'_, Message> {
        let ctx = Ctx::new(self, Scope::Dialog);
        let close = |label: &str| {
            ctx.text_button(
                FocusId::Named("update-close"),
                label.into(),
                Message::CloseUpdate,
                Length::Fixed(96.0),
            )
        };
        let page_link = |label: &str| {
            ctx.text_button(
                FocusId::Named("update-page"),
                format!("{label} \u{2197}"),
                Message::OpenReleasePage,
                Length::Shrink,
            )
        };
        let mut checks = column![].spacing(2);
        let mut buttons = row![].spacing(8).align_y(Alignment::Center);
        let (title, body) = match &self.update {
            UpdateState::None => return Space::new().into(),
            UpdateState::Offer {
                release,
                auto,
                dont_ask,
            } => {
                let installable =
                    matches!(self.install, Some(Install::Prefix(_))) && release.tarball.is_some();
                let then = if installable {
                    "It takes effect the next time Undavi starts."
                } else if self.install == Some(Install::Package) {
                    "Update it with your package manager."
                } else {
                    "Download it from the release page."
                };
                if installable {
                    checks = checks.push(ctx.checkbox(
                        "update-auto",
                        "Update automatically",
                        *auto,
                        Message::ToggleUpdateAuto,
                    ));
                }
                checks = checks.push(ctx.checkbox(
                    "update-dont-ask",
                    "Don't ask again",
                    *dont_ask,
                    Message::ToggleUpdateDontAsk,
                ));
                buttons = buttons
                    .push(page_link("What's new"))
                    .push(Space::new().width(Length::Fill));
                buttons = if installable {
                    buttons.push(close("Not now")).push(ctx.primary_button(
                        FocusId::Named("update-now"),
                        "Update".into(),
                        Message::UpdateNow,
                    ))
                } else {
                    buttons.push(ctx.primary_button(
                        FocusId::Named("update-ok"),
                        "OK".into(),
                        Message::CloseUpdate,
                    ))
                };
                (
                    format!("Undavi {} is available", release.version),
                    format!("You have version {}. {then}", about::VERSION),
                )
            }
            UpdateState::Installing { release, .. } => {
                buttons = buttons
                    .push(Space::new().width(Length::Fill))
                    .push(close("Hide"));
                (
                    format!("Updating to {}\u{2026}", release.version),
                    "Downloading and installing. You may be asked for your password.".into(),
                )
            }
            UpdateState::Installed { version } => {
                buttons = buttons
                    .push(Space::new().width(Length::Fill))
                    .push(close("Later"))
                    .push(ctx.primary_button(
                        FocusId::Named("update-restart"),
                        "Restart now".into(),
                        Message::RestartUpdated,
                    ));
                (
                    format!("Undavi {version} is installed"),
                    "Restart Undavi to start using it.".into(),
                )
            }
            UpdateState::Failed { error, page } => {
                if !page.is_empty() {
                    buttons = buttons.push(page_link("Release page"));
                }
                buttons = buttons
                    .push(Space::new().width(Length::Fill))
                    .push(ctx.primary_button(
                        FocusId::Named("update-ok"),
                        "Close".into(),
                        Message::CloseUpdate,
                    ));
                let mut chars = error.chars();
                let error = chars
                    .next()
                    .map(|c| c.to_uppercase().chain(chars).collect::<String>())
                    .unwrap_or_default();
                ("Couldn't update".into(), format!("{error}."))
            }
        };

        let width = 440f32.min(self.window_size.width - 2.0 * self.screen().layout.gutter);
        let card = container(
            column![
                column![
                    text(title).font(INTER_SEMIBOLD).size(18).color(TEXT),
                    text(body).size(14).color(TEXT_2),
                ]
                .spacing(6),
                checks,
                buttons,
            ]
            .spacing(16),
        )
        .padding(24)
        .width(width)
        .style(panel);
        // Opaque so clicks don't reach the controls underneath.
        opaque(center(opaque(card)).style(scrim))
    }
}

// ---------------------------------------------------------------------------
// Small view helpers

/// How far (and which way) the wheel turned.
fn scrolled(delta: mouse::ScrollDelta) -> f32 {
    match delta {
        mouse::ScrollDelta::Lines { y, .. } | mouse::ScrollDelta::Pixels { y, .. } => y,
    }
}

/// Rounded cover art with a soft shadow.
fn cover_image<'a>(
    handle: &Handle,
    size: f32,
    radius: f32,
    blur: f32,
    offset_y: f32,
    shadow: f32,
    opacity: f32,
) -> Element<'a, Message> {
    container(
        image(handle.clone())
            .width(size)
            .height(size)
            .border_radius(radius)
            .opacity(opacity)
            .filter_method(image::FilterMethod::Linear),
    )
    .style(cover_shadow(radius, blur, offset_y, shadow))
    .into()
}

fn section<'a>(title: &str, content: Element<'a, Message>) -> Element<'a, Message> {
    column![
        text(tracked(title))
            .font(INTER_SEMIBOLD)
            .size(11)
            .color(TEXT_3),
        content
    ]
    .spacing(10)
    .into()
}

fn ellipsize(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max - 1).collect();
        out.push('\u{2026}');
        out
    }
}

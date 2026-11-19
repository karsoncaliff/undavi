//! Layout and widgets: the header, stage, panels and picker.
//!
//! Every focusable control is built through [`Ctx`], which registers it with the
//! keyboard-focus model (`focus.rs`) while drawing it, so Tab order always
//! matches what's on screen.

use crate::about;
use crate::app::{
    ART_FADE, App, AudioOutage, Message, PANEL_SCROLL, PRESET_NAME_INPUT, Panel, SEEK_STEP,
    SPOTLIGHT_WIDTH, parse_hex,
};
use crate::art::ramp;
use crate::audio::SourceKind;
use crate::background::{Background, Layer, LayerKind, Overlay};
use crate::controls::{
    SwatchFill, band_slider, compact_slider, range_slider, slider, step_range, swatches, switch,
};
use crate::focus::{Entry, FocusId, Scope, Step, Stepper};
use crate::icons::{self, icon};
use crate::karaoke::Placement;
use crate::layout::{INFO_MAX, Layout};
use crate::lyrics::{Entry as LyricEntry, Lyrics, Source as LyricsSource, Status as LyricsStatus};
use crate::settings::{
    ACCENT_BLUE, Accent, AdaptSpeed, AppearanceSettings, BackgroundMode, BarValue, Channel,
    Corners, Detail, FREQ_MIN_GAP, FrameRate, FreqScale, GATE_OFF, GlowSource, GradientDir,
    LEVEL_MIN_SPAN, MAX_DELAY_MS, Mirror, Motion, NowPlayingMode, Palette, PresetKind, SilenceMode,
    SolidColor, Split, TuningSettings, VisualSettings, VisualStyle, format_db, format_hz,
    slider_to_hz,
};
use crate::theme::{self, *};
use crate::visualizer::{BarMap, Bars, Seek, is_solid, seek_height};
use iced::widget::image::Handle;
use iced::widget::text::Wrapping;
use iced::widget::{
    Canvas, Space, button, center, column, container, image, mouse_area, opaque, row, scrollable,
    stack, svg, text, text_input, tooltip,
};
use iced::{Alignment, Color, Element, Length, Padding, mouse};

/// Height of the wordmark in the header (including its one-unit padding, so the
/// letters themselves are 30 px tall).
const LOGO_HEIGHT: f32 = 36.0;
use std::rc::Rc;
use std::time::Duration;

// ---------------------------------------------------------------------------
// Focus-aware widget helpers

/// Builds focusable controls for one area of the UI and registers them for Tab.
struct Ctx<'a> {
    app: &'a App,
    scope: Scope,
    accent: Color,
}

/// What a segment shows.
enum SegLabel {
    Text(&'static str),
    Icon(&'static str),
}

struct Seg {
    label: SegLabel,
    selected: bool,
    msg: Message,
}

fn seg(label: &'static str, selected: bool, msg: Message) -> Seg {
    Seg {
        label: SegLabel::Text(label),
        selected,
        msg,
    }
}

impl<'a> Ctx<'a> {
    fn new(app: &'a App, scope: Scope) -> Self {
        Self {
            app,
            scope,
            accent: app.accent(),
        }
    }

    /// Register a focus stop; returns whether it's the focused one.
    fn focus(
        &self,
        id: FocusId,
        activate: Option<Message>,
        step: Option<Stepper<Message>>,
    ) -> bool {
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

    fn slider<F>(
        &self,
        id: &'static str,
        label: &str,
        display: String,
        value: f32,
        range: (f32, f32, f32),
        on_change: F,
    ) -> Element<'a, Message>
    where
        F: Fn(f32) -> Message + Clone + 'static,
    {
        let f = on_change.clone();
        let focused = self.focus(
            FocusId::Named(id),
            None,
            Some(Rc::new(move |s: Step| Some(f(s.apply(value, range))))),
        );
        slider(
            label.to_string(),
            display,
            value,
            range,
            self.accent,
            focused,
            on_change,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn range<F>(
        &self,
        id: &'static str,
        label: &str,
        display: String,
        (lo, hi): (f32, f32),
        max: f32,
        gap: f32,
        on_change: F,
    ) -> Element<'a, Message>
    where
        F: Fn((f32, f32)) -> Message + Clone + 'static,
    {
        let focused = [0, 1].map(|k| {
            let f = on_change.clone();
            self.focus(
                FocusId::Indexed(id, k),
                None,
                Some(Rc::new(move |s: Step| {
                    Some(f(step_range((lo, hi), k, s, max, gap)))
                })),
            )
        });
        let which = focused.iter().position(|&f| f);
        range_slider(
            label.to_string(),
            display,
            (lo, hi),
            max,
            gap,
            self.accent,
            which,
            on_change,
        )
    }

    fn band<F>(
        &self,
        id: &'static str,
        name: &'static str,
        db: i8,
        on_change: F,
    ) -> Element<'a, Message>
    where
        F: Fn(i8) -> Message + Clone + 'static,
    {
        let f = on_change.clone();
        let focused = self.focus(
            FocusId::Named(id),
            None,
            Some(Rc::new(move |s: Step| {
                Some(f(s.apply(db as f32, (-12.0, 12.0, 1.0)) as i8))
            })),
        );
        band_slider(name, db, self.accent, focused, on_change)
    }

    fn switch_row(
        &self,
        id: &'static str,
        label: &'a str,
        help: Option<&'a str>,
        on: bool,
        msg: Message,
    ) -> Element<'a, Message> {
        let focused = self.focus(FocusId::Named(id), Some(msg.clone()), None);
        let mut labels = column![text(label).font(INTER_MEDIUM).size(14).color(TEXT)]
            .spacing(2)
            .width(Length::Fill);
        if let Some(help) = help {
            labels = labels.push(text(help).size(12).color(TEXT_3));
        }
        row![labels, switch(on, self.accent, focused, msg)]
            .spacing(12)
            .align_y(Alignment::Center)
            .into()
    }

    /// A segmented control: one Tab stop; arrows move the selection.
    fn segmented(
        &self,
        id: &'static str,
        items: Vec<Seg>,
        width: Length,
        enabled: bool,
    ) -> Element<'a, Message> {
        let selected = items.iter().position(|s| s.selected).unwrap_or(0);
        let msgs: Vec<Message> = items.iter().map(|s| s.msg.clone()).collect();
        let focused = enabled
            && self.focus(
                FocusId::Named(id),
                None,
                Some(Rc::new(move |s: Step| {
                    msgs.get(s.apply_index(selected, msgs.len())).cloned()
                })),
            );
        let fill = matches!(width, Length::Fill);
        let r = items.into_iter().fold(row![].spacing(4), |r, item| {
            let content: Element<'a, Message> = match item.label {
                SegLabel::Text(label) => text(label)
                    .font(INTER_MEDIUM)
                    .size(13)
                    .wrapping(Wrapping::None)
                    .into(),
                SegLabel::Icon(glyph) => {
                    let color = match (enabled, item.selected) {
                        (false, _) => with_alpha(TEXT_2, 0.45),
                        (true, true) => TEXT,
                        (true, false) => TEXT_2,
                    };
                    icon(glyph, 20.0, color).into()
                }
            };
            r.push(
                button(center(content))
                    .width(if fill {
                        Length::Fill
                    } else {
                        Length::Fixed(44.0)
                    })
                    .height(44)
                    .padding(0)
                    .style(segment(item.selected))
                    .on_press_maybe(enabled.then_some(item.msg)),
            )
        });
        container(r)
            .padding(3)
            .width(width)
            .style(trough_ring(focused))
            .into()
    }

    fn swatches(
        &self,
        id: &'static str,
        fills: Vec<SwatchFill>,
        selected: usize,
        inner_ring: bool,
        on_pick: fn(usize) -> Message,
    ) -> Element<'a, Message> {
        let n = fills.len();
        let focused = self.focus(
            FocusId::Named(id),
            None,
            Some(Rc::new(move |s: Step| {
                Some(on_pick(s.apply_index(selected, n)))
            })),
        );
        swatches(fills, selected, inner_ring, focused, on_pick)
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

    fn secondary_button(&self, id: FocusId, label: String, msg: Message) -> Element<'a, Message> {
        let focused = self.focus(id, Some(msg.clone()), None);
        button(center(text(label).font(INTER_MEDIUM).size(14)))
            .width(Length::Fill)
            .height(44)
            .padding(0)
            .style(ringed(focused, control_button(false, 12.0)))
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
            tooltip::Position::Top,
        )
    }

    fn close_button(&self, panel: Panel, name: &'a str) -> Element<'a, Message> {
        self.icon_button(
            FocusId::Named("close-panel"),
            icons::CLOSE,
            name,
            Some(Message::TogglePanel(panel)),
        )
    }

    fn footer(
        &self,
        primary: String,
        primary_msg: Message,
        primary_id: &'static str,
        reset: Message,
    ) -> Element<'a, Message> {
        row![
            self.secondary_button(FocusId::Named(primary_id), primary, primary_msg),
            self.text_button(
                FocusId::Named("reset"),
                "Reset".into(),
                reset,
                Length::Fixed(88.0)
            ),
        ]
        .spacing(12)
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

impl App {
    /// How the screen is arranged at the current window size.
    pub(crate) fn layout(&self) -> Layout {
        Layout::new(
            self.window_size,
            self.appearance.now_playing,
            &self.pill_label(),
            self.panel != Panel::None,
        )
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
        let layout = self.layout();

        let header = self.header();
        let stage = self.stage();
        let panel = match self.panel {
            Panel::None => None,
            Panel::Tuning => Some(self.tuning_panel()),
            Panel::Visuals => Some(self.visuals_panel()),
            Panel::Appearance => Some(self.appearance_panel()),
            Panel::About => Some(self.about_panel()),
        };
        let body: Element<_> = match panel {
            // Not enough room beside the bars: the panel floats over them.
            // It gets a solid backing (panels are see-through, which is fine over the
            // background but not over bars), stops above the seek bar and transport so
            // those stay usable, and is opaque so clicks don't reach the bars.
            Some(panel) if layout.panel_overlay => {
                let transport = if self.platform.media {
                    seek_height() + 20.0 + 56.0
                } else {
                    48.0
                };
                stack![
                    stage,
                    container(opaque(container(panel).style(panel_backing)))
                        .align_right(Length::Fill)
                        .height(Length::Fill)
                        .padding(Padding::default().bottom(transport + 24.0))
                ]
                .into()
            }
            Some(panel) => row![stage, panel].spacing(24).into(),
            None => stage,
        };
        let body = container(body).height(Length::Fill).padding(Padding {
            top: 0.0,
            right: layout.right_margin,
            bottom: 24.0,
            left: layout.gutter,
        });

        let content = column![header, body];
        let mut root = stack![self.background(), content];
        if self.picker_open {
            root = root.push(self.source_picker());
        }
        let root = mouse_area(container(root).width(Length::Fill).height(Length::Fill));
        if self.ui_alpha <= 0.0 {
            // Hide the cursor along with the controls.
            root.interaction(mouse::Interaction::Hidden).into()
        } else {
            root.into()
        }
    }

    fn background(&self) -> Element<'_, Message> {
        let a = &self.appearance;
        let mut layers = Vec::new();
        let ambient = self
            .art
            .as_ref()
            .map(|art| &art.ambient)
            .unwrap_or(&self.default_ambient);
        match a.bg_mode {
            BackgroundMode::Solid => {}
            BackgroundMode::Ambient => layers.push(Layer {
                kind: LayerKind::Ambient(ambient),
                opacity: 1.0,
            }),
            BackgroundMode::AlbumArt => {
                let fade = self.fading_out.as_ref().map(|(old, start)| {
                    let t = self.now.saturating_duration_since(*start).as_secs_f32()
                        / ART_FADE.as_secs_f32();
                    (old, t.clamp(0.0, 1.0))
                });
                if let Some((old, _)) = &fade {
                    layers.push(Layer {
                        kind: LayerKind::Art(&old.handle),
                        opacity: 1.0,
                    });
                }
                let opacity = fade.map(|(_, t)| t).unwrap_or(1.0);
                match &self.blurred {
                    Some(b) => layers.push(Layer {
                        kind: LayerKind::Art(&b.handle),
                        opacity,
                    }),
                    // No cover (yet): fall back to ambient circles.
                    None => layers.push(Layer {
                        kind: LayerKind::Ambient(ambient),
                        opacity,
                    }),
                }
            }
        }
        let art = Canvas::new(Background {
            settings: a,
            time: self.background_time(),
            layers,
            pulse: self.bg_pulse,
        })
        .width(Length::Fill)
        .height(Length::Fill);
        if a.bg_mode.is_dynamic() {
            let overlay = Canvas::new(Overlay {
                dim_pct: a.dim_pct,
                pulse: self.bg_pulse,
            })
            .width(Length::Fill)
            .height(Length::Fill);
            stack![art, overlay].into()
        } else {
            art.into()
        }
    }

    fn header(&self) -> Element<'_, Message> {
        let ctx = Ctx::new(self, Scope::Main);
        // The logo opens the About panel.
        let about = Message::TogglePanel(Panel::About);
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

        let info = self.header_info();
        let accent = ctx.accent;
        let over_art = self.over_art();
        let alpha = self.ui_alpha;

        let layout = self.layout();
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
            header_button(self.picker_open, accent, over_art, 24.0, alpha),
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

        let panel_button = |p: Panel, id: &'static str, glyph: &'static str, name: &'static str| {
            let active = self.panel == p;
            let fg = if active { on_accent(accent) } else { TEXT };
            let focused = ctx.focus(FocusId::Named(id), Some(Message::TogglePanel(p)), None);
            tip(
                button(center(icon(glyph, 24.0, with_alpha(fg, alpha))))
                    .width(48)
                    .height(48)
                    .padding(0)
                    .style(ringed(
                        focused,
                        header_button(active, accent, over_art, 12.0, alpha),
                    ))
                    .on_press(Message::TogglePanel(p)),
                name,
                tooltip::Position::Bottom,
            )
        };

        let actions = row![
            pill,
            panel_button(Panel::Tuning, "open-tuning", icons::SLIDERS, "Tuning"),
            panel_button(Panel::Visuals, "open-visuals", icons::BARS, "Visuals"),
            panel_button(
                Panel::Appearance,
                "open-appearance",
                icons::APPEARANCE,
                "Appearance"
            ),
        ]
        .spacing(16)
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
    fn header_info(&self) -> Element<'_, Message> {
        let mode = self.layout().now_playing;
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
                    Some(h) => row![cover_image(h, 56.0, 8.0, 16.0, 6.0, 0.35), col]
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
                    r = r.push(cover_image(h, 28.0, 6.0, 8.0, 3.0, 0.3));
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
    fn spotlight(&self) -> Option<Element<'_, Message>> {
        if self.layout().now_playing != NowPlayingMode::Spotlight {
            return None;
        }
        let t = self.track.as_ref()?;
        let (text_2, text_3) = self.info_colors();
        let mut col = column![].spacing(6).width(SPOTLIGHT_WIDTH);
        if let Some(art) = self.art.as_ref().filter(|_| self.appearance.show_album_art) {
            col = col
                .push(cover_image(
                    &art.cover,
                    SPOTLIGHT_WIDTH,
                    16.0,
                    60.0,
                    24.0,
                    0.45,
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
        Some(container(col).center_y(Length::Fill).into())
    }

    fn stage(&self) -> Element<'_, Message> {
        let ctx = Ctx::new(self, Scope::Main);
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
            // Double-click the bars to toggle full screen.
            _ => mouse_area(
                Canvas::new(Bars {
                    values: self.processor.visible(),
                    peaks: self.processor.peaks(),
                    visuals: &self.visuals,
                    glow: if self.visuals.glow {
                        self.beat.pulse() * self.visuals.glow_amount as f32 / 100.0
                    } else {
                        0.0
                    },
                    cover: self.art.as_ref().map(|a| &a.disc),
                    spin: self.cover_spin(),
                    stops: self.bar_stops(),
                })
                .width(Length::Fill)
                .height(Length::Fill),
            )
            .on_double_click(Message::ToggleFullscreen)
            .into(),
        };
        // While tuning, a meter shows what the sliders are working with. It sits in
        // the bars' bottom corner, where the treble is usually short and lyrics never go.
        let bars: Element<_> = if self.panel == Panel::Tuning && self.audio_outage().is_none() {
            // A floating panel covers the right side, so the meter moves left.
            let meter = container(self.tuning_meter()).align_bottom(Length::Fill);
            let meter = if self.layout().panel_overlay {
                meter.align_left(Length::Fill)
            } else {
                meter.align_right(Length::Fill)
            };
            stack![bars, meter].into()
        } else {
            bars
        };
        // Lyrics sit beside the bars in Spotlight and above them otherwise.
        let bars: Element<_> = match self.spotlight() {
            Some(spot) => match self.lyrics_view(Placement::Column) {
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
            None => match self.lyrics_view(Placement::Strip) {
                Some(lyrics) => column![lyrics, bars].spacing(8).height(Length::Fill).into(),
                None => bars,
            },
        };

        // No player connection on this platform: no track, and no transport controls.
        let t = self.track.as_ref().filter(|_| self.platform.media);
        let alpha = self.ui_alpha;
        let seekable = t
            .filter(|n| n.can_seek)
            .and_then(|n| Some((n.position_at(self.now), n.length?)));
        let seek_focused = match seekable {
            Some((pos, len)) => ctx.focus(
                FocusId::Named("seek"),
                None,
                Some(Rc::new(move |s: Step| {
                    let big = SEEK_STEP * 3;
                    let target = match s {
                        Step::Less => pos.saturating_sub(SEEK_STEP),
                        Step::More => pos + SEEK_STEP,
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
        let seek = Canvas::new(Seek {
            time: seekable.map(|(pos, len)| (pos.as_secs_f32(), len.as_secs_f32())),
            phase: self.wave_phase,
            accent: ctx.accent,
            alpha,
            focused: seek_focused,
            on_seek: Message::Seek,
        })
        .width(Length::Fill)
        .height(seek_height());

        let has_media = t.is_some();
        let playing = t.is_some_and(|n| n.playing);
        let repeat_on = t.is_some_and(|n| n.repeat);
        let volume = t.and_then(|n| n.volume);
        let fg = |enabled: bool| if enabled { TEXT } else { ICON_OFF };

        let transport_button = |id: &'static str,
                                glyph: &'static str,
                                name: &'static str,
                                icon_size: f32,
                                size: f32,
                                color: Color,
                                msg: Option<Message>| {
            let focused = ctx.button_stop(FocusId::Named(id), msg.as_ref());
            tip(
                button(center(icon(glyph, icon_size, with_alpha(color, alpha))))
                    .width(size)
                    .height(size)
                    .padding(0)
                    .style(ringed(focused, ghost_button(size / 2.0, alpha)))
                    .on_press_maybe(msg),
                name,
                tooltip::Position::Top,
            )
        };

        let repeat = transport_button(
            "repeat",
            icons::REPEAT,
            if repeat_on {
                "Repeat: on"
            } else {
                "Repeat: off"
            },
            40.0,
            56.0,
            if repeat_on { TEXT } else { ICON_OFF },
            has_media.then_some(Message::ToggleRepeat),
        );
        let controls = row![
            transport_button(
                "previous",
                icons::PREV,
                "Previous track",
                22.0,
                48.0,
                fg(has_media),
                has_media.then_some(Message::Previous)
            ),
            transport_button(
                "play",
                if playing { icons::PAUSE } else { icons::PLAY },
                if playing { "Pause" } else { "Play" },
                36.0,
                56.0,
                fg(has_media),
                has_media.then_some(Message::PlayPause),
            ),
            transport_button(
                "next",
                icons::NEXT,
                "Next track",
                22.0,
                48.0,
                fg(has_media),
                has_media.then_some(Message::Next)
            ),
        ]
        .spacing(12)
        .align_y(Alignment::Center);

        let volume_button = mouse_area(transport_button(
            "volume",
            if volume == Some(0.0) {
                icons::VOLUME_MUTED
            } else {
                icons::VOLUME
            },
            "Volume",
            40.0,
            56.0,
            fg(volume.is_some()),
            volume.is_some().then_some(Message::ToggleVolume),
        ))
        .on_scroll(|delta| {
            let dy = match delta {
                mouse::ScrollDelta::Lines { y, .. } | mouse::ScrollDelta::Pixels { y, .. } => y,
            };
            Message::VolumeScroll(dy)
        });
        let mut right = row![].spacing(8).align_y(Alignment::Center);
        if let (true, Some(v)) = (self.volume_open && alpha > 0.0, volume) {
            right = right.push(self.volume_popover(&ctx, v as f32));
        }
        right = right.push(volume_button);
        let fullscreen = transport_button(
            "fullscreen",
            if self.fullscreen {
                icons::EXIT_FULLSCREEN
            } else {
                icons::FULLSCREEN
            },
            if self.fullscreen {
                "Exit full screen (F11)"
            } else {
                "Full screen (F11)"
            },
            24.0,
            48.0,
            TEXT,
            Some(Message::ToggleFullscreen),
        );
        // Without a music player connection there's nothing to play, seek or turn
        // up; only the full-screen button is left.
        let bottom = if self.platform.media {
            let right = row![fullscreen, right]
                .spacing(8)
                .align_y(Alignment::Center);
            let transport = row![
                container(repeat).width(Length::Fill),
                controls,
                container(right).align_right(Length::Fill)
            ]
            .align_y(Alignment::Center);
            container(column![seek, transport].spacing(20))
        } else {
            container(fullscreen).align_right(Length::Fill)
        }
        .max_width(1000);

        column![bars, container(bottom).center_x(Length::Fill)]
            .spacing(24)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// Mute toggle, volume slider and percentage, opened from the volume button.
    fn volume_popover<'a>(&'a self, ctx: &Ctx<'a>, volume: f32) -> Element<'a, Message> {
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
        .width(150);
        container(
            row![
                mute,
                level,
                text(format!("{}%", (volume * 100.0).round() as u32))
                    .size(13)
                    .color(TEXT_2)
                    .width(40)
                    .align_x(iced::alignment::Horizontal::Right),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .padding(Padding {
            top: 0.0,
            right: 16.0,
            bottom: 0.0,
            left: 6.0,
        })
        .height(52)
        .center_y(52)
        .style(popover)
        .into()
    }

    fn source_picker(&self) -> Element<'_, Message> {
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
        let layout = self.layout();
        let width = 300f32.min(self.window_size.width - 2.0 * layout.gutter);
        let menu = container(scrollable(list).height(Length::Shrink))
            .padding(8)
            .width(width)
            .max_height(420)
            .style(menu);

        // Line up with the pill's right edge (header padding + the three 48px
        // buttons and their gaps), unless that would push the menu off the left.
        let under_pill = layout.gutter + 48.0 * 3.0 + 16.0 * 3.0;
        let right = under_pill
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

    fn panel_scroll<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
        scrollable(content)
            .id(iced::widget::Id::new(PANEL_SCROLL))
            .direction(scrollable::Direction::Vertical(
                scrollable::Scrollbar::new()
                    .width(4)
                    .scroller_width(4)
                    .spacing(8),
            ))
            .height(Length::Fill)
            .into()
    }

    fn tuning_panel(&self) -> Element<'_, Message> {
        let ctx = Ctx::new(self, Scope::Panel);
        // The close button comes first in Tab order, matching its position.
        let close = (Panel::Tuning, "Close tuning");
        let t = &self.tuning;
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
        let header_close = ctx.close_button(close.0, close.1);

        let response = section(
            "Response",
            column![
                ctx.slider(
                    "sensitivity",
                    "Sensitivity",
                    format!("{}%", t.sensitivity),
                    t.sensitivity as f32,
                    (0.0, 100.0, 1.0),
                    set(|t, v| t.sensitivity = v as u8)
                ),
                ctx.slider(
                    "spread",
                    "Spread",
                    format!("{}%", t.spread),
                    t.spread as f32,
                    (0.0, 100.0, 1.0),
                    set(|t, v| t.spread = v as u8)
                ),
                text("Blends neighboring bars into a smoother, wave-like shape.")
                    .size(12)
                    .color(TEXT_3),
                ctx.slider(
                    "rise",
                    "Rise time",
                    format!("{} ms", t.rise_ms),
                    t.rise_ms as f32,
                    (5.0, 100.0, 1.0),
                    set(|t, v| t.rise_ms = v as u16)
                ),
                ctx.slider(
                    "fall",
                    "Fall time",
                    format!("{} ms", t.fall_ms),
                    t.fall_ms as f32,
                    (50.0, 800.0, 10.0),
                    set(|t, v| t.fall_ms = v as u16)
                ),
                ctx.slider(
                    "peak-hold",
                    "Peak hold",
                    format!("{} ms", t.peak_hold_ms),
                    t.peak_hold_ms as f32,
                    (0.0, 1000.0, 50.0),
                    set(|t, v| t.peak_hold_ms = v as u16)
                ),
                ctx.switch_row(
                    "gravity",
                    "Gravity",
                    Some("Bars and peak caps fall faster and faster, like real weights"),
                    t.gravity,
                    with(&|n| n.gravity = !n.gravity),
                ),
            ]
            .spacing(10)
            .into(),
        );

        // Dynamics: which levels count as empty and full, and the curve between them.
        let base = t.clone();
        let dynamics = section(
            "Dynamics",
            column![
                ctx.range(
                    "level",
                    "Level range",
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
                ),
                ctx.slider(
                    "contrast",
                    "Contrast",
                    contrast_label(t.contrast),
                    t.contrast as f32,
                    (0.0, 100.0, 1.0),
                    set(|t, v| t.contrast = v as u8)
                ),
                text("Low contrast lifts quiet sounds; high contrast makes loud hits stand out.")
                    .size(12)
                    .color(TEXT_3),
            ]
            .spacing(10)
            .into(),
        );

        let range_text = format!(
            "{} \u{2013} {}",
            format_hz(slider_to_hz(t.freq_lo as f32)),
            format_hz(slider_to_hz(t.freq_hi as f32))
        );
        let base = t.clone();
        let frequency = section(
            "Frequency",
            column![
                ctx.range(
                    "freq",
                    "Range",
                    range_text,
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
                ),
                labeled(
                    "Scale",
                    ctx.segmented(
                        "freq-scale",
                        [
                            (FreqScale::Log, "Log"),
                            (FreqScale::Mel, "Mel"),
                            (FreqScale::Linear, "Linear")
                        ]
                        .into_iter()
                        .map(|(f, label)| seg(
                            label,
                            t.freq_scale == f,
                            with(&|n| n.freq_scale = f)
                        ))
                        .collect(),
                        Length::Fill,
                        true,
                    ),
                    match t.freq_scale {
                        FreqScale::Log => "Every octave gets the same room.",
                        FreqScale::Mel =>
                            "Follows how we hear pitch; less room for the extreme highs.",
                        FreqScale::Linear =>
                            "Every bar covers the same Hz, so the bass shares a few bars.",
                    },
                ),
                ctx.slider(
                    "tilt",
                    "Tilt",
                    format!("{}/oct", format_db(t.tilt_db)),
                    t.tilt_db as f32,
                    (-3.0, 6.0, 1.0),
                    set(|t, v| t.tilt_db = v as i8)
                ),
                labeled(
                    "Detail",
                    ctx.segmented(
                        "detail",
                        [
                            (Detail::Fast, "Fast"),
                            (Detail::Balanced, "Balanced"),
                            (Detail::Detailed, "Detailed")
                        ]
                        .into_iter()
                        .map(|(d, label)| seg(label, t.detail == d, with(&|n| n.detail = d)))
                        .collect(),
                        Length::Fill,
                        true,
                    ),
                    "Detailed resolves low notes better but reacts a little later.",
                ),
                labeled(
                    "Bar value",
                    ctx.segmented(
                        "bar-value",
                        [
                            (BarValue::Loudest, "Loudest"),
                            (BarValue::Average, "Average")
                        ]
                        .into_iter()
                        .map(|(b, label)| seg(label, t.bar_value == b, with(&|n| n.bar_value = b)))
                        .collect(),
                        Length::Fill,
                        true,
                    ),
                    "How the frequencies inside each bar are combined.",
                ),
            ]
            .spacing(10)
            .into(),
        );

        let band =
            |id: &'static str, name: &'static str, db: i8, f: fn(&mut TuningSettings, i8)| {
                let base = t.clone();
                ctx.band(id, name, db, move |v| {
                    let mut next = base.clone();
                    f(&mut next, v);
                    Message::SetTuning(next)
                })
            };
        let bands = section(
            "Bands",
            row![
                band("bass", "Bass", t.bass_db, |t, v| t.bass_db = v),
                band("mids", "Mids", t.mids_db, |t, v| t.mids_db = v),
                band("highs", "Highs", t.highs_db, |t, v| t.highs_db = v),
            ]
            .into(),
        );

        let mut auto_gain = column![ctx.switch_row(
            "auto-gain",
            "Auto-gain",
            Some("Keeps adjusting so quiet parts aren't flat and loud ones don't max out"),
            t.auto_gain,
            with(&|n| n.auto_gain = !n.auto_gain),
        )]
        .spacing(10);
        if t.auto_gain {
            auto_gain = auto_gain.push(labeled(
                "Adapt speed",
                ctx.segmented(
                    "agc-speed",
                    [
                        (AdaptSpeed::Slow, "Slow"),
                        (AdaptSpeed::Medium, "Medium"),
                        (AdaptSpeed::Fast, "Fast"),
                    ]
                    .into_iter()
                    .map(|(a, label)| seg(label, t.agc_speed == a, with(&|n| n.agc_speed = a)))
                    .collect(),
                    Length::Fill,
                    true,
                ),
                "How quickly it catches up when the music gets quieter.",
            ));
        }
        let auto_gain = section("Auto-gain", auto_gain.into());

        let silence = section(
            "Silence",
            column![
                ctx.slider(
                    "gate",
                    "Noise gate",
                    if t.gate_db <= GATE_OFF {
                        "Off".into()
                    } else {
                        format!("{} dB", signed(t.gate_db))
                    },
                    t.gate_db as f32,
                    (GATE_OFF as f32, -30.0, 1.0),
                    set(|t, v| t.gate_db = v as i8)
                ),
                text("Anything quieter counts as silence, so hiss doesn't make the bars twitch.")
                    .size(12)
                    .color(TEXT_3),
                labeled(
                    "When it's silent",
                    ctx.segmented(
                        "when-silent",
                        [
                            (SilenceMode::Fade, "Fade out"),
                            (SilenceMode::Wave, "Idle wave"),
                            (SilenceMode::Freeze, "Freeze"),
                        ]
                        .into_iter()
                        .map(|(m, label)| seg(
                            label,
                            t.when_silent == m,
                            with(&|n| n.when_silent = m)
                        ))
                        .collect(),
                        Length::Fill,
                        true,
                    ),
                    match t.when_silent {
                        SilenceMode::Fade => "The bars fall flat.",
                        SilenceMode::Wave => "A slow wave keeps moving until the music is back.",
                        SilenceMode::Freeze =>
                            "The bars stop where they are and drawing pauses, which saves battery.",
                    },
                ),
            ]
            .spacing(10)
            .into(),
        );

        let beats = section(
            "Beats",
            column![
                ctx.slider(
                    "beat-sensitivity",
                    "Beat sensitivity",
                    format!("{}%", t.beat_sensitivity),
                    t.beat_sensitivity as f32,
                    (0.0, 100.0, 1.0),
                    set(|t, v| t.beat_sensitivity = v as u8)
                ),
                text("How strong a hit must be to set off Glow and the background pulse. Raise it for quiet music, lower it for heavy EDM.")
                    .size(12)
                    .color(TEXT_3),
            ]
            .spacing(10)
            .into(),
        );

        let timing = section(
            "Timing",
            column![
                ctx.slider(
                    "delay",
                    "Visual delay",
                    if t.delay_ms == 0 {
                        "Off".into()
                    } else {
                        format!("{} ms", t.delay_ms)
                    },
                    t.delay_ms as f32,
                    (0.0, MAX_DELAY_MS as f32, 10.0),
                    set(|t, v| t.delay_ms = v as u16)
                ),
                text(
                    "Holds the bars back to match audio that plays late, like Bluetooth headphones."
                )
                .size(12)
                .color(TEXT_3),
            ]
            .spacing(10)
            .into(),
        );

        let calibrate_label = match self.processor.calibration_remaining() {
            Some(s) => format!("Listening\u{2026} {}s", s.ceil() as u32),
            None => "Auto-calibrate".into(),
        };
        // Built before the footer so the presets come first in Tab order.
        let presets = self.presets_section(&ctx, PresetKind::Tuning);
        let footer = ctx.footer(
            calibrate_label,
            Message::AutoCalibrate,
            "calibrate",
            Message::ResetTuning,
        );

        self.panel_shell_with(
            "Tuning",
            "How the bars react to sound",
            header_close,
            column![
                response, dynamics, frequency, bands, auto_gain, silence, beats, timing, presets
            ]
            .spacing(18)
            .into(),
            footer,
        )
    }

    /// Like `panel_shell`, for panels that build their close button first (Tab order).
    fn panel_shell_with<'a>(
        &self,
        title: &'a str,
        subtitle: &'a str,
        close: Element<'a, Message>,
        content: Element<'a, Message>,
        footer: Element<'a, Message>,
    ) -> Element<'a, Message> {
        let header = row![
            column![
                text(title).font(INTER_SEMIBOLD).size(18).color(TEXT),
                text(subtitle).size(14).color(TEXT_2)
            ]
            .spacing(4)
            .width(Length::Fill),
            close,
        ]
        .align_y(Alignment::Start);

        container(column![header, Self::panel_scroll(content), footer].spacing(18))
            .width(self.layout().panel_width)
            .height(Length::Fill)
            .padding(20)
            .style(panel)
            .into()
    }

    fn visuals_panel(&self) -> Element<'_, Message> {
        let ctx = Ctx::new(self, Scope::Panel);
        let header_close = ctx.close_button(Panel::Visuals, "Close visuals");
        let v = &self.visuals;
        let set = |f: fn(&mut VisualSettings, f32)| {
            let base = v.clone();
            move |x: f32| {
                let mut next = base.clone();
                f(&mut next, x);
                Message::SetVisuals(next)
            }
        };
        let with = |f: &dyn Fn(&mut VisualSettings)| {
            let mut next = v.clone();
            f(&mut next);
            Message::SetVisuals(next)
        };

        let style = v.style;
        let is_wave = style == VisualStyle::Wave;
        let style_section = section(
            "Style",
            ctx.segmented(
                "style",
                [
                    (VisualStyle::Bars, "Bars"),
                    (VisualStyle::Radial, "Radial"),
                    (VisualStyle::Wave, "Wave"),
                ]
                .into_iter()
                .map(|(st, label)| seg(label, style == st, with(&|n| n.style = st)))
                .collect(),
                Length::Fill,
                true,
            ),
        );
        let mut bar_controls = column![
            ctx.slider(
                "bar-count",
                if is_wave { "Points" } else { "Bar count" },
                v.bar_count.to_string(),
                v.bar_count as f32,
                (16.0, 96.0, 1.0),
                set(|v, x| v.bar_count = x as u8),
            ),
            ctx.slider(
                "bar-width",
                if is_wave { "Line width" } else { "Bar width" },
                format!("{} px", v.bar_width),
                v.bar_width as f32,
                (2.0, 16.0, 1.0),
                set(|v, x| v.bar_width = x as u8),
            ),
            ctx.slider(
                "max-length",
                if is_wave { "Max height" } else { "Max length" },
                format!("{} px", v.max_length),
                v.max_length as f32,
                (100.0, 1200.0, 10.0),
                set(|v, x| v.max_length = x as u16),
            ),
        ]
        .spacing(10);
        // Spacing only applies to a row of bars; Radial spaces bars around the ring.
        if style == VisualStyle::Bars {
            bar_controls = bar_controls.push(ctx.slider(
                "spacing",
                "Spacing",
                format!("{} px", v.spacing),
                v.spacing as f32,
                (0.0, 20.0, 1.0),
                set(|v, x| v.spacing = x as u8),
            ));
        }
        if !is_wave {
            bar_controls = bar_controls.push(
                row![
                    text("Corners")
                        .font(INTER_MEDIUM)
                        .size(14)
                        .color(TEXT)
                        .width(Length::Fill),
                    ctx.segmented(
                        "corners",
                        [
                            (Corners::Square, icons::CORNER_SQUARE),
                            (Corners::Rounded, icons::CORNER_ROUNDED),
                            (Corners::Pill, icons::CORNER_PILL),
                        ]
                        .into_iter()
                        .map(|(c, glyph)| Seg {
                            label: SegLabel::Icon(glyph),
                            selected: v.corners == c,
                            msg: with(&|n| n.corners = c),
                        })
                        .collect(),
                        Length::Fixed(44.0 * 3.0 + 8.0 + 6.0),
                        true,
                    ),
                ]
                .align_y(Alignment::Center),
            );
        }
        let bars = section(if is_wave { "Line" } else { "Bars" }, bar_controls.into());

        let mut mirror = column![
            ctx.segmented(
                "mirror",
                [
                    (Mirror::Off, "Off"),
                    (Mirror::UpDown, "Up/down"),
                    (Mirror::LeftRight, "Left/right"),
                    (Mirror::Both, "Both"),
                ]
                .into_iter()
                .map(|(m, label)| seg(label, v.mirror == m, with(&|n| n.mirror = m)))
                .collect(),
                Length::Fill,
                true,
            )
        ]
        .spacing(6);
        if v.mirror.left_right() {
            mirror = mirror.push(
                container(labeled(
                    "Split",
                    ctx.segmented(
                        "split",
                        [
                            (Split::Off, "Off"),
                            (Split::Stereo, "Stereo"),
                            (Split::MidSide, "Mid/Side"),
                        ]
                        .into_iter()
                        .map(|(sp, label)| seg(label, v.split == sp, with(&|n| n.split = sp)))
                        .collect(),
                        Length::Fill,
                        true,
                    ),
                    match v.split {
                        Split::Off => "Both halves show the same spectrum.",
                        Split::Stereo => "Left channel on the left, right channel on the right.",
                        Split::MidSide => "What both channels share on the left, what differs between them on the right.",
                    },
                ))
                .padding(Padding::default().top(6)),
            );
        }
        // One spectrum for every bar: pick which channel it shows.
        if BarMap::for_visuals(v) != BarMap::Split {
            mirror = mirror.push(
                container(labeled(
                    "Channel",
                    ctx.segmented(
                        "channel",
                        [
                            (Channel::Mono, "Mono"),
                            (Channel::Left, "Left"),
                            (Channel::Right, "Right"),
                        ]
                        .into_iter()
                        .map(|(c, label)| seg(label, v.channel == c, with(&|n| n.channel = c)))
                        .collect(),
                        Length::Fill,
                        true,
                    ),
                    "Mono mixes both channels together.",
                ))
                .padding(Padding::default().top(6)),
            );
        }
        if style == VisualStyle::Radial {
            mirror = mirror.push(
                text("In Radial, Up/down also grows bars inward and Left/right puts the bass at the top.")
                    .size(12)
                    .color(TEXT_3),
            );
        }
        let mirror = section("Mirror", mirror.into());

        let mut details = column![
            row![
                ctx.chip(
                    "segments",
                    icons::SEGMENTS,
                    "Segments",
                    v.segments,
                    with(&|n| n.segments = !n.segments)
                ),
                ctx.chip(
                    "peak-caps",
                    icons::PEAK_CAPS,
                    "Peak caps",
                    v.peak_caps,
                    with(&|n| n.peak_caps = !n.peak_caps)
                ),
            ]
            .spacing(8)
        ]
        .spacing(10);
        if v.segments {
            details = details
                .push(ctx.slider(
                    "segment-size",
                    "Segment size",
                    format!("{} px", v.segment_size),
                    v.segment_size as f32,
                    (2.0, 16.0, 1.0),
                    set(|v, x| v.segment_size = x as u8),
                ))
                .push(ctx.slider(
                    "segment-gap",
                    "Segment gap",
                    format!("{} px", v.segment_gap),
                    v.segment_gap as f32,
                    (1.0, 8.0, 1.0),
                    set(|v, x| v.segment_gap = x as u8),
                ));
        }
        let details = section("Details", details.into());

        let mut glow = column![ctx.switch_row(
            "glow",
            "Glow on the beat",
            Some("Bars brighten with the bass or drums"),
            v.glow,
            with(&|n| n.glow = !n.glow),
        )]
        .spacing(10);
        if v.glow {
            glow = glow
                .push(
                    row![
                        text("Sync to")
                            .font(INTER_MEDIUM)
                            .size(14)
                            .color(TEXT)
                            .width(Length::Fixed(72.0)),
                        ctx.segmented(
                            "glow-src",
                            [(GlowSource::Bass, "Bass"), (GlowSource::Drums, "Drums")]
                                .into_iter()
                                .map(|(g, label)| seg(
                                    label,
                                    v.glow_source == g,
                                    with(&|n| n.glow_source = g)
                                ))
                                .collect(),
                            Length::Fill,
                            true,
                        ),
                    ]
                    .align_y(Alignment::Center),
                )
                .push(ctx.slider(
                    "glow-amount",
                    "Intensity",
                    format!("{}%", v.glow_amount),
                    v.glow_amount as f32,
                    (0.0, 100.0, 1.0),
                    set(|v, x| v.glow_amount = x as u8),
                ));
        }
        let glow = section("Glow", glow.into());

        let choices = self.palette_choices();
        let selected_swatch = choices
            .iter()
            .position(|p| std::mem::discriminant(p) == std::mem::discriminant(&v.palette))
            .unwrap_or(0);
        let swatch_fills = choices
            .iter()
            .map(|p| match (p, &self.art) {
                (Palette::Album, Some(art)) => SwatchFill::Image(art.swatch.clone()),
                (Palette::Album, None) => SwatchFill::Stops(ramp(self.target_accent()).to_vec()),
                (Palette::Custom(_), _) => match &v.palette {
                    Palette::Custom(stops) => SwatchFill::Stops(stops.clone()),
                    _ => SwatchFill::Add,
                },
                (p, _) => SwatchFill::Stops(p.stops()),
            })
            .collect();
        let mut color = column![
            row![
                text(tracked("Color"))
                    .font(INTER_SEMIBOLD)
                    .size(11)
                    .color(TEXT_3)
                    .width(Length::Fill),
                text(v.palette.name()).size(14).color(TEXT),
            ]
            .align_y(Alignment::Center),
            ctx.swatches(
                "palette",
                swatch_fills,
                selected_swatch,
                false,
                Message::PickSwatch,
            ),
        ]
        .spacing(12);
        if matches!(v.palette, Palette::Custom(_)) {
            color = color.push(self.custom_editor(&ctx));
        }
        if v.palette == Palette::Album {
            color = color.push(ctx.segmented(
                "album-fill",
                vec![
                    seg(
                        "Solid",
                        !v.album_gradient,
                        with(&|n| n.album_gradient = false),
                    ),
                    seg(
                        "Gradient",
                        v.album_gradient,
                        with(&|n| n.album_gradient = true),
                    ),
                ],
                Length::Fill,
                true,
            ));
            // Only meaningful when the accent is a fixed color, not the cover's.
            if v.album_gradient && self.appearance.accent != Accent::Album {
                color = color.push(ctx.switch_row(
                    "album-match-accent",
                    "Match accent color",
                    Some("Build the gradient from your accent instead of the cover"),
                    v.album_match_accent,
                    with(&|n| n.album_match_accent = !n.album_match_accent),
                ));
            }
        }
        let gradient_enabled = !is_solid(&self.bar_stops());
        color = color.push(
            ctx.segmented(
                "gradient",
                // Same two settings; the labels describe them for the current style.
                match style {
                    VisualStyle::Bars => [
                        (GradientDir::Across, "Across bars"),
                        (GradientDir::Along, "Along each bar"),
                    ],
                    VisualStyle::Radial => [
                        (GradientDir::Across, "Around the ring"),
                        (GradientDir::Along, "Inside out"),
                    ],
                    VisualStyle::Wave => [
                        (GradientDir::Across, "Left to right"),
                        (GradientDir::Along, "Base to peaks"),
                    ],
                }
                .into_iter()
                .map(|(g, label)| seg(label, v.gradient == g, with(&|n| n.gradient = g)))
                .collect(),
                Length::Fill,
                gradient_enabled,
            ),
        );

        let presets = self.presets_section(&ctx, PresetKind::Visuals);
        // Segments and peak caps are bar features; the wave has neither.
        let mut content = column![style_section, bars, mirror].spacing(18);
        if !is_wave {
            content = content.push(details);
        }
        let content = content.push(glow).push(color).push(presets);

        let saved = self
            .preset_saved_at
            .is_some_and(|t| self.now - t < Duration::from_millis(1500));
        let footer = ctx.footer(
            if saved {
                "Saved".into()
            } else {
                "Save as preset".into()
            },
            Message::SavePreset(PresetKind::Visuals),
            "save-preset",
            Message::ResetVisuals,
        );

        self.panel_shell_with(
            "Visuals",
            "How the bars look",
            header_close,
            content.into(),
            footer,
        )
    }

    /// Saved presets: click to load; the pencil opens rename / reorder / delete.
    /// Each panel has its own list. Visuals saves from its footer; Tuning and
    /// Appearance have their own Save button at the end of the list.
    fn presets_section<'a>(&'a self, ctx: &Ctx<'a>, kind: PresetKind) -> Element<'a, Message> {
        // (name, matches the current settings)
        let items: Vec<(&'a str, bool)> = match kind {
            PresetKind::Visuals => self
                .presets
                .iter()
                .map(|p| (p.name.as_str(), p.visuals == self.visuals))
                .collect(),
            PresetKind::Tuning => self
                .tuning_presets
                .iter()
                .map(|p| (p.name.as_str(), p.tuning == self.tuning))
                .collect(),
            PresetKind::Appearance => self
                .appearance_presets
                .iter()
                .map(|p| {
                    (
                        p.name.as_str(),
                        self.appearance_from_preset(p) == self.appearance,
                    )
                })
                .collect(),
        };
        let save = (kind != PresetKind::Visuals).then(|| {
            let saved = self
                .preset_saved_at
                .is_some_and(|t| self.now - t < Duration::from_millis(1500));
            ctx.secondary_button(
                FocusId::Named("save-preset"),
                if saved { "Saved" } else { "Save as preset" }.into(),
                Message::SavePreset(kind),
            )
        });
        if items.is_empty() {
            let help = match kind {
                PresetKind::Visuals => "Save as preset stores the current look here.",
                PresetKind::Tuning => "Save the current tuning to switch back to it in one click.",
                PresetKind::Appearance => {
                    "Save the current background, now playing, lyrics and accent settings."
                }
            };
            let mut content = column![text(help).size(12).color(TEXT_3)].spacing(10);
            if let Some(save) = save {
                content = content.push(save);
            }
            return section("Presets", content.into());
        }
        let last = items.len() - 1;
        let mut list = column![].spacing(8);
        for (i, (name, current)) in items.into_iter().enumerate() {
            if self.editing_preset == Some((kind, i)) {
                ctx.text_input_stop(FocusId::Indexed("preset-name", i), PRESET_NAME_INPUT);
                let name = text_input("Preset name", name)
                    .id(iced::widget::Id::new(PRESET_NAME_INPUT))
                    .on_input(move |s| Message::RenamePreset(kind, i, s))
                    .on_submit(Message::EditPreset(kind, None))
                    .size(14)
                    .padding([12, 12])
                    .style(move |_, status| hex_input_style(status, true));
                let armed = self.confirm_delete == Some((kind, i));
                let delete_focused = ctx.focus(
                    FocusId::Indexed("preset-delete", i),
                    Some(Message::DeletePreset(kind, i)),
                    None,
                );
                let delete = button(center(
                    text(if armed { "Delete?" } else { "Delete" })
                        .font(INTER_MEDIUM)
                        .size(14),
                ))
                .width(96)
                .height(44)
                .padding(0)
                .style(ringed(delete_focused, danger_button(armed)))
                .on_press(Message::DeletePreset(kind, i));
                list = list.push(
                    column![
                        name,
                        row![
                            ctx.icon_button(
                                FocusId::Indexed("preset-up", i),
                                icons::CHEVRON_UP,
                                "Move up",
                                (i > 0).then_some(Message::MovePreset(kind, i, -1))
                            ),
                            ctx.icon_button(
                                FocusId::Indexed("preset-down", i),
                                icons::CHEVRON_DOWN,
                                "Move down",
                                (i < last).then_some(Message::MovePreset(kind, i, 1))
                            ),
                            delete,
                            Space::new().width(Length::Fill),
                            ctx.secondary_button(
                                FocusId::Indexed("preset-done", i),
                                "Done".into(),
                                Message::EditPreset(kind, None)
                            ),
                        ]
                        .spacing(8)
                        .align_y(Alignment::Center),
                    ]
                    .spacing(8),
                );
            } else {
                let focused = ctx.focus(
                    FocusId::Indexed("preset", i),
                    Some(Message::LoadPreset(kind, i)),
                    None,
                );
                let mut label = row![
                    text(name)
                        .size(14)
                        .font(INTER_MEDIUM)
                        .wrapping(Wrapping::None)
                        .width(Length::Fill)
                ]
                .align_y(Alignment::Center);
                if current {
                    label = label.push(icon(icons::CHECK, 16.0, TEXT));
                }
                let load = tip(
                    button(container(label).center_y(Length::Fill))
                        .width(Length::Fill)
                        .height(44)
                        .padding([0, 14])
                        .style(ringed(focused, preset_row(current)))
                        .on_press(Message::LoadPreset(kind, i)),
                    if current {
                        "Current settings"
                    } else {
                        "Load preset"
                    },
                    tooltip::Position::Top,
                );
                list = list.push(
                    row![
                        load,
                        ctx.icon_button(
                            FocusId::Indexed("preset-edit", i),
                            icons::PENCIL,
                            "Rename, reorder or delete",
                            Some(Message::EditPreset(kind, Some(i)))
                        ),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                );
            }
        }
        if let Some(save) = save {
            list = list.push(save);
        }
        section("Presets", list.into())
    }

    fn appearance_panel(&self) -> Element<'_, Message> {
        let ctx = Ctx::new(self, Scope::Panel);
        let a = &self.appearance;
        let with = |f: &dyn Fn(&mut AppearanceSettings)| {
            let mut next = a.clone();
            f(&mut next);
            Message::SetAppearance(next)
        };
        let set = |f: fn(&mut AppearanceSettings, f32)| {
            let base = a.clone();
            move |x: f32| {
                let mut next = base.clone();
                f(&mut next, x);
                Message::SetAppearance(next)
            }
        };

        // Title and buttons share the first row so the subtitle gets the full width.
        let header = column![
            row![
                text("Appearance")
                    .font(INTER_SEMIBOLD)
                    .size(18)
                    .color(TEXT)
                    .width(Length::Fill),
                ctx.text_button(
                    FocusId::Named("reset"),
                    "Reset".into(),
                    Message::ResetAppearance,
                    Length::Shrink
                ),
                ctx.close_button(Panel::Appearance, "Close appearance"),
            ]
            .spacing(4)
            .align_y(Alignment::Center),
            text("Background, now playing and window")
                .size(13)
                .color(TEXT_2),
        ]
        .spacing(0);

        // Background: controls that don't apply to the current mode are removed.
        let mut background = column![
            ctx.segmented(
                "bg-mode",
                [
                    (BackgroundMode::Solid, "Solid"),
                    (BackgroundMode::AlbumArt, "Album art"),
                    (BackgroundMode::Ambient, "Ambient")
                ]
                .into_iter()
                // Album art needs a music player connection.
                .filter(|(m, _)| self.platform.media || *m != BackgroundMode::AlbumArt)
                .map(|(m, label)| seg(label, a.bg_mode == m, with(&|n| n.bg_mode = m)))
                .collect(),
                Length::Fill,
                true,
            )
        ]
        .spacing(10);
        match a.bg_mode {
            BackgroundMode::Solid => {
                let selected = SolidColor::ALL
                    .iter()
                    .position(|c| *c == a.solid_color)
                    .unwrap_or(0);
                background = background
                    .push(label_row("Color", a.solid_color.name()))
                    .push(
                        ctx.swatches(
                            "solid",
                            SolidColor::ALL
                                .iter()
                                .map(|c| SwatchFill::Stops(vec![c.rgb()]))
                                .collect(),
                            selected,
                            true,
                            Message::PickSolid,
                        ),
                    );
            }
            BackgroundMode::AlbumArt | BackgroundMode::Ambient => {
                background = background.push(
                    row![
                        text("Motion")
                            .font(INTER_MEDIUM)
                            .size(14)
                            .color(TEXT)
                            .width(Length::Fixed(64.0)),
                        ctx.segmented(
                            "motion",
                            [
                                (Motion::Still, "Still"),
                                (Motion::Drift, "Drift"),
                                (Motion::Spiral, "Spiral")
                            ]
                            .into_iter()
                            .map(|(m, label)| seg(label, a.motion == m, with(&|n| n.motion = m)))
                            .collect(),
                            Length::Fill,
                            true,
                        ),
                    ]
                    .align_y(Alignment::Center),
                );
                if a.bg_mode == BackgroundMode::AlbumArt {
                    background = background.push(ctx.slider(
                        "blur",
                        "Blur",
                        format!("{} px", a.blur_px),
                        a.blur_px as f32,
                        (0.0, 120.0, 1.0),
                        set(|a, x| a.blur_px = x as u8),
                    ));
                }
                background = background.push(ctx.slider(
                    "dim",
                    "Dim",
                    format!("{}%", a.dim_pct),
                    a.dim_pct as f32,
                    (0.0, 90.0, 1.0),
                    set(|a, x| a.dim_pct = x as u8),
                ));
                background = background.push(ctx.switch_row(
                    "bg-pulse",
                    "Pulse with the beat",
                    Some("The background swells with the bass or drums"),
                    a.bg_pulse,
                    with(&|n| n.bg_pulse = !n.bg_pulse),
                ));
                if a.bg_pulse {
                    background = background
                        .push(
                            row![
                                text("Sync to")
                                    .font(INTER_MEDIUM)
                                    .size(14)
                                    .color(TEXT)
                                    .width(Length::Fixed(72.0)),
                                ctx.segmented(
                                    "bg-pulse-src",
                                    [(GlowSource::Bass, "Bass"), (GlowSource::Drums, "Drums")]
                                        .into_iter()
                                        .map(|(g, label)| seg(
                                            label,
                                            a.bg_pulse_source == g,
                                            with(&|n| n.bg_pulse_source = g)
                                        ))
                                        .collect(),
                                    Length::Fill,
                                    true,
                                ),
                            ]
                            .align_y(Alignment::Center),
                        )
                        .push(ctx.slider(
                            "bg-pulse-amount",
                            "Intensity",
                            format!("{}%", a.bg_pulse_amount),
                            a.bg_pulse_amount as f32,
                            (0.0, 100.0, 1.0),
                            set(|a, x| a.bg_pulse_amount = x as u8),
                        ));
                }
            }
        }

        // Track info and lyrics need a music player connection; without one, leave
        // them out (built only when shown, so they don't take Tab stops).
        let media_sections = self.platform.media.then(|| {
            let mut now_playing = column![
                ctx.segmented(
                    "now-playing",
                    [
                        (NowPlayingMode::Header, "Header"),
                        (NowPlayingMode::Spotlight, "Spotlight"),
                        (NowPlayingMode::Minimal, "Minimal"),
                        (NowPlayingMode::Off, "Off"),
                    ]
                    .into_iter()
                    .map(|(m, label)| seg(label, a.now_playing == m, with(&|n| n.now_playing = m)))
                    .collect(),
                    Length::Fill,
                    true,
                )
            ]
            .spacing(4);
            if a.now_playing != NowPlayingMode::Off {
                now_playing = now_playing.push(ctx.switch_row(
                    "show-art",
                    "Show album art",
                    Some("Next to the track info"),
                    a.show_album_art,
                    with(&|n| n.show_album_art = !n.show_album_art),
                ));
            }

            let mut lyrics = column![ctx.switch_row(
                "lyrics",
                "Show lyrics",
                Some("Synced lyrics from LRCLIB, or a .lrc file next to the song"),
                a.lyrics,
                with(&|n| n.lyrics = !n.lyrics),
            )]
            .spacing(4);
            if a.lyrics {
                lyrics = lyrics
                    .push(ctx.slider(
                        "lyrics-size",
                        "Text size",
                        format!("{}%", a.lyrics_size),
                        a.lyrics_size as f32,
                        (70.0, 150.0, 5.0),
                        set(|a, x| a.lyrics_size = x as u8),
                    ))
                    .push(ctx.slider(
                        "lyrics-glow",
                        "Glow",
                        if a.lyrics_glow == 0 {
                            "Off".to_string()
                        } else {
                            format!("{}%", a.lyrics_glow)
                        },
                        a.lyrics_glow as f32,
                        (0.0, 100.0, 5.0),
                        set(|a, x| a.lyrics_glow = x as u8),
                    ))
                    .push(ctx.slider(
                        "lyrics-timing",
                        "Timing",
                        match a.lyrics_offset_ms {
                            0 => "In sync".to_string(),
                            ms if ms > 0 => format!("{:.2} s earlier", ms as f32 / 1000.0),
                            ms => format!("{:.2} s later", -ms as f32 / 1000.0),
                        },
                        a.lyrics_offset_ms as f32,
                        (-2000.0, 2000.0, 50.0),
                        set(|a, x| a.lyrics_offset_ms = x as i16),
                    ))
                    .push(text(self.lyrics_status_text()).size(12).color(TEXT_3));
            }
            (
                section("Now playing", now_playing.into()),
                section("Lyrics", lyrics.into()),
            )
        });

        let mut interface = column![ctx.switch_row(
            "auto-hide",
            "Auto-hide controls",
            Some("Hides menus when the mouse is still"),
            a.auto_hide,
            with(&|n| n.auto_hide = !n.auto_hide),
        )]
        .spacing(4);
        if a.auto_hide {
            interface = interface.push(ctx.slider(
                "hide-after",
                "Hide after",
                format!("{} s", a.hide_after_s),
                a.hide_after_s as f32,
                (1.0, 10.0, 1.0),
                set(|a, x| a.hide_after_s = x as u8),
            ));
        }
        if self.platform.always_on_top {
            interface = interface.push(ctx.switch_row(
                "on-top",
                "Always on top",
                None,
                a.always_on_top,
                with(&|n| n.always_on_top = !n.always_on_top),
            ));
        }
        interface = interface.push(
            container(labeled(
                "Frame rate",
                ctx.segmented(
                    "frame-rate",
                    [
                        (FrameRate::Fps30, "30 fps"),
                        (FrameRate::Fps60, "60 fps"),
                        (FrameRate::Display, "Display"),
                    ]
                    .into_iter()
                    .map(|(f, label)| seg(label, a.frame_rate == f, with(&|n| n.frame_rate = f)))
                    .collect(),
                    Length::Fill,
                    true,
                ),
                "Lower rates use less power. Display matches your screen's refresh rate.",
            ))
            .padding(Padding::default().top(6)),
        );

        let accents = self.accent_choices();
        let accent_swatches = accents
            .iter()
            .map(|acc| match (acc.rgb(), &self.art) {
                (Some(c), _) => SwatchFill::Stops(vec![c]),
                (None, Some(art)) => SwatchFill::Image(art.swatch.clone()),
                (None, None) => SwatchFill::Stops(vec![ACCENT_BLUE]),
            })
            .collect();
        let selected_accent = accents.iter().position(|x| *x == a.accent).unwrap_or(0);
        let accent_section = column![
            row![
                text(tracked("Accent"))
                    .font(INTER_SEMIBOLD)
                    .size(11)
                    .color(TEXT_3)
                    .width(Length::Fill),
                text(a.accent.name()).size(14).color(TEXT),
            ]
            .align_y(Alignment::Center),
            ctx.swatches(
                "accent",
                accent_swatches,
                selected_accent,
                false,
                Message::PickAccent
            ),
        ]
        .spacing(10);

        let mut content = column![section("Background", background.into())].spacing(16);
        if let Some((now_playing, lyrics)) = media_sections {
            content = content.push(now_playing).push(lyrics);
        }
        let content = content
            .push(section("Interface", interface.into()))
            .push(accent_section)
            .push(self.presets_section(&ctx, PresetKind::Appearance));

        container(column![header, Self::panel_scroll(content)].spacing(16))
            .width(self.layout().panel_width)
            .height(Length::Fill)
            .padding(20)
            .style(panel_translucent)
            .into()
    }

    fn about_panel(&self) -> Element<'_, Message> {
        let ctx = Ctx::new(self, Scope::Panel);
        let header = row![
            text("About")
                .font(INTER_SEMIBOLD)
                .size(18)
                .color(TEXT)
                .width(Length::Fill),
            ctx.close_button(Panel::About, "Close about"),
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

        container(column![header, Self::panel_scroll(content)].spacing(16))
            .width(self.layout().panel_width)
            .height(Length::Fill)
            .padding(20)
            .style(panel_translucent)
            .into()
    }

    fn lyrics_status_text(&self) -> String {
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

    fn custom_editor<'a>(&'a self, ctx: &Ctx<'a>) -> Element<'a, Message> {
        const IDS: [&str; 3] = ["hex-0", "hex-1", "hex-2"];
        let field = |i: usize, placeholder: &'static str| {
            ctx.text_input_stop(FocusId::Indexed("hex", i), IDS[i]);
            let valid =
                self.custom_hex[i].trim().is_empty() || parse_hex(&self.custom_hex[i]).is_some();
            text_input(placeholder, &self.custom_hex[i])
                .id(iced::widget::Id::new(IDS[i]))
                .on_input(move |s| Message::CustomHex(i, s))
                .size(13)
                .padding([10, 10])
                .style(move |_, status| hex_input_style(status, valid))
        };
        column![
            row![field(0, "#start"), field(1, "#middle"), field(2, "#end")].spacing(8),
            text("Hex colors, left to right. Leave a field empty to skip it.")
                .size(12)
                .color(TEXT_3),
        ]
        .spacing(6)
        .into()
    }
}

// ---------------------------------------------------------------------------
// Small view helpers

/// Label on the left, current value on the right.
fn label_row<'a>(label: &'a str, value: &'a str) -> Element<'a, Message> {
    row![
        text(label)
            .font(INTER_MEDIUM)
            .size(14)
            .color(TEXT)
            .width(Length::Fill),
        text(value).size(14).color(TEXT_2),
    ]
    .align_y(Alignment::Center)
    .into()
}

/// Rounded cover art with a soft shadow.
fn cover_image<'a>(
    handle: &Handle,
    size: f32,
    radius: f32,
    blur: f32,
    offset_y: f32,
    shadow: f32,
) -> Element<'a, Message> {
    container(
        image(handle.clone())
            .width(size)
            .height(size)
            .border_radius(radius)
            .filter_method(image::FilterMethod::Linear),
    )
    .style(cover_shadow(radius, blur, offset_y, shadow))
    .into()
}

/// A control with a label above it and help text below.
fn labeled<'a>(
    label: &'a str,
    control: Element<'a, Message>,
    help: &'a str,
) -> Element<'a, Message> {
    column![
        text(label).font(INTER_MEDIUM).size(14).color(TEXT),
        control,
        text(help).size(12).color(TEXT_3)
    ]
    .spacing(6)
    .into()
}

/// A whole number with a typographic minus.
fn signed(v: i8) -> String {
    if v < 0 {
        format!("\u{2212}{}", -(v as i16))
    } else {
        v.to_string()
    }
}

fn contrast_label(c: u8) -> String {
    match c {
        0..=35 => format!("Soft \u{00B7} {c}"),
        65..=100 => format!("Punchy \u{00B7} {c}"),
        _ => format!("Linear \u{00B7} {c}"),
    }
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

fn hex_input_style(status: text_input::Status, valid: bool) -> text_input::Style {
    let focused = matches!(status, text_input::Status::Focused { .. });
    text_input::Style {
        background: CONTROL_SUNKEN.into(),
        border: iced::Border {
            radius: 10.0.into(),
            width: if focused { 2.0 } else { 1.0 },
            color: if !valid {
                iced::color!(0xFF6B57)
            } else if focused {
                TEXT
            } else {
                CONTROL_SUNKEN
            },
        },
        icon: TEXT_2,
        placeholder: TEXT_3,
        value: TEXT,
        selection: with_alpha(ACCENT, 0.4),
    }
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

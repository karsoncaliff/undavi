//! The rows settings are made of: a label on the left and a control on the right,
//! or the control underneath where there's no room (or it needs all of it).

use super::navigation::page_icon;
use super::pictures::{DIAGRAM_SIZE, Diagram, Picture};
use crate::app::settings::{Change, FOUND_LIT, Page, SettingKey, SettingsMessage, row_id};
use crate::app::{App, Message, RowFocus};
use crate::controls::{
    SLIDER_HEIGHT, SwatchFill, range_slider, slider, step_range, swatches, switch,
};
use crate::focus::{FocusId, Scope, Step};
use crate::icons::{self, icon};
use crate::settings::{Action, MAX_KEYS};
use crate::theme::*;
use crate::view::quick::{quick_label, quick_slider, quick_tile};
use crate::view::{Ctx, tip};
use iced::widget::text::Wrapping;
use iced::widget::{
    Canvas, Space, button, center, column, container, row, text, text_input, tooltip,
};
use iced::{Alignment, Element, Length, Padding};
use serde_json::Value as Json;
use std::cell::{Cell, OnceCell, RefCell};
use std::ops::Range;
use std::rc::Rc;

/// Space inside a card, left and right.
const PAD: f32 = 16.0;
/// Below this, a slider goes under its label.
const WIDE: f32 = 420.0;
/// How wide one is beside it: a share of the row.
const SLIDER_SHARE: f32 = 0.32;
const SLIDER_WIDTH: (f32, f32) = (150.0, 320.0);
/// Room for a slider's value, so sliders line up.
const VALUE_WIDTH: f32 = 112.0;
/// The rough width of one character of a label, and a segment's padding.
const CHAR: f32 = 7.4;
const SEGMENT_PAD: f32 = 14.0;
const SEGMENT_GAP: f32 = 2.0;
/// Picture cards are at least this wide.
const CARD_MIN: f32 = 116.0;
const CARD_GAP: f32 = 8.0;
/// A group's heading, and the undo button (so a row doesn't grow for it).
const HEADING: f32 = 34.0;
const UNDO: f32 = 22.0;
/// The star in front of a setting's name, and the space after it.
const STAR: f32 = 22.0;
const STAR_GAP: f32 = 4.0;
/// Picture cards in the quick settings are at least this wide.
const QUICK_CARD_MIN: f32 = 84.0;

/// One choice of a segmented control.
pub struct Seg {
    pub label: String,
    pub selected: bool,
    pub msg: Message,
}

pub fn seg(label: impl Into<String>, selected: bool, msg: Message) -> Seg {
    Seg {
        label: label.into(),
        selected,
        msg,
    }
}

/// One choice with a picture.
pub struct Card {
    pub picture: Picture,
    pub label: String,
    /// Something's wrong with it (a shader that doesn't compile).
    pub error: bool,
    pub selected: bool,
    pub msg: Message,
}

/// A group of rows under a heading, which folds them away.
pub struct Group<'a> {
    pub title: String,
    pub rows: Vec<Element<'a, Message>>,
    page: Page,
    folded: bool,
    /// The heading has the keyboard focus.
    focused: bool,
}

impl<'a> Group<'a> {
    pub fn push(mut self, row: impl Into<Element<'a, Message>>) -> Self {
        self.rows.push(row.into());
        self
    }

    pub fn push_maybe(mut self, row: Option<Element<'a, Message>>) -> Self {
        self.rows.extend(row);
        self
    }

    /// The heading and its card.
    pub fn view(self) -> Element<'a, Message> {
        let chevron = if self.folded {
            icons::CHEVRON_RIGHT
        } else {
            icons::CHEVRON_DOWN
        };
        let heading = button(
            container(
                row![
                    icon(chevron, 18.0, TEXT_3),
                    text(self.title.clone())
                        .font(INTER_SEMIBOLD)
                        .size(13)
                        .color(TEXT_2),
                ]
                .spacing(4)
                .align_y(Alignment::Center),
            )
            .center_y(Length::Fill),
        )
        .width(Length::Fill)
        .height(HEADING)
        .padding([0, 4])
        .style(ringed(self.focused, tree_row(None, TEXT_2)))
        .on_press(SettingsMessage::Fold(self.page, self.title).into());
        if self.folded {
            return heading.into();
        }
        let mut rows = column![];
        for (i, row) in self.rows.into_iter().enumerate() {
            if i > 0 {
                rows = rows.push(
                    container(Space::new().height(1))
                        .width(Length::Fill)
                        .style(fill(LINE)),
                );
            }
            rows = rows.push(row);
        }
        column![heading, container(rows).width(Length::Fill).style(card)]
            .spacing(4)
            .into()
    }
}

/// The kinds of settings there are in the quick settings, in the order they're in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Choice,
    Slider,
    Switch,
    Color,
    Action,
}

/// A starred setting, as it is in the quick settings.
pub struct Quick<'a> {
    pub kind: Kind,
    /// Its place among the starred ones.
    pub order: usize,
    /// Its Tab stops.
    pub stops: Range<usize>,
    pub view: Element<'a, Message>,
}

/// Where a row goes.
#[derive(Clone, Copy)]
enum Place {
    /// The settings window.
    Page,
    /// The quick settings, at this place among the starred ones.
    Quick(usize),
}

/// Builds rows, in cards of one width. (Or, for the quick settings, only the starred
/// ones, the way they look there.)
pub struct Rows<'a> {
    pub ctx: Ctx<'a>,
    page: Page,
    /// The room inside a card.
    width: f32,
    /// The setting the next row is for, and what its undo button does.
    key: Cell<Option<(SettingKey, Option<Change>)>>,
    /// What it's called as a starred setting.
    star: Cell<Option<String>>,
    /// The starred ones built so far, for the quick settings, and where the Tab stops
    /// of the one being built start.
    quick: Option<RefCell<Vec<Quick<'a>>>>,
    stops: Cell<usize>,
    /// The section's settings, as they are and as Undo leaves them.
    json: OnceCell<(Json, Json)>,
    /// How many groups, and undoable rows, there have been.
    groups: Cell<usize>,
    undoable: Cell<usize>,
}

impl<'a> Rows<'a> {
    /// For cards `card` wide.
    pub fn new(app: &'a App, page: Page, card: f32) -> Self {
        Self {
            ctx: Ctx::new(app, Scope::Content),
            page,
            width: (card - 2.0 * PAD).max(0.0),
            key: Cell::new(None),
            star: Cell::new(None),
            quick: None,
            stops: Cell::new(0),
            json: OnceCell::new(),
            groups: Cell::new(0),
            undoable: Cell::new(0),
        }
    }

    /// The starred ones of a page, `width` wide, for the quick settings.
    pub fn quick(app: &'a App, page: Page, width: f32) -> Self {
        let ctx = Ctx::new(app, Scope::Quick);
        ctx.still.set(true);
        Self {
            ctx,
            width,
            quick: Some(RefCell::default()),
            ..Self::new(app, page, width)
        }
    }

    pub fn into_quick(self) -> Vec<Quick<'a>> {
        self.quick.map(RefCell::into_inner).unwrap_or_default()
    }

    /// Where the next row goes; `None` if it's left out.
    fn place(&self) -> Option<Place> {
        if self.quick.is_none() {
            return Some(Place::Page);
        }
        self.key.take();
        let star = self.star.take()?;
        let order = self.ctx.app.favorites.iter().position(|f| *f == star)?;
        self.star.set(Some(star));
        self.ctx.still.set(false);
        self.stops.set(self.ctx.app.focus_reg.len());
        Some(Place::Quick(order))
    }

    /// Put a row in the quick settings.
    fn add_quick(
        &self,
        order: usize,
        kind: Kind,
        view: impl Into<Element<'a, Message>>,
    ) -> Element<'a, Message> {
        self.star.take();
        self.ctx.still.set(true);
        if let Some(quick) = &self.quick {
            quick.borrow_mut().push(Quick {
                kind,
                order,
                stops: self.stops.get()..self.ctx.app.focus_reg.len(),
                view: view.into(),
            });
        }
        Space::new().into()
    }

    /// The icon of the page's section.
    fn glyph(&self) -> &'static str {
        page_icon(self.page)
    }

    /// Start a group. The rows built until the next one belong to it: when it's folded
    /// away, they aren't Tab stops.
    pub fn group(&self, title: impl Into<String>) -> Group<'a> {
        let title = title.into();
        if self.quick.is_some() {
            return Group {
                title,
                rows: Vec::new(),
                page: self.page,
                folded: false,
                focused: false,
            };
        }
        let folded = self.ctx.app.settings.folded(self.page, &title);
        let i = self.groups.get();
        self.groups.set(i + 1);
        self.ctx.still.set(false);
        let fold = SettingsMessage::Fold(self.page, title.clone()).into();
        let focused = self
            .ctx
            .focus(FocusId::Indexed("fold", i), Some(fold), None);
        self.ctx.still.set(folded);
        Group {
            title,
            rows: Vec::new(),
            page: self.page,
            folded,
            focused,
        }
    }

    /// The next row changes these settings of the section (by their saved names), and
    /// gets an undo button for them.
    pub fn on(&self, fields: &'static [&'static str]) -> &Self {
        if let Page::Section(id) = self.page {
            let (now, base) = self.json.get_or_init(|| self.ctx.app.section_json(id));
            debug_assert!(fields.iter().all(|f| now.get(f).is_some()), "{fields:?}");
            let differs = !base.is_null() && fields.iter().any(|f| now.get(f) != base.get(f));
            self.on_key(SettingKey::Fields(id, fields), differs);
        }
        self
    }

    /// The same for any setting, which differs from what Undo goes back to or not.
    pub fn on_key(&self, key: SettingKey, differs: bool) -> &Self {
        let change = self.ctx.app.change(&key, differs);
        self.star.set(key.favorite());
        self.key.set(Some((key, change)));
        self
    }

    /// The next row can be starred, though there's nothing to undo.
    pub fn on_star(&self, id: &str) -> &Self {
        self.star.set(Some(id.to_string()));
        self
    }

    /// The star in front of a setting's name.
    fn star(&self, id: String) -> Element<'a, Message> {
        let on = self.ctx.app.favorites.contains(&id);
        let (glyph, color, name) = if on {
            (
                icons::STAR_FILLED,
                self.ctx.accent,
                "Remove from quick settings",
            )
        } else {
            (icons::STAR, TEXT_3, "Add to quick settings")
        };
        tip(
            button(container(icon(glyph, 18.0, color)).center_y(Length::Fill))
                .width(STAR)
                .height(UNDO)
                .padding(Padding::default().left(1))
                .style(tree_row(None, TEXT))
                .on_press(Message::ToggleFavorite(id)),
            name,
            tooltip::Position::Top,
        )
    }

    /// The undo (or redo) button of the row's setting, if it has one.
    fn undo(&self, key: SettingKey, change: Option<Change>) -> Option<Element<'a, Message>> {
        let i = self.undoable.get();
        self.undoable.set(i + 1);
        let (glyph, name, msg) = match change {
            Some(Change::Undo) => (icons::UNDO, "Undo", SettingsMessage::Undo(key)),
            Some(Change::Redo) => (icons::REDO, "Redo", SettingsMessage::Redo(key)),
            None => return None,
        };
        let msg = Message::from(msg);
        let focused = (self.ctx).focus(FocusId::Indexed("undo", i), Some(msg.clone()), None);
        Some(tip(
            button(center(icon(glyph, 18.0, TEXT_2)))
                .width(28)
                .height(UNDO)
                .padding(0)
                .style(ringed(focused, tree_row(None, TEXT)))
                .on_press(msg),
            name,
            tooltip::Position::Top,
        ))
    }

    /// Remember the control of a row, so a search can go to it.
    pub fn note_focus(&self, label: &str, id: &FocusId) {
        if !self.ctx.still.get() {
            let star = self.star.take();
            self.star.set(star.clone());
            self.ctx.app.row_focus.borrow_mut().push(RowFocus {
                label: label.to_string(),
                focus: id.clone(),
                favorite: star,
            });
        }
    }

    /// A finished row: one a search can find, lit for a moment once it has.
    pub fn found(&self, label: &str, row: Element<'a, Message>) -> Element<'a, Message> {
        let lit = match &self.ctx.app.settings.found {
            Some((found, at)) if found == label => {
                let t = self.ctx.app.now.saturating_duration_since(*at);
                1.0 - (t.as_secs_f32() / FOUND_LIT.as_secs_f32()).clamp(0.0, 1.0)
            }
            _ => 0.0,
        };
        let accent = self.ctx.accent;
        container(row)
            .id(row_id(label))
            .width(Length::Fill)
            .style(move |_| container::Style {
                background: (lit > 0.0).then(|| with_alpha(accent, 0.22 * lit).into()),
                border: iced::border::rounded(12.0),
                ..container::Style::default()
            })
            .into()
    }

    /// The room inside a card.
    pub fn width(&self) -> f32 {
        self.width
    }

    fn wide(&self) -> bool {
        self.width >= WIDE
    }

    fn labels(&self, label: String, help: Option<String>) -> Element<'a, Message> {
        let star = self.star.take();
        let key = self.key.take();
        let name = text(label).font(INTER_MEDIUM).size(14).color(TEXT);
        // As tall with the buttons as without, so nothing moves when one appears.
        let (head, gap): (Element<'a, Message>, f32) = if key.is_some() || star.is_some() {
            let undo = key.and_then(|(key, change)| self.undo(key, change));
            let head = row![name]
                .extend(undo)
                .spacing(4)
                .align_y(Alignment::Center);
            (container(head).center_y(UNDO).into(), 0.0)
        } else {
            (name.into(), 2.0)
        };
        let mut col = column![head].spacing(gap).width(Length::Fill);
        if let Some(help) = help {
            col = col.push(text(help).size(12).color(TEXT_3));
        }
        // The star is in front of all of it.
        match star {
            Some(id) => row![self.star(id), col].spacing(STAR_GAP).into(),
            None => col.into(),
        }
    }

    fn padded(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
        container(content)
            .padding([6.0, PAD])
            .width(Length::Fill)
            .into()
    }

    /// Anything, with a card's padding.
    pub fn plain(&self, content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
        container(content)
            .padding([14.0, PAD])
            .width(Length::Fill)
            .into()
    }

    /// A label and a control beside it.
    pub fn row(
        &self,
        label: impl Into<String>,
        help: Option<String>,
        control: impl Into<Element<'a, Message>>,
    ) -> Element<'a, Message> {
        let label = label.into();
        let row = Self::padded(
            row![self.labels(label.clone(), help), control.into()]
                .spacing(16)
                .align_y(Alignment::Center),
        );
        self.found(&label, row)
    }

    /// A label (and the control's value) with the control underneath.
    fn stacked(
        &self,
        label: impl Into<String>,
        value: Option<String>,
        help: Option<String>,
        control: impl Into<Element<'a, Message>>,
    ) -> Element<'a, Message> {
        let label = label.into();
        let mut head = row![self.labels(label.clone(), help)].spacing(16);
        if let Some(value) = value {
            head = head.push(text(value).size(14).color(TEXT_2));
        }
        let row = container(column![head, control.into()].spacing(8))
            .padding([12.0, PAD])
            .width(Length::Fill);
        self.found(&label, row.into())
    }

    /// A slider with its value.
    fn with_value(
        &self,
        label: String,
        help: Option<String>,
        display: String,
        control: impl Into<Element<'a, Message>>,
    ) -> Element<'a, Message> {
        if !self.wide() {
            // A slider has its own space around the track.
            let head = row![
                self.labels(label.clone(), help),
                text(display).size(14).color(TEXT_2)
            ]
            .spacing(16);
            let row = container(column![head, control.into()])
                .padding(Padding {
                    top: 12.0,
                    right: PAD,
                    bottom: 2.0,
                    left: PAD,
                })
                .width(Length::Fill);
            return self.found(&label, row.into());
        }
        let width = (self.width * SLIDER_SHARE).clamp(SLIDER_WIDTH.0, SLIDER_WIDTH.1);
        let row = Self::padded(
            row![
                self.labels(label.clone(), help),
                container(control).width(width).height(SLIDER_HEIGHT),
                text(display)
                    .size(14)
                    .color(TEXT_2)
                    .wrapping(Wrapping::None)
                    .width(VALUE_WIDTH)
                    .align_x(iced::alignment::Horizontal::Right),
            ]
            .align_y(Alignment::Center),
        );
        self.found(&label, row)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn slider<F>(
        &self,
        id: impl Into<FocusId>,
        label: impl Into<String>,
        help: Option<&str>,
        display: String,
        value: f32,
        range: (f32, f32, f32),
        on_change: F,
    ) -> Element<'a, Message>
    where
        F: Fn(f32) -> Message + Clone + 'static,
    {
        let Some(place) = self.place() else {
            return Space::new().into();
        };
        let (id, label) = (id.into(), label.into());
        self.note_focus(&label, &id);
        let f = on_change.clone();
        let focused = self.ctx.focus(
            id,
            None,
            Some(Rc::new(move |s: Step| Some(f(s.apply(value, range))))),
        );
        if let Place::Quick(order) = place {
            let (glyph, accent) = (self.glyph(), self.ctx.accent);
            let view = quick_slider(
                glyph, label, display, value, range, accent, focused, on_change,
            );
            return self.add_quick(order, Kind::Slider, view);
        }
        let control = slider(value, range, self.ctx.accent, focused, on_change);
        self.with_value(label, help.map(Into::into), display, control)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn range<F>(
        &self,
        id: &'static str,
        label: &str,
        help: Option<&str>,
        display: String,
        (lo, hi): (f32, f32),
        max: f32,
        gap: f32,
        on_change: F,
    ) -> Element<'a, Message>
    where
        F: Fn((f32, f32)) -> Message + Clone + 'static,
    {
        let Some(place) = self.place() else {
            return Space::new().into();
        };
        self.note_focus(label, &FocusId::Indexed(id, 0));
        let focused = [0, 1].map(|k| {
            let f = on_change.clone();
            self.ctx.focus(
                FocusId::Indexed(id, k),
                None,
                Some(Rc::new(move |s: Step| {
                    Some(f(step_range((lo, hi), k, s, max, gap)))
                })),
            )
        });
        let which = focused.iter().position(|&f| f);
        let control = range_slider((lo, hi), max, gap, self.ctx.accent, which, on_change);
        if let Place::Quick(order) = place {
            let head = self.quick_head(label.into(), Some(display));
            return self.add_quick(order, Kind::Slider, column![head, control]);
        }
        self.with_value(label.into(), help.map(Into::into), display, control)
    }

    pub fn switch(
        &self,
        id: impl Into<FocusId>,
        label: impl Into<String>,
        help: Option<&str>,
        on: bool,
        msg: Message,
    ) -> Element<'a, Message> {
        let Some(place) = self.place() else {
            return Space::new().into();
        };
        let (id, label) = (id.into(), label.into());
        self.note_focus(&label, &id);
        let focused = self.ctx.focus(id, Some(msg.clone()), None);
        if let Place::Quick(order) = place {
            let view = quick_tile(self.glyph(), label, on, self.ctx.accent, focused, msg);
            return self.add_quick(order, Kind::Switch, view);
        }
        self.row(
            label,
            help.map(Into::into),
            switch(on, self.ctx.accent, focused, msg),
        )
    }

    /// A segmented control: one Tab stop; arrows move the selection. Beside the
    /// label if it fits, in one row or several underneath otherwise.
    pub fn segmented(
        &self,
        id: impl Into<FocusId>,
        label: impl Into<String>,
        help: Option<&str>,
        items: Vec<Seg>,
        enabled: bool,
    ) -> Element<'a, Message> {
        let Some(place) = self.place() else {
            return Space::new().into();
        };
        let (id, label) = (id.into(), label.into());
        if enabled {
            self.note_focus(&label, &id);
        }
        let help = help.map(str::to_string);
        let selected = items.iter().position(|s| s.selected).unwrap_or(0);
        let msgs: Vec<Message> = items.iter().map(|s| s.msg.clone()).collect();
        let focused = enabled
            && self.ctx.focus(
                id,
                None,
                Some(Rc::new(move |s: Step| {
                    msgs.get(s.apply_index(selected, msgs.len())).cloned()
                })),
            );
        let width = |s: &Seg| s.label.chars().count() as f32 * CHAR + 2.0 * SEGMENT_PAD;
        let natural: f32 = items.iter().map(width).sum::<f32>()
            + SEGMENT_GAP * items.len().saturating_sub(1) as f32
            + 4.0;
        let label_room = (label.chars().count() as f32 * CHAR + 24.0 + STAR + STAR_GAP).max(120.0);
        let beside = self.wide() && natural + label_room <= self.width;
        let per_row = if beside || natural <= self.width {
            items.len()
        } else {
            let widest = items.iter().map(width).fold(0.0, f32::max) + SEGMENT_GAP;
            ((self.width / widest) as usize).clamp(1, 4)
        }
        .max(1);

        let segment = |item: Seg| -> Element<'a, Message> {
            let label = text(item.label)
                .font(INTER_MEDIUM)
                .size(13)
                .wrapping(Wrapping::None);
            button(if beside {
                Element::from(container(label).center_y(Length::Fill))
            } else {
                center(label).into()
            })
            .width(if beside { Length::Shrink } else { Length::Fill })
            .height(44)
            .padding([0.0, SEGMENT_PAD])
            .style(dark_segment(item.selected))
            .on_press_maybe(enabled.then_some(item.msg))
            .into()
        };
        let mut rows = column![].spacing(SEGMENT_GAP);
        let mut line = row![].spacing(SEGMENT_GAP);
        let mut in_line = 0;
        for item in items {
            if in_line == per_row {
                rows = rows.push(std::mem::replace(&mut line, row![].spacing(SEGMENT_GAP)));
                in_line = 0;
            }
            line = line.push(segment(item));
            in_line += 1;
        }
        // A short last row keeps the others' widths.
        if !beside {
            for _ in in_line..per_row {
                line = line.push(Space::new().width(Length::Fill));
            }
        }
        let control = container(rows.push(line))
            .padding(2)
            .width(if beside { Length::Shrink } else { Length::Fill })
            .style(dark_trough(focused));
        if let Place::Quick(order) = place {
            let head = self.quick_head(label, None);
            return self.add_quick(order, Kind::Choice, column![head, control].spacing(6));
        }
        if beside {
            self.row(label, help, control)
        } else {
            self.stacked(label, None, help, control)
        }
    }

    /// Color swatches, under the label and the picked one's name.
    #[allow(clippy::too_many_arguments)]
    pub fn swatches(
        &self,
        id: impl Into<FocusId>,
        label: impl Into<String>,
        help: Option<&str>,
        name: String,
        fills: Vec<SwatchFill>,
        selected: usize,
        inner_ring: bool,
        on_pick: impl Fn(usize) -> Message + Clone + 'static,
    ) -> Element<'a, Message> {
        let Some(place) = self.place() else {
            return Space::new().into();
        };
        let n = fills.len();
        let pick = on_pick.clone();
        let (id, label) = (id.into(), label.into());
        self.note_focus(&label, &id);
        let focused = self.ctx.focus(
            id,
            None,
            Some(Rc::new(move |s: Step| {
                Some(pick(s.apply_index(selected, n)))
            })),
        );
        let control = swatches(fills, selected, inner_ring, focused, on_pick);
        if let Place::Quick(order) = place {
            let head = self.quick_head(label, Some(name));
            return self.add_quick(order, Kind::Color, column![head, control].spacing(4));
        }
        self.stacked(label, Some(name), help.map(Into::into), control)
    }

    /// A row of picture cards: one Tab stop; arrows move the selection.
    pub fn cards(
        &self,
        id: impl Into<FocusId>,
        label: impl Into<String>,
        cards: Vec<Card>,
    ) -> Element<'a, Message> {
        let Some(place) = self.place() else {
            return Space::new().into();
        };
        let accent = self.ctx.accent;
        let selected = cards.iter().position(|c| c.selected).unwrap_or(0);
        let msgs: Vec<Message> = cards.iter().map(|c| c.msg.clone()).collect();
        let (id, label) = (id.into(), label.into());
        self.note_focus(&label, &id);
        let focused = self.ctx.focus(
            id,
            None,
            Some(Rc::new(move |s: Step| {
                msgs.get(s.apply_index(selected, msgs.len())).cloned()
            })),
        );
        let least = match place {
            Place::Page => CARD_MIN,
            Place::Quick(_) => QUICK_CARD_MIN,
        };
        let fits = ((self.width + CARD_GAP) / (least + CARD_GAP)) as usize;
        let per_row = fits.min(cards.len()).clamp(1, 4);
        let mut rows = column![].spacing(CARD_GAP);
        let mut line = row![].spacing(CARD_GAP);
        let mut in_line = 0;
        for (i, c) in cards.into_iter().enumerate() {
            if in_line == per_row {
                rows = rows.push(std::mem::replace(&mut line, row![].spacing(CARD_GAP)));
                in_line = 0;
            }
            let color = match (c.error, c.selected) {
                (true, _) => DANGER,
                (false, true) => TEXT,
                (false, false) => TEXT_2,
            };
            let picture = Canvas::new(Diagram {
                picture: c.picture,
                color: if c.selected { accent } else { TEXT_2 },
            })
            .width(DIAGRAM_SIZE.width)
            .height(DIAGRAM_SIZE.height);
            let ring = focused && i == selected;
            line = line.push(
                button(center(
                    column![
                        picture,
                        text(c.label)
                            .font(INTER_MEDIUM)
                            .size(13)
                            .color(color)
                            .wrapping(Wrapping::None),
                    ]
                    .spacing(8)
                    .align_x(Alignment::Center),
                ))
                .width(Length::Fill)
                .height(92)
                .padding(0)
                .clip(true)
                .style(ringed(ring, picture_card(c.selected, accent)))
                .on_press(c.msg),
            );
            in_line += 1;
        }
        for _ in in_line..per_row {
            line = line.push(Space::new().width(Length::Fill));
        }
        let control = rows.push(line);
        if let Place::Quick(order) = place {
            let head = self.quick_head(label, None);
            return self.add_quick(order, Kind::Choice, column![head, control].spacing(6));
        }
        self.stacked(label, None, None, control)
    }

    /// A button beside its label.
    pub fn action(
        &self,
        id: &'static str,
        label: &str,
        help: &str,
        action: String,
        msg: Message,
    ) -> Element<'a, Message> {
        let Some(place) = self.place() else {
            return Space::new().into();
        };
        self.note_focus(label, &FocusId::Named(id));
        let focused = self.ctx.focus(FocusId::Named(id), Some(msg.clone()), None);
        if let Place::Quick(order) = place {
            let content = row![
                icon(self.glyph(), 18.0, TEXT_2),
                text(action).font(INTER_MEDIUM).size(13),
            ]
            .spacing(10)
            .align_y(Alignment::Center);
            let view = button(container(content).center_y(Length::Fill))
                .width(Length::Fill)
                .height(44)
                .padding([0, 14])
                .style(ringed(focused, raised_button(RAISED_2)))
                .on_press(msg);
            return self.add_quick(order, Kind::Action, view);
        }
        let action = button(center(text(action).font(INTER_MEDIUM).size(14)))
            .width(Length::Fixed(148.0))
            .height(44)
            .padding(0)
            .style(ringed(focused, raised_button(LINE)))
            .on_press(msg);
        self.row(label, Some(help.into()), action)
    }

    /// A setting's name in the quick settings, over its control, with its value or the
    /// section it's in.
    fn quick_head(&self, label: String, value: Option<String>) -> Element<'a, Message> {
        let section = self.page.name();
        let named = label.to_lowercase().contains(&section.to_lowercase());
        let (value, color) = match value {
            Some(value) => (value, TEXT_2),
            None if named => (String::new(), TEXT_3),
            None => (section.to_string(), TEXT_3),
        };
        row![
            container(quick_label(self.glyph(), label)).width(Length::Fill),
            text(value).size(13).color(color).wrapping(Wrapping::None),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .into()
    }

    /// A shortcut's keys, each a button that waits for another.
    pub fn shortcut(&self, action: Action) -> Element<'a, Message> {
        let app = self.ctx.app;
        let key = SettingKey::Shortcut(action);
        let differs = app.setting_change(&key) == Some(Change::Undo);
        self.on_key(key, differs);
        let keys = app.sections.controls.shortcuts.keys(action);
        let waiting = app.settings.capturing.filter(|(a, _)| *a == action);
        let label = action.name();
        let first = FocusId::Indexed("shortcut", action as usize * MAX_KEYS);
        self.note_focus(label, &first);

        // The keys there are, and room for one more.
        let mut slots = row![].spacing(6);
        let mut natural = label.chars().count() as f32 * CHAR + 24.0;
        for slot in 0..MAX_KEYS.min(keys.len() + 1) {
            let msg = Message::from(SettingsMessage::Capture(action, slot));
            let focused = self.ctx.focus(
                FocusId::Indexed("shortcut", action as usize * MAX_KEYS + slot),
                Some(msg.clone()),
                None,
            );
            let is_waiting = waiting == Some((action, slot));
            let (name, color) = match keys.get(slot) {
                _ if is_waiting => ("Press a key\u{2026}".to_string(), TEXT),
                Some(key) => (key.to_string(), TEXT),
                None => ("Add".to_string(), TEXT_3),
            };
            let empty = keys.get(slot).is_none() && !is_waiting;
            natural += name.chars().count() as f32 * CHAR + 28.0 + 6.0;
            let name = container(
                text(name)
                    .font(INTER_MEDIUM)
                    .size(13)
                    .color(color)
                    .wrapping(Wrapping::None),
            )
            .center_y(Length::Fill);
            let b = button(name).height(44).padding([0, 14]).on_press(msg);
            slots = slots.push(if empty {
                b.style(ringed(focused, text_button))
            } else {
                b.style(ringed(focused, key_button(is_waiting, self.ctx.accent)))
            });
        }
        let help = waiting.map(|_| "Esc cancels, Backspace takes it away".to_string());
        if natural <= self.width {
            self.row(label, help, slots)
        } else {
            self.stacked(label, None, help, slots)
        }
    }

    /// A line of plain text.
    pub fn note(&self, note: String) -> Element<'a, Message> {
        self.plain(text(note).size(13).color(TEXT_2))
    }

    /// A text field, for several in a row.
    pub fn field(
        &self,
        id: FocusId,
        widget: &'static str,
        placeholder: &'static str,
        value: &str,
        valid: bool,
        on_input: impl Fn(String) -> Message + 'a,
    ) -> Element<'a, Message> {
        self.ctx.text_input_stop(id, widget);
        text_input(placeholder, value)
            .id(iced::widget::Id::new(widget))
            .on_input(on_input)
            .size(13)
            .padding([13, 12])
            .style(field(false, valid))
            .into()
    }
}

//! The quick settings: the starred settings, in a panel under the header.

use super::settings::{Kind, Quick, Rows};
use super::{Ctx, Screen};
use crate::app::settings::{SettingsMessage, favorite_section};
use crate::app::{App, Message};
use crate::controls::fill_slider;
use crate::focus::{FocusId, Scope};
use crate::icons::{self, icon};
use crate::theme::*;
use iced::widget::text::Wrapping;
use iced::widget::{
    Space, button, center, column, container, mouse_area, opaque, row, scrollable, stack, text,
};
use iced::{Alignment, Color, Element, Length, Padding};

/// Space around everything in the panel, and its width.
const PAD: f32 = 14.0;
const WIDTH: f32 = 320.0;
const GAP: f32 = 8.0;
/// The height of a slider, and of a switch's tile.
const SLIDER: f32 = 44.0;
const TILE: f32 = 54.0;
/// The header's buttons, above the panel.
const TOP: f32 = 24.0 + 48.0 + 10.0;

impl App {
    /// The panel, and what closes it around it.
    pub(super) fn quick_settings(&self, screen: &Screen) -> Element<'_, Message> {
        let ctx = Ctx::new(self, Scope::Quick);
        let gutter = screen.layout.gutter;
        let width = WIDTH.min(self.window_size.width - 2.0 * gutter);

        let items = self.quick_items(width - 2.0 * PAD);
        let body: Element<'_, Message> = if items.is_empty() {
            self.quick_empty()
        } else {
            scrollable(quick_list(items))
                .direction(scrollable::Direction::Vertical(
                    scrollable::Scrollbar::new()
                        .width(4)
                        .scroller_width(4)
                        .spacing(6),
                ))
                .style(scrollbar(self.accent()))
                .into()
        };
        // Whatever height is left for the list.
        let chrome = 2.0 * PAD + 22.0 + 2.0 * 12.0 + SLIDER;
        let room = (self.window_size.height - TOP - 24.0 - chrome).max(TILE);

        let all = Message::from(SettingsMessage::Toggle);
        let focused = ctx.focus(FocusId::Named("all-settings"), Some(all.clone()), None);
        let all = button(
            container(
                row![
                    text("All settings")
                        .font(INTER_MEDIUM)
                        .size(14)
                        .width(Length::Fill),
                    icon(icons::CHEVRON_RIGHT, 20.0, TEXT_2),
                ]
                .align_y(Alignment::Center),
            )
            .center_y(Length::Fill),
        )
        .width(Length::Fill)
        .height(SLIDER)
        .padding([0, 14])
        .style(ringed(focused, raised_button(RAISED_2)))
        .on_press(all);

        let panel = container(
            column![
                text("Quick settings")
                    .font(INTER_SEMIBOLD)
                    .size(16)
                    .color(TEXT),
                container(body).max_height(room),
                all,
            ]
            .spacing(12),
        )
        .width(width)
        .padding(PAD)
        .style(quick_panel);
        let placed = container(opaque(panel))
            .align_right(Length::Fill)
            .padding(Padding {
                top: TOP,
                right: gutter,
                bottom: 0.0,
                left: 0.0,
            });
        stack![
            mouse_area(Space::new().width(Length::Fill).height(Length::Fill))
                .on_press(Message::ToggleQuick),
            placed,
        ]
        .into()
    }

    /// The starred settings that are there right now, the ones alike together.
    fn quick_items(&self, width: f32) -> Vec<Quick<'_>> {
        let start = self.focus_reg.len();
        let mut items = Vec::new();
        for id in self.section_list() {
            if self
                .favorites
                .iter()
                .any(|f| favorite_section(f) == Some(id))
            {
                let rows = Rows::quick(self, id.into(), width);
                let _ = self.page_groups(id.into(), &rows);
                items.extend(rows.into_quick());
            }
        }
        items.sort_by_key(|q| (q.kind, q.order));
        // Tab goes through them in that order too.
        let order = items.iter().map(|q| q.stops.clone());
        self.focus_reg.rearrange(start, order);
        items
    }

    fn quick_empty(&self) -> Element<'_, Message> {
        container(
            column![
                icon(icons::STAR, 28.0, TEXT_3),
                text("Star a setting to have it here.")
                    .size(14)
                    .color(TEXT_2),
            ]
            .spacing(10)
            .align_x(Alignment::Center),
        )
        .center_x(Length::Fill)
        .padding([18, 0])
        .into()
    }
}

/// The settings under each other, with switches in pairs.
fn quick_list<'a>(items: Vec<Quick<'a>>) -> Element<'a, Message> {
    let mut list = column![].spacing(GAP);
    let mut tile: Option<Element<'a, Message>> = None;
    for item in items {
        if item.kind != Kind::Switch {
            if let Some(single) = tile.take() {
                list = list.push(row![single, Space::new().width(Length::Fill)].spacing(GAP));
            }
            list = list.push(item.view);
            continue;
        }
        tile = match tile.take() {
            Some(first) => {
                list = list.push(row![first, item.view].spacing(GAP));
                None
            }
            None => Some(item.view),
        };
    }
    if let Some(single) = tile {
        list = list.push(row![single, Space::new().width(Length::Fill)].spacing(GAP));
    }
    list.into()
}

/// A setting's icon and name.
pub fn quick_label<'a>(glyph: &'static str, label: String) -> Element<'a, Message> {
    row![
        icon(glyph, 18.0, TEXT_2),
        text(label)
            .font(INTER_MEDIUM)
            .size(13)
            .color(TEXT)
            .wrapping(Wrapping::None),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

/// A slider that fills its whole bar, with its name and value on it.
#[allow(clippy::too_many_arguments)]
pub fn quick_slider<'a>(
    glyph: &'static str,
    label: String,
    display: String,
    value: f32,
    range: (f32, f32, f32),
    accent: Color,
    focused: bool,
    on_change: impl Fn(f32) -> Message + 'a,
) -> Element<'a, Message> {
    let bar = fill_slider(value, range, deepened(accent), focused, on_change)
        .width(Length::Fill)
        .height(SLIDER);
    let over = row![
        icon(glyph, 18.0, TEXT),
        text(label)
            .font(INTER_MEDIUM)
            .size(13)
            .color(TEXT)
            .width(Length::Fill)
            .wrapping(Wrapping::None),
        text(display)
            .font(INTER_MEDIUM)
            .size(13)
            .color(TEXT)
            .wrapping(Wrapping::None),
    ]
    .spacing(10)
    .align_y(Alignment::Center);
    let over = container(over)
        .padding([0, 14])
        .center_y(Length::Fill)
        .width(Length::Fill)
        .clip(true);
    container(stack![bar, over]).height(SLIDER).into()
}

/// A switch as a tile that lights up.
pub fn quick_tile<'a>(
    glyph: &'static str,
    label: String,
    on: bool,
    accent: Color,
    focused: bool,
    msg: Message,
) -> Element<'a, Message> {
    let accent = deepened(accent);
    let badge = container(center(icon(glyph, 16.0, TEXT)))
        .width(28)
        .height(28)
        .style(quick_badge(on));
    let state = if on { "On" } else { "Off" };
    let content = row![
        badge,
        column![
            text(label)
                .font(INTER_SEMIBOLD)
                .size(12)
                .color(TEXT)
                .wrapping(Wrapping::WordOrGlyph),
            text(state).size(11).color(with_alpha(TEXT, 0.75)),
        ]
        .spacing(1)
        .width(Length::Fill),
    ]
    .spacing(9)
    .align_y(Alignment::Center);
    button(container(content).center_y(Length::Fill))
        .width(Length::Fill)
        .height(TILE)
        .padding([0, 10])
        .clip(true)
        .style(ringed(focused, quick_tile_button(on, accent)))
        .on_press(msg)
        .into()
}

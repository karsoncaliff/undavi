//! The settings window: a tree of sections on the left, the section's settings in
//! the middle and a live preview on the right. (What its controls do is in
//! `app/settings.rs`.)

use crate::app::settings::{CONTENT_SCROLL, SettingsMessage, group_id};
use crate::app::{App, Message};
use crate::focus::{FocusId, Scope};
use crate::layout::SettingsLayout;
use crate::theme::*;
use crate::view::Ctx;
use iced::widget::text::Wrapping;
use iced::widget::{
    Space, button, center, column, container, mouse_area, opaque, row, scrollable, stack, text,
};
use iced::{Alignment, Element, Length, Padding};
pub(in crate::view) use rows::{Kind, Quick, Rows};

mod monitor;
mod navigation;
mod pictures;
mod presets;
mod preview;
mod rows;
mod sections;

/// Space on both sides of the content, in a roomy column and a narrow one.
const SIDES: f32 = 28.0;
const SIDES_NARROW: f32 = 16.0;
/// Below this, the preset button goes under the title, and the footer shortens.
const ROOMY: f32 = 470.0;
/// The header and footer, for the preset menu.
const HEADER_TOP: f32 = 24.0;
const BUTTON: f32 = 44.0;

impl App {
    pub(super) fn settings_window(&self) -> Element<'_, Message> {
        let layout = SettingsLayout::new(self.window_size, self.settings.show_preview);
        // Built in reading order, for Tab.
        let nav = self.settings_nav(&layout);
        let content = self.settings_content(&layout);
        let preview = match layout.preview {
            Some(width) => self.settings_preview(width),
            None => self.preview_strip(&layout),
        };
        let line = || container(Space::new().width(1).height(Length::Fill)).style(fill(RAISED));
        let window = container(row![nav, line(), content, line(), preview])
            .width(layout.size.width)
            .height(layout.size.height)
            .padding(1)
            .style(settings_window);
        // Opaque, so nothing reaches the app behind.
        opaque(center(opaque(window)).style(settings_scrim))
    }

    fn settings_content(&self, layout: &SettingsLayout) -> Element<'_, Message> {
        let page = self.settings.page;
        let roomy = layout.content >= ROOMY;
        let sides = if roomy { SIDES } else { SIDES_NARROW };
        let edge = Ctx::new(self, Scope::Edge);

        let title = column![
            text(page.name()).font(INTER_SEMIBOLD).size(22).color(TEXT),
            text(sections::description(page)).size(14).color(TEXT_2),
        ]
        .spacing(3)
        .width(Length::Fill);
        let header: Element<'_, Message> = match page.section() {
            None => title.into(),
            Some(id) if roomy => row![title, self.preset_button(&edge, id, layout, Length::Shrink)]
                .spacing(16)
                .align_y(Alignment::Start)
                .into(),
            Some(id) => column![title, self.preset_button(&edge, id, layout, Length::Fill)]
                .spacing(12)
                .into(),
        };
        let header = container(header).padding(Padding {
            top: HEADER_TOP,
            right: sides,
            bottom: 14.0,
            left: sides,
        });
        // In reading order the menu comes before the settings it's over.
        let menu = page.section().filter(|_| self.settings.menu).map(|id| {
            let top = if roomy {
                HEADER_TOP + BUTTON + 8.0
            } else {
                HEADER_TOP + 62.0 + BUTTON + 8.0
            };
            container(opaque(
                self.preset_menu(id, layout.size.height - top - 16.0),
            ))
            .align_right(Length::Fill)
            .padding(Padding {
                top,
                right: sides,
                bottom: 0.0,
                left: 0.0,
            })
        });

        let rows = Rows::new(self, page, layout.content - 2.0 * sides - 4.0);
        let groups = self
            .page_groups(page, &rows)
            .into_iter()
            .enumerate()
            .fold(column![].spacing(10), |col, (i, g)| {
                col.push(container(g.view()).id(group_id(i)))
            });
        let body = scrollable(container(groups).padding(Padding {
            top: 2.0,
            right: sides,
            bottom: 24.0,
            left: sides,
        }))
        .id(iced::widget::Id::new(CONTENT_SCROLL))
        .direction(scrollable::Direction::Vertical(
            scrollable::Scrollbar::new().width(4).scroller_width(4),
        ))
        .on_scroll(|v| SettingsMessage::Scrolled(v.absolute_offset().y).into())
        .width(Length::Fill)
        .height(Length::Fill)
        .style(scrollbar(self.accent()));

        let content = column![
            header,
            body,
            container(Space::new().height(1))
                .width(Length::Fill)
                .style(fill(RAISED)),
            self.settings_footer(&edge, sides, roomy),
        ]
        .width(Length::Fill)
        .height(Length::Fill);
        match menu {
            Some(menu) => stack![
                content,
                // A click anywhere else on the content closes it (the button too).
                mouse_area(Space::new().width(Length::Fill).height(Length::Fill))
                    .on_press(SettingsMessage::ToggleMenu.into()),
                menu,
            ]
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
            None => content.into(),
        }
    }

    fn settings_footer<'a>(
        &'a self,
        ctx: &Ctx<'a>,
        sides: f32,
        roomy: bool,
    ) -> Element<'a, Message> {
        let reset = self.settings.section().map(|id| {
            let label = if roomy {
                format!("Reset {} to defaults", id.name())
            } else {
                "Reset".into()
            };
            ctx.text_button(
                FocusId::Named("reset"),
                label,
                SettingsMessage::Reset.into(),
                Length::Shrink,
            )
        });
        let plain = |id: &'static str, label: &'static str, msg: SettingsMessage, main: bool| {
            let msg = Message::from(msg);
            let focused = ctx.focus(FocusId::Named(id), Some(msg.clone()), None);
            let accent = ctx.accent;
            let b = button(
                container(
                    text(label)
                        .font(INTER_MEDIUM)
                        .size(14)
                        .wrapping(Wrapping::None),
                )
                .center_y(Length::Fill),
            )
            .height(BUTTON)
            .padding([0, 20])
            .on_press(msg);
            if main {
                b.style(ringed(focused, accent_button(accent)))
            } else {
                b.style(ringed(focused, raised_button(RAISED_2)))
            }
        };
        container(
            row![]
                .extend(reset)
                .extend([
                    Space::new().width(Length::Fill).into(),
                    plain("cancel", "Cancel", SettingsMessage::Cancel, false).into(),
                    plain("done", "Done", SettingsMessage::Done, true).into(),
                ])
                .spacing(8)
                .align_y(Alignment::Center),
        )
        .padding(Padding {
            top: 12.0,
            right: sides,
            bottom: 12.0,
            left: sides - 12.0,
        })
        .into()
    }
}

/// A whole number with a typographic minus.
fn signed(v: i8) -> String {
    if v < 0 {
        format!("\u{2212}{}", -(v as i16))
    } else {
        v.to_string()
    }
}

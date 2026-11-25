//! The settings window's left column: search, and the tree of sections and groups.

use super::sections::Outline;
use crate::app::settings::{Page, SEARCH_INPUT, SettingsMessage, TREE_SCROLL};
use crate::app::{App, Message};
use crate::focus::{FocusId, Scope, Step};
use crate::icons::{self, icon};
use crate::layout::SettingsLayout;
use crate::settings::SectionId;
use crate::theme::*;
use crate::view::{Ctx, tip};
use iced::widget::{
    Column, Space, button, center, column, container, row, scrollable, stack, text, text_input,
    tooltip,
};
use iced::{Alignment, Element, Length, Padding};
use std::rc::Rc;

/// A row's height.
const ROW: f32 = 44.0;
/// Space around the tree.
const INSET: f32 = 12.0;
/// How far groups are indented, and where the line beside them is.
const INDENT: f32 = 30.0;
const GUIDE: f32 = 18.0;
/// The button that expands or collapses a page, around its arrow.
const ARROW: f32 = 32.0;
/// The height of the title, and its button.
const TITLE: f32 = 40.0;

pub fn page_icon(page: Page) -> &'static str {
    match page {
        Page::Section(id) => section_icon(id),
        Page::Presets => icons::PRESETS,
    }
}

pub fn section_icon(id: SectionId) -> &'static str {
    match id {
        SectionId::Sound => icons::SOUND,
        SectionId::Visualizer => icons::BARS,
        SectionId::Colors => icons::PALETTE,
        SectionId::Background => icons::BACKGROUND,
        SectionId::NowPlaying => icons::NOTE,
        SectionId::Playback => icons::PLAYBACK,
        SectionId::Lyrics => icons::LINES,
        SectionId::Controls => icons::CONTROLS,
        SectionId::General => icons::GENERAL,
    }
}

impl App {
    pub(super) fn settings_nav(&self, layout: &SettingsLayout) -> Element<'_, Message> {
        let ctx = Ctx::new(self, Scope::Tree);
        let content: Element<'_, Message> = if layout.compact_nav {
            self.compact_nav(&ctx)
        } else {
            let title = container(
                row![
                    text("Settings")
                        .font(INTER_SEMIBOLD)
                        .size(18)
                        .color(TEXT)
                        .width(Length::Fill),
                    self.expand_all(&ctx),
                ]
                .height(TITLE)
                .align_y(Alignment::Center),
            )
            .padding(Padding {
                top: 12.0,
                right: 4.0,
                bottom: 6.0,
                left: 22.0,
            });
            column![
                title,
                container(self.settings_search(&ctx)).padding([0.0, INSET]),
                scrollable(container(self.settings_tree(&ctx)).padding(Padding {
                    top: 0.0,
                    right: INSET,
                    bottom: INSET,
                    left: INSET,
                }))
                .id(iced::widget::Id::new(TREE_SCROLL))
                .direction(scrollable::Direction::Vertical(
                    scrollable::Scrollbar::new().width(4).scroller_width(4),
                ))
                .height(Length::Fill)
                .style(scrollbar(self.accent())),
            ]
            .spacing(0)
            .into()
        };
        container(content)
            .width(layout.nav)
            .height(Length::Fill)
            .style(settings_side(true))
            .into()
    }

    /// The button that expands the whole tree, or collapses it once it is. (While
    /// searching, everything is expanded.)
    fn expand_all<'a>(&'a self, ctx: &Ctx<'a>) -> Element<'a, Message> {
        if !self.settings.search.trim().is_empty() {
            return Space::new().into();
        }
        let all = self.settings.all_expanded(&self.pages());
        let (glyph, name) = if all {
            (icons::COLLAPSE, "Collapse all")
        } else {
            (icons::EXPAND, "Expand all")
        };
        let msg = Message::from(SettingsMessage::ExpandAll(!all));
        let focused = ctx.focus(FocusId::Named("expand-all"), Some(msg.clone()), None);
        tip(
            button(center(icon(glyph, 20.0, TEXT_2)))
                .width(TITLE)
                .height(TITLE)
                .padding(0)
                .style(ringed(focused, tree_row(None, TEXT_2)))
                .on_press(msg),
            name,
            tooltip::Position::Bottom,
        )
    }

    fn settings_search<'a>(&'a self, ctx: &Ctx<'a>) -> Element<'a, Message> {
        ctx.text_input_stop(FocusId::Named("settings-search"), SEARCH_INPUT);
        let field = text_input("Search", &self.settings.search)
            .id(iced::widget::Id::new(SEARCH_INPUT))
            .on_input(|s| SettingsMessage::Search(s).into())
            .on_submit(SettingsMessage::SearchGo.into())
            .size(14)
            .padding(Padding {
                top: 13.0,
                right: 12.0,
                bottom: 13.0,
                left: 38.0,
            })
            .style(field(false, true));
        let glass = container(icon(icons::SEARCH, 16.0, TEXT_2))
            .padding(Padding::default().left(13))
            .center_y(Length::Fill);
        container(stack![field, glass]).height(ROW).into()
    }

    /// The sections, with the groups of the expanded ones. While searching, only
    /// what matches, all of it expanded.
    fn settings_tree<'a>(&'a self, ctx: &Ctx<'a>) -> Element<'a, Message> {
        let query = self.settings.search.trim().to_lowercase();
        let mut tree = column![Space::new().height(12)].spacing(2);
        let mut found = false;
        for page in self.pages() {
            let outline = self.outline(page);
            let named = page.name().to_lowercase().contains(&query);
            let groups: Vec<(usize, Outline)> = outline
                .into_iter()
                .enumerate()
                .filter(|(_, g)| named || g.matches(&query))
                .collect();
            if groups.is_empty() {
                continue;
            }
            found = true;
            let searching = !query.is_empty();
            let open = searching || self.settings.expanded(page);
            tree = tree.push(self.page_row(ctx, page, open, searching));
            if open {
                tree = tree.push(self.group_rows(ctx, page, groups));
            }
        }
        if !found {
            tree = tree.push(
                container(text("No settings found").size(13).color(TEXT_3)).padding([12, 10]),
            );
        }
        tree.into()
    }

    /// A page, and the arrow that expands or collapses it without going to it. (The
    /// arrow keys do too. While searching, everything is expanded.)
    fn page_row<'a>(
        &'a self,
        ctx: &Ctx<'a>,
        page: Page,
        open: bool,
        searching: bool,
    ) -> Element<'a, Message> {
        let selected = self.settings.page == page;
        let msg = Message::from(SettingsMessage::Page(page));
        let focused = ctx.focus(
            FocusId::Indexed("section", page.index()),
            Some(msg.clone()),
            (!searching).then(|| {
                let step = move |s: Step| match s {
                    Step::Less => Some(SettingsMessage::Expand(page, false).into()),
                    Step::More => Some(SettingsMessage::Expand(page, true).into()),
                    _ => None,
                };
                Rc::new(step) as _
            }),
        );
        let glyph = if open {
            icons::CHEVRON_DOWN
        } else {
            icons::CHEVRON_RIGHT
        };
        let glyph = center(icon(glyph, 18.0, TEXT_3)).width(ARROW).height(ROW);
        let arrow: Element<'a, Message> = if searching {
            glyph.into()
        } else {
            button(glyph)
                .padding(0)
                .style(tree_row(None, TEXT_3))
                .on_press(SettingsMessage::Expand(page, !open).into())
                .into()
        };
        let tint = if selected { ctx.accent } else { TEXT_2 };
        let label = row![
            icon(page_icon(page), 18.0, tint),
            text(page.name()).font(INTER_MEDIUM).size(14).color(TEXT),
        ]
        .spacing(9)
        .align_y(Alignment::Center);
        button(
            row![arrow, container(label).center_y(Length::Fill)]
                .spacing(3)
                .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .height(ROW)
        .padding(0)
        .style(ringed(
            focused,
            tree_row(selected.then_some(RAISED_2), TEXT),
        ))
        .on_press(msg)
        .into()
    }

    fn group_rows<'a>(
        &'a self,
        ctx: &Ctx<'a>,
        page: Page,
        groups: Vec<(usize, Outline)>,
    ) -> Element<'a, Message> {
        let accent = ctx.accent;
        let rows = groups
            .into_iter()
            .fold(Column::new().spacing(1), |col, (i, g)| {
                let current = self.settings.page == page && self.settings.group == i;
                let msg = Message::from(SettingsMessage::Group(page, i));
                let focused = ctx.focus(
                    FocusId::Indexed("group", page.index() * 64 + i),
                    Some(msg.clone()),
                    None,
                );
                let (fill, color, font) = if current {
                    (Some(with_alpha(accent, 0.16)), TEXT, INTER_SEMIBOLD)
                } else {
                    (None, TEXT_2, INTER_MEDIUM)
                };
                col.push(
                    button(
                        container(text(g.title).font(font).size(13).color(color))
                            .center_y(Length::Fill),
                    )
                    .width(Length::Fill)
                    .height(ROW)
                    .padding([0, 10])
                    .style(ringed(focused, tree_row(fill, color)))
                    .on_press(msg),
                )
            });
        // The guide line is as tall as the rows beside it.
        let guide =
            container(container(Space::new().width(1).height(Length::Fill)).style(fill(LINE)))
                .padding(Padding::default().left(GUIDE));
        container(stack![
            container(rows).padding(Padding::default().left(INDENT)),
            guide
        ])
        .padding([2, 0])
        .into()
    }

    /// Only the pages' icons, for narrow windows.
    fn compact_nav<'a>(&'a self, ctx: &Ctx<'a>) -> Element<'a, Message> {
        let rows = self
            .pages()
            .into_iter()
            .fold(Column::new().spacing(4), |col, page| {
                let selected = self.settings.page == page;
                // Nothing to expand here: the selected one stays selected.
                let msg = (!selected).then(|| Message::from(SettingsMessage::Page(page)));
                let focused =
                    ctx.focus(FocusId::Indexed("section", page.index()), msg.clone(), None);
                let tint = if selected { ctx.accent } else { TEXT_2 };
                let row = button(center(icon(page_icon(page), 20.0, tint)))
                    .width(ROW)
                    .height(ROW)
                    .padding(0)
                    .style(ringed(
                        focused,
                        tree_row(selected.then_some(RAISED_2), TEXT),
                    ))
                    .on_press_maybe(msg);
                col.push(tip(row, page.name(), tooltip::Position::Right))
            });
        // In a short window there are more than fit.
        scrollable(container(rows).padding(Padding {
            top: 20.0,
            right: 0.0,
            bottom: INSET,
            left: (64.0 - ROW) / 2.0,
        }))
        .id(iced::widget::Id::new(TREE_SCROLL))
        .direction(scrollable::Direction::Vertical(
            scrollable::Scrollbar::hidden(),
        ))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }
}

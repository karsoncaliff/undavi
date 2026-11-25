//! A section's preset button and the menu it opens, and the Presets page, which has
//! every section's and the groups of them.

use super::navigation::section_icon;
use super::rows::{Group, Rows};
use super::sections::Outline;
use crate::app::settings::{
    Editing, MENU_SCROLL, PRESET_NAME_INPUT, SettingsMessage, preset_index,
};
use crate::app::{App, Message};
use crate::focus::{FocusId, Scope};
use crate::icons::{self, icon};
use crate::layout::SettingsLayout;
use crate::settings::{PresetGroup, PresetId, SectionId};
use crate::theme::*;
use crate::view::{Ctx, ellipsize, tip};
use iced::widget::text::Wrapping;
use iced::widget::{
    Space, button, center, column, container, row, scrollable, text, text_input, tooltip,
};
use iced::{Alignment, Color, Element, Length, Padding};

const WIDTH: f32 = 300.0;
/// The height of a row, and of the buttons at the bottom.
const ROW: f32 = 44.0;

/// How much a button at the bottom of the menu stands out.
#[derive(Clone, Copy)]
enum Weight {
    Main,
    Filled,
    Text,
}

impl App {
    /// "Preset: [name]", with a tag once the settings differ from it.
    pub(super) fn preset_button<'a>(
        &'a self,
        ctx: &Ctx<'a>,
        id: SectionId,
        layout: &SettingsLayout,
        width: Length,
    ) -> Element<'a, Message> {
        let msg = Message::from(SettingsMessage::ToggleMenu);
        let focused = ctx.focus(FocusId::Named("preset-menu"), Some(msg.clone()), None);
        let edited = self.section_edited(id);
        // Less room: a shorter name first, then no "Preset:".
        let room = if layout.content < 560.0 { 14 } else { 22 };
        let name = ellipsize(&self.sections.get(id).selected_name(), room);
        let mut content = row![icon(icons::LAYERS, 18.0, TEXT)]
            .spacing(8)
            .align_y(Alignment::Center);
        if layout.content >= 520.0 {
            content = content.push(
                text("Preset:")
                    .size(14)
                    .color(TEXT_2)
                    .wrapping(Wrapping::None),
            );
        }
        content = content.push(
            text(name)
                .font(INTER_MEDIUM)
                .size(14)
                .color(TEXT)
                .wrapping(Wrapping::None),
        );
        if edited {
            content = content.push(edited_tag(ctx.accent));
        }
        if width == Length::Fill {
            content = content.push(Space::new().width(Length::Fill));
        }
        content = content.push(icon(icons::CHEVRON_DOWN, 20.0, TEXT));
        button(container(content).center_y(Length::Fill))
            .width(width)
            .height(ROW)
            .padding([0, 14])
            .style(ringed(focused, raised_button(RAISED)))
            .on_press(msg)
            .into()
    }

    /// The menu. `room` is how tall it can get.
    pub(super) fn preset_menu(&self, id: SectionId, room: f32) -> Element<'_, Message> {
        let ctx = Ctx::new(self, Scope::Menu);
        let section = self.sections.get(id);
        let selected = section.selected();
        let edited = self.section_edited(id);
        let yours = section.yours();

        let heading = |s: &str| {
            container(text(tracked(s)).font(INTER_SEMIBOLD).size(11).color(TEXT_3)).padding([8, 10])
        };
        let mut list = column![].spacing(2);
        if !yours.is_empty() {
            list = list.push(heading("Yours"));
            for i in 0..yours.len() {
                list = list.push(self.your_preset(&ctx, id, i));
            }
        }
        list = list.push(heading("Built-in"));
        for (i, (name, media)) in section.built_in().into_iter().enumerate() {
            let preset = PresetId::BuiltIn(name.into());
            let current = selected == preset;
            if media && !self.platform.media && !current {
                continue;
            }
            list = list.push(preset_row(
                &ctx,
                FocusId::Indexed("built-in", i),
                name.into(),
                current,
                SettingsMessage::LoadPreset(id, preset),
            ));
        }

        // The note and buttons at the bottom, with their heights.
        let mut items: Vec<(Element<'_, Message>, f32)> = Vec::new();
        let action = |id: &'static str, label: &str, msg: SettingsMessage, weight: Weight| {
            let msg = Message::from(msg);
            let focused = ctx.focus(FocusId::Named(id), Some(msg.clone()), None);
            let accent = ctx.accent;
            let b = button(center(text(label.to_string()).font(INTER_MEDIUM).size(14)))
                .width(Length::Fill)
                .height(ROW)
                .padding(0)
                .on_press(msg);
            let b: Element<'_, Message> = match weight {
                Weight::Main => b.style(ringed(focused, accent_button(accent))).into(),
                Weight::Filled => b.style(ringed(focused, raised_button(LINE))).into(),
                Weight::Text => b.style(ringed(focused, text_button)).into(),
            };
            (b, ROW)
        };
        match (&selected, edited) {
            (_, false) => items.push(action(
                "save-new-preset",
                "Save current as new preset",
                SettingsMessage::SaveNewPreset,
                Weight::Filled,
            )),
            (PresetId::Yours(i), true) => {
                let name = yours.get(*i).copied().unwrap_or_default();
                items.push((note(format!("You changed {}.", ellipsize(name, 28))), 20.0));
                items.push(action(
                    "save-preset",
                    "Save changes",
                    SettingsMessage::SavePreset,
                    Weight::Main,
                ));
                items.push(action(
                    "save-new-preset",
                    "Save as new preset",
                    SettingsMessage::SaveNewPreset,
                    Weight::Filled,
                ));
                items.push(action(
                    "discard-preset",
                    "Discard changes",
                    SettingsMessage::DiscardChanges,
                    Weight::Text,
                ));
            }
            (PresetId::BuiltIn(name), true) => {
                items.push((
                    note(format!(
                        "You changed {name}. Built-in presets can't be overwritten."
                    )),
                    36.0,
                ));
                items.push(action(
                    "save-new-preset",
                    "Save as new preset",
                    SettingsMessage::SaveNewPreset,
                    Weight::Main,
                ));
                items.push(action(
                    "discard-preset",
                    "Discard changes",
                    SettingsMessage::DiscardChanges,
                    Weight::Text,
                ));
            }
        }
        let mut bottom = column![].spacing(6);
        let mut height = 0.0;
        for (item, h) in items {
            height += h + 6.0;
            bottom = bottom.push(item);
        }

        let title = container(
            text(format!("{} presets", id.name()))
                .font(INTER_SEMIBOLD)
                .size(14)
                .color(TEXT),
        )
        .padding([6, 10]);
        // The list scrolls in whatever's left.
        let chrome = 30.0 + height + 1.0 + 12.0 + 12.0 + 16.0;
        let list = container(
            scrollable(list)
                .id(iced::widget::Id::new(MENU_SCROLL))
                .direction(scrollable::Direction::Vertical(
                    scrollable::Scrollbar::new()
                        .width(4)
                        .scroller_width(4)
                        .spacing(4),
                ))
                .style(scrollbar(ctx.accent)),
        )
        .max_height((room - chrome).max(2.0 * ROW));
        let divider = container(Space::new().height(1))
            .width(Length::Fill)
            .style(fill(OUTLINE));
        container(column![title, list, divider, bottom].spacing(6))
            .width(WIDTH)
            .padding(8)
            .style(preset_menu)
            .into()
    }

    /// One of the user's presets: to load, with a pencil to edit it, or being edited.
    fn your_preset<'a>(&'a self, ctx: &Ctx<'a>, id: SectionId, i: usize) -> Element<'a, Message> {
        let section = self.sections.get(id);
        let yours = section.yours();
        let Some(name) = yours.get(i).copied() else {
            return Space::new().into();
        };
        let what = Editing::Preset(id, i);
        if self.settings.editing == Some(what) {
            return self.editor(ctx, what, i, name, yours.len() - 1, None);
        }
        let preset = PresetId::Yours(i);
        let load = preset_row(
            ctx,
            FocusId::Indexed("preset", preset_index(id, i)),
            name.to_string(),
            section.selected() == preset,
            SettingsMessage::LoadPreset(id, preset),
        );
        let edit = FocusId::Indexed("preset-edit", preset_index(id, i));
        row![load, pencil(ctx, edit, what)].spacing(2).into()
    }

    /// A preset (or group) being renamed, with the buttons to move or delete it, and
    /// `more` between.
    fn editor<'a>(
        &'a self,
        ctx: &Ctx<'a>,
        what: Editing,
        i: usize,
        name: &str,
        last: usize,
        more: Option<Element<'a, Message>>,
    ) -> Element<'a, Message> {
        ctx.text_input_stop(FocusId::Indexed("preset-name", i), PRESET_NAME_INPUT);
        let placeholder = match what {
            Editing::Preset(..) => "Preset name",
            Editing::Group(_) => "Group name",
        };
        let name = text_input(placeholder, name)
            .id(iced::widget::Id::new(PRESET_NAME_INPUT))
            .on_input(move |s| SettingsMessage::Rename(what, s).into())
            .on_submit(SettingsMessage::Edit(None).into())
            .size(14)
            .padding([12, 12])
            .style(field(false, true));
        let armed = self.settings.confirm_delete == Some(what);
        let delete_msg = Message::from(SettingsMessage::Delete(what));
        let up = ctx.icon_button(
            FocusId::Indexed("preset-up", i),
            icons::CHEVRON_UP,
            "Move up",
            (i > 0).then(|| SettingsMessage::Move(what, -1).into()),
        );
        let down = ctx.icon_button(
            FocusId::Indexed("preset-down", i),
            icons::CHEVRON_DOWN,
            "Move down",
            (i < last).then(|| SettingsMessage::Move(what, 1).into()),
        );
        let delete_focused = ctx.focus(
            FocusId::Indexed("preset-delete", i),
            Some(delete_msg.clone()),
            None,
        );
        let delete = button(center(
            text(if armed { "Delete?" } else { "Delete" })
                .font(INTER_MEDIUM)
                .size(14),
        ))
        .width(84)
        .height(ROW)
        .padding(0)
        .style(ringed(delete_focused, danger_button(armed)))
        .on_press(delete_msg);
        let done_msg = Message::from(SettingsMessage::Edit(None));
        let done_focused = ctx.focus(
            FocusId::Indexed("preset-done", i),
            Some(done_msg.clone()),
            None,
        );
        let done = button(center(text("Done").font(INTER_MEDIUM).size(14)))
            .width(Length::Fill)
            .height(ROW)
            .padding(0)
            .style(ringed(done_focused, raised_button(LINE)))
            .on_press(done_msg);
        column![name]
            .extend(more)
            .push(
                row![up, down, delete, done]
                    .spacing(6)
                    .align_y(Alignment::Center),
            )
            .spacing(6)
            .into()
    }

    // -----------------------------------------------------------------------
    // The Presets page

    /// What's on the Presets page ([`App::presets_groups`]).
    pub(super) fn presets_outline(&self) -> Vec<Outline> {
        let mut groups = Outline::new("Preset groups", &["New preset group"]);
        for g in &self.sections.groups {
            groups.push(&g.name, &row_name("Group", &g.name));
        }
        let mut out = vec![groups];
        out.extend(self.section_list().into_iter().map(|id| {
            let mut section = Outline::new(id.name(), &[]);
            let yours = self.sections.get(id).yours();
            for name in yours.into_iter().chain(self.built_in(id)) {
                section.push(name, &row_name(id.name(), name));
            }
            section
        }));
        out
    }

    /// A section's built-in presets that do something on this platform.
    fn built_in(&self, id: SectionId) -> Vec<&'static str> {
        let section = self.sections.get(id);
        let selected = section.selected();
        (section.built_in().into_iter())
            .filter(|(name, media)| {
                self.platform.media || !media || selected == PresetId::BuiltIn((*name).into())
            })
            .map(|(name, _)| name)
            .collect()
    }

    pub(super) fn presets_groups<'a>(&'a self, rows: &Rows<'a>) -> Vec<Group<'a>> {
        let mut out = vec![self.preset_groups(rows)];
        for id in self.section_list() {
            let ctx = &rows.ctx;
            let section = self.sections.get(id);
            let selected = section.selected();
            let mut group = rows.group(id.name());
            let yours = section.yours().len();
            for i in 0..yours {
                let preset = self.your_preset(ctx, id, i);
                group = group.push(self.listed(rows, id, PresetId::Yours(i), preset));
            }
            for (i, name) in self.built_in(id).into_iter().enumerate() {
                let preset = PresetId::BuiltIn(name.into());
                let load = preset_row(
                    ctx,
                    FocusId::Indexed("built-in", preset_index(id, i)),
                    name.into(),
                    selected == preset,
                    SettingsMessage::LoadPreset(id, preset.clone()),
                );
                group = group.push(self.listed(rows, id, preset, load));
            }
            out.push(group);
        }
        out
    }

    /// A preset in the list, marked as built in, or as changed since it was loaded.
    fn listed<'a>(
        &'a self,
        rows: &Rows<'a>,
        id: SectionId,
        preset: PresetId,
        content: Element<'a, Message>,
    ) -> Element<'a, Message> {
        let built_in = matches!(preset, PresetId::BuiltIn(_));
        let label = row_name(id.name(), &self.preset_name(id, &preset));
        let focus = match &preset {
            PresetId::Yours(i) => FocusId::Indexed("preset", preset_index(id, *i)),
            PresetId::BuiltIn(name) => {
                let i = self.built_in(id).iter().position(|n| n == name);
                FocusId::Indexed("built-in", preset_index(id, i.unwrap_or(0)))
            }
        };
        rows.note_focus(&label, &focus);
        let mut r = row![content].spacing(8).align_y(Alignment::Center);
        if self.sections.get(id).selected() == preset && self.section_edited(id) {
            r = r.push(edited_tag(self.accent()));
        }
        if built_in {
            r = r.push(
                text("Built-in")
                    .size(12)
                    .color(TEXT_3)
                    .wrapping(Wrapping::None),
            );
        }
        let row = container(r)
            .padding(Padding {
                top: 2.0,
                right: if built_in { 14.0 } else { 4.0 },
                bottom: 2.0,
                left: 4.0,
            })
            .width(Length::Fill);
        rows.found(&label, row.into())
    }

    /// The preset groups, and the way to a new one.
    fn preset_groups<'a>(&'a self, rows: &Rows<'a>) -> Group<'a> {
        let mut out = rows.group("Preset groups");
        let groups = &self.sections.groups;
        for (i, g) in groups.iter().enumerate() {
            let what = Editing::Group(i);
            out = out.push(if self.settings.editing == Some(what) {
                let choices = self.group_choices(rows, i, g);
                rows.plain(self.editor(
                    &rows.ctx,
                    what,
                    i,
                    &g.name,
                    groups.len() - 1,
                    Some(choices),
                ))
            } else {
                self.group_row(rows, i, g)
            });
        }
        out.push(rows.action(
            "new-group",
            "New preset group",
            "Loads a preset in several sections at once. Starts with the ones in use now",
            "New group".into(),
            SettingsMessage::NewGroup.into(),
        ))
    }

    /// A group: to load, with a pencil to edit it.
    fn group_row<'a>(
        &'a self,
        rows: &Rows<'a>,
        i: usize,
        g: &'a PresetGroup,
    ) -> Element<'a, Message> {
        let ctx = &rows.ctx;
        let msg = Message::from(SettingsMessage::LoadGroup(i));
        let focused = ctx.focus(FocusId::Indexed("group-load", i), Some(msg.clone()), None);
        let in_use =
            (self.sections).group_in_use(i, &|shader, saved| self.fill_values(shader, saved));
        let what: Vec<String> = (g.presets.iter())
            .map(|(id, preset)| format!("{}: {}", id.name(), self.preset_name(*id, preset)))
            .collect();
        let what = if what.is_empty() {
            "Nothing in it yet".to_string()
        } else {
            what.join("  \u{00B7}  ")
        };
        let label = row![
            mark(in_use, ctx.accent),
            column![
                text(g.name.as_str())
                    .font(INTER_MEDIUM)
                    .size(14)
                    .color(TEXT),
                text(what).size(12).color(TEXT_3),
            ]
            .spacing(2),
        ]
        .spacing(10)
        .align_y(Alignment::Center);
        let load = button(label)
            .width(Length::Fill)
            .padding([10, 10])
            .style(ringed(focused, tree_row(in_use.then_some(LINE), TEXT)))
            .on_press(msg);
        let edit = pencil(ctx, FocusId::Indexed("group-edit", i), Editing::Group(i));
        let label = row_name("Group", &g.name);
        rows.note_focus(&label, &FocusId::Indexed("group-load", i));
        let row = container(row![load, edit].spacing(2).align_y(Alignment::Center)).padding(4);
        rows.found(&label, row.into())
    }

    fn preset_name(&self, id: SectionId, preset: &PresetId) -> String {
        match preset {
            PresetId::BuiltIn(name) => name.clone(),
            PresetId::Yours(i) => (self.sections.get(id).yours().get(*i))
                .map(|n| (*n).to_string())
                .unwrap_or_default(),
        }
    }

    /// Which preset of each section is in the group being edited.
    fn group_choices<'a>(
        &'a self,
        rows: &Rows<'a>,
        i: usize,
        g: &'a PresetGroup,
    ) -> Element<'a, Message> {
        let ctx = &rows.ctx;
        let mut list = column![].spacing(2);
        for id in self.section_list() {
            let chosen = g.presets.get(&id);
            let open = self.settings.choosing == Some(id);
            let msg = Message::from(SettingsMessage::Choose(id));
            let focused = ctx.focus(
                FocusId::Indexed("group-choose", id as usize),
                Some(msg.clone()),
                None,
            );
            let (name, color) = match chosen {
                Some(preset) => (ellipsize(&self.preset_name(id, preset), 24), TEXT),
                None => ("Left as it is".into(), TEXT_3),
            };
            let arrow = if open {
                icons::CHEVRON_UP
            } else {
                icons::CHEVRON_DOWN
            };
            let label = row![
                icon(section_icon(id), 18.0, TEXT_2),
                text(id.name())
                    .font(INTER_MEDIUM)
                    .size(14)
                    .color(TEXT)
                    .width(Length::Fill),
                text(name).size(14).color(color).wrapping(Wrapping::None),
                icon(arrow, 20.0, TEXT_2),
            ]
            .spacing(10)
            .align_y(Alignment::Center);
            list = list.push(
                button(container(label).center_y(Length::Fill))
                    .width(Length::Fill)
                    .height(ROW)
                    .padding([0, 10])
                    .style(ringed(focused, tree_row(open.then_some(LINE), TEXT)))
                    .on_press(msg),
            );
            if open {
                list = list.push(self.group_options(ctx, i, id, chosen));
            }
        }
        list.into()
    }

    /// The presets of a section to choose from.
    fn group_options<'a>(
        &'a self,
        ctx: &Ctx<'a>,
        i: usize,
        id: SectionId,
        chosen: Option<&PresetId>,
    ) -> Element<'a, Message> {
        let mut options: Vec<(String, Option<PresetId>)> = vec![("Left as it is".into(), None)];
        for (k, name) in self.sections.get(id).yours().into_iter().enumerate() {
            options.push((name.to_string(), Some(PresetId::Yours(k))));
        }
        for name in self.built_in(id) {
            options.push((name.into(), Some(PresetId::BuiltIn(name.into()))));
        }
        let rows = options.into_iter().enumerate().fold(
            column![].spacing(1),
            |col, (k, (name, preset))| {
                col.push(preset_row(
                    ctx,
                    FocusId::Indexed("group-option", k),
                    name,
                    chosen == preset.as_ref(),
                    SettingsMessage::SetInGroup(i, id, preset),
                ))
            },
        );
        container(rows)
            .padding(Padding {
                top: 2.0,
                right: 0.0,
                bottom: 6.0,
                left: 28.0,
            })
            .into()
    }
}

fn edited_tag<'a>(accent: Color) -> Element<'a, Message> {
    container(
        text("Edited")
            .font(INTER_MEDIUM)
            .size(12)
            .color(lighten(accent, 0.25))
            .wrapping(Wrapping::None),
    )
    .padding([2, 8])
    .style(tag(accent))
    .into()
}

fn note<'a>(s: String) -> Element<'a, Message> {
    container(text(s).size(13).color(TEXT_2))
        .padding([0, 10])
        .into()
}

/// The label of a preset's row, for the search.
fn row_name(of: &str, name: &str) -> String {
    format!("{of}: {name}")
}

/// The check mark of what's in use, or the room for one.
fn mark<'a>(on: bool, accent: Color) -> Element<'a, Message> {
    if on {
        icon(icons::CHECK, 18.0, accent).into()
    } else {
        Space::new().width(18).into()
    }
}

/// The button that starts editing a preset or group.
fn pencil<'a>(ctx: &Ctx<'a>, focus: FocusId, what: Editing) -> Element<'a, Message> {
    let msg = Message::from(SettingsMessage::Edit(Some(what)));
    let focused = ctx.focus(focus, Some(msg.clone()), None);
    tip(
        button(center(icon(icons::PENCIL, 18.0, TEXT_2)))
            .width(ROW)
            .height(ROW)
            .padding(0)
            .style(ringed(focused, tree_row(None, TEXT)))
            .on_press(msg),
        match what {
            Editing::Preset(..) => "Rename, reorder or delete",
            Editing::Group(_) => "Change, reorder or delete",
        },
        tooltip::Position::Top,
    )
}

/// A preset to pick, with a check mark if it's the one in use.
fn preset_row<'a>(
    ctx: &Ctx<'a>,
    focus: FocusId,
    name: String,
    current: bool,
    msg: SettingsMessage,
) -> Element<'a, Message> {
    let msg = Message::from(msg);
    let focused = ctx.focus(focus, Some(msg.clone()), None);
    button(
        container(
            row![
                mark(current, ctx.accent),
                text(name)
                    .font(INTER_MEDIUM)
                    .size(14)
                    .color(TEXT)
                    .wrapping(Wrapping::None),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        )
        .center_y(Length::Fill),
    )
    .width(Length::Fill)
    .height(ROW)
    .padding([0, 10])
    .clip(true)
    .style(ringed(focused, tree_row(current.then_some(LINE), TEXT)))
    .on_press(msg)
    .into()
}

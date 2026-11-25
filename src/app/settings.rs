//! The settings window's state, and what its controls do. (How it looks is in
//! `view/settings/`.)
//!
//! Changes apply to the app right away, so the window only keeps what it needs to
//! put them back ([`SettingsWindow::before`], for Cancel) and where you were.

use super::{App, Message, custom_hex_strings, focus_text_input, set_on_top};
use crate::focus::FocusId;
use crate::settings::{
    Action, Combo, PlaybackSettings, PresetId, SectionId, Sections, ShaderValues, UpdateSettings,
};
use crate::shaders;
use crate::shaders::settings::{Resolved, Value};
use iced::advanced::widget::Id;
use iced::advanced::widget::operation::{Operation, Outcome, Scrollable};
use iced::widget::scrollable::AbsoluteOffset;
use iced::{Rectangle, Size, Task, Vector};
use serde_json::Value as Json;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

/// Widget ids, for focusing and scrolling.
pub(crate) const CONTENT_SCROLL: &str = "settings-content";
/// The tree, and the list in the preset menu.
pub(crate) const TREE_SCROLL: &str = "settings-tree";
pub(crate) const MENU_SCROLL: &str = "settings-presets";
pub(crate) const SEARCH_INPUT: &str = "settings-search";
pub(crate) const PRESET_NAME_INPUT: &str = "preset-name";

/// Space left above a group's heading when scrolling to it.
const GROUP_MARGIN: f32 = 8.0;

/// How far from the top a setting that was searched for ends up.
const FOUND_MARGIN: f32 = 96.0;
/// How long it stays lit.
pub(crate) const FOUND_LIT: Duration = Duration::from_millis(1800);

/// The widget id of a group on the page shown, to scroll to it.
pub(crate) fn group_id(i: usize) -> Id {
    Id::from(format!("settings-group-{i}"))
}

/// The widget id of a setting's row on the page shown, by its label.
pub(crate) fn row_id(label: &str) -> Id {
    Id::from(format!("setting-{label}"))
}

/// What the settings window shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Page {
    /// A section's settings.
    Section(SectionId),
    /// Every section's presets, and groups of them.
    Presets,
}

impl Page {
    const COUNT: usize = SectionId::ALL.len() + 1;

    /// Its place among all of them.
    pub fn index(self) -> usize {
        match self {
            Page::Section(id) => id as usize,
            Page::Presets => Self::COUNT - 1,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Page::Section(id) => id.name(),
            Page::Presets => "Presets",
        }
    }

    pub fn section(self) -> Option<SectionId> {
        match self {
            Page::Section(id) => Some(id),
            Page::Presets => None,
        }
    }
}

impl From<SectionId> for Page {
    fn from(id: SectionId) -> Self {
        Page::Section(id)
    }
}

/// A setting, as its undo button knows it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum SettingKey {
    /// Some of a section's settings, by their saved names (a range is two).
    Fields(SectionId, &'static [&'static str]),
    /// One the visualizer (or background) in use declares: the shader, and its id.
    Shader {
        background: bool,
        shader: String,
        id: String,
    },
    /// One of the update switches.
    Updates(&'static str),
    /// A keyboard shortcut.
    Shortcut(Action),
}

impl SettingKey {
    /// What a starred setting is called in the settings file. `None` for what can't be
    /// starred. (A shader's setting is the same one whichever shader is in use.)
    pub fn favorite(&self) -> Option<String> {
        Some(match self {
            SettingKey::Fields(id, names) => format!("{}.{}", id.id(), names.first()?),
            SettingKey::Shader { background, id, .. } => {
                let section = if *background {
                    SectionId::Background
                } else {
                    SectionId::Visualizer
                };
                format!("{}:{id}", section.id())
            }
            SettingKey::Updates(name) => format!("updates.{name}"),
            SettingKey::Shortcut(_) => return None,
        })
    }
}

/// The section a starred setting is in, by its id.
pub(crate) fn favorite_section(id: &str) -> Option<SectionId> {
    let (section, _) = id.split_once(['.', ':'])?;
    if section == "updates" {
        return Some(SectionId::General);
    }
    SectionId::ALL.into_iter().find(|s| s.id() == section)
}

/// What the button next to a setting's name does, if it has one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    Undo,
    Redo,
}

/// Where a search goes: a page, one of its groups, and the label of a row in it.
type Found = (Page, usize, Option<String>);

/// What's being renamed, moved or deleted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Editing {
    Preset(SectionId, usize),
    Group(usize),
}

/// The window shape the mini preview takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Shape {
    #[default]
    Window,
    Ultrawide,
    Narrow,
}

impl Shape {
    pub const ALL: [Shape; 3] = [Shape::Window, Shape::Ultrawide, Shape::Narrow];

    pub fn name(self) -> &'static str {
        match self {
            Shape::Window => "Window",
            Shape::Ultrawide => "Ultrawide",
            Shape::Narrow => "Narrow",
        }
    }

    pub fn caption(self) -> &'static str {
        match self {
            Shape::Window => "A regular desktop window.",
            Shape::Ultrawide => "A very wide screen.",
            Shape::Narrow => "A tall, narrow window, like one snapped to the side.",
        }
    }

    /// The size the screen is laid out at.
    pub fn size(self) -> Size {
        match self {
            Shape::Window => Size::new(1440.0, 900.0),
            Shape::Ultrawide => Size::new(2150.0, 900.0),
            Shape::Narrow => Size::new(620.0, 1000.0),
        }
    }
}

/// The part of the screen a section affects, for the preview's "Only …" switch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Part {
    Visualizer,
    Background,
    TrackInfo,
    Playback,
    Lyrics,
}

impl Part {
    /// `None`: the page affects the whole screen.
    pub fn of(page: Page) -> Option<Self> {
        Some(match page.section()? {
            SectionId::Sound | SectionId::Visualizer | SectionId::Colors => Part::Visualizer,
            SectionId::Background => Part::Background,
            SectionId::NowPlaying => Part::TrackInfo,
            SectionId::Playback => Part::Playback,
            SectionId::Lyrics => Part::Lyrics,
            SectionId::Controls | SectionId::General => return None,
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            Part::Visualizer => "Only the visualizer",
            Part::Background => "Only the background",
            Part::TrackInfo => "Only track info",
            Part::Playback => "Only the playback bar",
            Part::Lyrics => "Only lyrics",
        }
    }
}

pub(crate) struct SettingsWindow {
    pub open: bool,
    /// Hidden for a look at the app ("Peek at full size").
    pub peek: bool,
    pub page: Page,
    /// The group of `page` the tree highlights.
    pub group: usize,
    /// Which pages are expanded in the tree, by [`Page::index`].
    expanded: [bool; Page::COUNT],
    /// The groups folded away in the content, by their page and heading.
    folded: HashSet<(Page, String)>,
    pub search: String,
    /// The setting (by its label) a search went to, and when: it's lit for a moment.
    pub found: Option<(String, Instant)>,
    pub shape: Shape,
    /// The preview shows only the part of the screen the section affects.
    pub only: bool,
    pub show_preview: bool,
    /// The preset menu.
    pub menu: bool,
    /// What's being renamed, and what's armed for deletion.
    pub editing: Option<Editing>,
    pub confirm_delete: Option<Editing>,
    /// The shortcut being changed: its action, and which of its keys.
    pub capturing: Option<(Action, usize)>,
    /// The section whose preset is being chosen, for the group being edited.
    pub choosing: Option<SectionId>,
    /// How far the content is scrolled.
    pub scroll: f32,
    /// Where the content was scrolled to for a click in the tree: until it's
    /// scrolled away from there, the clicked group stays highlighted.
    pinned: Option<f32>,
    /// A group clicked in the tree is about to be scrolled to.
    seeking: bool,
    /// Everything as it was when the window opened.
    before: Option<Box<Before>>,
    /// What the undo buttons go back to: the same, or a section's last loaded preset.
    base: Option<Box<Before>>,
    /// The undone values, for the redo buttons.
    undone: HashMap<SettingKey, Vec<Json>>,
}

/// Settings to go back to.
#[derive(Clone)]
struct Before {
    sections: Sections,
    updates: UpdateSettings,
}

impl Default for SettingsWindow {
    fn default() -> Self {
        Self {
            open: false,
            peek: false,
            page: Page::Section(SectionId::Sound),
            group: 0,
            expanded: [false; Page::COUNT],
            folded: HashSet::new(),
            search: String::new(),
            found: None,
            shape: Shape::default(),
            only: false,
            show_preview: true,
            menu: false,
            editing: None,
            confirm_delete: None,
            capturing: None,
            choosing: None,
            scroll: 0.0,
            pinned: None,
            seeking: false,
            before: None,
            base: None,
            undone: HashMap::new(),
        }
    }
}

impl SettingsWindow {
    /// Showing (open, and not hidden for a peek).
    pub fn shown(&self) -> bool {
        self.open && !self.peek
    }

    pub fn expanded(&self, page: Page) -> bool {
        self.expanded[page.index()]
    }

    fn expand(&mut self, page: Page, on: bool) {
        self.expanded[page.index()] = on;
    }

    /// Whether every one of `pages` is expanded.
    pub fn all_expanded(&self, pages: &[Page]) -> bool {
        pages.iter().all(|p| self.expanded(*p))
    }

    /// Whether a group is folded away in the content.
    pub fn folded(&self, page: Page, title: &str) -> bool {
        // (A handful at most.)
        self.folded.iter().any(|(p, t)| *p == page && t == title)
    }

    fn fold(&mut self, page: Page, title: &str, on: bool) {
        if on {
            self.folded.insert((page, title.to_string()));
        } else {
            self.folded.retain(|(p, t)| *p != page || t != title);
        }
    }

    /// The section shown.
    pub fn section(&self) -> Option<SectionId> {
        self.page.section()
    }

    /// Show `page`, from its top.
    fn go_to(&mut self, page: Page) {
        self.page = page;
        self.expand(page, true);
        self.group = 0;
        self.scroll = 0.0;
        self.pinned = None;
        self.seeking = false;
    }

    /// A choice about updates made outside the window while it's open (in the update
    /// pop-up): Cancel leaves it.
    pub fn keep_updates(&mut self, choice: impl FnOnce(&mut UpdateSettings)) {
        if let Some(before) = &mut self.before {
            choice(&mut before.updates);
        }
    }

    /// The focused part of the screen, while the preview shows only that.
    pub fn focused_part(&self) -> Option<Part> {
        Part::of(self.page).filter(|_| self.only)
    }
}

/// What to find in the content.
#[derive(Debug, Clone, PartialEq)]
pub enum Goal {
    /// The group at the top.
    Top,
    /// A group, to scroll to it.
    Group(usize),
    /// A setting (by its label) of a group.
    Setting(usize, String),
}

/// Where the groups of the page shown are.
#[derive(Debug, Clone, Default)]
pub struct Located {
    /// How far to scroll for each group to be at the top, in order.
    groups: Vec<f32>,
    /// The same for the setting looked for.
    setting: Option<f32>,
    /// The furthest the content scrolls.
    max: f32,
}

#[derive(Debug, Clone)]
pub enum SettingsMessage {
    /// The gear: open the window, or close it like Done.
    Toggle,
    /// Open the window at a section, or close it if it's already there.
    Open(SectionId),
    /// Close the window, keeping every change.
    Done,
    /// Close the window, putting everything back.
    Cancel,

    /// A page in the tree.
    Page(Page),
    /// Expand or collapse one in the tree, without going to it.
    Expand(Page, bool),
    /// Expand or collapse all of them.
    ExpandAll(bool),
    /// A group in the tree.
    Group(Page, usize),
    /// Fold a group of the content away, or back out.
    Fold(Page, String),
    Search(String),
    /// Go to the setting that matches the search best.
    SearchGo,
    /// The content scrolled this far.
    Scrolled(f32),
    /// Where things are, as asked by [`locate`].
    Located(Goal, Located),

    /// Wait for the key to use for a shortcut (the place among the action's).
    Capture(Action, usize),
    /// Put a setting back, or forward again.
    Undo(SettingKey),
    Redo(SettingKey),

    Shape(Shape),
    ToggleOnly,
    TogglePreview,
    Peek,
    EndPeek,

    ToggleMenu,
    LoadPreset(SectionId, PresetId),
    /// Overwrite the preset in use.
    SavePreset,
    SaveNewPreset,
    DiscardChanges,
    /// Start (Some) or finish (None) renaming a preset or group.
    Edit(Option<Editing>),
    Rename(Editing, String),
    /// Move a preset or group up (-1) or down (+1).
    Move(Editing, isize),
    /// First press arms, second press deletes.
    Delete(Editing),
    /// The section shown, back to its defaults.
    Reset,

    /// Make a group of the presets in use.
    NewGroup,
    LoadGroup(usize),
    /// Show (or hide) the presets of a section to choose from, for the group being
    /// edited.
    Choose(SectionId),
    SetInGroup(usize, SectionId, Option<PresetId>),
}

impl From<SettingsMessage> for Message {
    fn from(m: SettingsMessage) -> Self {
        Message::Settings(m)
    }
}

impl App {
    /// The sections that do something on this platform: track info and lyrics
    /// need a music player connection.
    pub(crate) fn section_list(&self) -> Vec<SectionId> {
        SectionId::ALL
            .into_iter()
            .filter(|s| {
                self.platform.media
                    || !matches!(
                        s,
                        SectionId::NowPlaying | SectionId::Playback | SectionId::Lyrics
                    )
            })
            .collect()
    }

    /// The pages of the settings window.
    pub(crate) fn pages(&self) -> Vec<Page> {
        let sections = self.section_list().into_iter().map(Page::Section);
        sections.chain([Page::Presets]).collect()
    }

    /// Fills in what a shader declares and the saved values leave out (see
    /// [`crate::settings::Fill`]).
    pub(crate) fn fill_values(&self, shader: &str, saved: &ShaderValues) -> ShaderValues {
        let decls = [shaders::Kind::Visualizer, shaders::Kind::Scene]
            .into_iter()
            .find_map(|kind| self.shaders.prepared(shader, kind))
            .map(|p| &p.decls);
        let Some(decls) = decls else {
            return saved.clone();
        };
        let mut out = saved.clone();
        let resolved = decls.resolve(Some(saved), &self.shaders.warps());
        for (d, v) in decls.items.iter().zip(resolved.values) {
            out.insert(d.id.clone(), json_value(&v));
        }
        out
    }

    /// Whether a section's settings differ from its preset.
    pub(crate) fn section_edited(&self, id: SectionId) -> bool {
        self.sections
            .get(id)
            .edited(&|shader, saved| self.fill_values(shader, saved))
    }

    /// Make sure every section has the preset it's on (see
    /// [`crate::settings::SectionPresets::settle`]).
    pub(crate) fn settle_presets(&mut self) {
        let mut sections = std::mem::take(&mut self.sections);
        sections.settle(&|shader, saved| self.fill_values(shader, saved));
        self.sections = sections;
    }

    /// Change the settings wholesale (a preset, Reset, Cancel), then catch up with
    /// them.
    fn change_sections(&mut self, change: impl FnOnce(&mut Sections)) -> Task<Message> {
        let on_top = self.sections.general.always_on_top;
        change(&mut self.sections);
        self.settle_presets();
        self.custom_hex = custom_hex_strings(&self.sections.colors.palette);
        self.mark_dirty();
        let now = self.sections.general.always_on_top;
        if now == on_top {
            Task::none()
        } else {
            set_on_top(now)
        }
    }

    /// Close the preset menu, finishing a rename in progress.
    fn close_preset_menu(&mut self) {
        self.finish_edit();
        self.settings.menu = false;
    }

    fn finish_edit(&mut self) {
        match self.settings.editing {
            Some(Editing::Preset(id, i)) => self.sections.get_mut(id).finish_rename(i),
            Some(Editing::Group(i)) => self.sections.finish_group(i),
            None => {}
        }
        if self.settings.editing.take().is_some() {
            self.mark_dirty();
        }
        self.settings.confirm_delete = None;
        self.settings.choosing = None;
    }

    // -----------------------------------------------------------------------
    // Undo

    /// The settings a shader in use declares, with the values saved in `sections`.
    fn declared(&self, background: bool, sections: &Sections) -> Resolved {
        let (active, values) = if background {
            (&self.bg, &sections.background.values)
        } else {
            (&self.vis, &sections.visualizer.values)
        };
        let decls = &active.resolved.decls;
        decls.resolve(values.get(&active.name), &self.shaders.warps())
    }

    /// A setting's values, in `sections` (or `updates`). `None` if it's not around.
    fn setting_in(
        &self,
        key: &SettingKey,
        sections: &Sections,
        updates: &UpdateSettings,
    ) -> Option<Vec<Json>> {
        let fields = |json: Json, names: &[&str]| -> Option<Vec<Json>> {
            names.iter().map(|n| json.get(*n).cloned()).collect()
        };
        match key {
            SettingKey::Fields(id, names) => fields(sections.get(*id).json(), names),
            SettingKey::Updates(name) => fields(serde_json::to_value(updates).ok()?, &[name]),
            SettingKey::Shortcut(action) => {
                let keys = sections.controls.shortcuts.saved(*action);
                Some(vec![keys.into()])
            }
            SettingKey::Shader {
                background,
                shader,
                id,
            } => {
                let active = if *background { &self.bg } else { &self.vis };
                if active.name != *shader {
                    return None;
                }
                let r = self.declared(*background, sections);
                let i = r.decls.items.iter().position(|d| d.id == *id)?;
                Some(vec![json_value(&r.values[i])])
            }
        }
    }

    fn setting_now(&self, key: &SettingKey) -> Option<Vec<Json>> {
        self.setting_in(key, &self.sections, &self.updates)
    }

    fn setting_base(&self, key: &SettingKey) -> Option<Vec<Json>> {
        let base = self.settings.base.as_ref()?;
        self.setting_in(key, &base.sections, &base.updates)
    }

    /// What the button next to a setting's name does: whether it was changed, or the
    /// change was undone.
    pub(crate) fn setting_change(&self, key: &SettingKey) -> Option<Change> {
        let (now, base) = (self.setting_now(key)?, self.setting_base(key)?);
        self.change(key, now != base)
    }

    /// The same, knowing whether it differs from what Undo goes back to.
    pub(crate) fn change(&self, key: &SettingKey, differs: bool) -> Option<Change> {
        if differs {
            Some(Change::Undo)
        } else {
            let redo = self.settings.undone.contains_key(key);
            redo.then_some(Change::Redo)
        }
    }

    /// A section's settings as they are and as Undo leaves them, as they're saved.
    pub(crate) fn section_json(&self, id: SectionId) -> (Json, Json) {
        let base = self.settings.base.as_ref();
        (
            self.sections.get(id).json(),
            base.map_or(Json::Null, |b| b.sections.get(id).json()),
        )
    }

    /// What Undo goes back to, of what a shader in use declares.
    pub(crate) fn declared_base(&self, background: bool) -> Option<Resolved> {
        let base = self.settings.base.as_ref()?;
        Some(self.declared(background, &base.sections))
    }

    fn set_setting(&mut self, key: &SettingKey, values: Vec<Json>) -> Task<Message> {
        match key {
            SettingKey::Fields(id, names) => {
                if *id == SectionId::Sound {
                    self.processor.cancel_calibration();
                }
                let fields: Vec<(&str, Json)> = names.iter().copied().zip(values).collect();
                self.change_sections(|s| {
                    s.get_mut(*id).set_json(&fields);
                })
            }
            SettingKey::Shader {
                background,
                shader,
                id,
            } => {
                let all = if *background {
                    &mut self.sections.background.values
                } else {
                    &mut self.sections.visualizer.values
                };
                if let Some(value) = values.into_iter().next() {
                    all.entry(shader.clone())
                        .or_default()
                        .insert(id.clone(), value);
                    self.mark_dirty();
                }
                Task::none()
            }
            SettingKey::Shortcut(action) => {
                let keys = values.first().and_then(Json::as_array);
                let keys = keys.into_iter().flatten().filter_map(Json::as_str);
                let keys = keys.filter_map(Combo::parse).collect();
                self.sections.controls.shortcuts.give(*action, keys);
                self.mark_dirty();
                Task::none()
            }
            SettingKey::Updates(name) => {
                if let (Ok(mut json), Some(value)) = (
                    serde_json::to_value(&self.updates),
                    values.into_iter().next(),
                ) {
                    json[*name] = value;
                    if let Ok(updates) = serde_json::from_value(json) {
                        self.updates = updates;
                        self.mark_dirty();
                    }
                }
                Task::none()
            }
        }
    }

    /// A setting that was changed again after it was undone has nothing to redo.
    pub(crate) fn forget_redos(&mut self) {
        if self.settings.undone.is_empty() {
            return;
        }
        let mut undone = std::mem::take(&mut self.settings.undone);
        undone.retain(|key, _| {
            // A shader that isn't in use keeps its own.
            match (self.setting_now(key), self.setting_base(key)) {
                (Some(now), Some(base)) => now == base,
                _ => true,
            }
        });
        self.settings.undone = undone;
    }

    /// A preset was loaded: from here, the undo buttons of its section go back to it.
    fn rebase(&mut self, id: SectionId) {
        let Json::Object(json) = self.sections.get(id).json() else {
            return;
        };
        if let Some(base) = &mut self.settings.base {
            let fields: Vec<(&str, Json)> =
                json.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
            base.sections.get_mut(id).set_json(&fields);
        }
        self.settings.undone.retain(|key, _| match key {
            SettingKey::Fields(of, _) => *of != id,
            SettingKey::Shortcut(_) => id != SectionId::Controls,
            SettingKey::Shader { background, .. } => {
                id != if *background {
                    SectionId::Background
                } else {
                    SectionId::Visualizer
                }
            }
            SettingKey::Updates(_) => true,
        });
    }

    /// Load a preset and go on from it.
    fn load_preset(&mut self, id: SectionId, preset: &PresetId) -> Task<Message> {
        if id == SectionId::Sound {
            self.processor.cancel_calibration();
        }
        let task = self.change_sections(|s| {
            s.get_mut(id).select(preset);
        });
        self.rebase(id);
        task
    }

    // -----------------------------------------------------------------------

    /// The setting (or group, or page) that matches the search best: where it is, and
    /// the label of its row.
    fn search_result(&self) -> Option<Found> {
        let query = self.settings.search.trim().to_lowercase();
        if query.is_empty() {
            return None;
        }
        // The same words first, then the same start, then the start of a word.
        let rank = |s: &str| {
            let s = s.to_lowercase();
            if s == query {
                Some(0)
            } else if s.starts_with(&query) {
                Some(1)
            } else if s.contains(&format!(" {query}")) {
                Some(2)
            } else {
                s.contains(&query).then_some(3)
            }
        };
        let mut best: Option<(u8, Found)> = None;
        let mut offer = |rank: Option<u8>, to: Found| {
            if let Some(rank) = rank
                && best.as_ref().is_none_or(|(b, _)| rank < *b)
            {
                best = Some((rank, to));
            }
        };
        for page in self.pages() {
            offer(rank(page.name()), (page, 0, None));
            for (i, g) in self.outline(page).into_iter().enumerate() {
                offer(rank(&g.title), (page, i, None));
                for l in g.labels {
                    offer(rank(&l.text), (page, i, Some(l.row)));
                }
            }
        }
        best.map(|(_, to)| to)
    }

    fn open_settings(&mut self, section: Option<SectionId>) -> Task<Message> {
        if let Some(page) = section
            .map(Page::Section)
            .filter(|p| *p != self.settings.page)
        {
            self.settings.go_to(page);
        }
        // Nothing else is in the way while it's open.
        self.about_open = false;
        self.picker_open = false;
        self.quick_open = false;
        self.volume_open = false;
        self.focus = None;
        if !self.settings.expanded.contains(&true) {
            self.settings.expand(self.settings.page, true);
        }
        if !self.settings.open {
            self.settings.open = true;
            let before = Box::new(Before {
                sections: self.sections.clone(),
                updates: self.updates.clone(),
            });
            self.settings.base = Some(before.clone());
            self.settings.before = Some(before);
            self.settings.search.clear();
            self.reduced_motion.refresh();
        }
        self.settings.peek = false;
        scroll_content(self.settings.scroll)
    }

    fn close_settings(&mut self) {
        self.close_preset_menu();
        self.settings.open = false;
        self.settings.peek = false;
        self.settings.before = None;
        self.settings.base = None;
        self.settings.undone.clear();
        self.settings.found = None;
        self.settings.capturing = None;
        // Nothing is left to show it.
        self.monitor.clear();
        // Keyboard users return to the gear.
        self.focus = self.focus.take().map(|_| FocusId::Named("open-settings"));
    }

    pub(crate) fn on_settings(&mut self, message: SettingsMessage) -> Task<Message> {
        use SettingsMessage as M;
        let page = self.settings.page;
        match message {
            M::Toggle if self.settings.shown() => self.close_settings(),
            M::Toggle => return self.open_settings(None),
            M::Open(id) if !self.section_list().contains(&id) => {}
            M::Open(id) if self.settings.shown() && page == id.into() => self.close_settings(),
            M::Open(id) if self.settings.shown() => {
                self.close_preset_menu();
                self.settings.go_to(id.into());
                return scroll_content(0.0);
            }
            M::Open(id) => return self.open_settings(Some(id)),
            M::Done => self.close_settings(),
            M::Cancel => {
                let before = self.settings.before.take();
                self.close_settings();
                if let Some(before) = before {
                    let Before { sections, updates } = *before;
                    self.processor.cancel_calibration();
                    self.updates = updates;
                    return self.change_sections(|s| *s = sections);
                }
            }

            M::Page(p) if p == page => {
                self.close_preset_menu();
                let on = self.settings.expanded(p);
                self.settings.expand(p, !on);
            }
            M::Page(p) => {
                self.close_preset_menu();
                self.settings.go_to(p);
                return scroll_content(0.0);
            }
            M::Expand(p, on) => self.settings.expand(p, on),
            M::ExpandAll(on) => {
                for p in self.pages() {
                    self.settings.expand(p, on);
                }
            }
            M::Capture(action, slot) => self.settings.capturing = Some((action, slot)),
            M::Group(p, i) => {
                self.settings.found = None;
                return self.go_to_group(p, i, Goal::Group(i));
            }
            M::Fold(p, title) => {
                let on = self.settings.folded(p, &title);
                self.settings.fold(p, &title, !on);
                // What's below moves up or down; the tree follows.
                return locate(Goal::Top);
            }
            M::Search(text) => self.settings.search = text,
            M::SearchGo => {
                if let Some((p, i, label)) = self.search_result() {
                    let goal = match label {
                        Some(label) => {
                            self.settings.found = Some((label.clone(), Instant::now()));
                            Goal::Setting(i, label)
                        }
                        None => Goal::Group(i),
                    };
                    return self.go_to_group(p, i, goal);
                }
            }
            M::Undo(key) => {
                if let (Some(now), Some(base)) = (self.setting_now(&key), self.setting_base(&key)) {
                    let task = self.set_setting(&key, base);
                    self.settings.undone.insert(key, now);
                    return task;
                }
            }
            M::Redo(key) => {
                if let Some(values) = self.settings.undone.remove(&key) {
                    return self.set_setting(&key, values);
                }
            }
            M::Scrolled(y) => {
                self.settings.scroll = y;
                // Another section's content starts out scrolled like the last one's.
                if self.settings.seeking {
                    return Task::none();
                }
                if self
                    .settings
                    .pinned
                    .is_some_and(|pin| (pin - y).abs() < 1.0)
                {
                    return Task::none();
                }
                self.settings.pinned = None;
                return locate(Goal::Top);
            }
            M::Located(Goal::Top, at) => self.settings.group = at.group(self.settings.scroll),
            M::Located(goal, at) => {
                self.settings.seeking = false;
                let (y, label) = match &goal {
                    Goal::Group(i) => (at.groups.get(*i).copied(), None),
                    Goal::Setting(i, label) => (
                        // A little below the top, in what's around it.
                        at.setting
                            .map(|y| y - FOUND_MARGIN)
                            .or_else(|| at.groups.get(*i).copied()),
                        Some(label),
                    ),
                    Goal::Top => (None, None),
                };
                let mut tasks = Vec::new();
                // The keyboard goes on from the setting that was found.
                if let Some(label) = label
                    && let Some(id) = self.row_focus(label)
                {
                    self.focus = Some(id);
                    tasks.push(focus_text_input(super::NO_TEXT_INPUT));
                }
                if let Some(y) = y.map(|y| y.clamp(0.0, at.max.max(0.0))) {
                    self.settings.pinned = Some(y);
                    self.settings.scroll = y;
                    tasks.push(scroll_content(y));
                }
                return Task::batch(tasks);
            }

            M::Shape(shape) => self.settings.shape = shape,
            M::ToggleOnly => self.settings.only = !self.settings.only,
            M::TogglePreview => self.settings.show_preview = !self.settings.show_preview,
            M::Peek => {
                self.close_preset_menu();
                self.settings.peek = true;
                self.focus = self.focus.take().map(|_| FocusId::Named("end-peek"));
            }
            M::EndPeek => {
                self.settings.peek = false;
                self.focus = self.focus.take().map(|_| FocusId::Named("peek"));
                // The content is laid out anew; put it back where it was.
                return scroll_content(self.settings.scroll);
            }

            M::ToggleMenu if self.settings.menu => {
                self.close_preset_menu();
                self.focus = self.focus.take().map(|_| FocusId::Named("preset-menu"));
            }
            M::ToggleMenu => self.settings.menu = self.settings.section().is_some(),
            M::LoadPreset(id, preset) => {
                if self.settings.menu {
                    self.focus = self.focus.take().map(|_| FocusId::Named("preset-menu"));
                }
                self.close_preset_menu();
                return self.load_preset(id, &preset);
            }
            M::SavePreset => {
                self.close_preset_menu();
                self.focus = self.focus.take().map(|_| FocusId::Named("preset-menu"));
                if let Some(id) = self.settings.section()
                    && self.sections.get_mut(id).save()
                {
                    self.save();
                }
            }
            M::SaveNewPreset => {
                self.finish_edit();
                let Some(id) = self.settings.section() else {
                    return Task::none();
                };
                let i = self.sections.get_mut(id).save_new();
                self.save();
                // Open it for naming, with the cursor in its name field.
                self.settings.editing = Some(Editing::Preset(id, i));
                self.focus = self
                    .focus
                    .take()
                    .map(|_| FocusId::Indexed("preset-name", i));
                return focus_text_input(PRESET_NAME_INPUT);
            }
            M::DiscardChanges => {
                self.close_preset_menu();
                self.focus = self.focus.take().map(|_| FocusId::Named("preset-menu"));
                if let Some(id) = self.settings.section() {
                    let preset = self.sections.get(id).selected();
                    return self.load_preset(id, &preset);
                }
            }
            M::Edit(edit) => {
                let was = self.settings.editing;
                self.finish_edit();
                self.settings.editing = edit.filter(|e| match *e {
                    Editing::Preset(id, i) => i < self.sections.get(id).yours().len(),
                    Editing::Group(i) => i < self.sections.groups.len(),
                });
                if self.settings.editing.is_some() {
                    return focus_text_input(PRESET_NAME_INPUT);
                }
                if let Some(was) = was {
                    self.focus = self.focus.take().map(|_| edit_focus(was));
                }
            }
            M::Rename(what, name) => {
                match what {
                    Editing::Preset(id, i) => self.sections.get_mut(id).rename(i, &name),
                    Editing::Group(i) => self.sections.rename_group(i, &name),
                }
                self.mark_dirty();
            }
            M::Move(what, delta) => {
                self.settings.editing = Some(match what {
                    Editing::Preset(id, i) => {
                        Editing::Preset(id, self.sections.move_preset(id, i, delta))
                    }
                    Editing::Group(i) => Editing::Group(self.sections.move_group(i, delta)),
                });
                self.settings.confirm_delete = None;
                self.mark_dirty();
            }
            M::Delete(what) if self.settings.confirm_delete == Some(what) => {
                self.settings.editing = None;
                self.settings.confirm_delete = None;
                self.settings.choosing = None;
                match what {
                    Editing::Preset(id, i) => {
                        self.sections.remove_preset(id, i);
                        self.settle_presets();
                    }
                    Editing::Group(i) => self.sections.remove_group(i),
                }
                self.mark_dirty();
                // (On the Presets page there's nothing in its place.)
                let menu = self.settings.menu;
                self.focus = self
                    .focus
                    .take()
                    .filter(|_| menu)
                    .map(|_| FocusId::Named("preset-menu"));
            }
            M::Delete(what) => self.settings.confirm_delete = Some(what),
            M::Reset => {
                self.close_preset_menu();
                let Some(id) = self.settings.section() else {
                    return Task::none();
                };
                match id {
                    SectionId::Sound => self.processor.cancel_calibration(),
                    // The update switches are shown here, though they aren't part of
                    // its presets.
                    SectionId::General if self.platform.updates => {
                        self.updates = UpdateSettings::default();
                    }
                    _ => {}
                }
                return self.change_sections(|s| s.get_mut(id).reset());
            }

            M::NewGroup => {
                self.finish_edit();
                let list = self.section_list();
                let i = self.sections.new_group(&list);
                self.mark_dirty();
                self.settings.editing = Some(Editing::Group(i));
                self.focus = self
                    .focus
                    .take()
                    .map(|_| FocusId::Indexed("preset-name", i));
                return focus_text_input(PRESET_NAME_INPUT);
            }
            M::LoadGroup(i) => {
                self.finish_edit();
                let Some(ids) = self
                    .sections
                    .groups
                    .get(i)
                    .map(|g| g.presets.keys().copied().collect::<Vec<_>>())
                else {
                    return Task::none();
                };
                self.processor.cancel_calibration();
                let task = self.change_sections(|s| {
                    s.select_group(i);
                });
                for id in ids {
                    self.rebase(id);
                }
                return task;
            }
            M::Choose(id) => {
                self.settings.choosing = (self.settings.choosing != Some(id)).then_some(id);
            }
            M::SetInGroup(i, id, preset) => {
                self.sections.set_in_group(i, id, preset);
                self.settings.choosing = None;
                self.focus = self
                    .focus
                    .take()
                    .map(|_| FocusId::Indexed("group-choose", id as usize));
                self.mark_dirty();
            }
        }
        Task::none()
    }
}

impl App {
    /// Show a group of a page, unfolded, then scroll to `goal`.
    fn go_to_group(&mut self, page: Page, i: usize, goal: Goal) -> Task<Message> {
        if page != self.settings.page {
            self.close_preset_menu();
            self.settings.go_to(page);
        }
        if let Some(g) = self.outline(page).get(i) {
            let title = g.title.clone();
            self.settings.fold(page, &title, false);
        }
        self.settings.group = i;
        self.settings.seeking = true;
        locate(goal)
    }

    /// The control of the row with this label, as last built.
    fn row_focus(&self, label: &str) -> Option<FocusId> {
        let rows = self.row_focus.borrow();
        let row = rows.iter().find(|r| r.label == label)?;
        Some(row.focus.clone())
    }

    /// A searched-for setting stops being lit after a moment. Returns whether one is.
    pub(crate) fn fade_found(&mut self, now: Instant) -> bool {
        if (self.settings.found.as_ref())
            .is_some_and(|(_, at)| now.saturating_duration_since(*at) >= FOUND_LIT)
        {
            self.settings.found = None;
        }
        self.settings.found.is_some()
    }

    pub(crate) fn set_playback(&mut self, p: PlaybackSettings) {
        self.sections.playback.settings = p;
        self.mark_dirty();
    }
}

/// Where the keyboard is after something was renamed.
fn edit_focus(was: Editing) -> FocusId {
    match was {
        Editing::Preset(id, i) => FocusId::Indexed("preset-edit", preset_index(id, i)),
        Editing::Group(i) => FocusId::Indexed("group-edit", i),
    }
}

/// A preset's place among every section's, for focus ids.
pub(crate) fn preset_index(id: SectionId, i: usize) -> usize {
    id as usize * 1000 + i
}

/// A shader setting's value, as it's saved.
fn json_value(v: &Value) -> Json {
    match v {
        Value::Int(x) => (*x).into(),
        Value::Float(x) => f64::from(*x).into(),
        Value::Toggle(b) => (*b).into(),
        Value::Choice(s) => s.clone().into(),
        Value::Color([r, g, b]) => format!("#{r:02x}{g:02x}{b:02x}").into(),
    }
}

fn scroll_content(y: f32) -> Task<Message> {
    iced::widget::operation::scroll_to(
        Id::new(CONTENT_SCROLL),
        AbsoluteOffset {
            x: None,
            y: Some(y),
        },
    )
}

impl Located {
    /// The group at the top when scrolled to `y` (the last one that has reached
    /// it, or come close).
    fn group(&self, y: f32) -> usize {
        // At the very end the last groups can't reach the top, but the last one
        // is in full view.
        if self.max > 0.0 && y >= self.max - 1.0 {
            return self.groups.len().saturating_sub(1);
        }
        self.groups
            .iter()
            .rposition(|g| *g <= y + 24.0)
            .unwrap_or(0)
    }
}

/// Find where the groups (and the goal's setting) are in the content, as laid out,
/// then send [`SettingsMessage::Located`].
fn locate(goal: Goal) -> Task<Message> {
    struct Locate {
        top: Option<f32>,
        max: f32,
        groups: Vec<(usize, f32)>,
        setting: Option<(Id, Option<f32>)>,
    }

    impl Operation<Located> for Locate {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<Located>)) {
            operate(self);
        }

        fn container(&mut self, id: Option<&Id>, bounds: Rectangle) {
            // A section has a handful of groups; the ids say which is which.
            if let Some(id) = id
                && let Some(i) = (0..64).find(|i| *id == group_id(*i))
            {
                self.groups.push((i, bounds.y));
            }
            if let (Some(id), Some((wanted, at))) = (id, &mut self.setting)
                && id == wanted
                && at.is_none()
            {
                *at = Some(bounds.y);
            }
        }

        fn scrollable(
            &mut self,
            id: Option<&Id>,
            bounds: Rectangle,
            content_bounds: Rectangle,
            _translation: Vector,
            _state: &mut dyn Scrollable,
        ) {
            if id == Some(&Id::new(CONTENT_SCROLL)) {
                self.top = Some(bounds.y);
                self.max = content_bounds.height - bounds.height;
            }
        }

        fn finish(&self) -> Outcome<Located> {
            // A little room above the heading.
            let top = self.top.unwrap_or(0.0) + GROUP_MARGIN;
            let mut groups = self.groups.clone();
            groups.sort_by_key(|(i, _)| *i);
            Outcome::Some(Located {
                groups: groups.into_iter().map(|(_, y)| y - top).collect(),
                setting: (self.setting.as_ref()).and_then(|(_, at)| at.map(|y| y - top)),
                max: self.max.max(0.0),
            })
        }
    }

    iced::advanced::widget::operate(Locate {
        top: None,
        max: 0.0,
        groups: Vec::new(),
        setting: match &goal {
            Goal::Setting(_, label) => Some((row_id(label), None)),
            _ => None,
        },
    })
    .map(move |at| SettingsMessage::Located(goal.clone(), at).into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clicked_group_stays_highlighted() {
        let (mut app, _) = App::new();
        let at = || Located {
            groups: vec![0.0, 300.0, 700.0, 900.0],
            max: 600.0,
            ..Located::default()
        };
        let mut send = |m: SettingsMessage| {
            let _ = app.update(m.into());
            app.settings.group
        };
        send(SettingsMessage::Toggle);
        // Clicked while the content is still scrolled like the last section's.
        let background = SectionId::Background.into();
        assert_eq!(send(SettingsMessage::Group(background, 2)), 2);
        assert_eq!(send(SettingsMessage::Scrolled(450.0)), 2);
        assert_eq!(send(SettingsMessage::Located(Goal::Group(2), at())), 2);
        // It can't reach the top, but it's the one that was clicked.
        assert_eq!(send(SettingsMessage::Scrolled(600.0)), 2);
        // Scrolling away follows the content again.
        send(SettingsMessage::Scrolled(310.0));
        assert_eq!(send(SettingsMessage::Located(Goal::Top, at())), 1);
    }

    #[test]
    fn the_group_at_the_top() {
        let at = Located {
            groups: vec![0.0, 300.0, 700.0, 900.0],
            max: 600.0,
            ..Located::default()
        };
        assert_eq!(at.group(0.0), 0);
        assert_eq!(at.group(290.0), 1, "close counts");
        assert_eq!(at.group(400.0), 1);
        assert_eq!(at.group(600.0), 3, "at the end, the last one");
        // Everything fits: the first one.
        let at = Located {
            groups: vec![0.0, 200.0],
            ..Located::default()
        };
        assert_eq!(at.group(0.0), 0);
    }
}

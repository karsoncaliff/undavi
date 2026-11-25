//! Sections and their presets.
//!
//! Each section of the settings window ([`SectionId`]) keeps its settings, the
//! presets saved from them and the preset in use together, as a [`Section`].
//! [`SectionPresets`] is what the preset menu needs from any of them.

use super::{
    Accent, BackgroundSettings, ColorSettings, ControlSettings, Detail, FrameRate, GeneralSettings,
    LyricsSettings, NowPlayingMode, NowPlayingSettings, Palette, PlaybackSettings, PlaybackStyle,
    ShaderValues, TuningSettings, VisualSettings,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// A section of the settings window.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SectionId {
    /// How the bars react to sound ([`TuningSettings`]).
    Sound,
    /// Which visualizer, and its settings ([`VisualSettings`]).
    Visualizer,
    /// The palette and the accent ([`ColorSettings`]).
    Colors,
    /// Which background, its settings and effects ([`BackgroundSettings`]).
    Background,
    /// How the current track is shown ([`NowPlayingSettings`]).
    NowPlaying,
    /// The seek bar and the buttons around it ([`PlaybackSettings`]).
    Playback,
    /// The lyrics ([`LyricsSettings`]).
    Lyrics,
    /// The keyboard and the mouse ([`ControlSettings`]).
    Controls,
    /// Auto-hide, the window and updates ([`GeneralSettings`]).
    General,
}

impl SectionId {
    /// Every section, in the order they're listed.
    pub const ALL: [SectionId; 9] = [
        SectionId::Sound,
        SectionId::Visualizer,
        SectionId::Colors,
        SectionId::Background,
        SectionId::NowPlaying,
        SectionId::Playback,
        SectionId::Lyrics,
        SectionId::Controls,
        SectionId::General,
    ];

    /// What it's called in the settings file.
    pub fn id(self) -> &'static str {
        match self {
            SectionId::Sound => "sound",
            SectionId::Visualizer => "visualizer",
            SectionId::Colors => "colors",
            SectionId::Background => "background",
            SectionId::NowPlaying => "now_playing",
            SectionId::Playback => "playback",
            SectionId::Lyrics => "lyrics",
            SectionId::Controls => "controls",
            SectionId::General => "general",
        }
    }

    /// The name shown in the settings window.
    pub fn name(self) -> &'static str {
        match self {
            SectionId::Sound => "Sound",
            SectionId::Visualizer => "Visualizer",
            SectionId::Colors => "Colors",
            SectionId::Background => "Background",
            SectionId::NowPlaying => "Now playing",
            SectionId::Playback => "Playback bar",
            SectionId::Lyrics => "Lyrics",
            SectionId::Controls => "Controls",
            SectionId::General => "General",
        }
    }
}

/// A preset of some section.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PresetId {
    /// One that comes with Undavi, by its name.
    BuiltIn(String),
    /// One the user saved, by its place in [`Section::presets`].
    Yours(usize),
}

/// A saved set of one section's settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Preset<T> {
    /// The name the user gave it (at most 40 characters).
    pub name: String,
    /// The settings it restores.
    pub settings: T,
}

/// A preset that comes with Undavi.
#[derive(Clone, Debug, PartialEq)]
pub struct BuiltIn<T> {
    /// The name shown in the menu.
    pub name: &'static str,
    /// The settings it restores.
    pub settings: T,
}

/// Fills in the settings a shader (by name) declares but the saved values leave
/// out, so two sets of values that look the same also compare the same.
pub type Fill<'a> = &'a dyn Fn(&str, &ShaderValues) -> ShaderValues;

/// The settings of one section.
pub trait SectionSettings: Clone + PartialEq + Default {
    /// The presets that come with Undavi. The first one is the default settings.
    fn built_in() -> Vec<BuiltIn<Self>>;

    /// Bring every field back into its documented range (used after loading from
    /// disk).
    fn clamped(self) -> Self {
        self
    }

    /// What a preset stores: everything that's part of the section's look.
    fn for_preset(&self) -> Self {
        self.clone()
    }

    /// Take on a preset's settings, keeping what presets don't store.
    fn take(&mut self, preset: &Self) {
        *self = preset.clone();
    }

    /// The settings as compared with a preset's.
    fn filled(&self, _fill: Fill<'_>) -> Self {
        self.for_preset()
    }

    /// Whether these only make sense with a music player connection.
    fn needs_media(&self) -> bool {
        false
    }
}

/// One section: its settings, the presets saved from them, and the one in use.
/// Reads as its settings (`section.sensitivity`).
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Section<T> {
    /// The current settings.
    pub settings: T,
    /// The user's presets.
    pub presets: Vec<Preset<T>>,
    /// The preset the settings were last loaded from or saved as. `None` until
    /// [`SectionPresets::settle`] has looked for it.
    pub selected: Option<PresetId>,
}

impl<T> std::ops::Deref for Section<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.settings
    }
}

impl<T> std::ops::DerefMut for Section<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.settings
    }
}

impl<T: SectionSettings> Section<T> {
    /// The settings a preset restores, if it exists.
    fn preset(&self, id: &PresetId) -> Option<T> {
        match id {
            PresetId::BuiltIn(name) => T::built_in()
                .into_iter()
                .find(|b| b.name == name)
                .map(|b| b.settings),
            PresetId::Yours(i) => self.presets.get(*i).map(|p| p.settings.clone()),
        }
    }

    fn default_id() -> PresetId {
        let first = T::built_in().first().map_or("", |b| b.name);
        PresetId::BuiltIn(first.into())
    }

    /// Bring the settings, and every preset's, back into range.
    pub fn clamped(mut self) -> Self {
        self.settings = self.settings.clamped();
        for p in &mut self.presets {
            p.settings = p.settings.clone().clamped();
        }
        self
    }
}

/// What the preset menu does with a section, whatever its settings are.
pub trait SectionPresets {
    /// The settings, as they're saved.
    fn json(&self) -> Value;
    /// Change some of the settings, by their saved names. Returns whether that made
    /// sense.
    fn set_json(&mut self, fields: &[(&str, Value)]) -> bool;
    /// The names of the presets that come with Undavi, and whether each needs a
    /// music player connection.
    fn built_in(&self) -> Vec<(&'static str, bool)>;
    /// The names of the user's presets.
    fn yours(&self) -> Vec<&str>;
    /// The preset in use.
    fn selected(&self) -> PresetId;
    /// Its name.
    fn selected_name(&self) -> String;
    /// Whether the settings differ from it.
    fn edited(&self, fill: Fill<'_>) -> bool;
    /// Make sure the preset in use exists: if it doesn't, pick one the settings
    /// match, or the default one (they're then an edited version of it).
    fn settle(&mut self, fill: Fill<'_>);
    /// Load a preset. Returns whether it exists.
    fn select(&mut self, id: &PresetId) -> bool;
    /// Go back to the settings of the preset in use.
    fn discard(&mut self);
    /// Overwrite the preset in use with the current settings. Returns whether it
    /// could: built-in ones can't be overwritten.
    fn save(&mut self) -> bool;
    /// Save the current settings as a new preset and use it. Returns its place
    /// among the user's presets.
    fn save_new(&mut self) -> usize;
    /// Back to the default settings (and the default preset).
    fn reset(&mut self);
    /// Rename one of the user's presets.
    fn rename(&mut self, i: usize, name: &str);
    /// Tidy a name after typing: trimmed, and never empty.
    fn finish_rename(&mut self, i: usize);
    /// Move one of the user's presets up (-1) or down (+1). Returns its new place.
    fn shift(&mut self, i: usize, delta: isize) -> usize;
    /// Delete one of the user's presets.
    fn remove(&mut self, i: usize);
}

/// How long a name gets.
const MAX_NAME: usize = 40;

/// "Preset N" (or whatever `kind` is) with the first N not already taken.
fn next_name(kind: &str, taken: &[&str]) -> String {
    (1..)
        .map(|n| format!("{kind} {n}"))
        .find(|name| !taken.contains(&name.as_str()))
        .unwrap_or_default()
}

impl<T: SectionSettings + Serialize + serde::de::DeserializeOwned> SectionPresets for Section<T> {
    fn json(&self) -> Value {
        serde_json::to_value(&self.settings).unwrap_or_default()
    }

    fn set_json(&mut self, fields: &[(&str, Value)]) -> bool {
        let mut json = self.json();
        let Value::Object(map) = &mut json else {
            return false;
        };
        for (name, value) in fields {
            map.insert((*name).to_string(), value.clone());
        }
        match serde_json::from_value::<T>(json) {
            Ok(settings) => {
                self.settings = settings.clamped();
                true
            }
            Err(_) => false,
        }
    }

    fn built_in(&self) -> Vec<(&'static str, bool)> {
        T::built_in()
            .into_iter()
            .map(|b| (b.name, b.settings.needs_media()))
            .collect()
    }

    fn yours(&self) -> Vec<&str> {
        self.presets.iter().map(|p| p.name.as_str()).collect()
    }

    fn selected(&self) -> PresetId {
        self.selected.clone().unwrap_or_else(Self::default_id)
    }

    fn selected_name(&self) -> String {
        match self.selected() {
            PresetId::BuiltIn(name) => name,
            PresetId::Yours(i) => self
                .presets
                .get(i)
                .map(|p| p.name.clone())
                .unwrap_or_default(),
        }
    }

    fn edited(&self, fill: Fill<'_>) -> bool {
        self.preset(&self.selected())
            .is_none_or(|p| p.filled(fill) != self.settings.filled(fill))
    }

    fn settle(&mut self, fill: Fill<'_>) {
        if self
            .selected
            .as_ref()
            .is_some_and(|id| self.preset(id).is_some())
        {
            return;
        }
        let now = self.settings.filled(fill);
        let yours = self
            .presets
            .iter()
            .position(|p| p.settings.filled(fill) == now)
            .map(PresetId::Yours);
        let built_in = || {
            T::built_in()
                .into_iter()
                .find(|b| b.settings.filled(fill) == now)
                .map(|b| PresetId::BuiltIn(b.name.into()))
        };
        self.selected = Some(yours.or_else(built_in).unwrap_or_else(Self::default_id));
    }

    fn select(&mut self, id: &PresetId) -> bool {
        let Some(preset) = self.preset(id) else {
            return false;
        };
        self.settings.take(&preset);
        self.selected = Some(id.clone());
        true
    }

    fn discard(&mut self) {
        let id = self.selected();
        self.select(&id);
    }

    fn save(&mut self) -> bool {
        match self.selected() {
            PresetId::Yours(i) if i < self.presets.len() => {
                self.presets[i].settings = self.settings.for_preset();
                true
            }
            _ => false,
        }
    }

    fn save_new(&mut self) -> usize {
        let name = next_name("Preset", &self.yours());
        self.presets.push(Preset {
            name,
            settings: self.settings.for_preset(),
        });
        let i = self.presets.len() - 1;
        self.selected = Some(PresetId::Yours(i));
        i
    }

    fn reset(&mut self) {
        self.settings = T::default();
        self.selected = Some(Self::default_id());
    }

    fn rename(&mut self, i: usize, name: &str) {
        if let Some(p) = self.presets.get_mut(i) {
            p.name = name.chars().take(MAX_NAME).collect();
        }
    }

    fn finish_rename(&mut self, i: usize) {
        let Some(p) = self.presets.get(i) else { return };
        let trimmed = p.name.trim().to_string();
        let name = if trimmed.is_empty() {
            let mut others = self.yours();
            others.remove(i);
            next_name("Preset", &others)
        } else {
            trimmed
        };
        self.presets[i].name = name;
    }

    fn shift(&mut self, i: usize, delta: isize) -> usize {
        let Some(j) = i.checked_add_signed(delta) else {
            return i;
        };
        if i >= self.presets.len() || j >= self.presets.len() {
            return i;
        }
        self.presets.swap(i, j);
        self.selected = match self.selected.take() {
            Some(PresetId::Yours(k)) if k == i => Some(PresetId::Yours(j)),
            Some(PresetId::Yours(k)) if k == j => Some(PresetId::Yours(i)),
            other => other,
        };
        j
    }

    fn remove(&mut self, i: usize) {
        if i >= self.presets.len() {
            return;
        }
        self.presets.remove(i);
        self.selected = match self.selected.take() {
            Some(PresetId::Yours(k)) if k == i => None,
            Some(PresetId::Yours(k)) if k > i => Some(PresetId::Yours(k - 1)),
            other => other,
        };
    }
}

/// Every section.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Sections {
    /// How the bars react to sound.
    pub sound: Section<TuningSettings>,
    /// Which visualizer, and its settings.
    pub visualizer: Section<VisualSettings>,
    /// The palette and the accent.
    pub colors: Section<ColorSettings>,
    /// Which background, its settings and effects.
    pub background: Section<BackgroundSettings>,
    /// How the current track is shown.
    pub now_playing: Section<NowPlayingSettings>,
    /// The seek bar and the buttons around it.
    pub playback: Section<PlaybackSettings>,
    /// The lyrics.
    pub lyrics: Section<LyricsSettings>,
    /// The keyboard and the mouse.
    pub controls: Section<ControlSettings>,
    /// Auto-hide and the window. Saved as "window" before it was renamed.
    #[serde(alias = "window")]
    pub general: Section<GeneralSettings>,
    /// The user's preset groups.
    pub groups: Vec<PresetGroup>,
}

/// Presets of several sections, to load together.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PresetGroup {
    /// The name the user gave it (at most 40 characters).
    pub name: String,
    /// The preset of each section in it. The other sections are left as they are.
    pub presets: BTreeMap<SectionId, PresetId>,
}

impl Sections {
    /// A section's presets.
    pub fn get(&self, id: SectionId) -> &dyn SectionPresets {
        match id {
            SectionId::Sound => &self.sound,
            SectionId::Visualizer => &self.visualizer,
            SectionId::Colors => &self.colors,
            SectionId::Background => &self.background,
            SectionId::NowPlaying => &self.now_playing,
            SectionId::Playback => &self.playback,
            SectionId::Lyrics => &self.lyrics,
            SectionId::Controls => &self.controls,
            SectionId::General => &self.general,
        }
    }

    /// A section's presets, to change them.
    pub fn get_mut(&mut self, id: SectionId) -> &mut dyn SectionPresets {
        match id {
            SectionId::Sound => &mut self.sound,
            SectionId::Visualizer => &mut self.visualizer,
            SectionId::Colors => &mut self.colors,
            SectionId::Background => &mut self.background,
            SectionId::NowPlaying => &mut self.now_playing,
            SectionId::Playback => &mut self.playback,
            SectionId::Lyrics => &mut self.lyrics,
            SectionId::Controls => &mut self.controls,
            SectionId::General => &mut self.general,
        }
    }

    /// Settle every section (see [`SectionPresets::settle`]).
    pub fn settle(&mut self, fill: Fill<'_>) {
        for id in SectionId::ALL {
            self.get_mut(id).settle(fill);
        }
    }

    /// Move one of a section's presets up (-1) or down (+1), and keep the groups it's
    /// in on it. Returns its new place.
    pub fn move_preset(&mut self, id: SectionId, i: usize, delta: isize) -> usize {
        let j = self.get_mut(id).shift(i, delta);
        if i != j {
            for preset in self
                .groups
                .iter_mut()
                .filter_map(|g| g.presets.get_mut(&id))
            {
                match preset {
                    PresetId::Yours(k) if *k == i => *k = j,
                    PresetId::Yours(k) if *k == j => *k = i,
                    _ => {}
                }
            }
        }
        j
    }

    /// Delete one of a section's presets, and take it out of the groups it's in.
    pub fn remove_preset(&mut self, id: SectionId, i: usize) {
        if i >= self.get(id).yours().len() {
            return;
        }
        self.get_mut(id).remove(i);
        for g in &mut self.groups {
            match g.presets.get_mut(&id) {
                Some(PresetId::Yours(k)) if *k == i => {
                    g.presets.remove(&id);
                }
                Some(PresetId::Yours(k)) if *k > i => *k -= 1,
                _ => {}
            }
        }
    }

    /// Make a group of the presets in use in `sections`. Returns its place.
    pub fn new_group(&mut self, sections: &[SectionId]) -> usize {
        let taken: Vec<&str> = self.groups.iter().map(|g| g.name.as_str()).collect();
        let name = next_name("Group", &taken);
        let presets = sections
            .iter()
            .map(|&id| (id, self.get(id).selected()))
            .collect();
        self.groups.push(PresetGroup { name, presets });
        self.groups.len() - 1
    }

    /// Load every preset of a group. Returns whether it exists.
    pub fn select_group(&mut self, i: usize) -> bool {
        let Some(group) = self.groups.get(i) else {
            return false;
        };
        for (id, preset) in group.presets.clone() {
            self.get_mut(id).select(&preset);
        }
        true
    }

    /// Whether the presets in use are a group's, with nothing changed since.
    pub fn group_in_use(&self, i: usize, fill: Fill<'_>) -> bool {
        self.groups.get(i).is_some_and(|g| {
            !g.presets.is_empty()
                && g.presets.iter().all(|(&id, preset)| {
                    let section = self.get(id);
                    section.selected() == *preset && !section.edited(fill)
                })
        })
    }

    /// Put a section's preset into a group, or take the section out of it.
    pub fn set_in_group(&mut self, i: usize, id: SectionId, preset: Option<PresetId>) {
        if let Some(g) = self.groups.get_mut(i) {
            match preset {
                Some(preset) => g.presets.insert(id, preset),
                None => g.presets.remove(&id),
            };
        }
    }

    /// Rename a group.
    pub fn rename_group(&mut self, i: usize, name: &str) {
        if let Some(g) = self.groups.get_mut(i) {
            g.name = name.chars().take(MAX_NAME).collect();
        }
    }

    /// Tidy a group's name after typing: trimmed, and never empty.
    pub fn finish_group(&mut self, i: usize) {
        let Some(name) = self.groups.get(i).map(|g| g.name.trim().to_string()) else {
            return;
        };
        self.groups[i].name = if name.is_empty() {
            let others: Vec<&str> = (self.groups.iter().enumerate())
                .filter(|(k, _)| *k != i)
                .map(|(_, g)| g.name.as_str())
                .collect();
            next_name("Group", &others)
        } else {
            name
        };
    }

    /// Move a group up (-1) or down (+1). Returns its new place.
    pub fn move_group(&mut self, i: usize, delta: isize) -> usize {
        match i.checked_add_signed(delta) {
            Some(j) if i < self.groups.len() && j < self.groups.len() => {
                self.groups.swap(i, j);
                j
            }
            _ => i,
        }
    }

    /// Delete a group (not the presets in it).
    pub fn remove_group(&mut self, i: usize) {
        if i < self.groups.len() {
            self.groups.remove(i);
        }
    }

    /// Bring everything back into range, and take presets that don't exist out of the
    /// groups.
    pub fn clamped(self) -> Self {
        let mut out = self.in_range();
        let mut groups = std::mem::take(&mut out.groups);
        for g in &mut groups {
            g.presets.retain(|&id, preset| match preset {
                PresetId::Yours(i) => *i < out.get(id).yours().len(),
                PresetId::BuiltIn(name) => (out.get(id).built_in().iter()).any(|(n, _)| n == name),
            });
        }
        out.groups = groups;
        out
    }

    fn in_range(self) -> Self {
        Self {
            sound: self.sound.clamped(),
            visualizer: self.visualizer.clamped(),
            colors: self.colors.clamped(),
            background: self.background.clamped(),
            now_playing: self.now_playing.clamped(),
            playback: self.playback.clamped(),
            lyrics: self.lyrics.clamped(),
            controls: self.controls.clamped(),
            general: self.general.clamped(),
            groups: self.groups,
        }
    }
}

// ---------------------------------------------------------------------------
// What each section's presets hold, and the ones that come with Undavi

fn built_in<T>(name: &'static str, settings: T) -> BuiltIn<T> {
    BuiltIn { name, settings }
}

impl SectionSettings for TuningSettings {
    fn built_in() -> Vec<BuiltIn<Self>> {
        let d = Self::default;
        vec![
            built_in("Balanced", d()),
            built_in(
                "Punchy",
                Self {
                    spread: 20,
                    rise_ms: 6,
                    fall_ms: 130,
                    contrast: 68,
                    detail: Detail::Fast,
                    ..d()
                },
            ),
            built_in(
                "Smooth",
                Self {
                    spread: 75,
                    rise_ms: 30,
                    fall_ms: 420,
                    contrast: 42,
                    ..d()
                },
            ),
            built_in(
                "Bass heavy",
                Self {
                    bass_db: 9,
                    highs_db: -5,
                    ..d()
                },
            ),
            built_in(
                "Quiet music",
                Self {
                    auto_gain: true,
                    floor_db: -90,
                    gate_db: -85,
                    beat_sensitivity: 65,
                    ..d()
                },
            ),
        ]
    }

    fn clamped(self) -> Self {
        TuningSettings::clamped(self)
    }
}

/// One shader's values, for a built-in preset.
fn values(shader: &str, pairs: &[(&str, Value)]) -> BTreeMap<String, ShaderValues> {
    if pairs.is_empty() {
        return Default::default();
    }
    let values = pairs
        .iter()
        .map(|(id, v)| ((*id).to_string(), v.clone()))
        .collect();
    [(shader.to_string(), values)].into_iter().collect()
}

/// Only the picked shader's values: what a preset stores.
fn picked(shader: &str, all: &BTreeMap<String, ShaderValues>) -> BTreeMap<String, ShaderValues> {
    all.get(shader)
        .filter(|v| !v.is_empty())
        .map(|v| (shader.to_string(), v.clone()))
        .into_iter()
        .collect()
}

/// Set the picked shader's values from a preset, leaving the other shaders'.
fn take_picked(
    shader: &str,
    from: &BTreeMap<String, ShaderValues>,
    into: &mut BTreeMap<String, ShaderValues>,
) {
    match from.get(shader) {
        Some(v) => {
            into.insert(shader.to_string(), v.clone());
        }
        None => {
            into.remove(shader);
        }
    }
}

/// Only the picked shader's values, with the ones left out filled in.
fn filled_picked(
    shader: &str,
    all: &BTreeMap<String, ShaderValues>,
    fill: Fill<'_>,
) -> BTreeMap<String, ShaderValues> {
    let saved = all.get(shader).cloned().unwrap_or_default();
    [(shader.to_string(), fill(shader, &saved))]
        .into_iter()
        .collect()
}

impl SectionSettings for VisualSettings {
    fn built_in() -> Vec<BuiltIn<Self>> {
        let look = |shader: &str, pairs: &[(&str, Value)]| Self {
            shader: shader.into(),
            values: values(shader, pairs),
        };
        vec![
            built_in("Classic", look("bars", &[])),
            built_in("Upright", look("bars", &[("mirror", "Off".into())])),
            built_in(
                "Dense",
                look(
                    "bars",
                    &[
                        ("bar_count", 96.into()),
                        ("bar_width", 4.into()),
                        ("spacing", 4.into()),
                    ],
                ),
            ),
            built_in(
                "Chunky",
                look(
                    "bars",
                    &[
                        ("bar_count", 24.into()),
                        ("bar_width", 16.into()),
                        ("spacing", 14.into()),
                        ("corners", "Rounded".into()),
                    ],
                ),
            ),
            built_in(
                "LED meter",
                look(
                    "bars",
                    &[
                        ("mirror", "Off".into()),
                        ("bar_count", 32.into()),
                        ("bar_width", 14.into()),
                        ("segments", true.into()),
                        ("peak_caps", true.into()),
                    ],
                ),
            ),
            built_in(
                "Radial",
                look(
                    "radial",
                    &[
                        ("bar_count", 72.into()),
                        ("bar_width", 6.into()),
                        ("corners", "Pill".into()),
                        ("mirror", "Left/right".into()),
                    ],
                ),
            ),
            built_in("Wave", look("wave", &[("bar_width", 5.into())])),
        ]
    }

    fn for_preset(&self) -> Self {
        Self {
            shader: self.shader.clone(),
            values: picked(&self.shader, &self.values),
        }
    }

    fn take(&mut self, preset: &Self) {
        self.shader = preset.shader.clone();
        take_picked(&preset.shader, &preset.values, &mut self.values);
    }

    fn filled(&self, fill: Fill<'_>) -> Self {
        Self {
            shader: self.shader.clone(),
            values: filled_picked(&self.shader, &self.values, fill),
        }
    }
}

impl SectionSettings for ColorSettings {
    fn built_in() -> Vec<BuiltIn<Self>> {
        let colors = |palette, accent| Self {
            palette,
            accent,
            ..Self::default()
        };
        vec![
            built_in("Undavi", colors(Palette::Undavi, Accent::Sky)),
            built_in("Silver", colors(Palette::Silver, Accent::White)),
            built_in("Aurora", colors(Palette::Aurora, Accent::Mint)),
            built_in("Ember", colors(Palette::Ember, Accent::Rose)),
            built_in("Album art", colors(Palette::Album, Accent::Album)),
        ]
    }

    fn needs_media(&self) -> bool {
        self.palette == Palette::Album || self.accent == Accent::Album
    }
}

impl SectionSettings for BackgroundSettings {
    fn built_in() -> Vec<BuiltIn<Self>> {
        let look = |shader: &str, pairs: &[(&str, Value)]| Self {
            shader: shader.into(),
            values: values(shader, pairs),
            ..Self::default()
        };
        vec![
            built_in("Album art", look("album-art", &[])),
            built_in(
                "Vivid",
                Self {
                    dim_pct: 35,
                    saturation: 135,
                    contrast: 110,
                    ..look("album-art", &[("blur", 50.into())])
                },
            ),
            built_in(
                "Liquid",
                look(
                    "album-art",
                    &[
                        ("arrangement", "Single".into()),
                        ("animated", true.into()),
                        ("animation", "Liquid".into()),
                        ("blur", 40.into()),
                    ],
                ),
            ),
            built_in("Ambient", look("ambient", &[])),
            built_in("Solid", look("solid", &[])),
        ]
    }

    fn clamped(self) -> Self {
        BackgroundSettings::clamped(self)
    }

    fn for_preset(&self) -> Self {
        Self {
            values: picked(&self.shader, &self.values),
            ..self.clone()
        }
    }

    fn take(&mut self, preset: &Self) {
        let mut values = std::mem::take(&mut self.values);
        take_picked(&preset.shader, &preset.values, &mut values);
        *self = Self {
            values,
            ..preset.clone()
        };
    }

    fn filled(&self, fill: Fill<'_>) -> Self {
        Self {
            values: filled_picked(&self.shader, &self.values, fill),
            ..self.clone()
        }
    }
}

impl SectionSettings for NowPlayingSettings {
    fn built_in() -> Vec<BuiltIn<Self>> {
        let mode = |mode| Self {
            mode,
            ..Self::default()
        };
        vec![
            built_in("Header", mode(NowPlayingMode::Header)),
            built_in("Spotlight", mode(NowPlayingMode::Spotlight)),
            built_in("Minimal", mode(NowPlayingMode::Minimal)),
        ]
    }
}

impl SectionSettings for PlaybackSettings {
    fn built_in() -> Vec<BuiltIn<Self>> {
        let style = |style, compact| Self {
            style,
            compact,
            ..Self::default()
        };
        vec![
            built_in("Open", style(PlaybackStyle::Open, false)),
            built_in("Card", style(PlaybackStyle::Card, false)),
            built_in("Dock", style(PlaybackStyle::Dock, false)),
            built_in("Compact", style(PlaybackStyle::Card, true)),
        ]
    }

    fn clamped(self) -> Self {
        PlaybackSettings::clamped(self)
    }

    fn needs_media(&self) -> bool {
        true
    }
}

impl SectionSettings for LyricsSettings {
    fn built_in() -> Vec<BuiltIn<Self>> {
        vec![
            built_in("Standard", Self::default()),
            built_in(
                "Large",
                Self {
                    size: 135,
                    glow: 80,
                    ..Self::default()
                },
            ),
            built_in(
                "Subtle",
                Self {
                    size: 85,
                    glow: 0,
                    ..Self::default()
                },
            ),
        ]
    }

    fn clamped(self) -> Self {
        LyricsSettings::clamped(self)
    }

    fn needs_media(&self) -> bool {
        true
    }
}

impl SectionSettings for ControlSettings {
    fn built_in() -> Vec<BuiltIn<Self>> {
        vec![built_in("Standard", Self::default())]
    }

    fn clamped(self) -> Self {
        ControlSettings::clamped(self)
    }
}

impl SectionSettings for GeneralSettings {
    fn built_in() -> Vec<BuiltIn<Self>> {
        vec![
            built_in("Standard", Self::default()),
            built_in(
                "Always visible",
                Self {
                    auto_hide: false,
                    ..Self::default()
                },
            ),
            built_in(
                "Battery saver",
                Self {
                    frame_rate: FrameRate::Fps30,
                    ..Self::default()
                },
            ),
        ]
    }

    fn clamped(self) -> Self {
        GeneralSettings::clamped(self)
    }

    /// "Always on top" is about this window rather than the look, so presets
    /// leave it as it is.
    fn for_preset(&self) -> Self {
        Self {
            always_on_top: false,
            ..self.clone()
        }
    }

    fn take(&mut self, preset: &Self) {
        *self = Self {
            always_on_top: self.always_on_top,
            ..preset.clone()
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn as_is(_: &str, values: &ShaderValues) -> ShaderValues {
        values.clone()
    }

    #[test]
    fn every_section_starts_on_an_unedited_preset() {
        let mut s = Sections::default();
        s.settle(&as_is);
        for id in SectionId::ALL {
            let section = s.get(id);
            assert!(!section.edited(&as_is), "{}", id.name());
            assert_eq!(
                section.selected_name(),
                section.built_in()[0].0,
                "{}",
                id.name()
            );
            // Every built-in preset loads, and is then the one in use.
            for (name, _) in s.get(id).built_in() {
                let mut s = s.clone();
                assert!(s.get_mut(id).select(&PresetId::BuiltIn(name.into())));
                assert!(!s.get(id).edited(&as_is), "{} {name}", id.name());
                assert_eq!(s.get(id).selected_name(), name);
            }
        }
    }

    #[test]
    fn built_in_presets_cannot_be_overwritten() {
        let mut s = Section::<TuningSettings>::default();
        s.settle(&as_is);
        s.settings.sensitivity = 10;
        assert!(s.edited(&as_is));
        assert!(!s.save());
        s.discard();
        assert_eq!(s.settings, TuningSettings::default());
        assert!(!s.edited(&as_is));
    }

    #[test]
    fn saving_changing_and_discarding() {
        let mut s = Section::<TuningSettings>::default();
        s.settings.sensitivity = 10;
        assert_eq!(s.save_new(), 0);
        assert_eq!(s.selected_name(), "Preset 1");
        assert!(!s.edited(&as_is));
        s.settings.sensitivity = 20;
        assert!(s.edited(&as_is));
        assert!(s.save());
        assert_eq!(s.presets[0].settings.sensitivity, 20);
        s.settings.sensitivity = 30;
        s.discard();
        assert_eq!(s.settings.sensitivity, 20);
        s.reset();
        assert_eq!(s.selected_name(), "Balanced");
    }

    #[test]
    fn the_preset_in_use_follows_moves_and_deletes() {
        let mut s = Section::<TuningSettings>::default();
        for n in [10, 20, 30] {
            s.settings.sensitivity = n;
            s.save_new();
        }
        s.rename(2, "Third");
        assert_eq!(s.shift(2, -1), 1);
        assert_eq!(s.selected_name(), "Third");
        assert_eq!(s.shift(0, -1), 0, "already first");
        s.remove(0);
        assert_eq!(s.selected_name(), "Third");
        // Without its preset, the settings belong to one they match…
        s.settings = s.presets[1].settings.clone();
        s.remove(0);
        s.settle(&as_is);
        assert_eq!(s.selected, Some(PresetId::Yours(0)));
        // …or are an edited version of the default one.
        s.remove(0);
        s.settle(&as_is);
        assert_eq!(s.selected_name(), "Balanced");
        assert!(s.edited(&as_is));
    }

    #[test]
    fn emptied_names_get_a_new_default() {
        let mut s = Section::<TuningSettings>::default();
        s.save_new();
        s.save_new();
        s.rename(1, "  Night drive  ");
        s.finish_rename(1);
        assert_eq!(s.presets[1].name, "Night drive");
        s.rename(1, "   ");
        s.finish_rename(1);
        assert_eq!(s.presets[1].name, "Preset 2");
        s.rename(0, &"x".repeat(80));
        assert_eq!(s.presets[0].name.len(), 40);
    }

    #[test]
    fn shader_presets_leave_other_shaders_alone() {
        let mut s = Section::<VisualSettings>::default();
        let set = |s: &mut Section<VisualSettings>, shader: &str, id: &str, v: Value| {
            s.settings
                .values
                .entry(shader.into())
                .or_default()
                .insert(id.into(), v);
        };
        set(&mut s, "wave", "bar_width", 4.into());
        set(&mut s, "bars", "bar_count", 20.into());
        s.save_new();
        assert!(
            !s.presets[0].settings.values.contains_key("wave"),
            "a preset only stores the shader it's for"
        );
        // A built-in without values puts its shader back to the defaults.
        assert!(s.select(&PresetId::BuiltIn("Classic".into())));
        assert!(!s.settings.values.contains_key("bars"));
        assert!(s.settings.values.contains_key("wave"));
        // Changing a shader that isn't picked isn't an edit.
        set(&mut s, "radial", "bar_count", 60.into());
        assert!(!s.edited(&as_is));
    }

    #[test]
    fn always_on_top_is_not_part_of_a_preset() {
        let mut s = Section::<GeneralSettings>::default();
        s.settle(&as_is);
        s.settings.always_on_top = true;
        assert!(!s.edited(&as_is));
        assert!(s.select(&PresetId::BuiltIn("Battery saver".into())));
        assert!(s.settings.always_on_top);
        assert_eq!(s.settings.frame_rate, FrameRate::Fps30);
    }
}

//! The Controls section: keyboard shortcuts, and what the mouse does.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

/// Something a shortcut does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Action {
    /// Play or pause.
    PlayPause,
    /// Go back a track.
    Previous,
    /// Skip to the next track.
    Next,
    /// Seek back one step.
    SeekBack,
    /// Seek forward one step.
    SeekForward,
    /// Turn the player up one step.
    VolumeUp,
    /// Turn the player down one step.
    VolumeDown,
    /// Mute or unmute the player.
    Mute,
    /// Turn repeat on or off.
    Repeat,
    /// Enter or leave full screen.
    Fullscreen,
    /// Open or close the settings.
    Settings,
    /// Open or close the quick settings.
    Quick,
    /// Open or close the audio source menu.
    Source,
    /// Open or close the About panel.
    About,
    /// Read the song's info again.
    Refresh,
    /// Open the settings at Sound.
    Sound,
    /// Open the settings at Visualizer.
    Visualizer,
    /// Open the settings at Background.
    Background,
    /// Open the settings at General.
    General,
    /// Star the setting that has the keyboard focus.
    Favorite,
}

/// How the shortcuts are grouped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionGroup {
    /// The music player.
    Playback,
    /// What's on the screen.
    Window,
    /// The settings window.
    Settings,
}

impl Action {
    /// Every action, in the order they're listed.
    pub const ALL: [Action; 20] = [
        Action::PlayPause,
        Action::Previous,
        Action::Next,
        Action::SeekBack,
        Action::SeekForward,
        Action::VolumeUp,
        Action::VolumeDown,
        Action::Mute,
        Action::Repeat,
        Action::Fullscreen,
        Action::Settings,
        Action::Quick,
        Action::Source,
        Action::About,
        Action::Refresh,
        Action::Sound,
        Action::Visualizer,
        Action::Background,
        Action::General,
        Action::Favorite,
    ];

    /// What it's called in the settings file.
    pub fn id(self) -> &'static str {
        match self {
            Action::PlayPause => "play_pause",
            Action::Previous => "previous",
            Action::Next => "next",
            Action::SeekBack => "seek_back",
            Action::SeekForward => "seek_forward",
            Action::VolumeUp => "volume_up",
            Action::VolumeDown => "volume_down",
            Action::Mute => "mute",
            Action::Repeat => "repeat",
            Action::Fullscreen => "fullscreen",
            Action::Settings => "settings",
            Action::Quick => "quick_settings",
            Action::Source => "source",
            Action::About => "about",
            Action::Refresh => "refresh",
            Action::Sound => "open_sound",
            Action::Visualizer => "open_visualizer",
            Action::Background => "open_background",
            Action::General => "open_general",
            Action::Favorite => "favorite",
        }
    }

    /// What it's called in the settings window.
    pub fn name(self) -> &'static str {
        match self {
            Action::PlayPause => "Play or pause",
            Action::Previous => "Previous track",
            Action::Next => "Next track",
            Action::SeekBack => "Seek back",
            Action::SeekForward => "Seek forward",
            Action::VolumeUp => "Volume up",
            Action::VolumeDown => "Volume down",
            Action::Mute => "Mute",
            Action::Repeat => "Repeat",
            Action::Fullscreen => "Full screen",
            Action::Settings => "Settings",
            Action::Quick => "Quick settings",
            Action::Source => "Audio source",
            Action::About => "About",
            Action::Refresh => "Refresh track info",
            Action::Sound => "Open Sound",
            Action::Visualizer => "Open Visualizer",
            Action::Background => "Open Background",
            Action::General => "Open General",
            Action::Favorite => "Star the focused setting",
        }
    }

    /// Where it's listed.
    pub fn group(self) -> ActionGroup {
        match self {
            Action::PlayPause
            | Action::Previous
            | Action::Next
            | Action::SeekBack
            | Action::SeekForward
            | Action::VolumeUp
            | Action::VolumeDown
            | Action::Mute
            | Action::Repeat => ActionGroup::Playback,
            Action::Fullscreen
            | Action::Quick
            | Action::Source
            | Action::About
            | Action::Refresh => ActionGroup::Window,
            Action::Settings
            | Action::Sound
            | Action::Visualizer
            | Action::Background
            | Action::General
            | Action::Favorite => ActionGroup::Settings,
        }
    }

    /// Whether it needs a music player connection.
    pub fn needs_media(self) -> bool {
        self.group() == ActionGroup::Playback || self == Action::Refresh
    }

    /// Its keys until they're changed.
    fn defaults(self) -> Vec<Combo> {
        let keys: &[&str] = match self {
            Action::PlayPause => &["Space"],
            Action::SeekBack => &["ArrowLeft"],
            Action::SeekForward => &["ArrowRight"],
            Action::Fullscreen => &["f", "F11"],
            Action::Settings => &["s"],
            Action::Quick => &["q"],
            Action::Refresh => &["r"],
            Action::Sound => &["t"],
            Action::Visualizer => &["v"],
            Action::Background => &["b"],
            Action::General => &["i"],
            Action::Favorite => &["ctrl+d"],
            Action::Previous
            | Action::Next
            | Action::VolumeUp
            | Action::VolumeDown
            | Action::Mute
            | Action::Repeat
            | Action::Source
            | Action::About => &[],
        };
        keys.iter().filter_map(|k| Combo::parse(k)).collect()
    }
}

/// A key, and what's held down with it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Combo {
    /// Ctrl is held down.
    pub ctrl: bool,
    /// Alt is held down.
    pub alt: bool,
    /// Shift is held down.
    pub shift: bool,
    /// A character, or a key's name ("ArrowLeft", "F11").
    pub key: String,
}

impl Combo {
    /// Read one as it's saved: "ctrl+shift+s".
    pub fn parse(s: &str) -> Option<Self> {
        let mut out = Self {
            ctrl: false,
            alt: false,
            shift: false,
            key: String::new(),
        };
        // The key itself can be a plus.
        let mut rest = s.trim();
        while let Some((held, more)) = rest.split_once('+')
            && !more.is_empty()
        {
            match held.to_lowercase().as_str() {
                "ctrl" => out.ctrl = true,
                "alt" => out.alt = true,
                "shift" => out.shift = true,
                _ => return None,
            }
            rest = more;
        }
        out.key = if rest.chars().count() == 1 {
            rest.to_lowercase()
        } else {
            rest.to_string()
        };
        let named = out.key.chars().all(|c| c.is_ascii_alphanumeric());
        let reserved = ["Tab", "Escape", "Enter"].contains(&out.key.as_str());
        (!out.key.is_empty() && (named || out.key.chars().count() == 1) && !reserved).then_some(out)
    }

    /// As it's saved.
    fn saved(&self) -> String {
        let held = [
            (self.ctrl, "ctrl+"),
            (self.alt, "alt+"),
            (self.shift, "shift+"),
        ];
        let held: String = held.iter().filter(|(on, _)| *on).map(|(_, s)| *s).collect();
        held + &self.key
    }
}

/// As it's shown: "Ctrl + S".
impl fmt::Display for Combo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (on, name) in [
            (self.ctrl, "Ctrl"),
            (self.alt, "Alt"),
            (self.shift, "Shift"),
        ] {
            if on {
                write!(f, "{name} + ")?;
            }
        }
        match self.key.as_str() {
            "ArrowLeft" => f.write_str("Left"),
            "ArrowRight" => f.write_str("Right"),
            "ArrowUp" => f.write_str("Up"),
            "ArrowDown" => f.write_str("Down"),
            "PageUp" => f.write_str("Page Up"),
            "PageDown" => f.write_str("Page Down"),
            key if key.chars().count() == 1 => f.write_str(&key.to_uppercase()),
            key => f.write_str(key),
        }
    }
}

/// How many shortcuts an action has at most.
pub const MAX_KEYS: usize = 2;

/// The keyboard shortcuts. Only the ones that were changed are kept (and saved).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(from = "BTreeMap<String, Vec<String>>")]
#[serde(into = "BTreeMap<String, Vec<String>>")]
pub struct Shortcuts {
    changed: BTreeMap<Action, Vec<Combo>>,
}

impl From<BTreeMap<String, Vec<String>>> for Shortcuts {
    /// What doesn't make sense is left out.
    fn from(saved: BTreeMap<String, Vec<String>>) -> Self {
        let mut out = Self::default();
        for (id, keys) in saved {
            if let Some(action) = Action::ALL.into_iter().find(|a| a.id() == id) {
                out.set(
                    action,
                    keys.iter().filter_map(|k| Combo::parse(k)).collect(),
                );
            }
        }
        out
    }
}

impl From<Shortcuts> for BTreeMap<String, Vec<String>> {
    fn from(s: Shortcuts) -> Self {
        let saved = |keys: Vec<Combo>| keys.iter().map(Combo::saved).collect();
        s.changed
            .into_iter()
            .map(|(a, keys)| (a.id().to_string(), saved(keys)))
            .collect()
    }
}

impl Shortcuts {
    /// An action's keys.
    pub fn keys(&self, action: Action) -> Vec<Combo> {
        self.changed
            .get(&action)
            .cloned()
            .unwrap_or_else(|| action.defaults())
    }

    /// An action's keys, as they're saved.
    pub fn saved(&self, action: Action) -> Vec<String> {
        self.keys(action).iter().map(Combo::saved).collect()
    }

    /// The action a key is for.
    pub fn action(&self, combo: &Combo) -> Option<Action> {
        Action::ALL
            .into_iter()
            .find(|a| self.keys(*a).contains(combo))
    }

    /// Only these keys for an action.
    fn set(&mut self, action: Action, mut keys: Vec<Combo>) {
        keys.dedup();
        keys.truncate(MAX_KEYS);
        if keys == action.defaults() {
            self.changed.remove(&action);
        } else {
            self.changed.insert(action, keys);
        }
    }

    /// Give an action these keys, taking them from any others.
    pub fn give(&mut self, action: Action, keys: Vec<Combo>) {
        for other in Action::ALL {
            let mine = self.keys(other);
            if other != action && mine.iter().any(|k| keys.contains(k)) {
                let left = mine.into_iter().filter(|k| !keys.contains(k)).collect();
                self.set(other, left);
            }
        }
        self.set(action, keys);
    }

    /// Change one of an action's keys (by its place), add one after the last, or
    /// (`None`) take it away.
    pub fn bind(&mut self, action: Action, slot: usize, combo: Option<Combo>) {
        let mut keys = self.keys(action);
        match combo {
            Some(combo) => {
                let slot = match keys.iter().position(|k| *k == combo) {
                    // It's there already.
                    Some(_) => return,
                    None => slot.min(keys.len()),
                };
                if slot < keys.len() {
                    keys[slot] = combo;
                } else {
                    keys.push(combo);
                }
            }
            None if slot < keys.len() => {
                keys.remove(slot);
            }
            None => return,
        }
        self.give(action, keys);
    }
}

/// What double-clicking the bars does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DoubleClick {
    /// Enters or leaves full screen.
    #[default]
    Fullscreen,
    /// Plays or pauses.
    PlayPause,
    /// Nothing.
    Nothing,
}

/// What scrolling over the bars does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BarsScroll {
    /// Nothing.
    #[default]
    Nothing,
    /// Turns the player up or down.
    Volume,
    /// Seeks.
    Seek,
}

/// The Controls section.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ControlSettings {
    /// The keyboard shortcuts.
    pub shortcuts: Shortcuts,
    /// How far seeking by one step goes, 1..=60 seconds.
    pub seek_step_s: u8,
    /// How much one step changes the volume, 1..=25 %.
    pub volume_step: u8,
    /// What double-clicking the bars does.
    pub double_click: DoubleClick,
    /// What scrolling over the bars does.
    pub scroll: BarsScroll,
}

impl Default for ControlSettings {
    fn default() -> Self {
        Self {
            shortcuts: Shortcuts::default(),
            seek_step_s: 5,
            volume_step: 5,
            double_click: DoubleClick::Fullscreen,
            scroll: BarsScroll::Nothing,
        }
    }
}

impl ControlSettings {
    /// Bring every field back into its documented range.
    pub fn clamped(mut self) -> Self {
        self.seek_step_s = self.seek_step_s.clamp(1, 60);
        self.volume_step = self.volume_step.clamp(1, 25);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn combo(s: &str) -> Combo {
        Combo::parse(s).unwrap()
    }

    #[test]
    fn combos_are_read_saved_and_shown() {
        for (saved, shown) in [
            ("s", "S"),
            ("ctrl+shift+s", "Ctrl + Shift + S"),
            ("ArrowLeft", "Left"),
            ("F11", "F11"),
            ("ctrl++", "Ctrl + +"),
            ("+", "+"),
            ("alt+Space", "Alt + Space"),
        ] {
            assert_eq!(combo(saved).to_string(), shown);
            assert_eq!(combo(saved).saved(), saved);
        }
        assert_eq!(combo("Shift+S"), combo("shift+s"));
        for bad in [
            "",
            "ctrl+",
            "super+s",
            "Tab",
            "shift+Tab",
            "Escape",
            "Enter",
            "no such key",
        ] {
            assert_eq!(Combo::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn every_key_is_for_one_action() {
        let s = Shortcuts::default();
        for a in Action::ALL {
            for key in s.keys(a) {
                assert_eq!(s.action(&key), Some(a), "{key}");
            }
            assert_eq!(Action::ALL.iter().filter(|b| b.id() == a.id()).count(), 1);
        }
        assert_eq!(s.action(&combo("ctrl+s")), None, "only the exact keys");
    }

    #[test]
    fn shortcuts_change_and_move() {
        let mut s = Shortcuts::default();
        // A second one, and one instead of it.
        s.bind(Action::PlayPause, 1, Some(combo("k")));
        assert_eq!(s.saved(Action::PlayPause), ["Space", "k"]);
        s.bind(Action::PlayPause, 1, Some(combo("p")));
        assert_eq!(s.saved(Action::PlayPause), ["Space", "p"]);
        // Taken from another action.
        s.bind(Action::Next, 0, Some(combo("f")));
        assert_eq!(s.action(&combo("f")), Some(Action::Next));
        assert_eq!(s.saved(Action::Fullscreen), ["F11"]);
        // The same one twice is once.
        s.bind(Action::Next, 1, Some(combo("f")));
        assert_eq!(s.saved(Action::Next), ["f"]);
        // Taken away.
        s.bind(Action::Next, 0, None);
        assert_eq!(s.action(&combo("f")), None);
        s.bind(Action::Next, 0, None);
        // Back as it was: nothing to save.
        s.give(Action::Fullscreen, vec![combo("f"), combo("F11")]);
        s.bind(Action::PlayPause, 1, None);
        assert_eq!(s, Shortcuts::default());
    }

    #[test]
    fn only_changed_shortcuts_are_saved() {
        let mut s = ControlSettings::default();
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains(r#""shortcuts":{}"#), "{json}");
        s.shortcuts.bind(Action::Mute, 0, Some(combo("m")));
        s.shortcuts.bind(Action::Settings, 0, None);
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains(r#"{"mute":["m"],"settings":[]}"#), "{json}");
        assert_eq!(serde_json::from_str::<ControlSettings>(&json).unwrap(), s);
        // Unknown actions and keys are skipped.
        let odd = r#"{"shortcuts": {"fly": ["z"], "mute": ["Tab", "m", "super+m"]}}"#;
        let odd: ControlSettings = serde_json::from_str(odd).unwrap();
        assert_eq!(odd.shortcuts.saved(Action::Mute), ["m"]);
    }
}

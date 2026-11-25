# Settings

## Where they live

One JSON file, `undavi/settings.json`, in the platform's config folder:

| Platform | Path |
|---|---|
| Linux | `$XDG_CONFIG_HOME/undavi/settings.json`, usually `~/.config/undavi/settings.json` |
| Windows | `%APPDATA%\undavi\settings.json` |
| macOS | `~/Library/Application Support/undavi/settings.json` |

The [`dirs`](https://crates.io/crates/dirs) crate picks the folder
(`settings::config_path`). Presets live in the same file.

If there's no file yet but Muvijo (Undavi's old name) left one in
`muvijo/settings.json`, that one is read once; the next save writes the new
location.

To reset everything, quit Undavi and delete the file.

## Format

```json
{
  "version": 4,
  "sections": {
    "sound": {
      "settings": { "sensitivity": 72, "spread": 40, "...": "..." },
      "presets": [{ "name": "Quiet room", "settings": { "...": "..." } }],
      "selected": { "built_in": "Balanced" }
    },
    "visualizer": {
      "settings": {
        "shader": "bars",
        "values": { "bars": { "bar_count": 60, "mirror": "Both" }, "wave": { "bar_width": 4 } }
      },
      "presets": [{ "name": "Night", "settings": { "...": "..." } }],
      "selected": { "yours": 0 }
    },
    "colors": { "settings": { "palette": "undavi", "accent": "sky", "...": "..." }, "...": "..." },
    "background": {
      "settings": {
        "shader": "album-art",
        "values": { "album-art": { "blur": 40, "pulse": true } },
        "dim_pct": 70,
        "...": "..."
      },
      "...": "..."
    },
    "now_playing": { "settings": { "mode": "header", "...": "..." }, "...": "..." },
    "playback": { "settings": { "style": "card", "curve_height": 8, "...": "..." }, "...": "..." },
    "lyrics": { "settings": { "show": true, "...": "..." }, "...": "..." },
    "controls": {
      "settings": { "shortcuts": { "mute": ["m"], "next": ["ctrl+ArrowRight"] }, "...": "..." },
      "...": "..."
    },
    "general": { "settings": { "auto_hide": true, "...": "..." }, "...": "..." },
    "groups": [
      {
        "name": "Night drive",
        "presets": { "visualizer": { "yours": 0 }, "colors": { "built_in": "Ember" } }
      }
    ]
  },
  "source": { "kind": "app", "id": "spotify", "label": "Spotify" },
  "updates": { "check": true, "auto_install": false },
  "monitor": { "signal": true, "spectrum": true, "beats": true, "performance": true },
  "favorites": ["sound.sensitivity", "visualizer:glow"]
}
```

- Field names are the Rust field names; enum values are `snake_case`
  (`"album_art"`, `"mid_side"`). The API docs for `TuningSettings`,
  `VisualSettings`, `ColorSettings`, `BackgroundSettings`, `NowPlayingSettings`,
  `PlaybackSettings`, `LyricsSettings`, `ControlSettings` and `GeneralSettings` list every
  field with its range.
- Each of `sections` (but `groups`) is a section of the settings window: its current `settings`,
  the user's `presets`, and the preset in use (`selected`: a built-in by its name,
  or one of the user's by its place in the list). The built-in presets are in
  `src/settings/presets.rs`; the first one of a section is its defaults.
- `groups` are the user's preset groups: the preset of each section in it. They follow
  the user's presets as those are moved, and a preset that's deleted (or isn't there
  when the file is read) is taken out of them.
- In `playback`, what's `null` (or left out) is up to the bar's style:
  `PlaybackSettings::look` has what that comes to.
- `shortcuts` (in `controls`) has only the ones that were changed, by the action's name
  (`Action::id`): up to two keys each, a character or a key's name, after what's held
  down with it (`ctrl+`, `alt+`, `shift+`). No keys is no shortcut. Unknown actions and
  keys are skipped.
- `favorites` are the starred settings, in the order they were starred: a section and a
  field (`sound.sensitivity`; the first one, for a setting of two), a section and a
  shader's setting (`visualizer:glow`, for whichever visualizer has it), or an update
  switch (`updates.check`). Ones that don't exist are skipped.
- `source` is the audio source picked last (left out for System Audio). A device
  that's gone falls back to System Audio at startup; an app that isn't playing is
  waited for.
- `updates` holds the General section's update switches: `check` looks for a new
  release on startup, `auto_install` installs it without asking (see
  [packaging.md](packaging.md#updates)).
- The visualizer's and the background's `shader` name the visualizer and the
  background: a built-in (`bars`, `radial`, `wave`; `solid`, `album-art`,
  `ambient`) or a file in the `shaders` folder next to `settings.json`, without
  `.wgsl`. See [shaders.md](shaders.md).
- Their `values` hold each shader's own
  settings, by shader and then by setting id, as the shader declares them:
  numbers, `true`/`false`, a choice's label (`"Up/down"`), or a color as
  `"#rrggbb"`. Only changed settings are stored; anything missing or out of
  range is the shader's default. Values of a shader that isn't picked are kept,
  so switching back brings them back. A preset stores the values of the shader it
  picks, and loading it leaves the other shaders' alone.
- Everything else (the colors, and the background's Dim, color effects, vignette
  and grain) is the same for every shader.
- "Always on top" is never part of a preset: it's about the window, not the look.
- `monitor` holds which parts the signal monitor shows. Whether it's on isn't saved.
- Any field can be left out; it gets its default.

## Saving

Changes are written within a second (or at once when a preset is saved),
through a temporary file that's renamed into place, so a crash can't
leave a half-written file. Unit tests never save (`cfg!(test)`); integration
tests must not call `settings::save`, since they build the library without
`cfg(test)`.

## Reading, and what happens when it goes wrong

`settings::load` never fails:

- **Missing file:** the defaults.
- **Unreadable file** (not valid JSON, or wrong types): the defaults, and the
  file is copied to `settings.json.unreadable` first, so nothing is lost when the
  next save overwrites it.
- **Written by a newer Undavi** (`version` above `CONFIG_VERSION`): read as far as
  possible, and copied to `settings.json.v<N>` first, since saving drops fields
  this version doesn't know.
- **Out-of-range values** (a hand-edited file): pulled back into range by the
  `clamped` methods, or, for a shader's settings, when the shader reads them.
- **A shader that's gone or doesn't compile:** Bars (or Album art) stands in,
  and the pick and its values are kept for when the file is back.

## Versions and migrations

`version` is the file format, `settings::CONFIG_VERSION` the current one.

| Version | Undavi | Change |
|---|---|---|
| 0 | Muvijo, early Undavi | Files without a `version` field. Visuals presets also stored tuning. |
| 1 | 0.9 | Adds `version`. Old Visuals presets' tuning moves to Tuning presets. |
| 2 | 0.10 (never released on its own) | Visualizers and backgrounds become shaders with their own settings. The old Visuals fields (bar count, mirror, corners, glow, …) move into `values` for Bars, Radial and Wave (and a custom visualizer that was picked), the old background fields (layout, motion, blur, pulse, …) into `background_values` for Album art and Ambient, and the solid color into Solid's; `style` and `bg_mode` pick the shader. In the settings and in every preset. |
| 3 | 0.10 (never released on its own) | The Appearance panel splits into Background and Interface: `appearance` becomes `background` (`shader` and `values`) and `interface` (everything else), and each Appearance preset becomes a Background preset and an Interface preset of the same name (identical Interface presets are kept once). |
| 4 | 0.10 | One `sections` object, where each section of the settings window has its `settings`, `presets` and the preset in use (`selected`). Visuals splits into Visualizer and Colors (which also gets the accent); Interface into Now playing, Lyrics and General, with its effects joining Background. Presets split the same way and keep their names; parts that are only the defaults are left out, identical ones are kept once, and what a preset didn't store before is filled in from the current settings (an Appearance preset's background keeps its own effects). |

Most changes don't need a new version:

- **New field:** give it a default (every settings struct is `#[serde(default)]`).
- **Renamed field or value:** keep the old name as an alias, e.g.
  `#[serde(alias = "smoothing")] spread` or
  `#[serde(rename = "undavi", alias = "blue")] Undavi`.
- **Changed type:** a custom `deserialize_with` that accepts both, like
  `split_or_bool` for the old `stereo` switch.

Bump the version only when old data has to be *transformed*:

1. Increase `CONFIG_VERSION` and add a row to the table above and to its doc
   comment.
2. Add a step to `src/settings/versions.rs` that transforms the JSON of the version
   before, like `upgrade_to_v4`, and call it from `settings::upgrade`, which
   `settings::load` runs before reading the file into the structs.
3. Add a test in `tests/settings.rs` that loads a file of the old version.

A built-in shader's settings are part of the format too: renaming a setting id
or an option label loses what people saved under the old one, unless the
migration renames it.

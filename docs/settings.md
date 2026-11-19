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
  "version": 1,
  "tuning": { "sensitivity": 72, "spread": 40, "...": "..." },
  "visuals": { "bar_count": 43, "palette": "undavi", "...": "..." },
  "appearance": { "bg_mode": "album_art", "accent": "sky", "...": "..." },
  "presets": [{ "name": "Night", "visuals": { "...": "..." } }],
  "tuning_presets": [{ "name": "Quiet room", "tuning": { "...": "..." } }],
  "appearance_presets": [{ "name": "Movie night", "appearance": { "...": "..." } }],
  "source": { "kind": "app", "id": "spotify", "label": "Spotify" }
}
```

- Field names are the Rust field names; enum values are `snake_case`
  (`"album_art"`, `"mid_side"`). The API docs for `TuningSettings`,
  `VisualSettings` and `AppearanceSettings` list every field with its range.
- `presets` holds the Visuals presets; it kept its original name so older files
  still load.
- `source` is the audio source picked last (left out for System Audio). A device
  that's gone falls back to System Audio at startup; an app that isn't playing is
  waited for.
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
  `clamped` methods.

## Versions and migrations

`version` is the file format, `settings::CONFIG_VERSION` the current one.

| Version | Undavi | Change |
|---|---|---|
| 0 | Muvijo, early Undavi | Files without a `version` field. Visuals presets also stored tuning. |
| 1 | 0.9 | Adds `version`. Old Visuals presets' tuning moves to Tuning presets. |

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
2. Add a step to `settings::migrate`, guarded by `if s.version < N`.
3. Add a test in `tests/settings.rs` that loads a file of the old version.

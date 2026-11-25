//! Changes to the settings file's format: each step brings a file (as JSON) of
//! the version before it up to date.

use super::{Accent, ColorSettings, GeneralSettings, LyricsSettings, NowPlayingSettings};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

/// Every upgrade, oldest first: what [`super::load`] runs before reading a file
/// into [`super::Stored`] (then [`super::migrate`]).
pub fn upgrade(json: &mut Value) {
    upgrade_to_v1(json);
    upgrade_to_v2(json);
    upgrade_to_v3(json);
    upgrade_to_v4(json);
}

/// The fields named in `names`, under their new names.
fn part(from: &Map<String, Value>, names: &[(&str, &str)]) -> Map<String, Value> {
    names
        .iter()
        .filter_map(|(old, new)| Some(((*new).to_string(), from.get(*old)?.clone())))
        .collect()
}

fn version(json: &Value) -> u64 {
    json.get("version").and_then(Value::as_u64).unwrap_or(0)
}

/// Bring a settings file (as JSON) from before version 1 into the version 1
/// shape: Visuals presets used to carry tuning, which moves to Tuning presets of
/// the same name, so nothing saved is lost. Identical tunings are kept once.
/// (Field renames, like smoothing → spread, are handled by serde aliases.)
pub fn upgrade_to_v1(json: &mut Value) {
    if version(json) >= 1 {
        return;
    }
    let Some(root) = json.as_object_mut() else {
        return;
    };
    let mut tunings = match root.remove("tuning_presets") {
        Some(Value::Array(list)) => list,
        _ => Vec::new(),
    };
    if let Some(Value::Array(presets)) = root.get_mut("presets") {
        for p in presets.iter_mut().filter_map(Value::as_object_mut) {
            let Some(tuning) = p.remove("tuning").filter(|t| !t.is_null()) else {
                continue;
            };
            if tunings.iter().any(|t| t.get("tuning") == Some(&tuning)) {
                continue;
            }
            let mut moved = Map::new();
            moved.insert(
                "name".into(),
                p.get("name").cloned().unwrap_or_else(|| "Preset".into()),
            );
            moved.insert("tuning".into(), tuning);
            tunings.push(Value::Object(moved));
        }
    }
    if !tunings.is_empty() {
        root.insert("tuning_presets".into(), Value::Array(tunings));
    }
}

/// Bring a settings file (as JSON) from before version 2 into the version 2
/// shape: the Visuals and the background become a shader plus that shader's
/// settings. Every visual setting is carried over to Bars, Radial and Wave (and a
/// custom visualizer), the background's to Album art and Ambient (or Solid), in
/// the current settings and in every preset. Files already at version 2 are left
/// as they are.
pub fn upgrade_to_v2(json: &mut Value) {
    if version(json) >= 2 {
        return;
    }
    // Old snake_case enum values become the built-in shaders' labels.
    fn label(v: Option<Value>, table: &[(&str, &str)]) -> Option<Value> {
        let s = v?.as_str()?.to_string();
        table
            .iter()
            .find(|(old, _)| *old == s)
            .map(|(_, new)| Value::from(*new))
    }
    fn take(obj: &mut Map<String, Value>, key: &str) -> Option<Value> {
        obj.remove(key)
    }

    fn visuals(obj: &mut Map<String, Value>) {
        if obj.contains_key("shader") {
            return;
        }
        let mut shared = Map::new();
        for key in [
            "bar_count",
            "bar_width",
            "max_length",
            "spacing",
            "segment_size",
            "segment_gap",
            "segments",
            "peak_caps",
            "glow",
            "glow_amount",
        ] {
            if let Some(v) = take(obj, key) {
                shared.insert(key.into(), v);
            }
        }
        let corners = label(
            take(obj, "corners"),
            &[
                ("square", "Square"),
                ("rounded", "Rounded"),
                ("pill", "Pill"),
            ],
        );
        let mirror = label(
            take(obj, "mirror"),
            &[
                ("off", "Off"),
                ("up_down", "Up/down"),
                ("left_right", "Left/right"),
                ("both", "Both"),
            ],
        );
        // `split`, or the `stereo` switch older files had in its place.
        let split = match take(obj, "split").or_else(|| take(obj, "stereo")) {
            Some(Value::Bool(b)) => Some(Value::from(if b { "Stereo" } else { "Off" })),
            v => label(
                v,
                &[
                    ("off", "Off"),
                    ("stereo", "Stereo"),
                    ("mid_side", "Mid/Side"),
                ],
            ),
        };
        let channel = label(
            take(obj, "channel"),
            &[("mono", "Mono"), ("left", "Left"), ("right", "Right")],
        );
        let source = label(
            take(obj, "glow_source"),
            &[("bass", "Bass"), ("drums", "Drums")],
        );
        for (key, v) in [
            ("corners", corners),
            ("mirror", mirror),
            ("split", split),
            ("channel", channel),
            ("glow_source", source),
        ] {
            if let Some(v) = v {
                shared.insert(key.into(), v);
            }
        }
        let custom = take(obj, "custom_shader").and_then(|v| v.as_str().map(str::to_string));
        let shader = match take(obj, "style").as_ref().and_then(Value::as_str) {
            Some("radial") => "radial".to_string(),
            Some("wave") => "wave".to_string(),
            Some("custom") => custom
                .clone()
                .filter(|c| !c.is_empty())
                .unwrap_or("bars".into()),
            _ => "bars".to_string(),
        };
        let mut values = Map::new();
        let mut names = vec!["bars".to_string(), "radial".into(), "wave".into()];
        names.extend(custom.filter(|c| !c.is_empty()));
        for name in names {
            values.insert(name, Value::Object(shared.clone()));
        }
        obj.insert("shader".into(), shader.into());
        obj.insert("values".into(), Value::Object(values));
    }

    fn appearance(obj: &mut Map<String, Value>) {
        if obj.contains_key("background") {
            return;
        }
        let mode = take(obj, "bg_mode");
        let mode = mode.as_ref().and_then(Value::as_str).unwrap_or("album_art");
        let solid = take(obj, "solid_color");
        let solid = match solid.as_ref().and_then(Value::as_str) {
            Some("black") => "#000000",
            Some("midnight") => "#0f1424",
            Some("plum") => "#1c1322",
            _ => "#141416",
        };
        let mut moving = Map::new();
        let motion = label(
            take(obj, "motion"),
            &[
                ("still", "Still"),
                ("drift", "Drift"),
                ("rotate", "Rotate"),
                ("spiral", "Rotate"),
            ],
        );
        let animated = take(obj, "bg_animated");
        let custom = take(obj, "custom_shader").and_then(|v| v.as_str().map(str::to_string));
        let animation = take(obj, "animation");
        let animation = match animation.as_ref().and_then(Value::as_str) {
            Some("custom") => custom.clone().map(Value::from),
            _ => label(
                animation,
                &[("liquid", "Liquid"), ("warp", "Warp"), ("spiral", "Spiral")],
            ),
        };
        let pulse_source = label(
            take(obj, "bg_pulse_source"),
            &[("bass", "Bass"), ("drums", "Drums")],
        );
        let pulse_style = label(
            take(obj, "bg_pulse_style"),
            &[
                ("swell", "Swell"),
                ("brighten", "Brighten"),
                ("zoom", "Zoom"),
                ("focus", "Focus"),
            ],
        );
        for (key, v) in [
            ("motion", motion),
            ("animated", animated),
            ("animation", animation.clone()),
            ("speed", take(obj, "motion_speed")),
            ("pulse", take(obj, "bg_pulse")),
            ("pulse_source", pulse_source),
            ("pulse_amount", take(obj, "bg_pulse_amount")),
            ("pulse_style", pulse_style),
        ] {
            if let Some(v) = v {
                moving.insert(key.into(), v);
            }
        }
        let mut album = moving.clone();
        let layout = label(
            take(obj, "art_layout"),
            &[
                ("copies", "Copies"),
                ("single", "Single"),
                ("tiles", "Tiles"),
            ],
        );
        for (key, v) in [
            ("arrangement", layout),
            ("zoom", take(obj, "art_zoom")),
            ("blur", take(obj, "blur_px")),
        ] {
            if let Some(v) = v {
                album.insert(key.into(), v);
            }
        }
        let mut solid_values = Map::new();
        solid_values.insert("color".into(), solid.into());
        let mut values = Map::new();
        values.insert("solid".into(), Value::Object(solid_values));
        values.insert("album-art".into(), Value::Object(album));
        values.insert("ambient".into(), Value::Object(moving));

        let mut background = match mode {
            "solid" => "solid",
            "ambient" => "ambient",
            _ => "album-art",
        }
        .to_string();
        // A custom shader picked as the animation: a scene (rather than a warp)
        // becomes the background itself.
        let is_custom = animation.as_ref().and_then(Value::as_str) == custom.as_deref();
        if let (true, Some(name)) = (is_custom && background != "solid", &custom) {
            let file = crate::shaders::folder().map(|d| d.join(format!("{name}.wgsl")));
            let code = file.and_then(|f| std::fs::read_to_string(f).ok());
            if code.is_some_and(|c| crate::shaders::kind_of(&c) == Ok(crate::shaders::Kind::Scene))
            {
                background = name.clone();
            }
        }
        obj.insert("background".into(), background.into());
        obj.insert("background_values".into(), Value::Object(values));
    }

    let root = match json.as_object_mut() {
        Some(root) => root,
        None => return,
    };
    if let Some(Value::Object(v)) = root.get_mut("visuals") {
        visuals(v);
    }
    if let Some(Value::Object(a)) = root.get_mut("appearance") {
        appearance(a);
    }
    if let Some(Value::Array(presets)) = root.get_mut("presets") {
        for p in presets {
            if let Some(Value::Object(v)) = p.get_mut("visuals") {
                visuals(v);
            }
        }
    }
    if let Some(Value::Array(presets)) = root.get_mut("appearance_presets") {
        for p in presets {
            if let Some(Value::Object(a)) = p.get_mut("appearance") {
                appearance(a);
            }
        }
    }
}

/// Bring a settings file (as JSON) from before version 3 into the version 3
/// shape: the Appearance settings split into `background` (the shader and its
/// values, as `shader` and `values`) and `interface` (everything else). Each
/// Appearance preset becomes a Background preset and an Interface preset of the
/// same name; identical Interface presets are kept once. Run after
/// [`upgrade_to_v2`]; files already at version 3 are left as they are.
pub fn upgrade_to_v3(json: &mut Value) {
    if version(json) >= 3 {
        return;
    }
    let Some(root) = json.as_object_mut() else {
        return;
    };
    /// Split one Appearance object into (background, interface).
    fn split(mut appearance: Map<String, Value>) -> (Value, Value) {
        let mut background = Map::new();
        if let Some(v) = appearance.remove("background") {
            background.insert("shader".into(), v);
        }
        if let Some(v) = appearance.remove("background_values") {
            background.insert("values".into(), v);
        }
        (Value::Object(background), Value::Object(appearance))
    }
    if let Some(Value::Object(a)) = root.remove("appearance") {
        let (background, interface) = split(a);
        root.insert("background".into(), background);
        root.insert("interface".into(), interface);
    }
    if let Some(Value::Array(presets)) = root.remove("appearance_presets") {
        let mut backgrounds = Vec::new();
        let mut interfaces: Vec<Value> = Vec::new();
        for p in presets {
            let name = p.get("name").cloned().unwrap_or_else(|| "Preset".into());
            let Some(Value::Object(a)) = p.get("appearance").cloned() else {
                continue;
            };
            let (background, interface) = split(a);
            let mut b = Map::new();
            b.insert("name".into(), name.clone());
            b.insert("background".into(), background);
            // Its effects too: version 4 puts them back with the background.
            if let Value::Object(i) = &interface {
                b.insert("effects".into(), Value::Object(part(i, &EFFECTS)));
            }
            backgrounds.push(Value::Object(b));
            if !interfaces
                .iter()
                .any(|i| i.get("interface") == Some(&interface))
            {
                let mut i = Map::new();
                i.insert("name".into(), name);
                i.insert("interface".into(), interface);
                interfaces.push(Value::Object(i));
            }
        }
        root.insert("background_presets".into(), Value::Array(backgrounds));
        root.insert("interface_presets".into(), Value::Array(interfaces));
    }
}

/// The Background settings' effects, by their name in version 3's `interface`.
const EFFECTS: [(&str, &str); 7] = [
    ("dim_pct", "dim_pct"),
    ("bg_saturation", "saturation"),
    ("bg_brightness", "brightness"),
    ("bg_contrast", "contrast"),
    ("bg_tint_pct", "tint_pct"),
    ("vignette_pct", "vignette_pct"),
    ("grain_pct", "grain_pct"),
];
const NOW_PLAYING: [(&str, &str); 2] = [
    ("now_playing", "mode"),
    ("show_album_art", "show_album_art"),
];
const LYRICS: [(&str, &str); 5] = [
    ("lyrics", "show"),
    ("lyrics_size", "size"),
    ("lyrics_offset_ms", "offset_ms"),
    ("lyrics_glow", "glow"),
    ("lyrics_word_sync", "word_sync"),
];
const GENERAL: [(&str, &str); 5] = [
    ("auto_hide", "auto_hide"),
    ("hide_after_s", "hide_after_s"),
    ("show_on_move_only", "show_on_move_only"),
    ("always_on_top", "always_on_top"),
    ("frame_rate", "frame_rate"),
];
const COLORS: [&str; 4] = [
    "palette",
    "album_gradient",
    "album_match_accent",
    "gradient",
];

/// Bring a settings file (as JSON) from before version 4 into the version 4
/// shape: one `sections` object, where each section has its `settings` and its
/// `presets`. The Visuals settings split into Visualizer and Colors (which also
/// gets the accent), and Interface into Now playing, Lyrics and General, with its
/// effects joining Background.
///
/// Presets split the same way and keep their names. A part that's no different
/// from the defaults isn't kept, and identical ones are kept once. An Appearance
/// preset's background keeps the effects it was saved with; other parts of a preset
/// that its new section didn't have before (the accent, say) are filled in from the
/// current settings. Run after [`upgrade_to_v3`]; files already at
/// version 4 are left as they are.
pub fn upgrade_to_v4(json: &mut Value) {
    if version(json) >= 4 {
        return;
    }
    let Some(root) = json.as_object_mut() else {
        return;
    };

    fn object(v: Option<Value>) -> Map<String, Value> {
        match v {
            Some(Value::Object(o)) => o,
            _ => Map::new(),
        }
    }
    fn colors(visuals: &Map<String, Value>, accent: Option<&Value>) -> Map<String, Value> {
        let mut out: Map<String, Value> = COLORS
            .iter()
            .filter_map(|k| Some(((*k).to_string(), visuals.get(*k)?.clone())))
            .collect();
        if let Some(a) = accent {
            out.insert("accent".into(), a.clone());
        }
        out
    }
    /// A shader and the values of every shader.
    fn shader(from: &Map<String, Value>) -> Map<String, Value> {
        part(from, &[("shader", "shader"), ("values", "values")])
    }
    /// The same, with only the picked shader's values (what a preset stores).
    fn shader_preset(from: &Map<String, Value>, default: &str) -> Map<String, Value> {
        let mut out = shader(from);
        let name = out
            .get("shader")
            .and_then(Value::as_str)
            .unwrap_or(default)
            .to_string();
        if let Some(Value::Object(values)) = out.get_mut("values") {
            values.retain(|k, _| *k == name);
        }
        out
    }
    fn is_default<T: DeserializeOwned + Default + PartialEq>(v: &Map<String, Value>) -> bool {
        serde_json::from_value::<T>(Value::Object(v.clone())).is_ok_and(|t| t == T::default())
    }
    /// The presets of one old list, as (name, settings).
    fn presets(
        root: &mut Map<String, Value>,
        list: &str,
        key: &str,
    ) -> Vec<(Value, Map<String, Value>)> {
        let Some(Value::Array(list)) = root.remove(list) else {
            return Vec::new();
        };
        list.into_iter()
            .map(|p| {
                let mut p = object(Some(p));
                let name = p.remove("name").unwrap_or_else(|| "Preset".into());
                (name, object(p.remove(key)))
            })
            .collect()
    }
    fn push(list: &mut Vec<Value>, name: &Value, settings: Map<String, Value>) {
        let settings = Value::Object(settings);
        if list.iter().any(|p| p.get("settings") == Some(&settings)) {
            return;
        }
        let mut p = Map::new();
        p.insert("name".into(), name.clone());
        p.insert("settings".into(), settings);
        list.push(Value::Object(p));
    }

    let tuning = object(root.remove("tuning"));
    let visuals = object(root.remove("visuals"));
    let background = object(root.remove("background"));
    let interface = object(root.remove("interface"));
    let accent = interface.get("accent");
    let effects = part(&interface, &EFFECTS);

    // Lists of presets: sound, visualizer, colors, background, now playing,
    // lyrics, general.
    let mut out: [Vec<Value>; 7] = Default::default();
    for (name, t) in presets(root, "tuning_presets", "tuning") {
        push(&mut out[0], &name, t);
    }
    for (name, v) in presets(root, "presets", "visuals") {
        push(&mut out[1], &name, shader_preset(&v, "bars"));
        let c = colors(&v, None);
        if !is_default::<ColorSettings>(&c) {
            push(&mut out[2], &name, colors(&v, accent));
        }
    }
    // Presets from an Appearance panel (version 2 and before) come with the effects
    // they were saved with; version 3's didn't have any.
    let mut with_effects = false;
    if let Some(Value::Array(list)) = root.remove("background_presets") {
        for p in list {
            let mut p = object(Some(p));
            let name = p.remove("name").unwrap_or_else(|| "Preset".into());
            let own = match p.remove("effects") {
                Some(Value::Object(fx)) => Some(fx),
                _ => None,
            };
            with_effects |= own.is_some();
            let mut b = shader_preset(&object(p.remove("background")), "album-art");
            b.extend(own.unwrap_or_else(|| effects.clone()));
            push(&mut out[3], &name, b);
        }
    }
    for (name, i) in presets(root, "interface_presets", "interface") {
        let fx = part(&i, &EFFECTS);
        if !with_effects && !is_default::<super::BackgroundSettings>(&fx) {
            let mut b = shader_preset(&background, "album-art");
            b.extend(fx);
            push(&mut out[3], &name, b);
        }
        let is_sky = |a: &Value| {
            serde_json::from_value::<Accent>(a.clone()).ok()
                == Some(ColorSettings::default().accent)
        };
        if let Some(a) = i.get("accent").filter(|a| !is_sky(a)) {
            push(&mut out[2], &name, colors(&visuals, Some(a)));
        }
        let np = part(&i, &NOW_PLAYING);
        if !is_default::<NowPlayingSettings>(&np) {
            push(&mut out[4], &name, np);
        }
        let l = part(&i, &LYRICS);
        if !is_default::<LyricsSettings>(&l) {
            push(&mut out[5], &name, l);
        }
        let mut g = part(&i, &GENERAL);
        g.remove("always_on_top");
        if !is_default::<GeneralSettings>(&g) {
            push(&mut out[6], &name, g);
        }
    }

    let mut background = shader(&background);
    background.extend(effects);
    let settings = [
        tuning,
        shader(&visuals),
        colors(&visuals, accent),
        background,
        part(&interface, &NOW_PLAYING),
        part(&interface, &LYRICS),
        part(&interface, &GENERAL),
    ];
    let names = [
        "sound",
        "visualizer",
        "colors",
        "background",
        "now_playing",
        "lyrics",
        "general",
    ];
    let sections: Map<String, Value> = names
        .into_iter()
        .zip(settings.into_iter().zip(out))
        .map(|(name, (settings, presets))| {
            let mut s = Map::new();
            s.insert("settings".into(), Value::Object(settings));
            s.insert("presets".into(), Value::Array(presets));
            (name.to_string(), Value::Object(s))
        })
        .collect();
    root.insert("sections".into(), Value::Object(sections));
}

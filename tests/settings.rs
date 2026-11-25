//! The settings model and file format, through the public `undavi::settings` API.
//!
//! These never call `settings::save`: integration tests build the library without
//! `cfg(test)`, so it would write the real settings file.

use undavi::settings::*;

/// Load a settings file the way the app does: upgraded to the current shape
/// first, then migrated.
fn load_json(json: &str) -> Stored {
    let mut value: serde_json::Value = serde_json::from_str(json).unwrap();
    upgrade(&mut value);
    migrate(serde_json::from_value(value).unwrap())
}

/// One saved setting of a shader.
fn value<'a>(
    values: &'a std::collections::BTreeMap<String, ShaderValues>,
    shader: &str,
    id: &str,
) -> Option<&'a serde_json::Value> {
    values.get(shader)?.get(id)
}

/// The names of a section's presets.
fn names<T>(section: &Section<T>) -> String {
    let names: Vec<&str> = section.presets.iter().map(|p| p.name.as_str()).collect();
    names.join(",")
}

#[test]
fn appearance_splits_into_sections() {
    // A version 2 file: one Appearance object, and Appearance presets.
    let s = load_json(
        r##"{"version": 2,
            "appearance": {"background": "solid", "background_values": {"solid": {"color": "#000000"}},
                           "dim_pct": 40, "accent": "rose", "lyrics_size": 120},
            "appearance_presets": [
                {"name": "Night", "appearance": {"background": "ambient", "dim_pct": 80}},
                {"name": "Night copy", "appearance": {"background": "album-art", "dim_pct": 80}},
                {"name": "Day", "appearance": {"dim_pct": 10, "lyrics_size": 150}}
            ]}"##,
    );
    assert_eq!(s.version, CONFIG_VERSION);
    let sections = &s.sections;
    assert_eq!(sections.background.shader, "solid");
    assert_eq!(
        value(&sections.background.values, "solid", "color"),
        Some(&"#000000".into())
    );
    assert_eq!(sections.background.dim_pct, 40);
    assert_eq!(sections.colors.accent, Accent::Rose);
    assert_eq!(sections.lyrics.size, 120);
    // Every preset keeps its background together with its effects; what's only the
    // default isn't kept elsewhere.
    let backgrounds: Vec<(&str, &str, u8)> = sections
        .background
        .presets
        .iter()
        .map(|p| {
            (
                p.name.as_str(),
                p.settings.shader.as_str(),
                p.settings.dim_pct,
            )
        })
        .collect();
    assert_eq!(
        backgrounds,
        [
            ("Night", "ambient", 80),
            ("Night copy", "album-art", 80),
            ("Day", "album-art", 10),
        ]
    );
    assert_eq!(names(&sections.lyrics), "Day");
    assert_eq!(sections.lyrics.presets[0].settings.size, 150);
    assert_eq!(names(&sections.general), "");
    let saved = serde_json::to_string(&s).unwrap();
    for gone in ["appearance", "interface", "tuning_presets"] {
        assert!(!saved.contains(gone), "{gone}: {saved}");
    }
}

#[test]
fn panels_become_sections() {
    // A version 3 file, with each panel's settings and presets.
    let s = load_json(
        r#"{"version": 3,
            "tuning": {"sensitivity": 30},
            "visuals": {"shader": "wave", "values": {"wave": {"bar_width": 4}},
                        "palette": "ember", "gradient": "along"},
            "background": {"shader": "ambient", "values": {"ambient": {"blur": 20}}},
            "interface": {"dim_pct": 70, "bg_saturation": 60, "bg_tint_pct": 25, "accent": "mint",
                          "now_playing": "spotlight", "show_album_art": false,
                          "lyrics": false, "lyrics_glow": 10, "lyrics_offset_ms": -250,
                          "auto_hide": false, "always_on_top": true, "frame_rate": "30"},
            "presets": [{"name": "Night", "visuals": {"shader": "bars",
                "values": {"bars": {"bar_count": 20}, "wave": {"bar_width": 9}}, "palette": "aurora"}},
                {"name": "Plain", "visuals": {"shader": "radial"}}],
            "tuning_presets": [{"name": "Quiet", "tuning": {"sensitivity": 90}}],
            "background_presets": [{"name": "Calm", "background": {"shader": "solid"}}],
            "interface_presets": [
                {"name": "Movie", "interface": {"dim_pct": 85, "lyrics_size": 140, "accent": "rose",
                                                "now_playing": "minimal", "always_on_top": true}},
                {"name": "Same look", "interface": {"now_playing": "minimal"}}],
            "updates": {"check": false}}"#,
    );
    let sections = &s.sections;
    assert_eq!(sections.sound.sensitivity, 30);
    assert_eq!(sections.visualizer.shader, "wave");
    assert_eq!(
        value(&sections.visualizer.values, "wave", "bar_width"),
        Some(&4.into())
    );
    assert_eq!(sections.colors.palette, Palette::Ember);
    assert_eq!(sections.colors.gradient, GradientDir::Along);
    assert_eq!(sections.colors.accent, Accent::Mint);
    let b = &sections.background;
    assert_eq!(b.shader, "ambient");
    assert_eq!((b.dim_pct, b.saturation, b.tint_pct), (70, 60, 25));
    assert_eq!(b.contrast, 100, "left out: the default");
    assert_eq!(sections.now_playing.mode, NowPlayingMode::Spotlight);
    assert!(!sections.now_playing.show_album_art);
    let l = &sections.lyrics;
    assert!(!l.show && l.word_sync);
    assert_eq!((l.glow, l.offset_ms), (10, -250));
    let g = &sections.general;
    assert!(!g.auto_hide && g.always_on_top);
    assert_eq!(g.frame_rate, FrameRate::Fps30);
    assert!(!s.updates.check, "the rest of the file is kept");

    // Presets split the same way.
    assert_eq!(names(&sections.sound), "Quiet");
    assert_eq!(names(&sections.visualizer), "Night,Plain");
    let night = &sections.visualizer.presets[0].settings;
    assert_eq!(value(&night.values, "bars", "bar_count"), Some(&20.into()));
    assert!(
        !night.values.contains_key("wave"),
        "only the shader it's for"
    );
    // Colors: Night's palette with the current accent, and Movie's accent with the
    // current palette. Plain had the default colors.
    assert_eq!(names(&sections.colors), "Night,Movie");
    let colors = &sections.colors.presets;
    assert_eq!(colors[0].settings.palette, Palette::Aurora);
    assert_eq!(colors[0].settings.accent, Accent::Mint);
    assert_eq!(colors[1].settings.palette, Palette::Ember);
    assert_eq!(colors[1].settings.accent, Accent::Rose);
    // Backgrounds: Calm's with the current effects, and Movie's effects with the
    // current background.
    assert_eq!(names(&sections.background), "Calm,Movie");
    let backgrounds = &sections.background.presets;
    assert_eq!(backgrounds[0].settings.shader, "solid");
    assert_eq!(backgrounds[0].settings.dim_pct, 70);
    assert_eq!(backgrounds[1].settings.shader, "ambient");
    assert_eq!(backgrounds[1].settings.dim_pct, 85);
    assert_eq!(
        names(&sections.now_playing),
        "Movie",
        "identical ones are kept once"
    );
    assert_eq!(names(&sections.lyrics), "Movie");
    assert_eq!(
        names(&sections.general),
        "",
        "always on top was never part of a preset"
    );

    // Saved and loaded again, nothing changes.
    let saved = serde_json::to_string(&s).unwrap();
    assert_eq!(load_json(&saved).sections, s.sections);
    // A fresh file is the same as no file.
    assert_eq!(load_json("{}").sections, Sections::default());
}

#[test]
fn old_background_settings_move_to_the_shaders() {
    // The old Spiral motion is Rotate now, for both pictures; the old Blur was
    // only ever Album art's (Ambient keeps its own).
    let s = load_json(r#"{"appearance": {"motion": "spiral", "blur_px": 30, "dim_pct": 50}}"#);
    let a = &s.sections.background;
    assert_eq!(a.shader, "album-art");
    assert_eq!(a.dim_pct, 50, "the Dim stays global");
    for shader in ["album-art", "ambient"] {
        let v = &a.values;
        assert_eq!(
            value(v, shader, "motion"),
            Some(&"Rotate".into()),
            "{shader}"
        );
    }
    assert_eq!(value(&a.values, "album-art", "blur"), Some(&30.into()));
    assert_eq!(value(&a.values, "ambient", "blur"), None);
    let saved = serde_json::to_string(&s).unwrap();
    assert!(
        !saved.contains("blur_px") && !saved.contains("spiral"),
        "{saved}"
    );

    let s = load_json(r#"{"appearance": {"bg_mode": "solid", "solid_color": "midnight"}}"#);
    assert_eq!(s.sections.background.shader, "solid");
    assert_eq!(
        value(&s.sections.background.values, "solid", "color"),
        Some(&"#0f1424".into())
    );
}

#[test]
fn old_blue_palette_loads_as_undavi() {
    let c: ColorSettings = serde_json::from_str(r#"{"palette": "blue"}"#).unwrap();
    assert_eq!(c.palette, Palette::Undavi);
    let saved = serde_json::to_string(&c).unwrap();
    assert!(saved.contains(r#""palette":"undavi""#));
    assert_eq!(ColorSettings::default().palette, Palette::Undavi);
}

#[test]
fn files_carry_a_version_and_old_ones_migrate() {
    // A file from before versioning: presets with tuning, an out-of-range value.
    let json = r#"{"tuning": {"sensitivity": 250},
        "presets": [{"name": "Night", "visuals": {}, "tuning": {"sensitivity": 90}}]}"#;
    let s = load_json(json);
    assert_eq!(s.version, CONFIG_VERSION);
    assert_eq!(s.sections.sound.sensitivity, 100);
    assert_eq!(names(&s.sections.sound), "Night");
    let saved = serde_json::to_string(&s).unwrap();
    assert!(
        saved.starts_with(&format!(r#"{{"version":{CONFIG_VERSION},"#)),
        "{saved}"
    );
    assert_eq!(Stored::default().version, CONFIG_VERSION);
}

#[test]
fn settings_live_in_the_platform_config_folder() {
    if let (Some(path), Some(base)) = (config_path(), dirs::config_dir()) {
        assert_eq!(path, base.join("undavi").join("settings.json"));
    }
}

#[test]
fn renamed_fields_still_load() {
    // "smoothing" became "spread"; the stereo switch became the split choice
    // (now a setting of the visualizers).
    let t: TuningSettings = serde_json::from_str(r#"{"smoothing": 12}"#).unwrap();
    assert_eq!(t.spread, 12);
    let split = |json: &str| {
        let s = load_json(json);
        value(&s.sections.visualizer.values, "bars", "split").cloned()
    };
    assert_eq!(
        split(r#"{"visuals": {"stereo": true}}"#),
        Some("Stereo".into())
    );
    assert_eq!(
        split(r#"{"visuals": {"stereo": false}}"#),
        Some("Off".into())
    );
    assert_eq!(
        split(r#"{"visuals": {"split": "mid_side"}}"#),
        Some("Mid/Side".into())
    );
    let saved = serde_json::to_string(&load_json(r#"{"visuals": {"stereo": true}}"#)).unwrap();
    assert!(!saved.contains("stereo"), "{saved}");
    // The General section was called Window at first.
    let s = load_json(
        r#"{"version": 4, "sections": {"window": {"settings": {"auto_hide": false},
            "presets": [{"name": "Kiosk", "settings": {"hide_after_s": 1}}],
            "selected": {"yours": 0}}}}"#,
    );
    let general = &s.sections.general;
    assert!(!general.auto_hide);
    assert_eq!(names(general), "Kiosk");
    assert_eq!(general.selected, Some(PresetId::Yours(0)));
    let saved = serde_json::to_string(&s).unwrap();
    assert!(
        saved.contains(r#""general":"#) && !saved.contains(r#""window":"#),
        "{saved}"
    );
    let g: GeneralSettings = serde_json::from_str(r#"{"frame_rate": "30"}"#).unwrap();
    assert_eq!(g.frame_rate, FrameRate::Fps30);
}

#[test]
fn frequency_scales_cover_the_range() {
    for scale in [FreqScale::Log, FreqScale::Mel, FreqScale::Linear] {
        let edges: Vec<f32> = scale.edges(30.0, 16_000.0, 40).collect();
        assert_eq!(edges.len(), 41);
        assert!((edges[0] - 30.0).abs() < 0.01, "{scale:?} {}", edges[0]);
        assert!(
            (edges[40] - 16_000.0).abs() < 1.0,
            "{scale:?} {}",
            edges[40]
        );
        assert!(edges.windows(2).all(|w| w[1] > w[0]), "{scale:?}");
    }
    // Mel gives the low end more room than linear, and the top less than log.
    let first = |s: FreqScale| s.edges(30.0, 16_000.0, 40).nth(1).unwrap();
    let last = |s: FreqScale| s.edges(30.0, 16_000.0, 40).nth(39).unwrap();
    assert!(first(FreqScale::Log) < first(FreqScale::Mel));
    assert!(first(FreqScale::Mel) < first(FreqScale::Linear));
    assert!(last(FreqScale::Log) < last(FreqScale::Mel));
}

#[test]
fn old_presets_move_their_tuning_out() {
    let json = r#"{"presets": [
        {"name": "Night", "visuals": {}, "tuning": {"sensitivity": 90}},
        {"name": "Night copy", "visuals": {}, "tuning": {"sensitivity": 90}},
        {"name": "Party", "visuals": {}, "tuning": {"sensitivity": 40}},
        {"name": "Look only", "visuals": {}}
    ]}"#;
    let s = load_json(json);
    assert_eq!(
        names(&s.sections.sound),
        "Night,Party",
        "identical tunings are kept once"
    );
    assert_eq!(s.sections.sound.presets[1].settings.sensitivity, 40);
    // Their looks were all the same one.
    assert_eq!(names(&s.sections.visualizer), "Night");
    // Saved files no longer carry the old field.
    let out = serde_json::to_string(&s).unwrap();
    assert!(!out.contains("tuning"), "{out}");
}

#[test]
fn default_labels() {
    let t = TuningSettings::default();
    assert_eq!(format_hz(slider_to_hz(t.freq_lo as f32)), "30 Hz");
    assert_eq!(format_hz(slider_to_hz(t.freq_hi as f32)), "16 kHz");
    assert_eq!(format_db(t.bass_db), "+4 dB");
    assert_eq!(format_db(t.mids_db), "0 dB");
    assert_eq!(format_db(t.highs_db), "\u{2212}2 dB");
}

#[test]
fn accent_icon_contrast() {
    // Blue and rose get white icons; mint and white get dark ones.
    assert!(!wants_dark_icon(Accent::Blue.rgb().unwrap()));
    assert!(!wants_dark_icon(Accent::Rose.rgb().unwrap()));
    assert!(wants_dark_icon(Accent::Mint.rgb().unwrap()));
    assert!(wants_dark_icon(Accent::White.rgb().unwrap()));
    assert!(wants_dark_icon([0xF2, 0xA6, 0x7F])); // mockup album accent
}

#[test]
fn contrast_curve() {
    assert!((contrast_exponent(50) - 1.0).abs() < 1e-6);
    assert!((contrast_exponent(0) - 0.5).abs() < 1e-6);
    assert!((contrast_exponent(100) - 2.0).abs() < 1e-6);
}

#[test]
fn level_range_clamps() {
    let t = TuningSettings {
        floor_db: -10,
        ceiling_db: -5,
        ..Default::default()
    }
    .clamped();
    assert!(t.ceiling_db - t.floor_db >= LEVEL_MIN_SPAN);
}

#[test]
fn old_settings_files_still_load() {
    // A file saved before the new fields existed gets their defaults.
    let t: TuningSettings = serde_json::from_str(r#"{"sensitivity": 60}"#).unwrap();
    assert_eq!(t.sensitivity, 60);
    assert_eq!(t.floor_db, -80);
    assert_eq!(t.detail, Detail::Balanced);
}

#[test]
fn hz_formatting() {
    assert_eq!(format_hz(440.0), "440 Hz");
    assert_eq!(format_hz(2000.0), "2 kHz");
    assert_eq!(format_hz(2520.0), "2.5 kHz");
    assert_eq!(format_hz(12_600.0), "13 kHz");
}

#[test]
fn clamp_keeps_range_gap() {
    let t = TuningSettings {
        freq_lo: 95,
        freq_hi: 97,
        ..Default::default()
    }
    .clamped();
    assert!(t.freq_hi - t.freq_lo >= FREQ_MIN_GAP);
}

#[test]
fn old_visual_settings_move_to_the_shaders() {
    // Every visualizer gets the old settings (its shader clamps them; see
    // shaders::settings), and the old style picks the shader.
    let s = load_json(
        r#"{"visuals": {"style": "wave", "max_length": 5000, "mirror": "both", "palette": "aurora"},
            "presets": [{"name": "Club", "visuals": {"segments": true, "corners": "pill"}}]}"#,
    );
    let visualizer = &s.sections.visualizer;
    assert_eq!(visualizer.shader, "wave");
    assert_eq!(
        s.sections.colors.palette,
        Palette::Aurora,
        "the colors stay global"
    );
    for shader in ["bars", "radial", "wave"] {
        let v = &visualizer.values;
        assert_eq!(
            value(v, shader, "max_length"),
            Some(&5000.into()),
            "{shader}"
        );
        assert_eq!(value(v, shader, "mirror"), Some(&"Both".into()), "{shader}");
    }
    // Presets come along too.
    let club = &visualizer.presets[0].settings;
    assert_eq!(club.shader, "bars");
    assert_eq!(value(&club.values, "bars", "corners"), Some(&"Pill".into()));
    assert_eq!(value(&club.values, "bars", "segments"), Some(&true.into()));
    // A file that's already new is left alone.
    let saved = serde_json::to_string(&s).unwrap();
    let again = load_json(&saved);
    assert_eq!(again.sections.visualizer, *visualizer);
    assert_eq!(VisualSettings::default().shader, "bars");
}

#[test]
fn the_picked_source_is_remembered() {
    use undavi::audio::{Source, SourceKind};
    // Files without one start on System Audio.
    let old: Stored = serde_json::from_str("{}").unwrap();
    assert_eq!(old.source, None);

    let s = Stored {
        source: Some(Source {
            id: "spotify".into(),
            label: "Spotify".into(),
            kind: SourceKind::App,
        }),
        ..Stored::default()
    };
    let json = serde_json::to_string(&s).unwrap();
    assert!(json.contains(r#""kind":"app""#), "{json}");
    let back: Stored = serde_json::from_str(&json).unwrap();
    assert_eq!(back.source, s.source);
}

#[test]
fn preset_groups_are_saved_and_lose_what_is_gone() {
    let mut s = Stored::default();
    s.sections.colors.save_new();
    s.sections.new_group(&[SectionId::Colors, SectionId::Sound]);
    let json = serde_json::to_string(&s).unwrap();
    assert!(
        json.contains(r#""presets":{"sound":{"built_in":"Balanced"},"colors":{"yours":0}}"#),
        "{json}"
    );
    assert_eq!(load_json(&json).sections.groups, s.sections.groups);

    // Presets that aren't there any more (the file was edited by hand).
    let s = load_json(
        r#"{"version": 4, "sections": {"groups": [{"name": "Night", "presets": {
            "sound": {"built_in": "Punchy"}, "colors": {"yours": 3},
            "lyrics": {"built_in": "No such preset"}}}]}}"#,
    );
    let night = &s.sections.groups[0];
    assert_eq!(night.name, "Night");
    assert_eq!(
        night.presets.keys().copied().collect::<Vec<_>>(),
        [SectionId::Sound]
    );
}

#[test]
fn the_playback_bar_follows_its_style_unless_set() {
    let s = load_json(
        r#"{"version": 4, "sections": {"playback": {"settings": {"style": "dock",
        "curve_height": 40, "thickness": 1, "width": 123, "curve_speed": 9000}}}}"#,
    );
    let p = &s.sections.playback;
    assert_eq!(p.style, PlaybackStyle::Dock);
    // Out of range: pulled back.
    assert_eq!((p.curve_height, p.thickness), (Some(14), Some(2)));
    assert_eq!((p.width, p.curve_speed), (500, 300));
    // Left out: up to the style, and still so once saved.
    assert_eq!(p.curve_length, None);
    assert!(p.look().filled_play);
    let saved = serde_json::to_string(&s).unwrap();
    assert_eq!(load_json(&saved).sections.playback, *p);
    // A file from before there was a playback section looks like it did.
    let old = load_json(r#"{"version": 4, "sections": {}}"#);
    assert_eq!(old.sections.playback.settings, PlaybackSettings::default());
    assert_eq!(old.sections.playback.look().curve_length, 100);
}

#[test]
fn single_settings_are_read_and_put_back() {
    let mut s = Sections::default();
    let sound = s.get(SectionId::Sound).json();
    assert_eq!(sound["sensitivity"], TuningSettings::default().sensitivity);
    let section = s.get_mut(SectionId::Sound);
    assert!(section.set_json(&[("floor_db", (-60).into()), ("ceiling_db", (-5).into())]));
    assert_eq!((s.sound.floor_db, s.sound.ceiling_db), (-60, -5));
    // Nonsense changes nothing.
    let section = s.get_mut(SectionId::Sound);
    assert!(!section.set_json(&[("sensitivity", "loud".into())]));
    assert_eq!(s.sound.sensitivity, TuningSettings::default().sensitivity);
}

//! The settings model and file format, through the public `undavi::settings` API.
//!
//! These never call `settings::save`: integration tests build the library without
//! `cfg(test)`, so it would write the real settings file.

use undavi::settings::*;

#[test]
fn old_blue_palette_loads_as_undavi() {
    let v: VisualSettings = serde_json::from_str(r#"{"palette": "blue"}"#).unwrap();
    assert_eq!(v.palette, Palette::Undavi);
    let saved = serde_json::to_string(&v).unwrap();
    assert!(saved.contains(r#""palette":"undavi""#));
    assert_eq!(VisualSettings::default().palette, Palette::Undavi);
}

#[test]
fn files_carry_a_version_and_old_ones_migrate() {
    // A file from before versioning: presets with tuning, an out-of-range value.
    let json = r#"{"tuning": {"sensitivity": 250},
        "presets": [{"name": "Night", "visuals": {}, "tuning": {"sensitivity": 90}}]}"#;
    let s: Stored = serde_json::from_str(json).unwrap();
    assert_eq!(s.version, 0);
    let s = migrate(s);
    assert_eq!(s.version, CONFIG_VERSION);
    assert_eq!(s.tuning.sensitivity, 100);
    assert_eq!(s.tuning_presets[0].name, "Night");
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
    // "smoothing" became "spread"; the stereo switch became the split choice.
    let t: TuningSettings = serde_json::from_str(r#"{"smoothing": 12}"#).unwrap();
    assert_eq!(t.spread, 12);
    let on: VisualSettings = serde_json::from_str(r#"{"stereo": true}"#).unwrap();
    assert_eq!(on.split, Split::Stereo);
    let off: VisualSettings = serde_json::from_str(r#"{"stereo": false}"#).unwrap();
    assert_eq!(off.split, Split::Off);
    let new: VisualSettings = serde_json::from_str(r#"{"split": "mid_side"}"#).unwrap();
    assert_eq!(new.split, Split::MidSide);
    let saved = serde_json::to_string(&new).unwrap();
    assert!(saved.contains(r#""split":"mid_side""#) && !saved.contains("stereo"));
    let a: AppearanceSettings = serde_json::from_str(r#"{"frame_rate": "30"}"#).unwrap();
    assert_eq!(a.frame_rate, FrameRate::Fps30);
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
    let mut s: Stored = serde_json::from_str(json).unwrap();
    migrate_presets(&mut s);
    assert_eq!(s.presets.len(), 4);
    assert!(s.presets.iter().all(|p| p.tuning.is_none()));
    let names: Vec<&str> = s.tuning_presets.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["Night", "Party"], "identical tunings are kept once");
    assert_eq!(s.tuning_presets[1].tuning.sensitivity, 40);
    // Saved files no longer carry the old field.
    let out = serde_json::to_string(&s.presets[0]).unwrap();
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
fn max_length_defaults_and_clamps() {
    assert_eq!(VisualSettings::default().max_length, 400);
    let v: VisualSettings = serde_json::from_str(r#"{"max_length": 5000}"#).unwrap();
    assert_eq!(v.clamped().max_length, 1200);
    let v: VisualSettings = serde_json::from_str(r#"{"max_length": 12}"#).unwrap();
    assert_eq!(v.clamped().max_length, 100);
    let v: VisualSettings = serde_json::from_str(r#"{"max_length": 437}"#).unwrap();
    assert_eq!(v.clamped().max_length, 430, "steps of 10");
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

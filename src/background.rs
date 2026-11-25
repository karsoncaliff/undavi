//! The background: which shader draws it this frame, and with what. Every
//! background is a shader (`shaders/mod.rs`): Solid, Album art and Ambient are built
//! in, and the shaders folder can add more. The pixels are made on the GPU
//! (`backdrop.rs`); this works out the uniforms, the covers for the cross-fade
//! between tracks, and the settings the app itself reads from the shader.

use crate::app::Message;
use crate::backdrop::{self, AUDIO_FLOATS, COMPOSITE_FLOATS, Cover, Place, SCENE_FLOATS};
use crate::settings::{BackgroundSettings, GlowSource};
use crate::shaders::settings::Resolved;
use crate::shaders::{self, Prepared};
use iced::Rectangle;
use iced::mouse::Cursor;
use iced::widget::shader;

/// Below this Dim, add a soft scrim behind the header so its text stays readable.
const SCRIM_BELOW_DIM: u8 = 35;
/// How much the background swells at full pulse (Swell; Zoom swells twice as much).
const PULSE_SCALE: f32 = 0.06;
/// How much a full pulse lifts the dim (Swell; Brighten lifts twice as much).
const PULSE_LIFT: f32 = 0.3;
/// Focus: how much of the blur a full pulse takes away.
const PULSE_FOCUS: f32 = 0.67;

/// What a background does on the beat (its `pulse_style` setting).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PulseStyle {
    /// Swells a little and brightens.
    Swell,
    /// Only brightens, more.
    Brighten,
    /// Only zooms in, more.
    Zoom,
    /// Comes into focus: less blur on the beat.
    Focus,
}

impl PulseStyle {
    fn from_label(label: &str) -> Option<Self> {
        Some(match label {
            "Swell" => PulseStyle::Swell,
            "Brighten" => PulseStyle::Brighten,
            "Zoom" => PulseStyle::Zoom,
            "Focus" => PulseStyle::Focus,
            _ => return None,
        })
    }
}

/// The settings the app itself reads from the background, by their reserved ids
/// (`blur`, `speed`, `pulse`, `pulse_source`, `pulse_amount`, `pulse_style`;
/// see docs/shaders.md), and whether the global Dim applies (`@no-dim`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BgEngine {
    /// Logical px.
    pub blur: f32,
    /// How fast the background clock runs (1 = normal).
    pub speed: f32,
    pub pulse: bool,
    pub pulse_source: GlowSource,
    /// 0..=1.
    pub pulse_amount: f32,
    pub pulse_style: PulseStyle,
    pub dim: bool,
}

impl BgEngine {
    pub fn read(r: &Resolved) -> Self {
        Self {
            blur: r.float("blur").unwrap_or(0.0).max(0.0),
            speed: r.float("speed").map_or(1.0, |s| (s / 100.0).max(0.0)),
            pulse: r.toggle("pulse").unwrap_or(false),
            pulse_source: r
                .choice("pulse_source")
                .and_then(GlowSource::from_label)
                .unwrap_or(GlowSource::Bass),
            pulse_amount: r
                .float("pulse_amount")
                .map_or(0.5, |a| (a / 100.0).clamp(0.0, 1.0)),
            pulse_style: r
                .choice("pulse_style")
                .and_then(PulseStyle::from_label)
                .unwrap_or(PulseStyle::Swell),
            dim: !r.decls.no_dim,
        }
    }
}

/// How far the background swells at this pulse, as a fraction of its size.
pub fn swell_amount(style: PulseStyle, pulse: f32) -> f32 {
    pulse
        * match style {
            PulseStyle::Swell => PULSE_SCALE,
            PulseStyle::Zoom => 2.0 * PULSE_SCALE,
            PulseStyle::Brighten | PulseStyle::Focus => 0.0,
        }
}

/// How much of the dim the pulse lifts, 0..=1.
pub fn lift_amount(style: PulseStyle, pulse: f32) -> f32 {
    pulse
        * match style {
            PulseStyle::Swell => PULSE_LIFT,
            PulseStyle::Brighten => 2.0 * PULSE_LIFT,
            PulseStyle::Zoom | PulseStyle::Focus => 0.0,
        }
}

/// How much of the blur is left at this pulse (Focus sharpens on the beat).
pub fn blur_amount(style: PulseStyle, pulse: f32) -> f32 {
    match style {
        PulseStyle::Focus => 1.0 - PULSE_FOCUS * pulse,
        _ => 1.0,
    }
}

/// One frame of the background.
pub struct Background<'a> {
    pub place: Place,
    /// The effects over every background (dim, color, vignette, grain).
    pub settings: &'a BackgroundSettings,
    pub shader: &'a Prepared,
    pub resolved: &'a Resolved,
    pub engine: BgEngine,
    /// Seconds on the background clock (scaled by the `speed` setting).
    pub time: f32,
    /// The current cover.
    pub cover: Option<&'a Cover>,
    /// While the cover changes: the outgoing one (or none) and how far the
    /// fade to the current one is (0..1).
    pub fading: Option<(Option<&'a Cover>, f32)>,
    /// Beat pulse, 0..=1 (with the intensity applied).
    pub pulse: f32,
    /// The colors from the cover (or the defaults).
    pub ambient: [[u8; 3]; 4],
    /// The accent, for the tint (and shaders' `accent()`).
    pub tint: [u8; 3],
    /// The music, for shaders.
    pub audio: shaders::Audio,
}

fn rgb(c: [u8; 3]) -> [f32; 4] {
    [
        c[0] as f32 / 255.0,
        c[1] as f32 / 255.0,
        c[2] as f32 / 255.0,
        1.0,
    ]
}

fn rows<const N: usize>(rows: &[[f32; 4]]) -> [f32; N] {
    let mut out = [0.0; N];
    for (i, row) in rows.iter().enumerate() {
        out[4 * i..4 * i + 4].copy_from_slice(row);
    }
    out
}

impl Background<'_> {
    /// The frame's uniforms for a `w` × `h` (logical) window.
    pub fn primitive(&self, w: f32, h: f32) -> backdrop::Primitive {
        let s = self.settings;
        let e = &self.engine;
        let style = e.pulse_style;

        // Covers: the current one in slot A, the outgoing one in slot B (slot 0
        // means none: the Ambient circles stand in).
        let mut covers: [Option<Cover>; 2] = [self.cover.cloned(), None];
        let new_slot = if self.cover.is_some() { 1.0 } else { 0.0 };
        let (old_slot, fade) = match self.fading {
            Some((Some(old), t)) => {
                covers[1] = Some(old.clone());
                (2.0, t)
            }
            Some((None, t)) => (0.0, t),
            None => (new_slot, 1.0),
        };

        let base = crate::theme::BG;
        let scene: [f32; SCENE_FLOATS] = rows(&[
            [w, h, self.time, 0.0],
            [old_slot, new_slot, fade, 0.0],
            [self.pulse, swell_amount(style, self.pulse), 0.0, 0.0],
            rgb(self.ambient[0]),
            rgb(self.ambient[1]),
            rgb(self.ambient[2]),
            rgb(self.ambient[3]),
            [base.r, base.g, base.b, 1.0],
        ]);

        let dim = if e.dim { s.dim_pct } else { 0 };
        let lift = lift_amount(style, self.pulse);
        let scrim = if e.dim && dim < SCRIM_BELOW_DIM {
            0.5 * (1.0 - dim as f32 / SCRIM_BELOW_DIM as f32)
        } else {
            0.0
        };
        let composite: [f32; COMPOSITE_FLOATS] = rows(&[
            [w, h, self.time, 0.0],
            [
                s.saturation as f32 / 100.0,
                s.brightness as f32 / 100.0,
                s.contrast as f32 / 100.0,
                s.tint_pct as f32 / 100.0,
            ],
            rgb(self.tint),
            [
                dim as f32 / 100.0 * (1.0 - lift),
                s.vignette_pct as f32 / 100.0,
                s.grain_pct as f32 / 100.0,
                scrim,
            ],
        ]);

        let a = &self.audio;
        let mut audio = [0.0f32; AUDIO_FLOATS];
        audio[0..4].copy_from_slice(&[a.bass, a.mids, a.treble, a.beat]);
        audio[4] = a.level;
        audio[8..8 + shaders::BANDS].copy_from_slice(&a.bands);
        audio[24..28].copy_from_slice(&rgb(self.tint));

        backdrop::Primitive {
            place: self.place,
            scene,
            composite,
            blur: e.blur * blur_amount(style, self.pulse),
            covers,
            shader: self.shader.compiled.clone(),
            audio,
            settings: self.resolved.pack(),
        }
    }
}

impl shader::Program<Message> for Background<'_> {
    type State = ();
    type Primitive = backdrop::Primitive;

    fn draw(&self, _: &(), _: Cursor, bounds: Rectangle) -> backdrop::Primitive {
        self.primitive(bounds.width, bounds.height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pulse_styles() {
        use PulseStyle::*;
        assert_eq!(swell_amount(Zoom, 1.0), 2.0 * swell_amount(Swell, 1.0));
        assert_eq!(swell_amount(Brighten, 1.0), 0.0);
        assert_eq!(lift_amount(Brighten, 1.0), 2.0 * lift_amount(Swell, 1.0));
        assert_eq!(lift_amount(Focus, 1.0), 0.0);
        assert_eq!(swell_amount(Focus, 1.0), 0.0);
        assert!(blur_amount(Focus, 1.0) < blur_amount(Focus, 0.0));
        assert_eq!(blur_amount(Swell, 1.0), 1.0);
    }

    fn prepared(name: &str) -> Prepared {
        shaders::Library::new(None)
            .prepared(name, shaders::Kind::Scene)
            .unwrap()
            .clone()
    }

    #[test]
    fn reserved_settings_and_the_dim() {
        let album = prepared(shaders::ALBUM_ART);
        let mut saved = crate::settings::ShaderValues::new();
        saved.insert("blur".into(), 40.into());
        saved.insert("speed".into(), 150.into());
        saved.insert("pulse".into(), true.into());
        saved.insert("pulse_style".into(), "Focus".into());
        let r = album.decls.resolve(Some(&saved), &[]);
        let e = BgEngine::read(&r);
        assert_eq!(e.blur, 40.0);
        assert!((e.speed - 1.5).abs() < 1e-6);
        assert!(e.pulse && e.dim);
        assert_eq!(e.pulse_style, PulseStyle::Focus);

        let settings = BackgroundSettings::default();
        let bg = Background {
            place: Place::Screen,
            settings: &settings,
            shader: &album,
            resolved: &r,
            engine: e,
            time: 2.0,
            cover: None,
            fading: None,
            pulse: 1.0,
            ambient: [[0; 3]; 4],
            tint: [0; 3],
            audio: shaders::Audio::default(),
        };
        let p = bg.primitive(800.0, 600.0);
        assert!(p.blur < 40.0, "Focus loosens the blur on the beat");
        assert_eq!(
            p.composite[12],
            settings.dim_pct as f32 / 100.0,
            "Focus doesn't lift the dim"
        );
        assert_eq!(p.scene[4..7], [0.0, 0.0, 1.0], "no cover: slot 0, no fade");

        // Solid declares @no-dim.
        let solid = prepared(shaders::SOLID);
        let r = solid.decls.resolve(None, &[]);
        let bg = Background {
            shader: &solid,
            resolved: &r,
            engine: BgEngine::read(&r),
            ..bg
        };
        assert_eq!(bg.primitive(800.0, 600.0).composite[12], 0.0);
        assert_eq!(bg.primitive(800.0, 600.0).blur, 0.0);
    }

    #[test]
    fn covers_fade_from_slot_b_to_a() {
        let album = prepared(shaders::ALBUM_ART);
        let r = album.decls.resolve(None, &[]);
        let (old, new) = (
            Cover::new(image::RgbaImage::new(2, 2)),
            Cover::new(image::RgbaImage::new(2, 2)),
        );
        let settings = BackgroundSettings::default();
        let p = Background {
            place: Place::Screen,
            settings: &settings,
            shader: &album,
            resolved: &r,
            engine: BgEngine::read(&r),
            time: 0.0,
            cover: Some(&new),
            fading: Some((Some(&old), 0.25)),
            pulse: 0.0,
            ambient: [[0; 3]; 4],
            tint: [0; 3],
            audio: shaders::Audio::default(),
        }
        .primitive(800.0, 600.0);
        assert_eq!(p.scene[4..7], [2.0, 1.0, 0.25]);
        assert_eq!(p.covers[0].as_ref().map(|c| c.id), Some(new.id));
        assert_eq!(p.covers[1].as_ref().map(|c| c.id), Some(old.id));
    }
}

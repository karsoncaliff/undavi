//! Shaders: the built-in visualizers and backgrounds, and your own `.wgsl` files
//! in `~/.config/undavi/shaders/`. See `docs/shaders.md`.
//!
//! A file defines one of
//!
//! - `fn visualizer(uv: vec2<f32>, t: f32) -> vec4<f32>`: a visualizer, picked
//!   under Visualizer → Style (Bars, Radial and Wave are written this way too);
//! - `fn scene(uv: vec2<f32>, t: f32) -> vec3<f32>`: a background, picked under
//!   Background → Style (Solid, Album art and Ambient too);
//! - `fn warp(q: vec2<f32>, t: f32) -> vec2<f32>`: an animation for the Album art
//!   and Ambient backgrounds, offered next to Liquid, Warp and Spiral.
//!
//! Its `// @setting` lines declare its settings (`settings.rs`). The file goes in
//! front of the prelude it builds on (`visualizer.wgsl` or `scene.wgsl` in
//! `assets/shaders/`, after `common.wgsl`), then its generated `settings` struct
//! and the hooks calling into it. Everything is checked with naga before
//! it reaches the GPU, so a mistake shows as a message with its line instead of a
//! crash. Files are re-read when they change.

use settings::Decls;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant, SystemTime};

pub mod settings;

/// Math helpers and noise shared by every prelude.
const COMMON: &str = include_str!("../../assets/shaders/common.wgsl");
/// The prelude for backgrounds.
const SCENE: &str = include_str!("../../assets/shaders/scene.wgsl");
/// The prelude for visualizers.
const VISUALIZER: &str = include_str!("../../assets/shaders/visualizer.wgsl");

/// Where each prelude puts the `settings` uniform.
const SCENE_SETTINGS_BINDING: u32 = 5;
const VISUALIZER_SETTINGS_BINDING: u32 = 3;

const NO_WARP: &str = "fn undavi_custom_warp(q: vec2<f32>, t: f32) -> vec2<f32> { return q; }";
const CALL_WARP: &str =
    "fn undavi_custom_warp(q: vec2<f32>, t: f32) -> vec2<f32> { return warp(q, t); }";
const CALL_SCENE: &str =
    "fn undavi_custom_scene(uv: vec2<f32>, t: f32) -> vec3<f32> { return scene(uv, t); }";
const CALL_VISUALIZER: &str =
    "fn undavi_custom_visualizer(uv: vec2<f32>, t: f32) -> vec4<f32> { return visualizer(uv, t); }";

/// Built-in shader names (a file can't take them).
pub const BARS: &str = "bars";
pub const RADIAL: &str = "radial";
pub const WAVE: &str = "wave";
pub const SOLID: &str = "solid";
pub const ALBUM_ART: &str = "album-art";
pub const AMBIENT: &str = "ambient";

/// One of Undavi's own shaders.
pub struct Builtin {
    pub name: &'static str,
    pub label: &'static str,
    pub kind: Kind,
    pub source: &'static str,
}

/// The built-in visualizers, then backgrounds, in the order they're listed.
pub const BUILTINS: [Builtin; 6] = [
    Builtin {
        name: BARS,
        label: "Bars",
        kind: Kind::Visualizer,
        source: include_str!("../../assets/shaders/visualizers/bars.wgsl"),
    },
    Builtin {
        name: RADIAL,
        label: "Radial",
        kind: Kind::Visualizer,
        source: include_str!("../../assets/shaders/visualizers/radial.wgsl"),
    },
    Builtin {
        name: WAVE,
        label: "Wave",
        kind: Kind::Visualizer,
        source: include_str!("../../assets/shaders/visualizers/wave.wgsl"),
    },
    Builtin {
        name: SOLID,
        label: "Solid",
        kind: Kind::Scene,
        source: include_str!("../../assets/shaders/backgrounds/solid.wgsl"),
    },
    Builtin {
        name: ALBUM_ART,
        label: "Album art",
        kind: Kind::Scene,
        source: include_str!("../../assets/shaders/backgrounds/album-art.wgsl"),
    },
    Builtin {
        name: AMBIENT,
        label: "Ambient",
        kind: Kind::Scene,
        source: include_str!("../../assets/shaders/backgrounds/ambient.wgsl"),
    },
];

pub fn builtin(name: &str) -> Option<&'static Builtin> {
    BUILTINS.iter().find(|b| b.name == name)
}

/// The examples put in a new shaders folder.
pub const EXAMPLES: [(&str, &str); 5] = [
    (
        "kaleidoscope.wgsl",
        include_str!("../../assets/shaders/examples/kaleidoscope.wgsl"),
    ),
    (
        "ripple.wgsl",
        include_str!("../../assets/shaders/examples/ripple.wgsl"),
    ),
    (
        "plasma.wgsl",
        include_str!("../../assets/shaders/examples/plasma.wgsl"),
    ),
    (
        "led-meter.wgsl",
        include_str!("../../assets/shaders/examples/led-meter.wgsl"),
    ),
    (
        "orbit.wgsl",
        include_str!("../../assets/shaders/examples/orbit.wgsl"),
    ),
];

/// How often the folder is checked for new or changed files.
const RESCAN: Duration = Duration::from_secs(1);

/// What a shader is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// An animation for Album art and Ambient (`fn warp`).
    Warp,
    /// A background (`fn scene`).
    Scene,
    /// A visualizer (`fn visualizer`).
    Visualizer,
}

impl Kind {
    /// Background shaders (scenes, and the warps they offer) rather than visualizers.
    pub fn is_background(self) -> bool {
        self != Kind::Visualizer
    }
}

/// A complete shader for the GPU. The id changes whenever the source does, so
/// the GPU side knows when to build a new pipeline.
#[derive(Clone, Debug)]
pub struct Compiled {
    pub id: u64,
    pub source: Arc<str>,
}

impl Compiled {
    fn new(source: String) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self {
            id: NEXT.fetch_add(1, Ordering::Relaxed),
            source: source.into(),
        }
    }
}

/// A shader ready to use: its settings and its GPU source.
#[derive(Clone, Debug)]
pub struct Prepared {
    pub decls: Decls,
    pub compiled: Compiled,
}

/// The full source: the file first (so error line numbers match it; a warp
/// before the background it animates), then common.wgsl, the prelude, the
/// `settings` struct and the hooks.
fn compose(code: &str, kind: Kind, decls: &Decls, warp: Option<&str>) -> String {
    match kind {
        Kind::Visualizer => format!(
            "{code}\n{COMMON}\n{VISUALIZER}\n{}\n{CALL_VISUALIZER}\n",
            decls.wgsl(VISUALIZER_SETTINGS_BINDING)
        ),
        Kind::Scene | Kind::Warp => {
            let hook = if warp.is_some() { CALL_WARP } else { NO_WARP };
            format!(
                "{}{code}\n{COMMON}\n{SCENE}\n{}\n{hook}\n{CALL_SCENE}\n",
                warp.map_or(String::new(), |w| format!("{w}\n")),
                decls.wgsl(SCENE_SETTINGS_BINDING)
            )
        }
    }
}

/// Read a shader's settings, compose it and check it. `kind` is Visualizer or
/// Scene; `warp` is a custom warp (code and file name) for a background to offer.
pub fn prepare(
    code: &str,
    file: &str,
    kind: Kind,
    warp: Option<(&str, &str)>,
) -> Result<Prepared, String> {
    let mut decls = settings::parse(code, file)?;
    if let Some((warp_code, warp_file)) = warp {
        let mut extra = settings::parse(warp_code, warp_file)?;
        // A warp's settings go under its own heading unless it names one.
        let heading = warp_file.trim_end_matches(".wgsl").to_string();
        for d in &mut extra.items {
            d.section.get_or_insert_with(|| heading.clone());
        }
        decls = decls.merged(&extra, warp_file)?;
    }
    let source = compose(code, kind, &decls, warp.map(|(c, _)| c));
    validate(&source, warp.map_or(file, |(_, f)| f))?;
    Ok(Prepared {
        decls,
        compiled: Compiled::new(source),
    })
}

/// The built-ins, prepared once.
static BUILTIN_PREPARED: LazyLock<Vec<Result<Prepared, String>>> = LazyLock::new(|| {
    BUILTINS
        .iter()
        .map(|b| prepare(b.source, &format!("{}.wgsl", b.name), b.kind, None))
        .collect()
});

fn without_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut rest = src;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix("//") {
            rest = after.find('\n').map_or("", |i| &after[i..]);
        } else if let Some(after) = rest.strip_prefix("/*") {
            rest = after.find("*/").map_or("", |i| &after[i + 2..]);
            out.push(' ');
        } else {
            let c = rest.chars().next().unwrap_or(' ');
            out.push(c);
            rest = &rest[c.len_utf8()..];
        }
    }
    out
}

fn defines(src: &str, name: &str) -> bool {
    let words: Vec<&str> = src
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|w| !w.is_empty())
        .collect();
    words.windows(2).any(|w| w[0] == "fn" && w[1] == name)
}

/// Which kind of shader `src` is, from the function it defines.
pub fn kind_of(src: &str) -> Result<Kind, String> {
    let code = without_comments(src);
    let found: Vec<Kind> = [
        ("warp", Kind::Warp),
        ("scene", Kind::Scene),
        ("visualizer", Kind::Visualizer),
    ]
    .into_iter()
    .filter(|(name, _)| defines(&code, name))
    .map(|(_, kind)| kind)
    .collect();
    match found.as_slice() {
        [kind] => Ok(*kind),
        [] => Err(
            "It needs a `fn visualizer(uv: vec2<f32>, t: f32) -> vec4<f32>`, a \
             `fn scene(uv: vec2<f32>, t: f32) -> vec3<f32>` or a \
             `fn warp(q: vec2<f32>, t: f32) -> vec2<f32>`."
                .into(),
        ),
        _ => Err("It defines more than one of `warp`, `scene` and `visualizer`; keep one.".into()),
    }
}

/// Check a composed shader the way the GPU will (naga, which wgpu uses).
/// Errors point at the line and column in `file`.
pub fn validate(source: &str, file: &str) -> Result<(), String> {
    let module = naga::front::wgsl::parse_str(source)
        .map_err(|e| e.emit_to_string_with_path(source, file))?;
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::default(),
    )
    .validate(&module)
    .map(drop)
    .map_err(|e| e.emit_to_string_with_path(source, file))
}

/// Read and check one file. A warp is checked as Album art's animation. The kind
/// is known whenever the file says which function it defines, even if it
/// doesn't compile.
fn load(path: &Path, name: &str) -> (Option<Kind>, Option<Arc<str>>, Result<Prepared, String>) {
    let code = match std::fs::read_to_string(path) {
        Ok(code) => code,
        Err(e) => return (None, None, Err(format!("Couldn't read it: {e}."))),
    };
    let kind = match kind_of(&code) {
        Ok(kind) => kind,
        Err(e) => return (None, Some(code.into()), Err(e)),
    };
    let file = path
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let state = if builtin(name).is_some() {
        Err(format!(
            "\"{name}\" is the name of a built-in; rename the file to use it."
        ))
    } else {
        match kind {
            Kind::Warp => {
                let base = builtin(ALBUM_ART).map_or("", |b| b.source);
                prepare(base, "album-art.wgsl", Kind::Scene, Some((&code, &file)))
            }
            kind => prepare(&code, &file, kind, None),
        }
    };
    (Some(kind), Some(code.into()), state)
}

/// A shader: built in, or a file.
#[derive(Clone, Debug)]
pub struct Entry {
    /// The file name without `.wgsl` (or a built-in's name).
    pub name: String,
    /// What the settings show ("Bars", or the file name).
    pub label: String,
    pub builtin: bool,
    path: Option<PathBuf>,
    modified: Option<SystemTime>,
    /// What it is, when that's clear from the file.
    pub kind: Option<Kind>,
    /// The file's code (for building a background with it as the animation).
    code: Option<Arc<str>>,
    /// The shader, or why it can't be used. A warp's is Album art with it.
    pub state: Result<Prepared, String>,
}

impl Entry {
    /// Whether it belongs in the background list (`true`) or the visualizer list.
    /// Files of unknown kind show in both, with their error; warps show in the
    /// background list (they're picked inside Album art and Ambient).
    pub fn fits(&self, background: bool) -> bool {
        self.kind.is_none_or(|k| k.is_background() == background)
    }
}

/// A background with a custom warp as its animation, prepared once per pair
/// of file versions.
struct Combo {
    ids: (u64, u64),
    state: Result<Prepared, String>,
}

/// Every shader: the built-ins, then the shaders folder (re-read as files
/// appear, change or go).
pub struct Library {
    dir: Option<PathBuf>,
    pub entries: Vec<Entry>,
    last_scan: Option<Instant>,
    /// Bring new examples into the folder (once per run), for the user's folder.
    examples_due: bool,
    combos: HashMap<(String, String), Combo>,
}

impl Default for Library {
    fn default() -> Self {
        Self::new(None)
    }
}

/// `~/.config/undavi/shaders` (next to the settings file).
pub fn folder() -> Option<PathBuf> {
    crate::settings::config_path().and_then(|p| p.parent().map(|d| d.join("shaders")))
}

fn builtin_entries() -> Vec<Entry> {
    BUILTINS
        .iter()
        .zip(BUILTIN_PREPARED.iter())
        .map(|(b, state)| Entry {
            name: b.name.to_string(),
            label: b.label.to_string(),
            builtin: true,
            path: None,
            modified: None,
            kind: Some(b.kind),
            code: Some(b.source.into()),
            state: state.clone(),
        })
        .collect()
}

impl Library {
    pub fn new(dir: Option<PathBuf>) -> Self {
        Self {
            dir,
            entries: builtin_entries(),
            last_scan: None,
            examples_due: false,
            combos: HashMap::new(),
        }
    }

    /// The user's shaders folder. Once the folder exists, examples added in
    /// newer versions of Undavi appear in it too (see [`install_examples`]).
    pub fn user() -> Self {
        Self {
            examples_due: true,
            ..Self::new(folder())
        }
    }

    pub fn get(&self, name: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.name == name)
    }

    /// A usable shader of `kind` by name.
    pub fn prepared(&self, name: &str, kind: Kind) -> Option<&Prepared> {
        self.get(name)
            .filter(|e| e.kind == Some(kind))
            .and_then(|e| e.state.as_ref().ok())
    }

    /// The custom warps that work, for the backgrounds that offer them.
    pub fn warps(&self) -> Vec<String> {
        self.entries
            .iter()
            .filter(|e| e.kind == Some(Kind::Warp) && e.state.is_ok())
            .map(|e| e.name.clone())
            .collect()
    }

    /// Background `name` with custom warp `warp` as its animation: prepared the
    /// first time it's asked for (and again when either file changes).
    pub fn with_warp(&mut self, name: &str, warp: &str) -> Option<&Result<Prepared, String>> {
        let base = self.get(name)?;
        let w = self.get(warp).filter(|e| e.kind == Some(Kind::Warp))?;
        let ids = (
            base.state.as_ref().map_or(0, |p| p.compiled.id),
            w.state.as_ref().map_or(0, |p| p.compiled.id),
        );
        let key = (name.to_string(), warp.to_string());
        if self.combos.get(&key).is_none_or(|c| c.ids != ids) {
            let state = match (&base.code, &w.code) {
                (Some(code), Some(warp_code)) => prepare(
                    code,
                    &format!("{name}.wgsl"),
                    Kind::Scene,
                    Some((warp_code, &format!("{warp}.wgsl"))),
                ),
                _ => Err("This shader can't be read.".into()),
            };
            self.combos.insert(key.clone(), Combo { ids, state });
        }
        self.combos.get(&key).map(|c| &c.state)
    }

    /// Look for new, changed or removed files (at most once a second). Returns
    /// whether anything changed.
    pub fn refresh(&mut self, now: Instant) -> bool {
        if self
            .last_scan
            .is_some_and(|t| now.saturating_duration_since(t) < RESCAN)
        {
            return false;
        }
        self.last_scan = Some(now);
        self.rescan()
    }

    fn rescan(&mut self) -> bool {
        let Some(dir) = &self.dir else {
            return false;
        };
        if self.examples_due && dir.is_dir() {
            self.examples_due = false;
            if let Err(e) = install_examples(dir) {
                eprintln!("undavi: couldn't add the example shaders: {e}");
            }
        }
        let mut files: Vec<(String, PathBuf, Option<SystemTime>)> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "wgsl"))
            .filter_map(|p| {
                let name = p.file_stem()?.to_string_lossy().into_owned();
                let modified = std::fs::metadata(&p).and_then(|m| m.modified()).ok();
                Some((name, p, modified))
            })
            .collect();
        files.sort_by(|a, b| a.0.cmp(&b.0));

        let old_files: Vec<&Entry> = self.entries.iter().filter(|e| !e.builtin).collect();
        let mut changed = files.len() != old_files.len();
        let mut entries = builtin_entries();
        for (name, path, modified) in files {
            match old_files.iter().find(|e| e.path.as_ref() == Some(&path)) {
                Some(old) if old.modified == modified => entries.push((*old).clone()),
                _ => {
                    changed = true;
                    let (kind, code, state) = load(&path, &name);
                    entries.push(Entry {
                        label: name.clone(),
                        name,
                        builtin: false,
                        path: Some(path),
                        modified,
                        kind,
                        code,
                        state,
                    });
                }
            }
        }
        self.entries = entries;
        changed
    }
}

/// Create the shaders folder (with the examples, if it has no shaders yet) and
/// return it.
pub fn prepare_folder() -> Result<PathBuf, String> {
    let dir = folder().ok_or("there's no config folder")?;
    // Tests never write to the user's config.
    if cfg!(test) {
        return Ok(dir);
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    install_examples(&dir)?;
    Ok(dir)
}

/// Copy built-in shader `name` into the shaders folder (as `<name>-copy.wgsl`,
/// or `-copy-2`, … if taken), to start a shader of your own from it. Returns the
/// new file.
pub fn copy_builtin(name: &str) -> Result<PathBuf, String> {
    let b = builtin(name).ok_or_else(|| format!("{name} isn't a built-in shader"))?;
    let dir = prepare_folder()?;
    let free = (1..100)
        .map(|n| match n {
            1 => dir.join(format!("{name}-copy.wgsl")),
            n => dir.join(format!("{name}-copy-{n}.wgsl")),
        })
        .find(|p| !p.exists())
        .ok_or("too many copies already")?;
    // Tests never write to the user's config.
    if !cfg!(test) {
        std::fs::write(&free, b.source).map_err(|e| format!("{}: {e}", free.display()))?;
    }
    Ok(free)
}

/// Which examples a folder has already been given, one file name per line.
const EXAMPLES_RECORD: &str = ".examples";

/// Put the examples this folder hasn't had yet into it, and note them in
/// [`EXAMPLES_RECORD`]. So a newer version's examples arrive in an existing
/// folder, while an example you deleted (or changed) is left as you left it.
/// Returns the files written.
pub fn install_examples(dir: &Path) -> Result<Vec<&'static str>, String> {
    let record = dir.join(EXAMPLES_RECORD);
    let mut had: Vec<String> = std::fs::read_to_string(&record)
        .map(|s| s.lines().map(str::to_string).collect())
        .unwrap_or_default();
    let mut written = Vec::new();
    for (name, code) in EXAMPLES {
        if had.iter().any(|h| h == name) {
            continue;
        }
        // A folder from before the record: keep what's there.
        let path = dir.join(name);
        if !path.exists() {
            std::fs::write(&path, code).map_err(|e| format!("{name}: {e}"))?;
            written.push(name);
        }
        had.push(name.to_string());
    }
    if !written.is_empty() || !record.exists() {
        std::fs::write(&record, had.join("\n") + "\n")
            .map_err(|e| format!("{}: {e}", record.display()))?;
    }
    Ok(written)
}

// ---------------------------------------------------------------------------
// Audio for the shaders

/// Spectrum bands handed to shaders.
pub const BANDS: usize = 16;
/// Their range, in Hz.
const BAND_RANGE: (f32, f32) = (30.0, 16_000.0);

/// The music as shaders see it, all 0..1, eased so it doesn't flicker.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Audio {
    pub bands: [f32; BANDS],
    pub bass: f32,
    pub mids: f32,
    pub treble: f32,
    pub beat: f32,
    pub level: f32,
}

/// 16 log-spaced bands of a spectrum (dBFS per FFT bin), each mapped from
/// `floor..ceiling` dB to 0..1 like the bars' level range.
pub fn bands(
    mags_db: &[f32],
    sample_rate: f32,
    fft_size: usize,
    floor: f32,
    ceiling: f32,
) -> [f32; BANDS] {
    let mut out = [0.0; BANDS];
    if mags_db.is_empty() || fft_size == 0 {
        return out;
    }
    let hz_per_bin = sample_rate / fft_size as f32;
    let last = mags_db.len() - 1;
    let (lo, hi) = (BAND_RANGE.0.ln(), BAND_RANGE.1.ln());
    let span = (ceiling - floor).max(1.0);
    for (i, v) in out.iter_mut().enumerate() {
        let f0 = (lo + (hi - lo) * i as f32 / BANDS as f32).exp();
        let f1 = (lo + (hi - lo) * (i + 1) as f32 / BANDS as f32).exp();
        let k0 = ((f0 / hz_per_bin).round() as usize).min(last);
        let k1 = ((f1 / hz_per_bin).round() as usize).clamp(k0, last);
        let bins = &mags_db[k0..=k1];
        // Average the power, not the dB values.
        let power: f32 =
            bins.iter().map(|db| 10f32.powf(db / 10.0)).sum::<f32>() / bins.len() as f32;
        let db = 10.0 * power.max(1e-12).log10();
        *v = ((db - floor) / span).clamp(0.0, 1.0);
    }
    out
}

impl Audio {
    /// Ease toward this frame's `bands` (quick to rise, slower to fall), and
    /// take the beat pulse and the input level as they are.
    pub fn follow(&mut self, bands: Option<&[f32; BANDS]>, beat: f32, level: f32, dt: f32) {
        let rise = 1.0 - (-dt / 0.03).exp();
        let fall = 1.0 - (-dt / 0.25).exp();
        for (i, v) in self.bands.iter_mut().enumerate() {
            let target = bands.map_or(0.0, |b| b[i]);
            let k = if target > *v { rise } else { fall };
            *v += (target - *v) * k;
        }
        let mean = |r: std::ops::Range<usize>| {
            let n = r.len() as f32;
            self.bands[r].iter().sum::<f32>() / n
        };
        self.bass = mean(0..4);
        self.mids = mean(4..10);
        self.treble = mean(10..BANDS);
        self.beat = beat.clamp(0.0, 1.0);
        self.level = level.clamp(0.0, 1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Prepare a file the way the library does.
    fn check(code: &str, file: &str) -> Result<Prepared, String> {
        match kind_of(code)? {
            Kind::Warp => prepare(
                builtin(ALBUM_ART).unwrap().source,
                "album-art.wgsl",
                Kind::Scene,
                Some((code, file)),
            ),
            kind => prepare(code, file, kind, None),
        }
    }

    #[test]
    fn builtin_shaders_are_valid() {
        for (b, state) in BUILTINS.iter().zip(BUILTIN_PREPARED.iter()) {
            let p = state.as_ref().unwrap_or_else(|e| panic!("{}: {e}", b.name));
            assert!(!p.decls.items.is_empty() || b.name == SOLID, "{}", b.name);
        }
        validate(
            include_str!("../../assets/shaders/kawase.wgsl"),
            "kawase.wgsl",
        )
        .unwrap();
        validate(
            include_str!("../../assets/shaders/composite.wgsl"),
            "composite.wgsl",
        )
        .unwrap();
        // The built-in visualizers all declare the settings the app reads.
        for name in [BARS, RADIAL, WAVE] {
            let p = Library::new(None)
                .prepared(name, Kind::Visualizer)
                .cloned()
                .unwrap();
            for id in [
                "bar_count",
                "mirror",
                "split",
                "channel",
                "glow",
                "glow_source",
                "glow_amount",
            ] {
                assert!(p.decls.items.iter().any(|d| d.id == id), "{name}: {id}");
            }
        }
    }

    #[test]
    fn examples_are_valid() {
        for (name, code) in EXAMPLES {
            check(code, name).unwrap_or_else(|e| panic!("{name}: {e}"));
        }
    }

    #[test]
    fn the_guides_samples_are_valid() {
        let guide = include_str!("../../docs/shaders.md");
        let mut checked = 0;
        for block in guide.split("```wgsl\n").skip(1) {
            let code = block.split("```").next().unwrap_or("");
            // Signatures alone (no body) are just there to read.
            if !code.contains('{') {
                continue;
            }
            check(code, "shaders.md").unwrap_or_else(|e| panic!("{e}\n{code}"));
            checked += 1;
        }
        assert!(checked >= 3, "{checked}");
    }

    #[test]
    fn kinds_come_from_the_function_defined() {
        assert_eq!(
            kind_of("fn warp(q: vec2<f32>, t: f32) -> vec2<f32> { return q; }"),
            Ok(Kind::Warp)
        );
        assert_eq!(
            kind_of(
                "// fn warp(...) is in the comment only\nfn scene(uv: vec2<f32>, t: f32) -> vec3<f32> { return vec3<f32>(uv, 0.0); }"
            ),
            Ok(Kind::Scene)
        );
        assert!(kind_of("fn other() {}").is_err());
        assert!(kind_of("fn warp() {} fn scene() {}").is_err());
        assert_eq!(
            kind_of("fn visualizer(uv: vec2<f32>, t: f32) -> vec4<f32> { return vec4<f32>(0.0); }"),
            Ok(Kind::Visualizer)
        );
        // `warping` isn't `warp`.
        assert!(kind_of("fn warping() {}").is_err());
    }

    #[test]
    fn errors_point_at_the_users_line() {
        let code = "// a broken shader\nfn visualizer(uv: vec2<f32>, t: f32) -> vec4<f32> {\n    return vec4<f32>(nope);\n}\n";
        let err = check(code, "broken.wgsl").unwrap_err();
        assert!(err.contains("broken.wgsl:3:"), "{err}");
        assert!(err.contains("nope"), "{err}");
        // In a warp too, though it's built into Album art.
        let code = "// a broken warp\nfn warp(q: vec2<f32>, t: f32) -> vec2<f32> {\n    return q * nope;\n}\n";
        let err = check(code, "broken.wgsl").unwrap_err();
        assert!(err.contains("broken.wgsl:3:"), "{err}");
        // A wrong return type is caught too, and so is a bad setting line.
        assert!(
            check(
                "fn scene(uv: vec2<f32>, t: f32) -> vec3<f32> { return uv; }",
                "x.wgsl"
            )
            .is_err()
        );
        let err = check("// @setting n: int(1) = 1 \"N\"\nfn scene(uv: vec2<f32>, t: f32) -> vec3<f32> { return vec3<f32>(0.0); }", "x.wgsl").unwrap_err();
        assert!(err.starts_with("x.wgsl:1:"), "{err}");
    }

    #[test]
    fn settings_reach_the_shader() {
        let code = "// @setting rings: int(1, 12) = 4 \"Rings\"\n\
                    // @setting shape: choice(\"Soft\", \"Hard\") = \"Soft\" \"Edges\"\n\
                    fn visualizer(uv: vec2<f32>, t: f32) -> vec4<f32> {\n\
                        let hard = settings.shape == SHAPE_HARD;\n\
                        return vec4<f32>(f32(settings.rings) / 12.0, select(0.0, 1.0, hard), 0.0, 1.0);\n\
                    }";
        let p = check(code, "rings.wgsl").unwrap();
        assert_eq!(p.decls.items.len(), 2);
        // A typo in a setting's name is a compile error at its line.
        let typo = code.replace("settings.rings", "settings.ring");
        assert!(
            check(&typo, "rings.wgsl")
                .unwrap_err()
                .contains("rings.wgsl:5:")
        );
    }

    #[test]
    fn warps_bring_their_settings_into_the_background() {
        let dir = std::env::temp_dir().join(format!("undavi-warps-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("wobble.wgsl"),
            "// @setting amount: float(0, 1) = 0.5 \"Amount\"\nfn warp(q: vec2<f32>, t: f32) -> vec2<f32> { return q * (1.0 + settings.amount * 0.1); }",
        )
        .unwrap();
        // Taken names and clashing settings are reported.
        std::fs::write(dir.join("bars.wgsl"), EXAMPLES[4].1).unwrap();
        std::fs::write(
            dir.join("clash.wgsl"),
            "// @setting blur: int(0, 9) = 1 \"Blur\"\nfn warp(q: vec2<f32>, t: f32) -> vec2<f32> { return q; }",
        )
        .unwrap();
        let mut lib = Library::new(Some(dir.clone()));
        lib.rescan();
        assert_eq!(lib.warps(), ["wobble"]);
        let bars = lib
            .entries
            .iter()
            .find(|e| !e.builtin && e.name == "bars")
            .unwrap();
        assert!(bars.state.as_ref().unwrap_err().contains("built-in"));
        assert!(
            lib.get("clash")
                .unwrap()
                .state
                .as_ref()
                .unwrap_err()
                .contains("already a setting")
        );

        let p = lib
            .with_warp(AMBIENT, "wobble")
            .unwrap()
            .as_ref()
            .unwrap()
            .clone();
        let amount = p.decls.items.iter().find(|d| d.id == "amount").unwrap();
        assert_eq!(amount.section.as_deref(), Some("wobble"));
        assert!(
            p.decls.items.iter().any(|d| d.id == "blur"),
            "Ambient's own settings too"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn library_reloads_changed_files() {
        let dir = std::env::temp_dir().join(format!("undavi-shaders-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("mine.wgsl");
        let good =
            |body: &str| format!("fn visualizer(uv: vec2<f32>, t: f32) -> vec4<f32> {{ {body} }}");
        std::fs::write(&file, good("return vec4<f32>(1.0);")).unwrap();
        let mut lib = Library::new(Some(dir.clone()));
        assert_eq!(
            lib.entries.len(),
            BUILTINS.len(),
            "the built-ins come first"
        );
        assert!(lib.rescan());
        let first = lib.get("mine").unwrap().state.as_ref().unwrap().compiled.id;
        assert!(!lib.rescan(), "nothing changed");

        // A broken edit shows the error; mtimes can be coarse, so set one.
        std::fs::write(&file, good("return 1;")).unwrap();
        let later = SystemTime::now() + Duration::from_secs(5);
        std::fs::File::options()
            .write(true)
            .open(&file)
            .unwrap()
            .set_modified(later)
            .unwrap();
        assert!(lib.rescan());
        assert!(lib.get("mine").unwrap().state.is_err());

        std::fs::write(&file, good("return vec4<f32>(0.5);")).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&file)
            .unwrap()
            .set_modified(later + Duration::from_secs(5))
            .unwrap();
        assert!(lib.rescan());
        let second = lib.get("mine").unwrap().state.as_ref().unwrap().compiled.id;
        assert_ne!(first, second);

        std::fs::remove_file(&file).unwrap();
        assert!(lib.rescan());
        assert_eq!(lib.entries.len(), BUILTINS.len());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn new_examples_arrive_and_deleted_ones_stay_gone() {
        let dir = std::env::temp_dir().join(format!("undavi-examples-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // A folder from an older version: some examples, one of them edited, no record.
        std::fs::write(dir.join("kaleidoscope.wgsl"), "// mine now").unwrap();
        std::fs::write(dir.join("ripple.wgsl"), EXAMPLES[1].1).unwrap();
        let written = install_examples(&dir).unwrap();
        assert!(!written.contains(&"kaleidoscope.wgsl") && !written.contains(&"ripple.wgsl"));
        assert_eq!(written.len(), EXAMPLES.len() - 2);
        assert_eq!(
            std::fs::read_to_string(dir.join("kaleidoscope.wgsl")).unwrap(),
            "// mine now",
            "edits are kept"
        );

        // Delete one: it doesn't come back.
        std::fs::remove_file(dir.join("orbit.wgsl")).unwrap();
        assert!(install_examples(&dir).unwrap().is_empty());
        assert!(!dir.join("orbit.wgsl").exists());

        // The library brings them in on its first scan of an existing folder.
        let fresh = dir.join("fresh");
        std::fs::create_dir_all(&fresh).unwrap();
        let mut lib = Library {
            examples_due: true,
            ..Library::new(Some(fresh.clone()))
        };
        lib.rescan();
        assert_eq!(lib.entries.len(), BUILTINS.len() + EXAMPLES.len());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bands_follow_the_spectrum() {
        // 48 kHz, 4096-point FFT: loud lows, quiet highs.
        let mags: Vec<f32> = (0..=2048)
            .map(|k| if k < 20 { -10.0 } else { -90.0 })
            .collect();
        let b = bands(&mags, 48_000.0, 4096, -80.0, 0.0);
        assert!(b[0] > 0.8, "{b:?}");
        assert_eq!(b[BANDS - 1], 0.0);
        let mut audio = Audio::default();
        for _ in 0..60 {
            audio.follow(Some(&b), 0.5, 0.3, 1.0 / 60.0);
        }
        assert!(audio.bass > audio.treble);
        assert_eq!(audio.beat, 0.5);
        // Silence: everything falls back.
        for _ in 0..120 {
            audio.follow(None, 0.0, 0.0, 1.0 / 60.0);
        }
        assert!(audio.bass < 0.01);
    }
}

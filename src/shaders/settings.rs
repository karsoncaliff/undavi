//! Settings a shader declares for itself, in comments at the top of its file:
//!
//! ```text
//! // @section Bars
//! // @setting bar_count: int(16, 96) = 43 "Bar count"
//! // @setting max_length: int(100, 1200, 10) = 400 "Max length" "px"
//! // @setting corners: choice("Square", "Rounded", "Pill") = "Square" "Corners"
//! // @setting segments: toggle = false "Segments"
//! // @help Draws bars as stacks of blocks, like an LED meter.
//! // @setting segment_size: int(2, 16) = 6 "Segment size" "px" if segments
//! // @setting tint: color(#ff8800, #4b7bf5) = #ff8800 "Tint"
//! ```
//!
//! Undavi builds the section's controls from these, saves the values per shader
//! (see `src/settings/mods.rs`), and hands them to the shader every frame as a uniform
//! struct named `settings` (`settings.bar_count`, …). See `docs/shaders.md`.

use std::collections::BTreeMap;

/// A shader's saved values by setting id, as JSON: numbers, `true`/`false`, a
/// choice's label, or a color as `"#rrggbb"`. Anything missing or out of range
/// falls back to the declared default.
pub type ShaderValues = BTreeMap<String, serde_json::Value>;

/// Choice option standing for "every custom warp" (backgrounds only).
pub const WARPS: &str = "warps";

#[derive(Clone, Debug, PartialEq)]
pub enum Type {
    Int {
        min: i32,
        max: i32,
        step: i32,
    },
    Float {
        min: f32,
        max: f32,
        step: f32,
    },
    Toggle,
    /// Options in order; `warps` appends every custom warp's name.
    Choice {
        options: Vec<String>,
        warps: bool,
    },
    /// Colors offered as swatches (any color can be saved).
    Color {
        swatches: Vec<[u8; 3]>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Int(i32),
    Float(f32),
    Toggle(bool),
    /// The option's label.
    Choice(String),
    Color([u8; 3]),
}

/// When a setting shows: another setting is on (or off), or a choice is (or
/// isn't) one of some options; several joined by `or` or by `and`.
#[derive(Clone, Debug, PartialEq)]
pub enum Cond {
    Toggle {
        id: String,
        on: bool,
    },
    Choice {
        id: String,
        any_of: Vec<String>,
        negate: bool,
    },
    Any(Vec<Cond>),
    All(Vec<Cond>),
}

impl Cond {
    /// Every (setting, wants a toggle) it refers to.
    fn refs(&self) -> Vec<(&str, bool)> {
        match self {
            Cond::Toggle { id, .. } => vec![(id, true)],
            Cond::Choice { id, .. } => vec![(id, false)],
            Cond::Any(cs) | Cond::All(cs) => cs.iter().flat_map(Cond::refs).collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Decl {
    pub id: String,
    pub ty: Type,
    pub default: Value,
    pub label: String,
    /// Shown after the value ("px", "%").
    pub unit: Option<String>,
    pub cond: Option<Cond>,
    pub help: Option<String>,
    /// The `@section` it's under.
    pub section: Option<String>,
}

/// Everything a file declares.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Decls {
    pub items: Vec<Decl>,
    /// `@no-dim`: the global Dim doesn't apply (a background of a chosen color).
    pub no_dim: bool,
}

// ---------------------------------------------------------------------------
// Parsing

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Ident(String),
    Num(f64),
    Str(String),
    Hex([u8; 3]),
    Punct(&'static str),
}

fn tokens(s: &str) -> Result<Vec<Tok>, String> {
    let mut out = Vec::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == '"' {
            let start = i + 1;
            let end = chars[start..]
                .iter()
                .position(|&c| c == '"')
                .ok_or("a label is missing its closing quote")?;
            out.push(Tok::Str(chars[start..start + end].iter().collect()));
            i = start + end + 1;
        } else if c == '#' {
            let hex: String = chars[i + 1..]
                .iter()
                .take_while(|c| c.is_ascii_hexdigit())
                .collect();
            let rgb = crate::app::parse_hex(&hex)
                .filter(|_| hex.len() == 6)
                .ok_or_else(|| format!("`#{hex}` isn't a color like #4b7bf5"))?;
            out.push(Tok::Hex(rgb));
            i += 1 + hex.len();
        } else if c.is_ascii_digit()
            || (c == '-' && chars.get(i + 1).is_some_and(|d| d.is_ascii_digit()))
            || c == '.'
        {
            let start = i;
            i += 1;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            let text: String = chars[start..i].iter().collect();
            out.push(Tok::Num(
                text.parse()
                    .map_err(|_| format!("`{text}` isn't a number"))?,
            ));
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            out.push(Tok::Ident(chars[start..i].iter().collect()));
        } else if c == '!' && chars.get(i + 1) == Some(&'=') {
            out.push(Tok::Punct("!="));
            i += 2;
        } else {
            let p = match c {
                ':' => ":",
                '=' => "=",
                '(' => "(",
                ')' => ")",
                ',' => ",",
                '|' => "|",
                '!' => "!",
                _ => return Err(format!("unexpected `{c}`")),
            };
            out.push(Tok::Punct(p));
            i += 1;
        }
    }
    Ok(out)
}

struct Cursor {
    toks: Vec<Tok>,
    at: usize,
}

impl Cursor {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.at)
    }
    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.at).cloned();
        self.at += 1;
        t
    }
    fn punct(&mut self, p: &str) -> Result<(), String> {
        match self.next() {
            Some(Tok::Punct(q)) if q == p => Ok(()),
            _ => Err(format!("expected `{p}`")),
        }
    }
    fn eat(&mut self, p: &str) -> bool {
        if self.peek()
            == Some(&Tok::Punct(match p {
                "," => ",",
                "|" => "|",
                "!" => "!",
                _ => return false,
            }))
        {
            self.at += 1;
            true
        } else {
            false
        }
    }
    fn ident(&mut self) -> Result<String, String> {
        match self.next() {
            Some(Tok::Ident(s)) => Ok(s),
            _ => Err("expected a name".into()),
        }
    }
    fn num(&mut self) -> Result<f64, String> {
        match self.next() {
            Some(Tok::Num(n)) => Ok(n),
            _ => Err("expected a number".into()),
        }
    }
    fn string(&mut self) -> Result<String, String> {
        match self.next() {
            Some(Tok::Str(s)) => Ok(s),
            _ => Err("expected a label in quotes".into()),
        }
    }
}

fn is_identifier(s: &str) -> bool {
    s.chars()
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c == '_')
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn parse_type(c: &mut Cursor) -> Result<Type, String> {
    let name = c.ident()?;
    let args = |c: &mut Cursor| -> Result<Vec<f64>, String> {
        c.punct("(")?;
        let mut v = vec![c.num()?];
        while c.eat(",") {
            v.push(c.num()?);
        }
        c.punct(")")?;
        Ok(v)
    };
    match name.as_str() {
        "int" => {
            let a = args(c)?;
            let (min, max) = (a[0] as i32, *a.get(1).ok_or("int needs (min, max)")? as i32);
            let step = a.get(2).map_or(1, |s| *s as i32).max(1);
            if min >= max {
                return Err("the minimum must be below the maximum".into());
            }
            Ok(Type::Int { min, max, step })
        }
        "float" => {
            let a = args(c)?;
            let (min, max) = (
                a[0] as f32,
                *a.get(1).ok_or("float needs (min, max)")? as f32,
            );
            let step = a.get(2).map_or((max - min) / 100.0, |s| *s as f32);
            if min >= max || step <= 0.0 {
                return Err("the minimum must be below the maximum".into());
            }
            Ok(Type::Float { min, max, step })
        }
        "toggle" => Ok(Type::Toggle),
        "choice" => {
            c.punct("(")?;
            let mut options = Vec::new();
            let mut warps = false;
            loop {
                match c.next() {
                    Some(Tok::Str(s)) => options.push(s),
                    Some(Tok::Ident(w)) if w == WARPS => warps = true,
                    _ => return Err("choice options are labels in quotes".into()),
                }
                if !c.eat(",") {
                    break;
                }
            }
            c.punct(")")?;
            if options.is_empty() {
                return Err("a choice needs at least one option".into());
            }
            Ok(Type::Choice { options, warps })
        }
        "color" => {
            let mut swatches = Vec::new();
            if c.peek() == Some(&Tok::Punct("(")) {
                c.punct("(")?;
                loop {
                    match c.next() {
                        Some(Tok::Hex(rgb)) => swatches.push(rgb),
                        _ => return Err("color swatches are like #4b7bf5".into()),
                    }
                    if !c.eat(",") {
                        break;
                    }
                }
                c.punct(")")?;
            }
            Ok(Type::Color { swatches })
        }
        other => Err(format!(
            "unknown kind `{other}` (use int, float, toggle, choice or color)"
        )),
    }
}

fn parse_default(c: &mut Cursor, ty: &Type) -> Result<Value, String> {
    let bad = || "the default doesn't fit the kind".to_string();
    Ok(match (ty, c.next()) {
        (Type::Int { min, max, .. }, Some(Tok::Num(n))) => Value::Int((n as i32).clamp(*min, *max)),
        (Type::Float { min, max, .. }, Some(Tok::Num(n))) => {
            Value::Float((n as f32).clamp(*min, *max))
        }
        (Type::Toggle, Some(Tok::Ident(b))) if b == "true" || b == "false" => {
            Value::Toggle(b == "true")
        }
        (Type::Choice { options, .. }, Some(Tok::Str(s))) => {
            if !options.contains(&s) {
                return Err(format!("\"{s}\" isn't one of the options"));
            }
            Value::Choice(s)
        }
        (Type::Color { .. }, Some(Tok::Hex(rgb))) => Value::Color(rgb),
        _ => return Err(bad()),
    })
}

/// One condition, or several joined by `or` (or by `and`; not both).
fn parse_cond(c: &mut Cursor) -> Result<Cond, String> {
    let first = parse_clause(c)?;
    let mut rest = Vec::new();
    let mut joiner: Option<String> = None;
    while let Some(Tok::Ident(w)) = c.peek() {
        let w = w.clone();
        if w != "or" && w != "and" {
            break;
        }
        if joiner.as_ref().is_some_and(|j| *j != w) {
            return Err("use either `or` or `and` in one condition, not both".into());
        }
        c.next();
        rest.push(parse_clause(c)?);
        joiner = Some(w);
    }
    Ok(match joiner.as_deref() {
        None => first,
        Some("or") => Cond::Any(std::iter::once(first).chain(rest).collect()),
        _ => Cond::All(std::iter::once(first).chain(rest).collect()),
    })
}

fn parse_clause(c: &mut Cursor) -> Result<Cond, String> {
    let negated = c.eat("!");
    let id = c.ident()?;
    match c.peek() {
        Some(Tok::Punct(p @ ("=" | "!="))) => {
            if negated {
                return Err("use `!=` to compare".into());
            }
            let negate = *p == "!=";
            c.next();
            let mut any_of = vec![c.string()?];
            while c.eat("|") {
                any_of.push(c.string()?);
            }
            Ok(Cond::Choice { id, any_of, negate })
        }
        _ => Ok(Cond::Toggle { id, on: !negated }),
    }
}

/// `id: TYPE = DEFAULT "Label" ["unit"] [if COND]`
fn parse_setting(rest: &str, section: &Option<String>) -> Result<Decl, String> {
    let mut c = Cursor {
        toks: tokens(rest)?,
        at: 0,
    };
    let id = c.ident()?;
    if !is_identifier(&id) || id.starts_with("undavi") {
        return Err(format!(
            "`{id}` can't be a setting name (lowercase letters, digits and _)"
        ));
    }
    c.punct(":")?;
    let ty = parse_type(&mut c)?;
    c.punct("=")?;
    let default = parse_default(&mut c, &ty)?;
    let label = c.string()?;
    let unit = match c.peek() {
        Some(Tok::Str(_)) => Some(c.string()?),
        _ => None,
    };
    let cond = match c.next() {
        None => None,
        Some(Tok::Ident(w)) if w == "if" => Some(parse_cond(&mut c)?),
        Some(_) => return Err("expected `if` or the end of the line".into()),
    };
    if c.peek().is_some() {
        return Err("unexpected text at the end".into());
    }
    Ok(Decl {
        id,
        ty,
        default,
        label,
        unit,
        cond,
        help: None,
        section: section.clone(),
    })
}

/// Read every `// @…` line of `src`. Errors name the file and line.
pub fn parse(src: &str, file: &str) -> Result<Decls, String> {
    let mut decls = Decls::default();
    let mut section = None;
    for (n, line) in src.lines().enumerate() {
        let at = |e: String| format!("{file}:{}: {e}", n + 1);
        let Some(rest) = line.trim_start().strip_prefix("//") else {
            continue;
        };
        let Some(rest) = rest.trim_start().strip_prefix('@') else {
            continue;
        };
        let (word, rest) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        let rest = rest.trim();
        match word {
            "section" => section = Some(rest.to_string()),
            "setting" => {
                let decl = parse_setting(rest, &section).map_err(at)?;
                if decls.items.iter().any(|d| d.id == decl.id) {
                    return Err(at(format!("`{}` is declared twice", decl.id)));
                }
                decls.items.push(decl);
            }
            "help" => match decls.items.last_mut() {
                Some(d) => d.help = Some(rest.to_string()),
                None => return Err(at("`@help` goes after the `@setting` it explains".into())),
            },
            "no-dim" => decls.no_dim = true,
            other => return Err(at(format!("unknown `@{other}`"))),
        }
    }
    // Conditions must name settings declared in the file, of the right kind.
    for d in &decls.items {
        let Some(cond) = &d.cond else { continue };
        for (id, want_toggle) in cond.refs() {
            match decls.items.iter().find(|o| o.id == id) {
                Some(o) if matches!(o.ty, Type::Toggle) == want_toggle => {}
                Some(_) => {
                    return Err(format!(
                        "{file}: `{}` shows `if {id}`, but `{id}` is the other kind of setting",
                        d.id
                    ));
                }
                None => {
                    return Err(format!(
                        "{file}: `{}` shows `if {id}`, but there's no `{id}`",
                        d.id
                    ));
                }
            }
        }
    }
    Ok(decls)
}

// ---------------------------------------------------------------------------
// Values

impl Decl {
    /// The choice's options, with every custom warp after them when it asks.
    pub fn options(&self, warps: &[String]) -> Vec<String> {
        match &self.ty {
            Type::Choice { options, warps: w } => {
                let mut all = options.clone();
                if *w {
                    all.extend(warps.iter().cloned());
                }
                all
            }
            _ => Vec::new(),
        }
    }

    /// A saved value read as this setting, kept in range; `None` if it doesn't fit.
    pub fn read(&self, json: &serde_json::Value, warps: &[String]) -> Option<Value> {
        match &self.ty {
            Type::Int { min, max, step } => {
                let v = json.as_f64()?.round() as i32;
                let v = min + ((v - min) / step) * step;
                Some(Value::Int(v.clamp(*min, *max)))
            }
            Type::Float { min, max, .. } => {
                Some(Value::Float((json.as_f64()? as f32).clamp(*min, *max)))
            }
            Type::Toggle => json.as_bool().map(Value::Toggle),
            Type::Choice { .. } => {
                let s = json.as_str()?;
                self.options(warps)
                    .iter()
                    .any(|o| o == s)
                    .then(|| Value::Choice(s.to_string()))
            }
            Type::Color { .. } => {
                let s = json.as_str()?.strip_prefix('#')?;
                crate::app::parse_hex(s)
                    .filter(|_| s.len() == 6)
                    .map(Value::Color)
            }
        }
    }
}

/// Every declared setting with its current value (saved, or the default).
#[derive(Clone, Debug, PartialEq)]
pub struct Resolved {
    pub decls: Decls,
    pub values: Vec<Value>,
    /// Custom warps' names, for choices that offer them.
    pub warps: Vec<String>,
}

impl Decls {
    pub fn resolve(&self, saved: Option<&ShaderValues>, warps: &[String]) -> Resolved {
        let values = self
            .items
            .iter()
            .map(|d| {
                saved
                    .and_then(|s| s.get(&d.id))
                    .and_then(|j| d.read(j, warps))
                    .unwrap_or_else(|| d.default.clone())
            })
            .collect();
        Resolved {
            decls: self.clone(),
            values,
            warps: warps.to_vec(),
        }
    }

    /// These settings followed by `other`'s (a custom warp's, for a background).
    pub fn merged(&self, other: &Decls, file: &str) -> Result<Decls, String> {
        let mut out = self.clone();
        for d in &other.items {
            if out.items.iter().any(|o| o.id == d.id) {
                return Err(format!(
                    "{file}: `{}` is already a setting of the background it animates; rename it",
                    d.id
                ));
            }
            out.items.push(d.clone());
        }
        Ok(out)
    }

    /// The WGSL for these settings: the `settings` uniform at `binding`, and a
    /// constant for each choice option (`CORNERS_PILL`, …).
    pub fn wgsl(&self, binding: u32) -> String {
        let mut s = String::from("struct UndaviSettings {\n");
        if self.items.is_empty() {
            s.push_str("    @size(16) undavi_none: i32,\n");
        }
        for d in &self.items {
            let ty = match d.ty {
                Type::Float { .. } => "f32",
                Type::Color { .. } => "vec3<f32>",
                _ => "i32",
            };
            s.push_str(&format!("    @size(16) {}: {ty},\n", d.id));
        }
        s.push_str("};\n");
        s.push_str(&format!(
            "@group(0) @binding({binding}) var<uniform> settings: UndaviSettings;\n"
        ));
        for d in &self.items {
            if let Type::Choice { options, .. } = &d.ty {
                for (i, o) in options.iter().enumerate() {
                    s.push_str(&format!("const {}: i32 = {i};\n", constant(&d.id, o)));
                }
            }
        }
        s
    }
}

/// `MIRROR_UP_DOWN` for the "Up/down" option of `mirror`.
pub fn constant(id: &str, option: &str) -> String {
    let mut name = id.to_ascii_uppercase();
    name.push('_');
    let mut last_underscore = true;
    for c in option.chars() {
        if c.is_ascii_alphanumeric() {
            name.push(c.to_ascii_uppercase());
            last_underscore = false;
        } else if !last_underscore {
            name.push('_');
            last_underscore = true;
        }
    }
    name.trim_end_matches('_').to_string()
}

impl Resolved {
    fn index(&self, id: &str) -> Option<usize> {
        self.decls.items.iter().position(|d| d.id == id)
    }

    pub fn get(&self, id: &str) -> Option<&Value> {
        self.index(id).map(|i| &self.values[i])
    }

    /// Reserved settings the app reads (bar count, mirror, glow, blur, …).
    pub fn int(&self, id: &str) -> Option<i32> {
        match self.get(id)? {
            Value::Int(v) => Some(*v),
            Value::Float(v) => Some(v.round() as i32),
            _ => None,
        }
    }
    pub fn float(&self, id: &str) -> Option<f32> {
        match self.get(id)? {
            Value::Float(v) => Some(*v),
            Value::Int(v) => Some(*v as f32),
            _ => None,
        }
    }
    pub fn toggle(&self, id: &str) -> Option<bool> {
        match self.get(id)? {
            Value::Toggle(v) => Some(*v),
            _ => None,
        }
    }
    pub fn choice(&self, id: &str) -> Option<&str> {
        match self.get(id)? {
            Value::Choice(s) => Some(s),
            _ => None,
        }
    }

    /// Whether setting `i` shows, given the others.
    pub fn visible(&self, i: usize) -> bool {
        self.decls.items[i]
            .cond
            .as_ref()
            .is_none_or(|c| self.holds(c))
    }

    fn holds(&self, cond: &Cond) -> bool {
        match cond {
            Cond::Toggle { id, on } => self.toggle(id) == Some(*on),
            Cond::Choice { id, any_of, negate } => {
                let hit = self
                    .choice(id)
                    .is_some_and(|v| any_of.iter().any(|o| o == v));
                hit != *negate
            }
            Cond::Any(cs) => cs.iter().any(|c| self.holds(c)),
            Cond::All(cs) => cs.iter().all(|c| self.holds(c)),
        }
    }

    /// The uniform bytes for `Decls::wgsl`'s struct: 16 bytes per setting.
    pub fn pack(&self) -> Vec<u8> {
        let mut out = vec![0u8; 16 * self.values.len().max(1)];
        for (i, (d, v)) in self.decls.items.iter().zip(&self.values).enumerate() {
            let at = &mut out[16 * i..16 * i + 16];
            match v {
                Value::Int(x) => at[..4].copy_from_slice(&x.to_ne_bytes()),
                Value::Float(x) => at[..4].copy_from_slice(&x.to_ne_bytes()),
                Value::Toggle(b) => at[..4].copy_from_slice(&i32::from(*b).to_ne_bytes()),
                Value::Choice(s) => {
                    let i = d
                        .options(&self.warps)
                        .iter()
                        .position(|o| o == s)
                        .unwrap_or(0) as i32;
                    at[..4].copy_from_slice(&i.to_ne_bytes());
                }
                Value::Color(c) => {
                    for (k, ch) in c.iter().enumerate() {
                        at[4 * k..4 * k + 4].copy_from_slice(&(*ch as f32 / 255.0).to_ne_bytes());
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
// @section Bars
// @setting bar_count: int(16, 96) = 43 "Bar count"
// @setting max_length: int(100, 1200, 10) = 400 "Max length" "px"
// @setting corners: choice("Square", "Rounded", "Pill") = "Square" "Corners"
// @section Mirror
// @setting mirror: choice("Off", "Up/down", "Left/right", "Both") = "Up/down" "Mirror"
// @setting split: choice("Off", "Stereo", "Mid/Side") = "Stereo" "Split" if mirror = "Left/right" | "Both"
// @setting segments: toggle = false "Segments"
// @help Blocks, like an LED meter.
// @setting segment_size: int(2, 16) = 6 "Segment size" "px" if segments
// @setting speed: float(0, 3, 0.1) = 1 "Speed"
// @setting tint: color(#ff8800, #4b7bf5) = #ff8800 "Tint"
fn visualizer(uv: vec2<f32>, t: f32) -> vec4<f32> { return vec4<f32>(0.0); }
"#;

    #[test]
    fn parses_every_kind() {
        let d = parse(SAMPLE, "x.wgsl").unwrap();
        assert_eq!(d.items.len(), 9);
        let max = &d.items[1];
        assert_eq!(
            max.ty,
            Type::Int {
                min: 100,
                max: 1200,
                step: 10
            }
        );
        assert_eq!(max.unit.as_deref(), Some("px"));
        assert_eq!(max.section.as_deref(), Some("Bars"));
        assert_eq!(d.items[3].section.as_deref(), Some("Mirror"));
        assert_eq!(
            d.items[4].cond,
            Some(Cond::Choice {
                id: "mirror".into(),
                any_of: vec!["Left/right".into(), "Both".into()],
                negate: false
            })
        );
        assert_eq!(
            d.items[5].help.as_deref(),
            Some("Blocks, like an LED meter.")
        );
        assert_eq!(d.items[8].default, Value::Color([0xff, 0x88, 0x00]));
    }

    #[test]
    fn values_resolve_clamp_and_show() {
        let d = parse(SAMPLE, "x.wgsl").unwrap();
        let mut saved = ShaderValues::new();
        saved.insert("bar_count".into(), 500.into());
        saved.insert("max_length".into(), 415.into());
        saved.insert("corners".into(), "Nope".into());
        saved.insert("mirror".into(), "Both".into());
        let r = d.resolve(Some(&saved), &[]);
        assert_eq!(r.int("bar_count"), Some(96), "clamped");
        assert_eq!(r.int("max_length"), Some(410), "on a step");
        assert_eq!(r.choice("corners"), Some("Square"), "unknown: the default");
        assert!(r.visible(4), "split shows with Both");
        assert!(!r.visible(6), "segment size hides without segments");

        let d = parse(
            "// @setting mirror: choice(\"Off\", \"Both\") = \"Off\" \"M\"\n\
             // @setting split: choice(\"Off\", \"Stereo\") = \"Stereo\" \"S\"\n\
             // @setting channel: choice(\"Mono\") = \"Mono\" \"C\" if mirror = \"Off\" or split = \"Off\"",
            "x.wgsl",
        )
        .unwrap();
        assert!(d.resolve(None, &[]).visible(2), "mirror off: channel shows");
        let mut s = ShaderValues::new();
        s.insert("mirror".into(), "Both".into());
        assert!(
            !d.resolve(Some(&s), &[]).visible(2),
            "split by both: hidden"
        );
        let bytes = r.pack();
        assert_eq!(bytes.len(), 16 * 9);
        assert_eq!(
            i32::from_ne_bytes(bytes[48..52].try_into().unwrap()),
            3,
            "Both is option 3"
        );
    }

    #[test]
    fn makes_wgsl_with_constants() {
        let d = parse(SAMPLE, "x.wgsl").unwrap();
        let w = d.wgsl(3);
        assert!(w.contains("@size(16) bar_count: i32,"));
        assert!(w.contains("@size(16) speed: f32,"));
        assert!(w.contains("@size(16) tint: vec3<f32>,"));
        assert!(w.contains("var<uniform> settings: UndaviSettings;"));
        assert!(w.contains("const MIRROR_UP_DOWN: i32 = 1;"));
        assert!(w.contains("const SPLIT_MID_SIDE: i32 = 2;"));
        assert!(Decls::default().wgsl(3).contains("undavi_none"));
    }

    #[test]
    fn mistakes_name_the_line() {
        let bad = |src: &str| parse(src, "x.wgsl").unwrap_err();
        assert!(bad("\n// @setting n: range(1, 2) = 1 \"N\"").starts_with("x.wgsl:2:"));
        assert!(bad("// @setting n: int(1, 9) = 3").contains("label"));
        assert!(bad("// @setting c: choice(\"A\") = \"B\" \"C\"").contains("isn't one of"));
        assert!(bad("// @setting a: int(0, 1) = 0 \"A\" if nothing").contains("no `nothing`"));
        assert!(bad("// @frobnicate").contains("unknown `@frobnicate`"));
        assert!(bad("// @setting Big: toggle = true \"B\"").contains("can't be a setting name"));
        // Ordinary comments are left alone.
        assert_eq!(
            parse("// just a comment\n// email me @ home", "x.wgsl")
                .unwrap()
                .items
                .len(),
            0
        );
    }
}

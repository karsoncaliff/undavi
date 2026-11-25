//! Lyrics: lookup, parsing and timing.
//!
//! Sources, in order: a `.lrc` file next to a local song, then LRCLIB (lrclib.net).
//! Lyrics are fetched on demand for the current song, kept in memory for the
//! session only, and never written to disk or redistributed.
//!
//! [`LyricsLoader`] does the lookup on a background thread. The parsers
//! ([`parse_lrc`], [`parse_lyricsfile`]) are usable on their own; times are in
//! seconds from the start of the song.
//!
//! # Examples
//!
//! ```
//! use undavi::lyrics::{Entry, Lyrics, parse_lrc};
//!
//! let entries = parse_lrc("[00:12.00]First line\n[00:15.00]Second line");
//! // The 12-second intro before the first line becomes an interlude.
//! assert_eq!(entries[0], Entry::Interlude { start: 0.0, end: 12.0 });
//! let Entry::Line(first) = &entries[1] else { unreachable!() };
//! assert_eq!((first.text.as_str(), first.start, first.end), ("First line", 12.0, 15.0));
//! // At 13 s, the first line is the one being sung.
//! assert_eq!(Lyrics::active(&entries, 13.0), Some(1));
//! ```

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const LRCLIB: &str = "https://lrclib.net/api";
/// The biggest `.lrc` file read.
const MAX_LRC: u64 = 2 * 1024 * 1024;
/// A gap at least this long between lines shows the interlude dots.
const INTERLUDE_MIN_S: f32 = 3.5;
/// A search result's duration must be this close to the track's.
const DURATION_TOLERANCE_S: f64 = 3.0;

/// One word of a line, with when it's sung.
#[derive(Clone, Debug, PartialEq)]
pub struct Word {
    /// Includes its trailing space, if any.
    pub text: String,
    /// When the word starts, in seconds.
    pub start: f32,
    /// When the word ends, in seconds.
    pub end: f32,
}

/// A sung line.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    /// When the line starts, in seconds.
    pub start: f32,
    /// When the line ends (the next line's start, or a guess for the last one).
    pub end: f32,
    /// The whole line's text.
    pub text: String,
    /// The line split into words (concatenated, they give `text`).
    pub words: Vec<Word>,
    /// Word times came from the source. Otherwise every word carries the line's
    /// times and the whole line lights up together.
    pub word_synced: bool,
}

/// What the view scrolls through: sung lines and instrumental breaks.
#[derive(Clone, Debug, PartialEq)]
pub enum Entry {
    /// A sung line.
    Line(Line),
    /// An instrumental gap between lines (shown as dots).
    Interlude {
        /// When the gap starts, in seconds.
        start: f32,
        /// When the next line starts, in seconds.
        end: f32,
    },
}

impl Entry {
    /// When this entry starts, in seconds.
    pub fn start(&self) -> f32 {
        match self {
            Entry::Line(l) => l.start,
            Entry::Interlude { start, .. } => *start,
        }
    }
}

/// Where lyrics came from (the app credits the source).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// The LRCLIB database (lrclib.net).
    Lrclib,
    /// A `.lrc` file next to the song.
    File,
}

/// Lyrics for one song.
#[derive(Clone, Debug, PartialEq)]
pub enum Lyrics {
    /// Time-synced lines, in order.
    Synced {
        /// Lines and interludes, sorted by start time.
        entries: Vec<Entry>,
        /// Where they came from.
        source: Source,
    },
    /// Text without timing.
    Plain {
        /// The lines, in order.
        lines: Vec<String>,
        /// Where they came from.
        source: Source,
    },
    /// The song is known to have no lyrics.
    Instrumental,
}

impl Lyrics {
    /// Index of the entry playing at `t` (the last one that has started), if any.
    pub fn active(entries: &[Entry], t: f32) -> Option<usize> {
        let n = entries.partition_point(|e| e.start() <= t);
        n.checked_sub(1)
    }
}

// ---------------------------------------------------------------------------
// Timing helpers

/// Words for a line without word timing. They all share the line's times, so
/// the whole line lights up at once instead of guessing where each word falls.
fn whole_line_words(text: &str, start: f32, end: f32) -> Vec<Word> {
    split_words(text)
        .into_iter()
        .map(|text| Word { text, start, end })
        .collect()
}

/// Words keeping their trailing whitespace ("I ", "been ", "here").
pub fn split_words(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for ch in text.trim().chars() {
        if ch.is_whitespace() {
            if !cur.is_empty() && !cur.ends_with(' ') {
                cur.push(' ');
            }
        } else {
            if cur.ends_with(' ') {
                out.push(std::mem::take(&mut cur));
            }
            cur.push(ch);
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Fill in line and word ends, drop empty lines, and add interludes.
/// In LRC, an empty line marks where the previous line ends.
fn finish(mut lines: Vec<Line>, has_real_ends: bool) -> Vec<Entry> {
    lines.sort_by(|a, b| a.start.total_cmp(&b.start));
    let next: Vec<(f32, bool)> = (0..lines.len())
        .map(|i| match lines.get(i + 1) {
            Some(n) => (n.start, n.text.trim().is_empty()),
            None => (f32::INFINITY, false),
        })
        .collect();

    for (line, &(next_start, next_is_marker)) in lines.iter_mut().zip(&next) {
        if line.text.trim().is_empty() {
            continue;
        }
        if !has_real_ends || line.end <= line.start {
            // LRC only has start times: assume the line is sung for a length-based
            // stretch, so a long pause before the next line can show as a break.
            let chars = line.text.chars().count() as f32;
            let likely = 4.0f32.max(chars * 0.15);
            line.end = if next_is_marker {
                next_start
            } else if next_start - line.start > likely + INTERLUDE_MIN_S {
                line.start + likely
            } else {
                next_start.min(line.start + 30.0)
            };
        }
        line.end = line.end.min(next_start);
        if line.words.is_empty() {
            line.words = whole_line_words(&line.text, line.start, line.end);
            line.word_synced = false;
        } else {
            // Each word ends where the next begins; the last ends with the line.
            let line_end = line.end;
            let n = line.words.len();
            for k in 0..n {
                let end = if k + 1 < n {
                    line.words[k + 1].start
                } else {
                    line_end
                };
                line.words[k].end = end.max(line.words[k].start + 0.05);
            }
        }
    }
    lines.retain(|l| !l.text.trim().is_empty());
    let count = lines.len();

    let mut entries = Vec::with_capacity(count + 4);
    let mut prev_end = 0.0f32;
    for line in lines {
        if line.start - prev_end >= INTERLUDE_MIN_S {
            entries.push(Entry::Interlude {
                start: prev_end,
                end: line.start,
            });
        }
        prev_end = line.end;
        entries.push(Entry::Line(line));
    }
    entries
}

// ---------------------------------------------------------------------------
// LRC

fn parse_timestamp(s: &str) -> Option<f32> {
    // mm:ss, mm:ss.xx or mm:ss:xx
    let s = s.trim();
    let (min, rest) = s.split_once(':')?;
    let min: f32 = min.parse().ok()?;
    let rest = rest.replacen(':', ".", 1);
    let sec: f32 = rest.parse().ok()?;
    Some(min * 60.0 + sec)
}

/// Parses LRC, including multiple timestamps per line and `<mm:ss.xx>` word tags.
pub fn parse_lrc(text: &str) -> Vec<Entry> {
    let mut lines = Vec::new();
    for raw in text.lines() {
        let mut rest = raw.trim();
        let mut stamps = Vec::new();
        while let Some(stripped) = rest.strip_prefix('[') {
            let Some(close) = stripped.find(']') else {
                break;
            };
            match parse_timestamp(&stripped[..close]) {
                Some(t) => stamps.push(t),
                None => {
                    // A metadata tag like [ar:Artist]; not a lyric line.
                    stamps.clear();
                    break;
                }
            }
            rest = stripped[close + 1..].trim_start();
        }
        if stamps.is_empty() {
            continue;
        }

        // Enhanced LRC: <00:12.34>word <00:12.80>word
        let mut words = Vec::new();
        let mut plain = String::new();
        if rest.contains('<') {
            let mut chunk = rest;
            while let Some(open) = chunk.find('<') {
                plain.push_str(&chunk[..open]);
                if let Some(w) = words.last_mut() {
                    let w: &mut Word = w;
                    w.text.push_str(&chunk[..open]);
                }
                let Some(close) = chunk[open..].find('>') else {
                    break;
                };
                let tag = &chunk[open + 1..open + close];
                chunk = &chunk[open + close + 1..];
                if let Some(t) = parse_timestamp(tag) {
                    words.push(Word {
                        text: String::new(),
                        start: t,
                        end: t,
                    });
                }
            }
            plain.push_str(chunk);
            if let Some(w) = words.last_mut() {
                w.text.push_str(chunk);
            }
            words.retain(|w| !w.text.trim().is_empty());
        } else {
            plain = rest.to_string();
        }
        let text = plain.split_whitespace().collect::<Vec<_>>().join(" ");

        for &start in &stamps {
            let word_synced = !words.is_empty() && stamps.len() == 1;
            lines.push(Line {
                start,
                end: start,
                text: text.clone(),
                words: if word_synced {
                    words.clone()
                } else {
                    Vec::new()
                },
                word_synced,
            });
        }
    }
    finish(lines, false)
}

// ---------------------------------------------------------------------------
// LRCLIB "lyricsfile" (a small YAML document with per-word times)

fn yaml_scalar(raw: &str) -> String {
    let raw = raw.trim();
    if let Some(inner) = raw.strip_prefix('\'').and_then(|r| r.strip_suffix('\'')) {
        return inner.replace("''", "'");
    }
    if let Some(inner) = raw.strip_prefix('"').and_then(|r| r.strip_suffix('"')) {
        let mut out = String::new();
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                match chars.next() {
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some(other) => out.push(other),
                    None => {}
                }
            } else {
                out.push(c);
            }
        }
        return out;
    }
    raw.to_string()
}

/// True while a quoted YAML scalar hasn't been closed yet.
fn quote_open(raw: &str) -> bool {
    let raw = raw.trim();
    match raw.chars().next() {
        Some('\'') => {
            let body = &raw[1..];
            // '' is an escaped quote; a lone ' closes.
            body.replace("''", "").matches('\'').count() == 0
        }
        Some('"') => {
            let body = &raw[1..];
            let mut escaped = false;
            for c in body.chars() {
                match (escaped, c) {
                    (true, _) => escaped = false,
                    (false, '\\') => escaped = true,
                    (false, '"') => return false,
                    _ => {}
                }
            }
            true
        }
        _ => false,
    }
}

/// Parses the `lines:` section of an LRCLIB lyricsfile. Returns `None` if it has no lines.
pub fn parse_lyricsfile(text: &str) -> Option<Vec<Entry>> {
    #[derive(PartialEq)]
    enum Target {
        Line,
        Word,
    }
    let mut lines: Vec<Line> = Vec::new();
    let mut in_lines = false;
    // A multi-line value being collected: (target, key, raw text so far, indent).
    let mut pending: Option<(Target, String, String, usize)> = None;

    let apply = |lines: &mut Vec<Line>, target: Target, key: &str, raw: &str| {
        let Some(line) = lines.last_mut() else { return };
        match (target, key) {
            (Target::Line, "text") => line.text = yaml_scalar(raw),
            (Target::Line, "start_ms") => {
                line.start = raw.trim().parse::<f32>().unwrap_or(0.0) / 1000.0
            }
            (Target::Line, "end_ms") => {
                line.end = raw.trim().parse::<f32>().unwrap_or(0.0) / 1000.0
            }
            (Target::Word, "text") => {
                if let Some(w) = line.words.last_mut() {
                    w.text = yaml_scalar(raw);
                }
            }
            (Target::Word, "start_ms") => {
                if let Some(w) = line.words.last_mut() {
                    w.start = raw.trim().parse::<f32>().unwrap_or(0.0) / 1000.0;
                }
            }
            _ => {}
        }
    };

    for raw_line in text.lines() {
        let indent = raw_line.len() - raw_line.trim_start().len();
        let trimmed = raw_line.trim();

        if let Some((target, key, mut acc, key_indent)) = pending.take() {
            let continues =
                indent > key_indent && !trimmed.starts_with("- ") && !trimmed.contains(": ")
                    || quote_open(&acc);
            if continues && !trimmed.is_empty() {
                acc.push(' ');
                acc.push_str(trimmed);
                if quote_open(&acc) || indent > key_indent {
                    pending = Some((target, key, acc, key_indent));
                    continue;
                }
                apply(&mut lines, target, &key, &acc);
                continue;
            }
            apply(&mut lines, target, &key, &acc);
        }

        if indent == 0 {
            in_lines = trimmed == "lines:" || (in_lines && trimmed.starts_with("- "));
            if !in_lines {
                continue;
            }
        }
        if !in_lines || trimmed.is_empty() || trimmed == "lines:" {
            continue;
        }

        // Item starts: "- key: value" at indent 0 (a line) or 2 (a word).
        let (item_start, body, key_indent) = match trimmed.strip_prefix("- ") {
            Some(b) => (true, b, indent + 2),
            None => (false, trimmed, indent),
        };
        let target = if key_indent >= 4 {
            Target::Word
        } else {
            Target::Line
        };
        if item_start {
            match target {
                Target::Line => lines.push(Line {
                    start: 0.0,
                    end: 0.0,
                    text: String::new(),
                    words: Vec::new(),
                    word_synced: false,
                }),
                Target::Word => {
                    if let Some(l) = lines.last_mut() {
                        l.words.push(Word {
                            text: String::new(),
                            start: 0.0,
                            end: 0.0,
                        });
                        l.word_synced = true;
                    }
                }
            }
        }
        if let Some((key, value)) = body.split_once(':') {
            let value = value.trim_start();
            if value.is_empty() {
                continue; // "words:" and other block headers
            }
            pending = Some((
                target,
                key.trim().to_string(),
                value.to_string(),
                key_indent,
            ));
        }
    }
    if let Some((target, key, acc, _)) = pending {
        apply(&mut lines, target, &key, &acc);
    }

    // Drop words that came out empty; a line without words lights up as a whole.
    for l in &mut lines {
        l.words.retain(|w| !w.text.trim().is_empty());
        l.word_synced = !l.words.is_empty();
    }
    if lines.is_empty() {
        return None;
    }
    Some(finish(lines, true))
}

// ---------------------------------------------------------------------------
// Lookup

/// The song to find lyrics for.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Song {
    /// Track title.
    pub title: String,
    /// Artist names.
    pub artist: String,
    /// Album title (may be empty).
    pub album: String,
    /// Whole seconds; 0 when unknown.
    pub duration_s: u32,
    /// `file://` URL for local songs.
    pub url: Option<String>,
}

/// Where a lookup stands.
#[derive(Clone, Debug)]
pub enum Status {
    /// Still looking.
    Loading,
    /// Found these lyrics.
    Found(Arc<Lyrics>),
    /// Nothing found, the lookup failed, or no track is wanted.
    NotFound,
}

struct Shared {
    track: Option<Song>,
    status: Status,
}

/// Looks up lyrics on a background thread, one song at a time.
///
/// Tell it the current song with [`LyricsLoader::want`] every frame (only changes
/// cost anything) and read the result with [`LyricsLoader::status`]. Results are
/// cached in memory for the session.
pub struct LyricsLoader {
    shared: Arc<Mutex<Shared>>,
    requests: Sender<Request>,
    wanted: Option<Song>,
}

/// The song to look up (or none).
struct Request {
    song: Option<Song>,
    /// Look again, whatever was found before.
    again: bool,
}

impl LyricsLoader {
    /// Start the lookup thread.
    pub fn start() -> Self {
        let shared = Arc::new(Mutex::new(Shared {
            track: None,
            status: Status::NotFound,
        }));
        let (tx, rx) = mpsc::channel();
        let worker_shared = shared.clone();
        let _ = std::thread::Builder::new()
            .name("undavi-lyrics".into())
            .spawn(move || worker(rx, worker_shared));
        Self {
            shared,
            requests: tx,
            wanted: None,
        }
    }

    /// Ask for lyrics for `track` (or none). Only changes are sent to the worker.
    pub fn want(&mut self, track: Option<Song>) {
        if self.wanted != track {
            self.wanted = track.clone();
            let _ = self.requests.send(Request {
                song: track,
                again: false,
            });
        }
    }

    /// Look for the wanted track's lyrics again, even if there were none before.
    pub fn retry(&self) {
        let _ = self.requests.send(Request {
            song: self.wanted.clone(),
            again: true,
        });
    }

    /// Lyrics for the wanted track, if they've arrived.
    pub fn status(&self) -> Status {
        match self.shared.lock() {
            Ok(s) if s.track == self.wanted => s.status.clone(),
            Ok(_) if self.wanted.is_some() => Status::Loading,
            _ => Status::NotFound,
        }
    }
}

fn worker(rx: Receiver<Request>, shared: Arc<Mutex<Shared>>) {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .user_agent(crate::about::user_agent())
        .http_status_as_error(false)
        .build()
        .into();
    // Session-only cache so replaying a song doesn't refetch. Never written to disk.
    let mut cache: HashMap<Song, Option<Arc<Lyrics>>> = HashMap::new();

    while let Ok(mut request) = rx.recv() {
        // Skip tracks that were already replaced by a newer request.
        while let Ok(newer) = rx.try_recv() {
            let same = newer.song == request.song;
            request = Request {
                again: newer.again || (same && request.again),
                ..newer
            };
        }
        if let (true, Some(song)) = (request.again, &request.song) {
            cache.remove(song);
        }
        let Some(track) = request.song else {
            if let Ok(mut s) = shared.lock() {
                s.track = None;
                s.status = Status::NotFound;
            }
            continue;
        };
        if let Ok(mut s) = shared.lock() {
            s.track = Some(track.clone());
            s.status = Status::Loading;
        }
        let found = match cache.get(&track) {
            Some(hit) => hit.clone(),
            None => {
                let found = lookup(&agent, &track).map(Arc::new);
                cache.insert(track.clone(), found.clone());
                found
            }
        };
        if let Ok(mut s) = shared.lock()
            && s.track.as_ref() == Some(&track)
        {
            s.status = match found {
                Some(l) => Status::Found(l),
                None => Status::NotFound,
            };
        }
    }
}

fn lookup(agent: &ureq::Agent, track: &Song) -> Option<Lyrics> {
    if let Some(lyrics) = local_lrc(track) {
        return Some(lyrics);
    }
    lrclib(agent, track).unwrap_or_else(|e| {
        eprintln!("undavi: lyrics lookup failed: {e}");
        None
    })
}

/// `song.lrc` next to `song.flac`, for local players.
fn local_lrc(track: &Song) -> Option<Lyrics> {
    let path = track.url.as_deref()?.strip_prefix("file://")?;
    let path = PathBuf::from(crate::art::percent_decode(path)).with_extension("lrc");
    let bytes = crate::art::read_file(&path, MAX_LRC).ok()?;
    let entries = parse_lrc(&String::from_utf8(bytes).ok()?);
    (!entries.is_empty()).then_some(Lyrics::Synced {
        entries,
        source: Source::File,
    })
}

/// Strip decorations that keep exact matches from hitting: " - Remastered 2011", "(feat. X)".
fn clean_title(title: &str) -> String {
    let mut t = title.to_string();
    if let Some(i) = t.find(" - ") {
        let tail = t[i + 3..].to_lowercase();
        if [
            "remaster", "live", "version", "edit", "mono", "stereo", "mix",
        ]
        .iter()
        .any(|k| tail.contains(k))
        {
            t.truncate(i);
        }
    }
    for open in ['(', '['] {
        if let Some(i) = t.find(open) {
            let tail = t[i..].to_lowercase();
            if tail.contains("feat") || tail.contains("with ") || tail.contains("remaster") {
                t.truncate(i);
            }
        }
    }
    t.trim().to_string()
}

#[derive(serde::Deserialize)]
struct Record {
    #[serde(default)]
    duration: Option<f64>,
    #[serde(default)]
    instrumental: bool,
    #[serde(default, rename = "plainLyrics")]
    plain: Option<String>,
    #[serde(default, rename = "syncedLyrics")]
    synced: Option<String>,
    #[serde(default)]
    lyricsfile: Option<String>,
}

fn lrclib(agent: &ureq::Agent, track: &Song) -> Result<Option<Lyrics>, String> {
    let title = clean_title(&track.title);
    let artist = track
        .artist
        .split(',')
        .next()
        .unwrap_or("")
        .trim()
        .to_string();

    // Exact match first (needs the duration), then a search.
    if track.duration_s > 0 {
        let mut resp = agent
            .get(format!("{LRCLIB}/get"))
            .query("track_name", &title)
            .query("artist_name", &artist)
            .query("album_name", &track.album)
            .query("duration", track.duration_s.to_string())
            .call()
            .map_err(|e| e.to_string())?;
        if resp.status() == 200 {
            let record: Record = resp.body_mut().read_json().map_err(|e| e.to_string())?;
            if let Some(l) = to_lyrics(record) {
                return Ok(Some(l));
            }
        }
    }

    let mut resp = agent
        .get(format!("{LRCLIB}/search"))
        .query("track_name", &title)
        .query("artist_name", &artist)
        .call()
        .map_err(|e| e.to_string())?;
    if resp.status() != 200 {
        return Ok(None);
    }
    let records: Vec<Record> = resp.body_mut().read_json().map_err(|e| e.to_string())?;
    let close = |r: &Record| {
        track.duration_s == 0
            || r.duration
                .is_some_and(|d| (d - track.duration_s as f64).abs() <= DURATION_TOLERANCE_S)
    };
    // Prefer word-synced, then line-synced, then plain — among duration matches.
    let rank = |r: &Record| {
        if r.lyricsfile
            .as_deref()
            .is_some_and(|f| f.contains("words:"))
        {
            3
        } else if r.synced.as_deref().is_some_and(|s| !s.is_empty()) {
            2
        } else if r.plain.is_some() || r.instrumental {
            1
        } else {
            0
        }
    };
    let best = records.into_iter().filter(close).max_by_key(rank);
    Ok(best.and_then(to_lyrics))
}

fn to_lyrics(r: Record) -> Option<Lyrics> {
    if r.instrumental {
        return Some(Lyrics::Instrumental);
    }
    if let Some(entries) = r.lyricsfile.as_deref().and_then(parse_lyricsfile)
        && entries.iter().any(|e| matches!(e, Entry::Line(_)))
    {
        return Some(Lyrics::Synced {
            entries,
            source: Source::Lrclib,
        });
    }
    if let Some(synced) = r.synced.as_deref() {
        let entries = parse_lrc(synced);
        if !entries.is_empty() {
            return Some(Lyrics::Synced {
                entries,
                source: Source::Lrclib,
            });
        }
    }
    let plain = r.plain?;
    let lines: Vec<String> = plain.lines().map(str::to_string).collect();
    (!lines.iter().all(|l| l.trim().is_empty())).then_some(Lyrics::Plain {
        lines,
        source: Source::Lrclib,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_cleanup() {
        assert_eq!(clean_title("Song - Remastered 2011"), "Song");
        assert_eq!(clean_title("Song (feat. Someone)"), "Song");
        assert_eq!(clean_title("Love - Me Do"), "Love - Me Do");
    }
}

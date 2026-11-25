//! Scrolling, karaoke-style lyrics (in the spirit of Spicy Lyrics).
//!
//! - The current line is bright; its words fill left to right as they're sung and lift slightly.
//!   Lines without word timing light up all at once.
//! - Other lines are dimmer the further they are from the current one, fading at the edges.
//! - Instrumental breaks show three dots that fill up over the break.
//! - Scrolling is eased; clicking a line seeks to it.

use crate::lyrics::{Entry, Line, Lyrics, Word, split_words};
use crate::theme::{self, TEXT};
use iced::advanced::text::Paragraph as _;
use iced::alignment::Vertical;
use iced::mouse::{self, Cursor};
use iced::widget::canvas::{self, Action, Event, Frame, Geometry, Path};
use iced::widget::text::{Alignment, LineHeight, Shaping, Wrapping};
use iced::{Color, Pixels, Point, Rectangle, Renderer, Size, Theme};
use std::cell::{Cell, RefCell};
use std::time::Instant;

/// Unsung words on the current line, and the brightest other lines.
const UNSUNG_ALPHA: f32 = 0.42;
const NEAR_ALPHA: f32 = 0.32;
const FAR_ALPHA: f32 = 0.12;
/// How far sung words rise, as a fraction of the font size.
const LIFT: f32 = 0.06;
/// Line highlight fade in / out, in seconds.
const FADE_IN_S: f32 = 0.15;
const FADE_OUT_S: f32 = 0.3;
/// Scroll easing time constant, in seconds.
const SCROLL_TAU: f32 = 0.22;
/// Glow rings around lit characters: (radius as a fraction of the font size,
/// number of copies, opacity of each copy at full glow).
const GLOW_RINGS: [(f32, usize, f32); 2] = [(0.045, 8, 0.11), (0.10, 12, 0.045)];
/// Sung words keep this much glow while their line is current...
const AFTERGLOW: f32 = 0.3;
/// ...after the flash fades with this time constant, in seconds.
const AFTERGLOW_TAU: f32 = 0.5;

/// One piece of text to draw: content, position, opacity, glow strength (0..=1).
struct Piece<'s> {
    text: &'s str,
    at: Point,
    alpha: f32,
    glow: f32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Placement {
    /// A few centered lines above the bars.
    Strip,
    /// A tall, left-aligned column beside the bars.
    Column,
}

pub struct LyricsView<'a, M> {
    pub lyrics: &'a Lyrics,
    /// Lyrics time in seconds (playback position plus the user's offset).
    pub time: f32,
    /// Playback progress 0..=1, used to scroll unsynced lyrics.
    pub progress: Option<f32>,
    pub placement: Placement,
    pub font_size: f32,
    /// Strength of the glow on lit characters, 0..=1 (0 = off).
    pub glow: f32,
    /// Fill words as they're sung; `false` lights whole lines, ignoring word timing.
    pub word_sync: bool,
    pub on_seek: fn(f32) -> M,
}

#[derive(Default)]
pub struct State {
    layout: RefCell<Option<Laid>>,
    scroll: Cell<f32>,
    last_draw: Cell<Option<Instant>>,
}

/// Word positions, computed once per (lyrics, width, size).
struct Laid {
    key: (usize, u32, u32),
    items: Vec<Item>,
    total: f32,
}

struct Item {
    y: f32,
    height: f32,
    /// (word index, x, y within the item, width)
    words: Vec<(usize, f32, f32, f32)>,
    /// Text pieces for plain lyrics (synced lines use their `Line::words`).
    plain: Vec<String>,
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    // A zero-width or infinite edge (inf - inf is NaN) acts as a plain step.
    let span = e1 - e0;
    if span.is_nan() || span <= 0.0 {
        return if x >= e1 { 1.0 } else { 0.0 };
    }
    let t = ((x - e0) / span).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl<M> LyricsView<'_, M> {
    fn font(&self) -> iced::Font {
        theme::INTER_SEMIBOLD
    }

    fn row_height(&self) -> f32 {
        self.font_size * 1.3
    }

    fn gap(&self) -> f32 {
        match self.placement {
            Placement::Strip => self.font_size * 0.35,
            Placement::Column => self.font_size * 0.55,
        }
    }

    fn measure(&self, s: &str) -> f32 {
        let paragraph = <Renderer as iced::advanced::text::Renderer>::Paragraph::with_text(
            iced::advanced::Text {
                content: s,
                bounds: Size::INFINITE,
                size: Pixels(self.font_size),
                line_height: LineHeight::default(),
                font: self.font(),
                align_x: Alignment::Left,
                align_y: Vertical::Top,
                shaping: Shaping::Advanced,
                wrapping: Wrapping::None,
            },
        );
        paragraph.min_bounds().width
    }

    fn layout(&self, width: f32) -> Laid {
        let space = (self.measure("a b") - self.measure("ab")).max(self.font_size * 0.2);
        let row_h = self.row_height();
        let gap = self.gap();
        let center = self.placement == Placement::Strip;

        let place = |pieces: &[&str]| -> (Vec<(usize, f32, f32, f32)>, f32) {
            // Greedy wrap into rows.
            let mut rows: Vec<Vec<(usize, f32)>> = Vec::new();
            let mut row_w: Vec<f32> = Vec::new();
            let (mut row, mut cur) = (Vec::new(), 0.0);
            for (i, piece) in pieces.iter().enumerate() {
                let w = self.measure(piece.trim());
                let needed = if cur > 0.0 { cur + space + w } else { w };
                if needed > width && cur > 0.0 {
                    rows.push(std::mem::take(&mut row));
                    row_w.push(cur);
                    cur = w;
                } else {
                    cur = needed;
                }
                row.push((i, w));
            }
            rows.push(row);
            row_w.push(cur);
            let mut out = Vec::new();
            for (r, (row, w)) in rows.iter().zip(&row_w).enumerate() {
                let mut x = if center { (width - w) / 2.0 } else { 0.0 };
                for &(i, ww) in row {
                    out.push((i, x, r as f32 * row_h, ww));
                    x += ww + space;
                }
            }
            (out, rows.len() as f32 * row_h)
        };

        let mut items = Vec::new();
        let mut y = 0.0;
        match self.lyrics {
            Lyrics::Synced { entries, .. } => {
                for e in entries {
                    let (words, height) = match e {
                        Entry::Line(l) => {
                            let pieces: Vec<&str> =
                                l.words.iter().map(|w| w.text.as_str()).collect();
                            place(&pieces)
                        }
                        Entry::Interlude { .. } => (Vec::new(), row_h),
                    };
                    items.push(Item {
                        y,
                        height,
                        words,
                        plain: Vec::new(),
                    });
                    y += height + gap;
                }
            }
            Lyrics::Plain { lines, .. } => {
                for line in lines {
                    let plain = split_words(line);
                    let pieces: Vec<&str> = plain.iter().map(String::as_str).collect();
                    let (words, height) = if plain.is_empty() {
                        (Vec::new(), row_h * 0.5)
                    } else {
                        place(&pieces)
                    };
                    items.push(Item {
                        y,
                        height,
                        words,
                        plain,
                    });
                    y += height + gap;
                }
            }
            Lyrics::Instrumental => {}
        }
        Laid {
            key: (0, 0, 0),
            items,
            total: y,
        }
    }

    fn key(&self, width: f32) -> (usize, u32, u32) {
        (
            self.lyrics as *const Lyrics as usize,
            width.to_bits(),
            self.font_size.to_bits(),
        )
    }

    fn entries(&self) -> &[Entry] {
        match self.lyrics {
            Lyrics::Synced { entries, .. } => entries,
            _ => &[],
        }
    }

    /// Where the view should be scrolled to right now.
    fn target_scroll(&self, laid: &Laid, height: f32) -> f32 {
        match self.lyrics {
            Lyrics::Synced { entries, .. } => {
                let i = Lyrics::active(entries, self.time).unwrap_or(0);
                let Some(item) = laid.items.get(i) else {
                    return 0.0;
                };
                let anchor = match self.placement {
                    Placement::Strip => height / 2.0,
                    Placement::Column => height * 0.38,
                };
                item.y + item.height / 2.0 - anchor
            }
            _ => {
                let p = self.progress.unwrap_or(0.0);
                (laid.total - height * 0.6).max(0.0) * p - height * 0.15
            }
        }
    }

    /// How far `word` is filled (0..=1) and when it stops being sung. Without word
    /// timing, or with word sync turned off, the whole line lights up together (it
    /// still eases in with the line's activeness).
    fn word_fill(&self, line: &Line, word: &Word) -> (f32, f32) {
        if line.word_synced && self.word_sync {
            let p = ((self.time - word.start) / (word.end - word.start).max(0.01)).clamp(0.0, 1.0);
            (p, word.end)
        } else {
            (if self.time >= line.start { 1.0 } else { 0.0 }, line.end)
        }
    }

    /// How "lit" entry `i` is: 1 while it's the current one, easing in and out.
    fn activeness(&self, i: usize) -> f32 {
        let entries = self.entries();
        let Some(e) = entries.get(i) else { return 0.0 };
        let start = e.start();
        let until = entries
            .get(i + 1)
            .map(|n| n.start())
            .unwrap_or(f32::INFINITY);
        let t = self.time;
        // The last entry has no next one, so it stays lit.
        let fade_out = if until.is_finite() {
            1.0 - smoothstep(until, until + FADE_OUT_S, t)
        } else {
            1.0
        };
        smoothstep(start, start + FADE_IN_S, t) * fade_out
    }
}

impl<M> canvas::Program<M> for LyricsView<'_, M> {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &Event,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> Option<Action<M>> {
        let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event else {
            return None;
        };
        let p = cursor.position_in(bounds)?;
        let entries = self.entries();
        let layout = state.layout.borrow();
        let laid = layout.as_ref()?;
        let y = p.y + state.scroll.get();
        let i = laid
            .items
            .iter()
            .position(|it| y >= it.y && y <= it.y + it.height)?;
        match entries.get(i)? {
            Entry::Line(l) => Some(Action::publish((self.on_seek)(l.start)).and_capture()),
            Entry::Interlude { .. } => None,
        }
    }

    fn draw(
        &self,
        state: &State,
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let (w, h) = (bounds.width, bounds.height);

        // (Re)layout only when the lyrics, width or size change.
        let key = self.key(w);
        let fresh = state.layout.borrow().as_ref().is_none_or(|l| l.key != key);
        if fresh {
            let mut laid = self.layout(w);
            laid.key = key;
            *state.layout.borrow_mut() = Some(laid);
        }
        let layout = state.layout.borrow();
        let Some(laid) = layout.as_ref() else {
            return vec![frame.into_geometry()];
        };

        // Eased scroll toward the current line; jump on seeks and fresh layouts.
        let now = Instant::now();
        let dt = state
            .last_draw
            .get()
            .map(|t| (now - t).as_secs_f32())
            .unwrap_or(1.0)
            .min(0.25);
        state.last_draw.set(Some(now));
        let target = self.target_scroll(laid, h);
        let mut scroll = state.scroll.get();
        if fresh || (target - scroll).abs() > h * 2.5 {
            scroll = target;
        } else {
            scroll += (target - scroll) * (1.0 - (-dt / SCROLL_TAU).exp());
        }
        state.scroll.set(scroll);

        let size = self.font_size;
        let fade = h * if self.placement == Placement::Strip {
            0.3
        } else {
            0.2
        };
        let active = Lyrics::active(self.entries(), self.time);
        let text = |content: &str, at: Point, color: Color| canvas::Text {
            content: content.to_string(),
            position: at,
            color,
            size: Pixels(size),
            font: self.font(),
            align_x: Alignment::Left,
            align_y: Vertical::Top,
            shaping: Shaping::Advanced,
            ..canvas::Text::default()
        };

        for (i, item) in laid.items.iter().enumerate() {
            let top = item.y - scroll;
            if top > h || top + item.height < 0.0 {
                continue;
            }
            let mid = top + item.height / 2.0;
            let edge = smoothstep(0.0, fade, mid) * smoothstep(0.0, fade, h - mid);
            if edge <= 0.01 {
                continue;
            }
            let distance = active.map(|a| a.abs_diff(i)).unwrap_or(i + 1);
            let resting = match (self.placement, distance) {
                (_, 0) | (_, 1) => NEAR_ALPHA,
                (Placement::Strip, _) => NEAR_ALPHA * 0.6,
                (Placement::Column, d) => (NEAR_ALPHA * 0.8f32.powi(d as i32 - 1)).max(FAR_ALPHA),
            };

            match self.lyrics {
                Lyrics::Plain { .. } => {
                    for &(k, x, y, _) in &item.words {
                        let a = 0.7 * edge;
                        frame.fill_text(text(
                            item.plain[k].trim(),
                            Point::new(x, top + y),
                            Color { a, ..TEXT },
                        ));
                    }
                }
                Lyrics::Synced { entries, .. } => match &entries[i] {
                    Entry::Line(line) => {
                        let lit = self.activeness(i);
                        let mut pieces: Vec<Piece<'_>> = Vec::new();
                        for &(k, x, y, ww) in &item.words {
                            let word = &line.words[k];
                            let (p, end) = self.word_fill(line, word);
                            let label = word.text.trim();
                            let lift = LIFT * size * lit * (1.0 - (1.0 - p).powi(2));
                            let at = Point::new(x, top + y - lift);
                            let dim = edge * (resting + (UNSUNG_ALPHA - resting) * lit);
                            let bright = edge * (resting + (1.0 - resting) * lit);
                            // Full glow while being sung, then an afterglow that settles.
                            let since = self.time - end;
                            let word_glow = if since <= 0.0 {
                                1.0
                            } else {
                                AFTERGLOW + (1.0 - AFTERGLOW) * (-since / AFTERGLOW_TAU).exp()
                            } * lit
                                * edge;
                            if lit <= 0.001 || p <= 0.0 {
                                pieces.push(Piece {
                                    text: label,
                                    at,
                                    alpha: dim,
                                    glow: 0.0,
                                });
                            } else if p >= 1.0 {
                                pieces.push(Piece {
                                    text: label,
                                    at,
                                    alpha: bright,
                                    glow: word_glow,
                                });
                            } else {
                                // Karaoke fill. Canvas clipping doesn't apply to text, so the word
                                // being sung is drawn per character, brightening as the fill passes.
                                let fill_x = ww * p;
                                let bounds: Vec<usize> = label
                                    .char_indices()
                                    .map(|(b, _)| b)
                                    .chain(std::iter::once(label.len()))
                                    .collect();
                                let offsets: Vec<f32> = bounds
                                    .iter()
                                    .map(|&b| {
                                        if b == 0 {
                                            0.0
                                        } else {
                                            self.measure(&label[..b])
                                        }
                                    })
                                    .collect();
                                for k in 0..bounds.len() - 1 {
                                    let (x0, x1) = (offsets[k], offsets[k + 1]);
                                    let lit_part =
                                        ((fill_x - x0) / (x1 - x0).max(0.5)).clamp(0.0, 1.0);
                                    pieces.push(Piece {
                                        text: &label[bounds[k]..bounds[k + 1]],
                                        at: Point::new(at.x + x0, at.y),
                                        alpha: dim + (bright - dim) * lit_part,
                                        glow: word_glow * lit_part,
                                    });
                                }
                            }
                        }
                        // Glow first, underneath: soft copies in rings around each lit piece.
                        if self.glow > 0.0 {
                            for piece in pieces.iter().filter(|pc| pc.glow > 0.01) {
                                for (radius, copies, opacity) in GLOW_RINGS {
                                    let r = radius * size;
                                    let a = opacity * piece.glow * self.glow;
                                    for c in 0..copies {
                                        let angle =
                                            std::f32::consts::TAU * c as f32 / copies as f32;
                                        let at = Point::new(
                                            piece.at.x + r * angle.cos(),
                                            piece.at.y + r * angle.sin(),
                                        );
                                        frame.fill_text(text(piece.text, at, Color { a, ..TEXT }));
                                    }
                                }
                            }
                        }
                        for piece in &pieces {
                            frame.fill_text(text(
                                piece.text,
                                piece.at,
                                Color {
                                    a: piece.alpha,
                                    ..TEXT
                                },
                            ));
                        }
                    }
                    Entry::Interlude { start, end } => {
                        let lit = self.activeness(i);
                        if lit <= 0.01 {
                            continue;
                        }
                        let p = ((self.time - start) / (end - start).max(0.01)).clamp(0.0, 1.0);
                        let breathe = 1.0 + 0.08 * (std::f32::consts::TAU * self.time / 1.4).sin();
                        let closing = ((end - self.time) / 0.35).clamp(0.0, 1.0);
                        let r = size * 0.17 * breathe * closing;
                        let step = size * 0.55;
                        let x0 = match self.placement {
                            Placement::Strip => w / 2.0 - step,
                            Placement::Column => r.max(size * 0.17),
                        };
                        for k in 0..3 {
                            let fill = (p * 3.0 - k as f32).clamp(0.0, 1.0);
                            let a = edge * lit * (0.3 + 0.7 * fill);
                            frame.fill(
                                &Path::circle(Point::new(x0 + k as f32 * step, mid), r),
                                Color { a, ..TEXT },
                            );
                        }
                    }
                },
                Lyrics::Instrumental => {}
            }
        }

        // Credit the source in the tall layout.
        if self.placement == Placement::Column
            && let Lyrics::Synced {
                source: crate::lyrics::Source::Lrclib,
                ..
            }
            | Lyrics::Plain {
                source: crate::lyrics::Source::Lrclib,
                ..
            } = self.lyrics
        {
            frame.fill_text(canvas::Text {
                content: "Lyrics from LRCLIB".into(),
                position: Point::new(0.0, h - 14.0),
                color: Color { a: 0.35, ..TEXT },
                size: Pixels(11.0),
                font: theme::INTER,
                align_y: Vertical::Top,
                ..canvas::Text::default()
            });
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &State,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> mouse::Interaction {
        let Some(p) = cursor.position_in(bounds) else {
            return mouse::Interaction::default();
        };
        let layout = state.layout.borrow();
        let Some(laid) = layout.as_ref() else {
            return mouse::Interaction::default();
        };
        let y = p.y + state.scroll.get();
        let over_line = laid
            .items
            .iter()
            .zip(self.entries())
            .any(|(it, e)| matches!(e, Entry::Line(_)) && y >= it.y && y <= it.y + it.height);
        if over_line {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lyrics::parse_lrc;

    fn view(lyrics: &Lyrics, time: f32) -> LyricsView<'_, f32> {
        LyricsView {
            lyrics,
            time,
            progress: None,
            placement: Placement::Column,
            font_size: 28.0,
            glow: 0.5,
            word_sync: true,
            on_seek: |t| t,
        }
    }

    #[test]
    fn last_line_lights_up() {
        let lyrics = Lyrics::Synced {
            entries: parse_lrc("[00:01.00]first\n[00:03.00]second\n[00:05.00]last"),
            source: crate::lyrics::Source::Lrclib,
        };
        let last = view(&lyrics, 0.0).entries().len() - 1;
        let lit = |t: f32| view(&lyrics, t).activeness(last);
        assert_eq!(lit(4.0), 0.0);
        assert_eq!(lit(6.0), 1.0);
        assert_eq!(lit(500.0), 1.0);
        // Earlier lines still fade out when the next one starts.
        assert_eq!(view(&lyrics, 6.0).activeness(last - 1), 0.0);
    }

    #[test]
    fn word_sync_off_lights_whole_lines() {
        let entries = parse_lrc("[00:01.00]<00:01.00>one <00:02.00>two\n[00:04.00]next");
        let line = entries
            .iter()
            .find_map(|e| match e {
                Entry::Line(l) if l.word_synced => Some(l.clone()),
                _ => None,
            })
            .expect("a word-synced line");
        let lyrics = Lyrics::Synced {
            entries,
            source: crate::lyrics::Source::Lrclib,
        };
        let second = &line.words[1];
        // Word by word: halfway through "one", "two" hasn't started.
        let by_word = view(&lyrics, 1.5);
        assert_eq!(by_word.word_fill(&line, second).0, 0.0);
        // Line sync: every word is lit as soon as the line starts.
        let mut whole = view(&lyrics, 1.5);
        whole.word_sync = false;
        assert_eq!(whole.word_fill(&line, &line.words[0]), (1.0, line.end));
        assert_eq!(whole.word_fill(&line, second), (1.0, line.end));
    }

    #[test]
    fn smoothstep_never_returns_nan() {
        assert_eq!(smoothstep(f32::INFINITY, f32::INFINITY, 3.0), 0.0);
        assert_eq!(smoothstep(2.0, 2.0, 3.0), 1.0);
        assert_eq!(smoothstep(0.0, 1.0, 0.5), 0.5);
    }
}

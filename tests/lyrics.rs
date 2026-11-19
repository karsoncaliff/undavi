//! Parsing and timing of lyrics, through the public `undavi::lyrics` API.

use undavi::lyrics::{Entry, Line, Lyrics, parse_lrc, parse_lyricsfile};

fn lines(entries: &[Entry]) -> Vec<&Line> {
    entries
        .iter()
        .filter_map(|e| match e {
            Entry::Line(l) => Some(l),
            _ => None,
        })
        .collect()
}

#[test]
fn lrc_basics() {
    let lrc = "[ar:Someone]\n[00:12.00]First line here\n[00:15.50]Second\n[00:16.00]\n[00:30.00][01:00.00]Chorus";
    let entries = parse_lrc(lrc);
    let ls = lines(&entries);
    assert_eq!(ls.len(), 4);
    assert_eq!(ls[0].text, "First line here");
    assert_eq!(ls[0].start, 12.0);
    assert_eq!(ls[0].end, 15.5);
    // The empty [00:16.00] line ends "Second"; 14 s of silence follows.
    assert_eq!(ls[1].end, 16.0);
    assert_eq!(
        entries[0],
        Entry::Interlude {
            start: 0.0,
            end: 12.0
        }
    );
    assert!(
        entries
            .iter()
            .any(|e| matches!(e, Entry::Interlude { .. }) && e.start() > 15.0)
    );
    assert_eq!(ls[2].start, 30.0);
    assert_eq!(ls[3].start, 60.0);
    // Words cover the text in order and light up together with the line.
    let joined: String = ls[0].words.iter().map(|w| w.text.as_str()).collect();
    assert_eq!(joined.trim(), "First line here");
    assert!(!ls[0].word_synced);
    assert!(
        ls[0]
            .words
            .iter()
            .all(|w| w.start == ls[0].start && w.end == ls[0].end)
    );
}

#[test]
fn enhanced_lrc_words() {
    let entries = parse_lrc("[00:01.00]<00:01.00>Hello <00:01.50>there\n[00:04.00]Next");
    let l = lines(&entries)[0];
    assert!(l.word_synced);
    assert_eq!(l.words.len(), 2);
    assert_eq!(l.words[1].start, 1.5);
    assert_eq!(l.words[0].end, 1.5);
    assert_eq!(l.text, "Hello there");
}

#[test]
fn lyricsfile_with_words() {
    let doc = "version: '1.0'\nmetadata:\n  title: X\n  duration_ms: 215000\nlines:\n- text: I been tryna call\n  start_ms: 29470\n  end_ms: 32330\n  words:\n  - text: 'I '\n    start_ms: 29470\n  - text: 'been '\n    start_ms: 29843\n  - text: 'tryna '\n    start_ms: 30020\n  - text: call\n    start_ms: 30414\n- text: 'It''s a long one: with a colon'\n  start_ms: 40000\n  end_ms: 43000\n";
    let entries = parse_lyricsfile(doc).expect("parsed");
    let ls = lines(&entries);
    assert_eq!(ls.len(), 2);
    assert!(ls[0].word_synced);
    assert_eq!(ls[0].words.len(), 4);
    assert_eq!(ls[0].words[0].text, "I ");
    assert!((ls[0].words[3].end - 32.33).abs() < 1e-3);
    assert_eq!(ls[1].text, "It's a long one: with a colon");
    assert!(!ls[1].word_synced);
    // Without word timing every word shares the line's times.
    assert!(
        ls[1]
            .words
            .iter()
            .all(|w| w.start == ls[1].start && w.end == ls[1].end)
    );
    // 7.7 s gap between the lines becomes an interlude.
    assert!(
        entries
            .iter()
            .any(|e| matches!(e, Entry::Interlude { start, .. } if (*start - 32.33).abs() < 1e-3))
    );
}

#[test]
fn lyricsfile_folded_text() {
    let doc = "lines:\n- text: a very long line that the yaml writer\n    folded onto the next line\n  start_ms: 1000\n  end_ms: 5000\n";
    let entries = parse_lyricsfile(doc).unwrap();
    assert_eq!(
        lines(&entries)[0].text,
        "a very long line that the yaml writer folded onto the next line"
    );
}

#[test]
fn active_entry() {
    let entries = parse_lrc("[00:05.00]a\n[00:07.00]b");
    assert_eq!(Lyrics::active(&entries, 1.0), Some(0)); // the opening interlude
    assert!(
        matches!(entries[Lyrics::active(&entries, 6.0).unwrap()], Entry::Line(ref l) if l.text == "a")
    );
    assert!(
        matches!(entries[Lyrics::active(&entries, 9.0).unwrap()], Entry::Line(ref l) if l.text == "b")
    );
}

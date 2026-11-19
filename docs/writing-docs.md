# Writing docs

How Undavi's documentation is written, so new docs read like the existing ones.

## Where things go

| Kind | Where | For |
|---|---|---|
| API docs | `///` and `//!` comments in `src/` | People using or changing a module. |
| Explanations | `docs/*.md` | How something works across modules. |
| Using the app | `README.md` | Users. |
| Working on it | `CONTRIBUTING.md` | Contributors. |
| What changed | `CHANGELOG.md` | Everyone. |

Code comments (`//`) explain *why* the code does something non-obvious. Don't
restate what the code says.

## API docs

The library has `#![warn(missing_docs)]`: every public item in the public modules
(`audio`, `spectrum`, `settings`, `lyrics`, `media`) needs a doc comment. Check
before sending a change; this also catches broken links:

```sh
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
```

### Modules

Start every file with a `//!` block:

1. One line saying what the module is for.
2. A short paragraph on how its main types fit together.
3. For public modules, an `# Examples` section that compiles and runs.

```rust
//! From spectra to what's drawn: bar heights, peak caps, beats and silence.
//!
//! Pure math with no audio or UI, so it runs the same in tests. Each frame: ...
//!
//! # Examples
//!
//! ```
//! ...
//! ```
```

### Items

- **First line:** one sentence. Types are noun phrases ("A running capture
  stream."); functions say what they do ("Open `source` and start recording.").
  Rustdoc shows this line in lists, so it has to stand alone.
- **Then:** anything a caller needs that the signature doesn't say: units,
  ranges, what `None` means, side effects, threads.
- **Units and ranges** go in every numeric field's doc: `0..=100`, `ms`, `dB`,
  `dBFS`, `Hz`, `0..=1`. Use `..=` for inclusive ranges.
- **Sections**, in this order, when they apply:
  - `# Errors` for anything returning `Result`: when it fails.
  - `# Panics` for anything that can panic: when.
  - `# Examples` for entry points and anything easy to misuse.
- **Links:** refer to other items with intra-doc links (``[`Processor::update`]``),
  not plain backticks, so renames break the build instead of the docs.
- **Fields of settings** say what the user sees, then the range, e.g.
  `/// Background blur, 0..=120 px.`

### Examples

Doc examples are tests (`cargo test --doc`). Keep them short, deterministic and
free of devices or network: feed `Analyzer::push` instead of opening a capture,
parse a string instead of fetching lyrics.

## Markdown pages

- One `#` title, then `##` sections. Sentence case for headings.
- Lead with what the reader needs; put background after.
- Tables for anything with more than two attributes per row.
- Code blocks have a language (`sh`, `rust`, `json`, `text`).
- Link to code by module and item name (`settings::migrate`), not line numbers.
- When code and a doc disagree, fix the doc in the same change. The constants in
  [Audio pipeline](audio-pipeline.md) are the ones most likely to drift.

## Style

- Plain words, short sentences, active voice.
- Write for someone who knows Rust but not this codebase.
- "The app" or "Undavi", not "we".

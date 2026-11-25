# Contributing to Undavi

Thanks for helping! This page covers building Undavi, how the code is organized,
and what a change needs before it's merged.

## Building

You need Rust 1.90 or newer (install it with [rustup](https://rustup.rs)).

**Windows**

Use the default (MSVC) toolchain. rustup offers to install the Visual Studio C++ build
tools it needs; they include the Windows SDK.

**Linux** also needs the development files for PipeWire, ALSA, D-Bus and the window
system. Clang is needed because the PipeWire bindings are generated at build time.

**Arch Linux**

```sh
sudo pacman -S --needed rustup clang pkgconf pipewire alsa-lib dbus libxkbcommon wayland
```

**Debian / Ubuntu**

```sh
sudo apt install build-essential pkg-config clang libclang-dev \
    libpipewire-0.3-dev libspa-0.2-dev libasound2-dev libdbus-1-dev \
    libxkbcommon-dev libwayland-dev
```

**Fedora**

```sh
sudo dnf install gcc clang clang-devel pkgconf-pkg-config \
    pipewire-devel alsa-lib-devel dbus-devel libxkbcommon-devel wayland-devel
```

Then:

```sh
cargo run              # debug build
cargo run --release    # what users get; much smoother
```

If colors or rendering look wrong, try another graphics backend to tell a driver issue
from an app issue: set `WGPU_BACKEND` to `gl` (or on Windows, `dx12` or `vulkan`).

## Where things are

```text
src/
  lib.rs, main.rs     the library and the binary (main just calls undavi::run)
  audio/ …            the public engine: audio, spectrum, settings, lyrics, media
  app/, view/ …       the app: state, update loop, widgets, drawing
tests/                integration tests of the public API
docs/                 how things work (start with docs/architecture.md)
assets/               fonts, icons, shaders, the wordmark
install.sh, .ps1      the install scripts for Linux and Windows
packaging/            PKGBUILD, desktop file, metainfo, the Windows installer,
                      third-party licenses
vendor/iced_wgpu/     a patched iced renderer (see its PATCHED.md)
dev/                  the project's to-do list and reference images
```

[docs/architecture.md](docs/architecture.md) explains the modules, threads and
what happens each frame.

## Tests

```sh
cargo test
```

- **Unit tests** sit next to the code (`#[cfg(test)] mod tests`) and may use
  private items. App behavior (focus, settings, presets, idle mode) is tested this
  way, by driving `App::update` and `App::view`.
- **Integration tests** in `tests/` use only the public API, like any other user
  of the library. Prefer them for anything in `audio`, `spectrum`, `settings`,
  `lyrics` and `media` that can be tested from outside.
- **Doc examples** are tests too (`cargo test --doc`).
- **Snapshots** render every screen to PNG files for a visual check. They're
  slow, so they're opt-in:

  ```sh
  cargo test snapshots -- --ignored   # writes target/snapshots/*.png
  ```

  Snapshots are drawn offscreen, so they can't catch problems with the real
  window surface. Check those in the running app.

Two rules for tests:

- **Never write the user's settings.** `settings::save` does nothing in unit
  tests, but integration tests build the library normally, so they must not
  call it.
- **No devices, player or network.** Feed audio with `Analyzer::push`, parse lyrics
  from strings.

## Before sending a change

```sh
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
```

(In PowerShell, set it before: `$env:RUSTDOCFLAGS = '-D warnings'`.)

GitHub Actions runs the same checks on every pull request
(`.github/workflows/ci.yml`), on Linux and Windows, plus a release build, the install
scripts and the Windows installer, the desktop and AppStream files, and a compile check
on macOS. The macOS check is informational until that platform is supported. If you add
a build dependency, add it to the workflow and the lists above.

- Public items in the public modules need doc comments; the build warns
  otherwise. [docs/writing-docs.md](docs/writing-docs.md) describes the format.
- If a change affects how something works, update the page in `docs/` that
  describes it, and add a line to `CHANGELOG.md` under **Unreleased**.
- Dependencies: after changing `Cargo.toml` or `Cargo.lock`, regenerate the
  third-party license notices with `python3 packaging/third-party-licenses.py`
  (CI checks that the file is current).
- Settings: new fields need a default, and renamed fields keep their old name as
  an alias. See [docs/settings.md](docs/settings.md#versions-and-migrations)
  before changing the file format.
- UI changes: include a screenshot or snapshot, and check keyboard navigation
  (Tab, arrows, Enter, Escape) and a narrow window.

### Commits

Write the subject as what the change does, in the imperative ("Add a noise
gate", "Fix gravity building up speed at rest"), under about 70 characters. Use
the body for why.

## Project rules

- **Lyrics are never stored.** They're fetched from LRCLIB (or read from a local
  `.lrc` file) for the song that's playing, kept in memory for the session, and
  never written to disk or redistributed. Please keep it that way.
- **Name and logo.** Forks and modified versions need their own name and logo;
  see [TRADEMARKS.md](TRADEMARKS.md).

## License

Undavi is licensed under the GNU GPL version 3 only, with the additional terms in
[NOTICE](NOTICE). By contributing, you agree that your contribution is licensed
the same way.

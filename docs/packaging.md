# Packaging

Everything for distribution lives in `packaging/`. The app ID is `undavi`
everywhere: the Wayland `app_id`, the `.desktop` file and the icon names must
match, or compositors and docks show a generic icon.

## What a package contains

| File | Installed to |
|---|---|
| `target/release/undavi` | `bin/undavi` |
| `packaging/linux/undavi.desktop` | `share/applications/undavi.desktop` |
| `packaging/linux/undavi.metainfo.xml` | `share/metainfo/undavi.metainfo.xml` |
| `assets/icons/linux/hicolor/<size>/apps/undavi.*` | `share/icons/hicolor/<size>/apps/` |
| `LICENSE`, `NOTICE`, `assets/fonts/OFL-Inter.txt`, `packaging/THIRD-PARTY-LICENSES.txt` | `share/licenses/undavi/` |

Fonts, the wordmark and the in-app icon are compiled into the binary, so nothing
else needs installing.

## install.sh

```sh
cargo build --release --locked
sudo packaging/install.sh                       # into /usr/local
sudo packaging/install.sh --uninstall           # remove it again
PREFIX=/usr DESTDIR="$pkgdir" packaging/install.sh  # staged, for a package
```

The release tarball has the same layout, so the same commands work in it.

## Arch Linux

`packaging/arch/PKGBUILD` builds the current checkout:

```sh
cd packaging/arch && makepkg -si
```

`packaging/aur/PKGBUILD` is the AUR package: it builds a tagged release from
GitHub, with the tests in `check()`. [Releasing](releasing.md#4-publish) covers
updating it.

Runtime dependencies: `alsa-lib`, `dbus`, `gcc-libs`, `glibc`, `libpipewire`,
`libxkbcommon`, `wayland`, `vulkan-icd-loader`, plus the X11 libraries
(`libx11`, `libxcursor`, `libxrandr`, `libxi`) for X11 sessions. Building needs
`cargo` and `clang` (the PipeWire bindings are generated with bindgen).

## Other formats

- **Flatpak / Flathub:** needs a reverse-DNS app ID (for example
  `io.github.karsoncaliff.Undavi`) in the manifest, the `.desktop` file, the
  metainfo and `APP_ID` in `src/lib.rs`, and the icons renamed to match. The
  sandbox needs PipeWire access (`--filesystem=xdg-run/pipewire-0`), MPRIS on the
  session bus (`--talk-name=org.mpris.MediaPlayer2.*`), and network access
  (`--share=network`) for lyrics and album art.
- **Debian, Fedora and others:** `install.sh` with `DESTDIR` and `PREFIX=/usr`
  does the file layout; the package's build dependencies are the development
  headers listed in [CONTRIBUTING](../CONTRIBUTING.md#building).

## Checking the metadata

```sh
desktop-file-validate packaging/linux/undavi.desktop
appstreamcli validate --no-net packaging/linux/undavi.metainfo.xml
```

The metainfo validator currently warns that the ID isn't reverse-DNS; that goes
away with the Flatpak change above.

## Trademarks

Distribution packages may keep the Undavi name and logo as long as they only
make packaging changes; see [TRADEMARKS.md](../TRADEMARKS.md).

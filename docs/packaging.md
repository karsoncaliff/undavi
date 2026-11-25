# Packaging

Everything for distribution lives in `packaging/`, with `install.sh` at the root of the
repository. The app ID is `undavi`
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
| `assets/icons/ui/LICENSE` | `share/licenses/undavi/Apache-Material-Symbols.txt` |

Fonts, the wordmark and the in-app icon are compiled into the binary, so nothing
else needs installing.

## install.sh

```sh
cargo build --release --locked
sudo ./install.sh                       # into /usr/local
sudo ./install.sh --uninstall           # remove it again
PREFIX=/usr DESTDIR="$pkgdir" ./install.sh  # staged, for a package
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

## Updates

On x86-64 Linux, Undavi asks GitHub for the latest published release when it
starts (`src/update.rs`; drafts and pre-releases are skipped). What it does with a
newer one depends on how it was installed:

| Installed with | The pop-up |
|---|---|
| pacman (`pacman -Qo` owns the binary) | Says to update with the package manager. |
| `install.sh` or the release tarball (`<prefix>/bin/undavi`) | Offers to install it. |
| Neither (run from `target/`, or `cargo install`) | Never checks. |

Installing downloads `undavi-<version>-x86_64-linux.tar.gz` and its `.sha256`
from the repository's own releases, over HTTPS only, to `~/.cache/undavi/update`
(removed afterwards), and checks the hash. It then copies the tarball to a private
temporary folder, checks that copy, unpacks it there and runs its install script
(`install.sh` at its root, or `packaging/install.sh`) with `PREFIX` set to the
current install's prefix. When that prefix isn't writable, all of that second part
runs through `pkexec`, which asks for the user's password. So nothing runs as root
from a folder other programs of the user could change. The new version starts with
the next launch; the pop-up offers a restart.

The hash comes from the same release as the tarball: it catches a broken or altered
download, not a release published by someone else. Releases aren't signed.

So releases must keep the tarball's name and its checksum file, and a working
install script inside it, or updating fails with a message and a link to the
release page.

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

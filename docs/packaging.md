# Packaging

Everything for distribution lives in `packaging/` (the Windows installer too), with
`install.sh` (Linux) and `install.ps1` (Windows) at the root of the repository. On Linux, the app ID is `undavi`
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

## The Windows installer

`packaging/windows/undavi.iss` is for [Inno Setup](https://jrsoftware.org/isinfo.php) 6
or 7 (GitHub's Windows images come with 6):

```powershell
cargo build --release --locked
iscc packaging\windows\undavi.iss    # target\installer\undavi-<version>-x86_64-windows-setup.exe
```

It installs the same files to the same folder as `install.ps1` below, also for the
current user and without administrator rights, with a Start menu entry, a desktop
shortcut if wanted, and an uninstaller of its own (`unins000.exe`). The version comes
from `undavi.exe`.

The two kinds of install don't mix: the installer takes over from `install.ps1` (removing
it and its entry), and `install.ps1` refuses to touch what the installer installed.

## install.ps1

```powershell
cargo build --release --locked
powershell -ExecutionPolicy Bypass -File install.ps1              # for this user
powershell -ExecutionPolicy Bypass -File install.ps1 -Uninstall   # remove it again
powershell -ExecutionPolicy Bypass -File install.ps1 -Prefix D:\Undavi -FilesOnly
```

| File | Installed to |
|---|---|
| `target/release/undavi.exe` | `bin\undavi.exe` |
| `LICENSE`, `NOTICE`, `assets/fonts/OFL-Inter.txt`, `packaging/THIRD-PARTY-LICENSES.txt` | `licenses\` |
| `assets/icons/ui/LICENSE` | `licenses\Apache-Material-Symbols.txt` |
| `install.ps1` | `install.ps1`, for uninstalling |

That's in `%LOCALAPPDATA%\Programs\Undavi`, or the `-Prefix`. It also adds a Start menu
entry, and one under **Installed apps** in the Windows settings; `-FilesOnly` leaves both
out. Everything is for the current user, without administrator rights. A running
`undavi.exe` can't be overwritten, so it's renamed first (`undavi.exe.old-…`) and removed
by a later install, or the next time Undavi starts.

The script has to work in Windows PowerShell 5.1, which every Windows has. Keep it plain
ASCII: 5.1 doesn't read a file without a byte order mark as UTF-8.

The release zip has the same layout, so the same commands work in it. `build.rs` puts
the icon (`assets/icons/windows/undavi.ico`) and the version into the program, and
`.cargo/config.toml` builds the C runtime in, so it needs nothing that isn't part of
Windows.

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

On x86-64 Linux and Windows, Undavi asks GitHub for the latest published release when
it starts (`src/update/`; drafts and pre-releases are skipped). With **Unstable
versions** on (General in the settings), it's the release with the highest version
instead, pre-releases included. Versions are ordered as in Semantic Versioning, so
1.0.0-alpha comes before 1.0.0-alpha.2, and both before 1.0.0. Turning it off again on a
pre-release means waiting for the finished version: Undavi never goes back to an older
one. What it does with a newer one depends on how it was installed:

| Installed with | The pop-up |
|---|---|
| pacman (`pacman -Qo` owns the binary) | Says to update with the package manager. |
| `install.sh` or the release tarball (`<prefix>/bin/undavi`) | Offers to install it. |
| The Windows installer or `install.ps1` (`<prefix>\bin\undavi.exe`, with `unins000.exe` or `install.ps1` in the prefix) | Offers to install it. |
| Neither (run from `target/`, or `cargo install`) | Never checks. |

Installing downloads `undavi-<version>-x86_64-linux.tar.gz` (on Windows,
`undavi-<version>-x86_64-windows-setup.exe` or `undavi-<version>-x86_64-windows.zip`,
whichever kind this copy came from) and its `.sha256` from the repository's own
releases, over HTTPS only, to `undavi/update` in the cache folder (`~/.cache`,
`%LOCALAPPDATA%`; removed afterwards), and checks the hash. The new version starts with
the next launch; the pop-up offers a restart.

- **Linux:** it then copies the tarball to a private temporary folder, checks that
  copy, unpacks it there and runs its install script (`install.sh` at its root, or
  `packaging/install.sh`) with `PREFIX` set to the current install's prefix. When that
  prefix isn't writable, all of that runs through `pkexec`, which asks for the user's
  password. So nothing runs as root from a folder other programs of the user could
  change.
- **Windows:** it runs the installer silently for the current install's prefix, or
  unpacks the zip and runs its `install.ps1` for it, with the `tar.exe` and PowerShell in
  the system folder. Nothing runs as administrator: if the prefix isn't writable,
  updating fails.

The hash comes from the same release as the download: it catches a broken or altered
one, not a release published by someone else. Releases aren't signed.

So releases must keep the files' names, their checksum files, and a working install
script inside the archives, or updating fails with a message and a link to the release
page.

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

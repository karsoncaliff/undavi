# Releasing

How to put out a new version of Undavi. Versions follow
[Semantic Versioning](https://semver.org); the first public release is 0.9.0.

## 1. Prepare

1. **Version.** Set `version` in `Cargo.toml`, then run `cargo build` so
   `Cargo.lock` follows. Set `pkgver` (and `pkgrel=1`) in
   `packaging/arch/PKGBUILD` and `packaging/aur/PKGBUILD`.
2. **Changelog.** In `CHANGELOG.md`, rename **Unreleased** to the new version
   with today's date, start a new empty **Unreleased** section above it, and
   update the links at the bottom.
3. **AppStream.** Add a `<release version="…" date="…">` entry at the top of
   `<releases>` in `packaging/linux/undavi.metainfo.xml`, with a one-line
   summary. Software centers show this.
4. **Licenses.** If dependencies changed, run
   `python3 packaging/third-party-licenses.py`.
5. **Settings.** If the settings file format changed, check that
   `CONFIG_VERSION` and its migration are in place
   (see [Settings](settings.md#versions-and-migrations)).

## 2. Check

```sh
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --locked
cargo test snapshots -- --ignored        # then look through target/snapshots/
```

Then run the release build (`cargo run --release`) and try it with real music:
System Audio, an app, Music player, the settings window, lyrics, full screen, and a
narrow window.

## 3. Tag

Commit, push to `main`, and wait for CI to pass. Then:

```sh
git tag -a v0.10.0 -m "Undavi 0.10.0"
git push origin v0.10.0
```

The **Release** workflow checks that the tag matches `Cargo.toml`, runs the
tests, builds `undavi-<version>-x86_64-linux.tar.gz` (with a `.sha256`), and
creates a **draft** GitHub release with this version's CHANGELOG section as the
notes.

## 4. Publish

1. Open the draft under **Releases** on GitHub. Add screenshots or videos to the
   notes if you like, then **Publish release**.
2. **AUR.** In a clone of the AUR package (`ssh://aur@aur.archlinux.org/undavi.git`),
   copy in `packaging/aur/PKGBUILD`, then:

   ```sh
   updpkgsums                         # fill in the checksum of the tagged source
   makepkg -si                        # build, test and install it once
   makepkg --printsrcinfo > .SRCINFO
   git commit -am "Update to 0.10.0" && git push
   ```

   Copy the updated checksum back into `packaging/aur/PKGBUILD` in this
   repository.

## The prebuilt tarball

It's built on Ubuntu 24.04, so it needs glibc 2.39 or newer: Ubuntu 24.04,
Fedora 40, Debian 13, Linux Mint 22, and current Arch, openSUSE Tumbleweed and
their derivatives. Older systems build from source. Users install it with:

```sh
tar xf undavi-0.10.0-x86_64-linux.tar.gz
cd undavi-0.10.0-x86_64-linux
sudo ./install.sh
```

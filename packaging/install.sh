#!/bin/sh
# Install a built Undavi into a prefix (default /usr/local). Used by the PKGBUILDs
# and the release tarball; also works on its own:
#
#   cargo build --release && sudo packaging/install.sh
#   sudo packaging/install.sh --uninstall      # remove what it installed
#
# Set DESTDIR to stage the files somewhere else (packaging), PREFIX to change
# /usr/local. Use the same PREFIX to uninstall.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
prefix=${PREFIX:-/usr/local}
dest=${DESTDIR:-}
target="$dest$prefix"

case "${1:-}" in
    "") ;;
    --uninstall)
        rm -f "$target/bin/undavi" \
            "$target/share/applications/undavi.desktop" \
            "$target/share/metainfo/undavi.metainfo.xml" \
            "$target"/share/icons/hicolor/*/apps/undavi.png \
            "$target"/share/icons/hicolor/*/apps/undavi.svg
        rm -rf "$target/share/licenses/undavi"
        echo "Removed Undavi from $target. Settings stay in ~/.config/undavi."
        exit 0
        ;;
    *)
        echo "usage: $0 [--uninstall]" >&2
        exit 2
        ;;
esac

install -Dm755 "$root/target/release/undavi" "$target/bin/undavi"
install -Dm644 "$root/packaging/linux/undavi.desktop" "$target/share/applications/undavi.desktop"
install -Dm644 "$root/packaging/linux/undavi.metainfo.xml" "$target/share/metainfo/undavi.metainfo.xml"
for dir in "$root"/assets/icons/linux/hicolor/*/apps; do
    size=$(basename "$(dirname "$dir")")
    for file in "$dir"/undavi.*; do
        install -Dm644 "$file" "$target/share/icons/hicolor/$size/apps/$(basename "$file")"
    done
done
# Undavi's license, the bundled font's license, and the notices of the Rust crates
# compiled into the binary, which their licenses ask to travel with it.
for file in "$root"/LICENSE "$root"/NOTICE "$root"/assets/fonts/OFL-Inter.txt \
    "$root"/packaging/THIRD-PARTY-LICENSES.txt; do
    if [ -s "$file" ]; then
        install -Dm644 "$file" "$target/share/licenses/undavi/$(basename "$file")"
    fi
done

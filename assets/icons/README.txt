Undavi icon set
===============

Every PNG was rendered directly from the SVG at its final size (no downscaling),
and the .ico and .icns files contain those same PNGs unchanged.

source/
  undavi.svg          Your original icon.
  undavi-macos.svg    macOS version: an 824 px rounded square (corner radius 185.4)
                      centred on a 1024 px canvas, per Apple's icon grid.
  undavi-linux.svg    Linux version: drawn on a 128 px grid with an 8 px margin,
                      so it doesn't look oversized next to other app icons.

png/
  undavi-16.png ... undavi-1024.png
  Your original, edge to edge. Use these for the in-app window icon
  (e.g. load the 256 or 64 px one), the README, and the website.

windows/undavi.ico
  16, 20, 24, 32, 40, 48, 64, 96, 128 and 256 px. Windows picks the exact
  size it needs for each DPI. Embed it in the .exe from build.rs with a crate
  such as winresource or embed-resource.

macos/undavi.icns
  16 to 1024 px, including the @2x retina sizes. Put it in
  Undavi.app/Contents/Resources/ and set CFBundleIconFile in Info.plist.
  undavi-macos-1024.png is the same icon as a single PNG.

linux/hicolor/
  16x16 ... 512x512 PNGs plus scalable/apps/undavi.svg.
  Install into /usr/share/icons/hicolor/ (or /app/share/icons/hicolor/ in a
  Flatpak). Rename the files to your app ID (e.g. io.github.yourname.Undavi)
  so they match the Icon= line in your .desktop file and the Flatpak app ID.

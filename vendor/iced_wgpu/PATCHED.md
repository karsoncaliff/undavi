# iced_wgpu 0.14.0, patched

A copy of [iced_wgpu 0.14.0](https://crates.io/crates/iced_wgpu) from crates.io,
used through `[patch.crates-io]` in Undavi's `Cargo.toml`.

iced is MIT-licensed, © 2019 Héctor Ramón and the iced contributors
(https://github.com/iced-rs/iced/blob/master/LICENSE).

## The changes

1. `src/window/compositor.rs`, surface format selection: with iced's default
`web-colors` feature, iced picks the first non-sRGB format the surface offers.
NVIDIA's Vulkan driver lists `Rgba16Float` first, and compositors read a float
surface as linear light, so every color came out much lighter than intended
(#1C1C21 showed as #5C5C64). The patch prefers `Bgra8Unorm`/`Rgba8Unorm`, then
any other non-sRGB format that isn't a float format.

2. `src/image/vector.rs`, SVG rasterizing: resvg/tiny-skia return premultiplied
pixels, but the image pipeline blends with straight alpha (`SrcAlpha`), so the
anti-aliased edges of full-color SVGs (Undavi's wordmark) came out with a dark
fringe. The patch un-premultiplies the pixels before uploading them. Tinted
SVGs (icons) were already fine, since their color is replaced anyway.

Drop this copy (and the `[patch.crates-io]` entry) once upstream iced fixes
both.

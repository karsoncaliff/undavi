//! Monochrome SVG icons on a 24×24 grid. Drawn in black and tinted when rendered.

use iced::widget::{Svg, svg};
use iced::{Color, Length};

macro_rules! stroked {
    ($width:literal, $body:literal) => {
        concat!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width=""#,
            $width,
            r#"" stroke-linecap="round" stroke-linejoin="round">"#,
            $body,
            "</svg>"
        )
    };
}

macro_rules! filled {
    ($body:literal) => {
        concat!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="black">"#,
            $body,
            "</svg>"
        )
    };
}

pub const MONITOR: &str = stroked!(
    "2",
    r#"<rect x="3" y="4" width="18" height="12" rx="2"/><path d="M8 20h8M12 16v4"/>"#
);
pub const SLIDERS: &str = stroked!(
    "2",
    r#"<path d="M5 21v-7M5 10V3M12 21v-9M12 8V3M19 21v-5M19 12V3M2 14h6M9 8h6M16 16h6"/>"#
);
pub const APPEARANCE: &str = stroked!(
    "2",
    r#"<rect x="3" y="3" width="18" height="18" rx="3"/><circle cx="9" cy="9" r="2"/><path d="m21 15-4.5-4.5L6 21"/>"#
);
pub const BARS: &str = stroked!("2", r#"<path d="M6 20v-6M12 20V4M18 20v-10"/>"#);
pub const CLOSE: &str = stroked!("2", r#"<path d="M18 6 6 18M6 6l12 12"/>"#);
pub const REPEAT: &str = stroked!(
    "2",
    r#"<path d="m17 2 4 4-4 4"/><path d="M3 11v-1a4 4 0 0 1 4-4h14"/><path d="m7 22-4-4 4-4"/><path d="M21 13v1a4 4 0 0 1-4 4H3"/>"#
);
pub const PREV: &str = filled!(
    r#"<rect x="4" y="5" width="2.5" height="14" rx="1"/><path d="M20 5.5v13a1 1 0 0 1-1.5.86l-10-6.5a1 1 0 0 1 0-1.72l10-6.5A1 1 0 0 1 20 5.5z"/>"#
);
pub const NEXT: &str = filled!(
    r#"<rect x="17.5" y="5" width="2.5" height="14" rx="1"/><path d="M4 5.5v13a1 1 0 0 0 1.5.86l10-6.5a1 1 0 0 0 0-1.72l-10-6.5A1 1 0 0 0 4 5.5z"/>"#
);
pub const PLAY: &str = filled!(
    r#"<path d="M7 4.5v15a1 1 0 0 0 1.53.85l12-7.5a1 1 0 0 0 0-1.7l-12-7.5A1 1 0 0 0 7 4.5z"/>"#
);
pub const PAUSE: &str = filled!(
    r#"<rect x="5" y="3" width="5" height="18" rx="1"/><rect x="14" y="3" width="5" height="18" rx="1"/>"#
);
pub const VOLUME: &str = stroked!(
    "2",
    r#"<path d="M11 5 6 9H2v6h4l5 4V5z"/><path d="M15.5 8.5a5 5 0 0 1 0 7"/><path d="M19 5a10 10 0 0 1 0 14"/>"#
);
pub const VOLUME_MUTED: &str = stroked!(
    "2",
    r#"<path d="M11 5 6 9H2v6h4l5 4V5z"/><path d="m22 9-6 6M16 9l6 6"/>"#
);
pub const PENCIL: &str = stroked!(
    "2",
    r#"<path d="M17 3a2.85 2.85 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5Z"/><path d="m15 5 4 4"/>"#
);
pub const CHEVRON_UP: &str = stroked!("2", r#"<path d="m18 15-6-6-6 6"/>"#);
pub const CHEVRON_DOWN: &str = stroked!("2", r#"<path d="m6 9 6 6 6-6"/>"#);
pub const CHECK: &str = stroked!("2.5", r#"<path d="M20 6 9 17l-5-5"/>"#);
pub const FULLSCREEN: &str = stroked!(
    "2",
    r#"<path d="M8 3H5a2 2 0 0 0-2 2v3M21 8V5a2 2 0 0 0-2-2h-3M3 16v3a2 2 0 0 0 2 2h3M16 21h3a2 2 0 0 0 2-2v-3"/>"#
);
pub const EXIT_FULLSCREEN: &str = stroked!(
    "2",
    r#"<path d="M8 3v3a2 2 0 0 1-2 2H3M21 8h-3a2 2 0 0 1-2-2V3M3 16h3a2 2 0 0 1 2 2v3M16 21v-3a2 2 0 0 1 2-2h3"/>"#
);
pub const SEGMENTS: &str = filled!(
    r#"<rect x="9" y="4" width="6" height="4" rx="0.5"/><rect x="9" y="10" width="6" height="4" rx="0.5"/><rect x="9" y="16" width="6" height="4" rx="0.5"/>"#
);
pub const PEAK_CAPS: &str = filled!(
    r#"<rect x="9" y="3" width="6" height="2.5" rx="0.5"/><rect x="9" y="8" width="6" height="13" rx="0.5"/>"#
);
pub const CORNER_SQUARE: &str = filled!(r#"<rect x="9" y="4" width="6" height="16"/>"#);
pub const CORNER_ROUNDED: &str = filled!(r#"<rect x="9" y="4" width="6" height="16" rx="1.5"/>"#);
pub const CORNER_PILL: &str = filled!(r#"<rect x="9" y="4" width="6" height="16" rx="3"/>"#);

/// An icon at `size` px, tinted with `color`. The tint ignores alpha, so the
/// color's alpha becomes the icon's opacity (used for auto-hide fading).
pub fn icon<'a>(source: &'static str, size: f32, color: Color) -> Svg<'a> {
    let opaque = Color { a: 1.0, ..color };
    svg(svg::Handle::from_memory(source.as_bytes()))
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .opacity(color.a)
        .style(move |_, _| svg::Style {
            color: Some(opaque),
        })
}

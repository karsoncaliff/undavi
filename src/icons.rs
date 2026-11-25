//! Monochrome SVG icons on a 24×24 grid, tinted when rendered. They're Google's
//! Material Symbols (Rounded), in `assets/icons/ui/` under their own names.

use iced::widget::{Svg, svg};
use iced::{Color, Length};

macro_rules! icons {
    ($($(#[$doc:meta])* $name:ident = $file:literal;)*) => {
        $(
            $(#[$doc])*
            pub const $name: &str =
                include_str!(concat!("../assets/icons/ui/", $file, ".svg"));
        )*
    };
}

icons! {
    /// The settings window.
    SETTINGS = "settings";
    QUICK = "tune";
    /// The audio source.
    MONITOR = "monitor";
    /// Read the song's info again.
    REFRESH = "refresh";
    CLOSE = "close";
    CHECK = "check";
    SEARCH = "search";
    PENCIL = "edit";
    ARROW_LEFT = "arrow-back";
    CHEVRON_UP = "keyboard-arrow-up";
    CHEVRON_DOWN = "keyboard-arrow-down";
    CHEVRON_RIGHT = "keyboard-arrow-right";
    /// Peek at full size.
    EYE = "visibility";
    /// Presets.
    LAYERS = "layers";
    /// A changed setting.
    /// A setting for the quick settings, and one that's in them.
    STAR = "star";
    STAR_FILLED = "star-filled";
    UNDO = "undo";
    REDO = "redo";
    /// Expand or collapse the whole tree.
    EXPAND = "unfold-more";
    COLLAPSE = "unfold-less";
    /// The preview column.
    HIDE_COLUMN = "right-panel-close";
    SHOW_COLUMN = "right-panel-open";

    // Playback
    PLAY = "play-arrow";
    PAUSE = "pause";
    PREV = "skip-previous";
    NEXT = "skip-next";
    REPEAT = "repeat";
    VOLUME = "volume-up";
    VOLUME_MUTED = "volume-off";
    FULLSCREEN = "fullscreen";
    EXIT_FULLSCREEN = "fullscreen-exit";

    // The sections of the settings window
    SOUND = "graphic-eq";
    /// The Visualizer section, and the signal monitor's Spectrum.
    BARS = "equalizer";
    PALETTE = "palette";
    BACKGROUND = "image";
    NOTE = "music-note";
    PLAYBACK = "line-end";
    LINES = "lyrics";
    CONTROLS = "keyboard";
    GENERAL = "settings";
    /// The list of every section's presets.
    PRESETS = "stack";

    // The rest of the signal monitor's parts: Signal, Beats and System
    ACTIVITY = "vital-signs";
    ZAP = "bolt";
    GAUGE = "speed";
}

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

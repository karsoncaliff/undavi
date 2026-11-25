//! How the screen and the settings window adapt to the window size: what fits
//! beside what, and what gets compacted or left out when it doesn't.

use crate::app::{PANEL_WIDTH, SPOTLIGHT_WIDTH};
use crate::settings::NowPlayingMode;
use iced::Size;

/// Side padding of the header and the stage in a roomy window, and in a narrow one.
const GUTTER: f32 = 32.0;
const GUTTER_NARROW: f32 = 16.0;
/// Below this width the narrow gutter is used.
const NARROW: f32 = 720.0;
/// The stage (bars and transport) is never squeezed narrower than this to fit a
/// panel beside it; the panel floats over it instead.
const STAGE_MIN: f32 = 560.0;
/// Space between the stage and a panel beside it, and the right margin of the
/// area below the header (never wider than the gutter).
const PANEL_GAP: f32 = 24.0;
const RIGHT_MARGIN: f32 = 24.0;

/// Header pieces, in px.
/// The wordmark, which opens About.
const LOGO_W: f32 = 36.0 * crate::about::TITLE_ASPECT + 10.0 + REFRESH;
/// The refresh button beside it.
pub const REFRESH: f32 = 40.0;
const HEADER_BUTTON: f32 = 48.0;
/// Buttons after the source pill: Quick settings and Settings.
const HEADER_BUTTONS: f32 = 2.0;
/// Space between them, and after the pill.
pub const HEADER_SPACING: f32 = 12.0;
/// Source pill: padding + icon + gap, and the rough width of one label character.
const PILL_CHROME: f32 = 20.0 + 22.0 + 10.0 + 20.0;
const PILL_CHAR: f32 = 8.6;
/// Track info narrower than this isn't worth showing in the header.
const INFO_MIN: f32 = 180.0;
/// The widest the header's track info gets.
pub const INFO_MAX: f32 = 560.0;
/// Spotlight needs its column, a gap, and room for the lyrics and bars beside it.
const SPOTLIGHT_MIN_STAGE: f32 = SPOTLIGHT_WIDTH + 48.0 + 480.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    /// The window's width.
    pub width: f32,
    /// Left/right padding of the header, and left padding below it (where an open
    /// panel starts, under the wordmark).
    pub gutter: f32,
    /// Right padding below the header.
    pub right_margin: f32,
    /// The About panel floats over the stage instead of sitting beside it.
    pub panel_overlay: bool,
    pub panel_width: f32,
    /// Width the stage gets (with a panel beside it, if open).
    pub stage_width: f32,
    /// The source pill shows only its icon.
    pub compact_pill: bool,
    /// Room for the header's track info (`None`: leave it out).
    pub info_width: Option<f32>,
    /// The Now playing mode that fits (Spotlight falls back to Header when narrow).
    pub now_playing: NowPlayingMode,
}

impl Layout {
    /// `pill_label` is the source pill's text; `panel_open` whether the About panel
    /// shows.
    pub fn new(window: Size, mode: NowPlayingMode, pill_label: &str, panel_open: bool) -> Self {
        let w = window.width;
        let gutter = if w < NARROW { GUTTER_NARROW } else { GUTTER };
        let panel_width = PANEL_WIDTH.min(w - 2.0 * GUTTER_NARROW).max(0.0);

        let right_margin = RIGHT_MARGIN.min(gutter);
        let full_stage = (w - gutter - right_margin).max(0.0);
        let beside = full_stage - PANEL_GAP - panel_width;
        let panel_overlay = beside < STAGE_MIN;
        let stage_width = if panel_open && !panel_overlay {
            beside
        } else {
            full_stage
        };

        let now_playing = match mode {
            NowPlayingMode::Spotlight if stage_width < SPOTLIGHT_MIN_STAGE => {
                NowPlayingMode::Header
            }
            m => m,
        };

        // The header centers its track info between two equal sides, each as wide
        // as the wider of the logo and the buttons.
        let available = w - 2.0 * gutter;
        let actions = |pill: f32| pill + HEADER_BUTTONS * (HEADER_BUTTON + HEADER_SPACING);
        let full_pill = PILL_CHROME + PILL_CHAR * pill_label.chars().count() as f32;
        let wants_info = matches!(
            now_playing,
            NowPlayingMode::Header | NowPlayingMode::Minimal
        );
        let room = |pill: f32| available - 2.0 * LOGO_W.max(actions(pill));
        let compact_pill = if wants_info {
            room(full_pill) < INFO_MIN
        } else {
            available < LOGO_W + actions(full_pill) + HEADER_SPACING
        };
        let pill = if compact_pill {
            HEADER_BUTTON
        } else {
            full_pill
        };
        let info_width = (wants_info && room(pill) >= INFO_MIN).then(|| room(pill).min(INFO_MAX));

        Self {
            width: w,
            gutter,
            right_margin,
            panel_overlay,
            panel_width,
            stage_width,
            compact_pill,
            info_width,
            now_playing,
        }
    }

    /// Lyrics strip text scale: a little smaller on a narrow stage.
    pub fn lyrics_scale(&self) -> f32 {
        (self.stage_width / 900.0).clamp(0.75, 1.0)
    }

    /// Where the source picker's right edge goes, from the window's: under the
    /// pill's, past the buttons beside it.
    pub fn under_pill(&self) -> f32 {
        self.gutter + HEADER_BUTTONS * (HEADER_BUTTON + HEADER_SPACING)
    }
}

// ---------------------------------------------------------------------------
// The settings window

/// Space around the settings window, and the least of it in a small one.
const WINDOW_MARGIN: Size = Size::new(40.0, 28.0);
const WINDOW_MARGIN_SMALL: f32 = 16.0;
/// It doesn't grow past this on big screens.
const WINDOW_MAX: Size = Size::new(1640.0, 1040.0);
/// The navigation tree, or only its icons.
const NAV: f32 = 240.0;
const NAV_COMPACT: f32 = 64.0;
/// The content and the preview share what's left, this much to the content.
const CONTENT_SHARE: f32 = 4.0 / 7.0;
const CONTENT_MIN: f32 = 400.0;
const PREVIEW_MIN: f32 = 300.0;
/// The collapsed preview.
const STRIP: f32 = 56.0;

/// How the settings window is arranged.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SettingsLayout {
    pub size: Size,
    /// The tree shows only the sections' icons.
    pub compact_nav: bool,
    pub nav: f32,
    pub content: f32,
    /// The preview column's width; `None` when it's collapsed to a strip.
    pub preview: Option<f32>,
    /// There's room to show the preview.
    pub preview_fits: bool,
    pub strip: f32,
}

impl SettingsLayout {
    /// `show_preview` is whether the preview should show, if there's room.
    pub fn new(window: Size, show_preview: bool) -> Self {
        let margin = |length: f32, roomy: f32, margin: f32| {
            if length >= roomy {
                margin
            } else {
                WINDOW_MARGIN_SMALL
            }
        };
        let size = Size::new(
            (window.width - 2.0 * margin(window.width, 1200.0, WINDOW_MARGIN.width))
                .clamp(0.0, WINDOW_MAX.width),
            (window.height - 2.0 * margin(window.height, 760.0, WINDOW_MARGIN.height))
                .clamp(0.0, WINDOW_MAX.height),
        );
        let w = size.width;
        // The preview stays as long as it fits beside the content, with the tree
        // down to its icons if need be.
        let preview_fits = w >= NAV_COMPACT + CONTENT_MIN + PREVIEW_MIN;
        let shown = show_preview && preview_fits;
        let side = if shown { PREVIEW_MIN } else { STRIP };
        let compact_nav = w < NAV + CONTENT_MIN + side;
        let nav = if compact_nav { NAV_COMPACT } else { NAV };
        let (content, preview) = if shown {
            let rest = w - nav;
            let content = (rest * CONTENT_SHARE)
                .max(CONTENT_MIN)
                .min(rest - PREVIEW_MIN)
                .round();
            (content, Some(rest - content))
        } else {
            ((w - nav - STRIP).max(0.0), None)
        };
        Self {
            size,
            compact_nav,
            nav,
            content,
            preview,
            preview_fits,
            strip: STRIP,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(w: f32, mode: NowPlayingMode, panel: bool) -> Layout {
        Layout::new(Size::new(w, 900.0), mode, "System Audio", panel)
    }

    #[test]
    fn roomy_window_keeps_everything() {
        let l = at(1440.0, NowPlayingMode::Header, true);
        assert!(!l.panel_overlay && !l.compact_pill);
        assert_eq!(l.panel_width, PANEL_WIDTH);
        // The track info stays centered between the logo and the buttons.
        assert_eq!(l.info_width, Some(INFO_MAX));
        assert_eq!(l.gutter, GUTTER);
        assert_eq!(l.now_playing, NowPlayingMode::Header);
    }

    #[test]
    fn narrow_windows_compact_then_drop() {
        // The two windows from the bug report (923 and 606 px wide).
        let l = at(923.0, NowPlayingMode::Header, true);
        assert!(
            l.panel_overlay,
            "the panel floats instead of squeezing the bars"
        );
        assert_eq!(l.stage_width, 923.0 - GUTTER - RIGHT_MARGIN);
        assert!(l.info_width.is_some_and(|w| w >= INFO_MIN));

        let l = at(606.0, NowPlayingMode::Header, true);
        assert_eq!(l.gutter, GUTTER_NARROW);
        assert!(l.compact_pill);
        assert!(
            l.info_width.is_some(),
            "the icon leaves room for track info"
        );
        let smallest = at(480.0, NowPlayingMode::Header, false);
        assert_eq!(smallest.info_width, None, "no room for track info");
        assert!(l.panel_width <= 606.0 - 2.0 * GUTTER_NARROW);
    }

    #[test]
    fn the_settings_window_fits_any_window() {
        // The reference size: like the mockup.
        let l = SettingsLayout::new(Size::new(1440.0, 900.0), true);
        assert_eq!(l.size, Size::new(1360.0, 844.0));
        assert_eq!((l.nav, l.content, l.preview), (NAV, 640.0, Some(480.0)));
        // Hiding the preview gives its room to the content.
        let hidden = SettingsLayout::new(Size::new(1440.0, 900.0), false);
        assert_eq!(hidden.content, 1360.0 - NAV - STRIP);
        assert_eq!(hidden.preview, None);
        // Big screens: it stops growing.
        let big = SettingsLayout::new(Size::new(3840.0, 2160.0), true);
        assert_eq!(big.size, WINDOW_MAX);
        // Every width adds up, with room for the content.
        for w in (480..2000).step_by(7) {
            for show in [true, false] {
                let l = SettingsLayout::new(Size::new(w as f32, 700.0), show);
                let side = l.preview.unwrap_or(l.strip);
                assert!(
                    (l.nav + l.content + side - l.size.width).abs() < 0.01,
                    "{w}"
                );
                assert!(l.preview.is_none_or(|p| p >= PREVIEW_MIN), "{w}");
                assert!(l.content >= 328.0, "{w}: {}", l.content);
                assert!(l.preview.is_some() == (show && l.preview_fits), "{w}");
            }
        }
        // Half a screen: the tree gives way to the preview, or the preview to it.
        assert!(SettingsLayout::new(Size::new(923.0, 1031.0), true).compact_nav);
        assert!(!SettingsLayout::new(Size::new(923.0, 1031.0), false).compact_nav);
        assert!(!SettingsLayout::new(Size::new(606.0, 1031.0), true).preview_fits);
    }

    #[test]
    fn spotlight_needs_room() {
        assert_eq!(
            at(1440.0, NowPlayingMode::Spotlight, false).now_playing,
            NowPlayingMode::Spotlight
        );
        assert_eq!(
            at(700.0, NowPlayingMode::Spotlight, false).now_playing,
            NowPlayingMode::Header
        );
        // Without track info to fit, the pill keeps its label longer.
        assert!(!at(700.0, NowPlayingMode::Off, false).compact_pill);
    }
}

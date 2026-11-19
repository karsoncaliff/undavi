//! How the screen adapts to the window size: what fits beside what, and what
//! gets compacted or left out when it doesn't.

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
const LOGO_W: f32 = 36.0 * crate::about::TITLE_ASPECT + 10.0;
const HEADER_BUTTON: f32 = 48.0;
const HEADER_SPACING: f32 = 16.0;
/// Source pill: padding + icon + gap, and the rough width of one label character.
const PILL_CHROME: f32 = 20.0 + 22.0 + 10.0 + 20.0;
const PILL_CHAR: f32 = 8.6;
/// Track info narrower than this isn't worth showing in the header.
const INFO_MIN: f32 = 200.0;
/// The widest the header's track info gets.
pub const INFO_MAX: f32 = 560.0;
/// Spotlight needs its column, a gap, and room for the lyrics and bars beside it.
const SPOTLIGHT_MIN_STAGE: f32 = SPOTLIGHT_WIDTH + 48.0 + 480.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    /// Left/right padding of the header, and left padding of the stage.
    pub gutter: f32,
    /// Right padding below the header (where an open panel ends).
    pub right_margin: f32,
    /// Panels float over the stage instead of sitting beside it.
    pub panel_overlay: bool,
    pub panel_width: f32,
    /// Width the stage gets (with a panel beside it, if one is open).
    pub stage_width: f32,
    /// The source pill shows only its icon.
    pub compact_pill: bool,
    /// Room for the header's track info (`None`: leave it out).
    pub info_width: Option<f32>,
    /// The Now playing mode that fits (Spotlight falls back to Header when narrow).
    pub now_playing: NowPlayingMode,
}

impl Layout {
    /// `pill_label` is the source pill's text; `panel_open` whether a panel shows.
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
        let actions = |pill: f32| pill + 3.0 * (HEADER_BUTTON + HEADER_SPACING);
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
        assert!(l.compact_pill);
        assert!(l.info_width.is_some_and(|w| w >= INFO_MIN));

        let l = at(606.0, NowPlayingMode::Header, true);
        assert_eq!(l.gutter, GUTTER_NARROW);
        assert!(l.compact_pill);
        assert_eq!(l.info_width, None, "no room for track info");
        assert!(l.panel_width <= 606.0 - 2.0 * GUTTER_NARROW);
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

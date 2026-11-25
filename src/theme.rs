//! Design tokens (colors, fonts) and shared widget styles.

use iced::font::Weight;
use iced::widget::{button, container, scrollable};
use iced::{Background, Border, Color, Font, Shadow, Theme, color};

// Surfaces use the eight grays in assets/ui.ase, darkest first:
// 131316 1C1C21 26262C 2F3037 393A41 4B4C52 5B5C62 6A6B70.
pub const BG: Color = color!(0x131316);
pub const PANEL: Color = color!(0x1C1C21);
pub const CONTROL: Color = color!(0x2F3037);
pub const CONTROL_SUNKEN: Color = color!(0x26262C);
pub const TRACK: Color = color!(0x393A41);
pub const ACCENT: Color = color!(0x4B7BF5);
pub const BAR: Color = color!(0xE4E4E7);
pub const SEEK_HANDLE: Color = color!(0xD9D9DE);
pub const TEXT: Color = color!(0xF5F5F7);
pub const TEXT_2: Color = color!(0xA0A0A8);
pub const TEXT_3: Color = color!(0x8E8E96);
pub const ICON_OFF: Color = color!(0x6A6B70);
pub const CUSTOM_RING: Color = color!(0x4B4C52);
/// Errors and destructive actions.
pub const DANGER: Color = color!(0xFF6B57);
/// Higher-contrast track-info text over album-art / ambient backgrounds.
pub const TEXT_2_ON_ART: Color = color!(0xD0D0D6);
pub const TEXT_3_ON_ART: Color = color!(0xC4C4CC);
/// The volume popover's fill.
pub const CONTROL_ON_ART: Color = Color::from_rgba8(0x2F, 0x30, 0x37, 0.85);
/// The settings window: its sides (the tree and the preview), the content between
/// them, the cards in it and what's on them.
pub const SIDE: Color = color!(0x151518);
pub const CONTENT: Color = color!(0x1A1A1E);
pub const CARD: Color = color!(0x202025);
pub const LINE: Color = color!(0x2A2A30);
pub const FIELD: Color = color!(0x1E1E22);
pub const RAISED: Color = color!(0x26262B);
pub const RAISED_2: Color = color!(0x2A2A30);
pub const TROUGH: Color = color!(0x111113);
pub const TROUGH_SELECTED: Color = color!(0x3A3A42);
pub const MENU: Color = color!(0x222227);
/// Outlines of the settings window and of picture cards' diagrams.
pub const OUTLINE: Color = color!(0x2F3037);
pub const DIAGRAM: Color = color!(0x4B4C52);
/// The playback bar along the bottom of the window.
pub const DOCK: Color = color!(0x0E0E10);
/// The radius of the settings window's corners.
pub const WINDOW_RADIUS: f32 = 20.0;
/// The About panel: translucent so the moving background shows through.
pub const PANEL_TRANSLUCENT: Color = Color::from_rgba8(0x1C, 0x1C, 0x21, 0.84);

pub const INTER: Font = Font::with_name("Inter");
pub const INTER_MEDIUM: Font = Font {
    weight: Weight::Medium,
    ..INTER
};
pub const INTER_SEMIBOLD: Font = Font {
    weight: Weight::Semibold,
    ..INTER
};

pub const FONTS: [&[u8]; 3] = [
    include_bytes!("../assets/fonts/Inter-400.ttf"),
    include_bytes!("../assets/fonts/Inter-500.ttf"),
    include_bytes!("../assets/fonts/Inter-600.ttf"),
];

/// Iced has no letter-spacing, so tracked-out uppercase labels get thin spaces between letters.
pub fn tracked(s: &str) -> String {
    let upper = s.to_uppercase();
    let mut out = String::with_capacity(upper.len() * 4);
    for (i, ch) in upper.chars().enumerate() {
        if i > 0 {
            out.push('\u{2009}');
        }
        out.push(ch);
    }
    out
}

pub fn with_alpha(c: Color, a: f32) -> Color {
    Color { a: c.a * a, ..c }
}

/// Mix `c` toward white by `t` (hover states).
pub fn lighten(c: Color, t: f32) -> Color {
    Color {
        r: c.r + (1.0 - c.r) * t,
        g: c.g + (1.0 - c.g) * t,
        b: c.b + (1.0 - c.b) * t,
        a: c.a,
    }
}

fn rounded(radius: f32) -> Border {
    Border {
        radius: radius.into(),
        ..Border::default()
    }
}

/// Filled control (header buttons, close button, secondary footer button).
pub fn control_button(
    active: bool,
    radius: f32,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let base = if active { ACCENT } else { CONTROL };
        let bg = match status {
            button::Status::Hovered => lighten(base, 0.06),
            button::Status::Pressed => lighten(base, 0.12),
            _ => base,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: TEXT,
            border: rounded(radius),
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

/// Header button: accent fill when active, and otherwise a faint light one that the
/// background shows through. `alpha` fades it for auto-hide.
pub fn header_button(
    active: bool,
    accent: Color,
    radius: f32,
    alpha: f32,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let bg = match (active, status) {
            (true, button::Status::Hovered) => lighten(accent, 0.06),
            (true, button::Status::Pressed) => lighten(accent, 0.12),
            (true, _) => accent,
            (false, button::Status::Hovered) => with_alpha(TEXT, 0.16),
            (false, button::Status::Pressed) => with_alpha(TEXT, 0.21),
            (false, _) => with_alpha(TEXT, 0.10),
        };
        button::Style {
            background: Some(Background::Color(with_alpha(bg, alpha))),
            text_color: with_alpha(TEXT, alpha),
            border: rounded(radius),
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

/// Icon color on an accent-filled button: dark on light accents.
pub fn on_accent(accent: Color) -> Color {
    let c = [accent.r, accent.g, accent.b].map(|v| (v * 255.0).round() as u8);
    if crate::settings::wants_dark_icon(c) {
        BG
    } else {
        Color::WHITE
    }
}

/// Icon-only transport button: no fill, faint highlight on hover.
pub fn ghost_button(radius: f32, alpha: f32) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let bg = match status {
            button::Status::Hovered => Some(Background::Color(with_alpha(TEXT, 0.06 * alpha))),
            button::Status::Pressed => Some(Background::Color(with_alpha(TEXT, 0.10 * alpha))),
            _ => None,
        };
        button::Style {
            background: bg,
            text_color: TEXT,
            border: rounded(radius),
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

/// Plain text button ("Reset").
pub fn text_button(_: &Theme, status: button::Status) -> button::Style {
    button::Style {
        background: match status {
            button::Status::Hovered | button::Status::Pressed => {
                Some(Background::Color(with_alpha(TEXT, 0.05)))
            }
            _ => None,
        },
        text_color: if status == button::Status::Hovered {
            TEXT
        } else {
            TEXT_2
        },
        border: rounded(12.0),
        shadow: Shadow::default(),
        snap: true,
    }
}

pub fn trough(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(CONTROL_SUNKEN)),
        border: rounded(12.0),
        ..container::Style::default()
    }
}

pub fn toggle_chip(on: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let mut bg = if on {
            Color::from_rgba8(75, 123, 245, 0.18)
        } else {
            CONTROL_SUNKEN
        };
        if status == button::Status::Hovered {
            bg = lighten(bg, 0.04);
        }
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: TEXT,
            border: Border {
                radius: 12.0.into(),
                width: if on { 1.0 } else { 0.0 },
                color: ACCENT,
            },
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

/// Panel scrollbar: the scroller takes the accent color, lighter while hovered or dragged.
pub fn scrollbar(accent: Color) -> impl Fn(&Theme, scrollable::Status) -> scrollable::Style {
    move |theme, status| {
        let scroller = match status {
            scrollable::Status::Dragged {
                is_vertical_scrollbar_dragged: true,
                ..
            } => lighten(accent, 0.25),
            scrollable::Status::Hovered {
                is_vertical_scrollbar_hovered: true,
                ..
            } => lighten(accent, 0.15),
            _ => accent,
        };
        let mut style = scrollable::default(theme, status);
        style.vertical_rail.scroller.background = Background::Color(scroller);
        style
    }
}

pub fn panel(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(PANEL)),
        border: rounded(20.0),
        ..container::Style::default()
    }
}

/// Solid backing for a panel floating over the bars (narrow windows).
pub fn panel_backing(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(BG)),
        border: rounded(20.0),
        ..container::Style::default()
    }
}

pub fn panel_translucent(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(PANEL_TRANSLUCENT)),
        border: rounded(20.0),
        ..container::Style::default()
    }
}

/// Rounded cover art with a soft drop shadow.
pub fn cover_shadow(
    radius: f32,
    blur: f32,
    offset_y: f32,
    alpha: f32,
) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        border: rounded(radius),
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, alpha),
            offset: iced::Vector::new(0.0, offset_y),
            blur_radius: blur,
        },
        ..container::Style::default()
    }
}

pub fn menu(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(PANEL)),
        border: Border {
            radius: 16.0.into(),
            width: 1.0,
            color: CONTROL,
        },
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.5),
            offset: iced::Vector::new(0.0, 12.0),
            blur_radius: 32.0,
        },
        ..container::Style::default()
    }
}

pub fn menu_item(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let bg = match status {
            button::Status::Hovered | button::Status::Pressed => Some(CONTROL),
            _ if selected => Some(with_alpha(CONTROL, 0.6)),
            _ => None,
        };
        button::Style {
            background: bg.map(Background::Color),
            text_color: if selected { TEXT } else { TEXT_2 },
            border: rounded(10.0),
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

pub fn app_style(_: &crate::app::App, _: &Theme) -> iced::theme::Style {
    iced::theme::Style {
        background_color: BG,
        text_color: TEXT,
    }
}

/// Adds the keyboard focus ring (2px `text`) to any button style.
pub fn ringed<F>(focused: bool, style: F) -> impl Fn(&Theme, button::Status) -> button::Style
where
    F: Fn(&Theme, button::Status) -> button::Style,
{
    move |theme, status| {
        let mut s = style(theme, status);
        if focused {
            s.border.width = 2.0;
            s.border.color = TEXT;
        }
        s
    }
}

pub fn tooltip(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(color!(0x0E0E10))),
        text_color: Some(TEXT),
        border: Border {
            radius: 8.0.into(),
            width: 1.0,
            color: CONTROL,
        },
        ..container::Style::default()
    }
}

/// Destructive button; `armed` once the first click asked for confirmation.
pub fn danger_button(armed: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let red = DANGER;
        let bg = match (armed, status) {
            (true, button::Status::Hovered) => lighten(red, 0.1),
            (true, _) => red,
            (false, button::Status::Hovered | button::Status::Pressed) => with_alpha(red, 0.18),
            (false, _) => CONTROL_SUNKEN,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: if armed { BG } else { red },
            border: rounded(12.0),
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

/// Main action in a pop-up: filled with the accent.
pub fn accent_button(accent: Color) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let bg = match status {
            button::Status::Hovered => lighten(accent, 0.1),
            button::Status::Pressed => lighten(accent, 0.18),
            _ => accent,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: on_accent(accent),
            border: rounded(12.0),
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

/// A checkbox's box: accent-filled when checked, outlined when not.
pub fn check_box(on: bool, accent: Color) -> container::Style {
    container::Style {
        background: on.then_some(Background::Color(accent)),
        border: Border {
            radius: 5.0.into(),
            width: if on { 0.0 } else { 1.5 },
            color: TEXT_3,
        },
        ..container::Style::default()
    }
}

/// Dims the app behind the settings window.
pub fn settings_scrim(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.6))),
        ..container::Style::default()
    }
}

/// The settings window's outline (its columns paint its inside).
pub fn settings_window(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(CONTENT)),
        border: Border {
            radius: WINDOW_RADIUS.into(),
            width: 1.0,
            color: OUTLINE,
        },
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.5),
            offset: iced::Vector::new(0.0, 16.0),
            blur_radius: 48.0,
        },
        ..container::Style::default()
    }
}

/// A side of the settings window, with its outside corners rounded.
pub fn settings_side(left: bool) -> impl Fn(&Theme) -> container::Style {
    let radius = if left {
        iced::border::left(WINDOW_RADIUS - 1.0)
    } else {
        iced::border::right(WINDOW_RADIUS - 1.0)
    };
    move |_| container::Style {
        background: Some(Background::Color(SIDE)),
        border: Border {
            radius,
            ..Border::default()
        },
        ..container::Style::default()
    }
}

/// A plain fill (dividers).
pub fn fill(color: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(color)),
        ..container::Style::default()
    }
}

/// A group of rows in the settings window.
pub fn card(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(CARD)),
        border: rounded(14.0),
        ..container::Style::default()
    }
}

/// A small tinted tag ("Edited").
pub fn tag(accent: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(with_alpha(accent, 0.2))),
        border: rounded(9.0),
        ..container::Style::default()
    }
}

/// The preset menu.
pub fn preset_menu(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(MENU)),
        border: Border {
            radius: 14.0.into(),
            width: 1.0,
            color: DIAGRAM,
        },
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.5),
            offset: iced::Vector::new(0.0, 12.0),
            blur_radius: 32.0,
        },
        ..container::Style::default()
    }
}

/// A row of the navigation tree (or the preset menu): filled while selected.
pub fn tree_row(
    fill: Option<Color>,
    text: Color,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let bg = match (fill, status) {
            (Some(c), _) => Some(c),
            (None, button::Status::Hovered | button::Status::Pressed) => {
                Some(with_alpha(TEXT, 0.04))
            }
            (None, _) => None,
        };
        button::Style {
            background: bg.map(Background::Color),
            text_color: text,
            border: rounded(10.0),
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

/// A filled button in the settings window.
pub fn raised_button(base: Color) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let bg = match status {
            button::Status::Hovered => lighten(base, 0.06),
            button::Status::Pressed => lighten(base, 0.12),
            _ => base,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: TEXT,
            border: rounded(11.0),
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

/// The floating "Back to settings" button.
pub fn floating_button(_: &Theme, status: button::Status) -> button::Style {
    let base = with_alpha(CARD, 0.94);
    button::Style {
        background: Some(Background::Color(match status {
            button::Status::Hovered => lighten(base, 0.06),
            button::Status::Pressed => lighten(base, 0.12),
            _ => base,
        })),
        text_color: TEXT,
        border: Border {
            radius: 22.0.into(),
            width: 1.0,
            color: DIAGRAM,
        },
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.45),
            offset: iced::Vector::new(0.0, 6.0),
            blur_radius: 24.0,
        },
        snap: true,
    }
}

/// A choice with a picture: outlined with the accent while selected.
pub fn picture_card(
    selected: bool,
    accent: Color,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let base = if selected { RAISED_2 } else { RAISED };
        let bg = match status {
            button::Status::Hovered => lighten(base, 0.04),
            button::Status::Pressed => lighten(base, 0.08),
            _ => base,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: if selected { TEXT } else { TEXT_2 },
            border: Border {
                radius: 12.0.into(),
                width: if selected { 2.0 } else { 0.0 },
                color: accent,
            },
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

/// A segmented control in the settings window; the ring sits in its padding when
/// focused.
pub fn dark_trough(focused: bool) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(TROUGH)),
        border: Border {
            radius: 11.0.into(),
            width: if focused { 2.0 } else { 0.0 },
            color: TEXT,
        },
        ..container::Style::default()
    }
}

pub fn dark_segment(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let (bg, fg) = match (selected, status) {
            (_, button::Status::Disabled) => (
                selected.then_some(with_alpha(TROUGH_SELECTED, 0.45)),
                with_alpha(TEXT_2, 0.45),
            ),
            (true, _) => (Some(TROUGH_SELECTED), TEXT),
            (false, button::Status::Hovered) => (Some(with_alpha(TEXT, 0.05)), TEXT),
            (false, _) => (None, TEXT_2),
        };
        button::Style {
            background: bg.map(Background::Color),
            text_color: fg,
            border: rounded(9.0),
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

/// A text field in the settings window.
pub fn field(
    focused: bool,
    valid: bool,
) -> impl Fn(&Theme, iced::widget::text_input::Status) -> iced::widget::text_input::Style {
    use iced::widget::text_input::{Status, Style};
    move |_, status| {
        let focused = focused || matches!(status, Status::Focused { .. });
        Style {
            background: Background::Color(FIELD),
            border: Border {
                radius: 11.0.into(),
                width: if focused { 2.0 } else { 1.0 },
                color: if !valid {
                    DANGER
                } else if focused {
                    TEXT
                } else {
                    OUTLINE
                },
            },
            icon: TEXT_2,
            placeholder: TEXT_3,
            value: TEXT,
            selection: with_alpha(ACCENT, 0.4),
        }
    }
}

/// Dims everything behind a pop-up.
pub fn scrim(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(with_alpha(BG, 0.6))),
        ..container::Style::default()
    }
}

/// The playback bar as a card. `opacity` is how solid, `alpha` fades it for auto-hide.
pub fn playback_card(opacity: f32, alpha: f32) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(with_alpha(PANEL, opacity * alpha))),
        border: Border {
            radius: 20.0.into(),
            width: 1.0,
            color: with_alpha(OUTLINE, opacity * alpha),
        },
        ..container::Style::default()
    }
}

/// A play button in a filled circle.
pub fn play_circle(radius: f32, alpha: f32) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let bg = match status {
            button::Status::Hovered => Color::WHITE,
            button::Status::Pressed => BAR,
            button::Status::Disabled => with_alpha(TEXT, 0.35),
            button::Status::Active => TEXT,
        };
        button::Style {
            background: Some(Background::Color(with_alpha(bg, alpha))),
            text_color: BG,
            border: rounded(radius),
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

/// The accent, dark enough for white text on it.
pub fn deepened(accent: Color) -> Color {
    let linear = |c: f32| {
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let light = |c: Color| 0.2126 * linear(c.r) + 0.7152 * linear(c.g) + 0.0722 * linear(c.b);
    let mut out = accent;
    // White on it has a contrast of 4.5 from here down.
    while light(out) > 0.18 {
        out = Color {
            r: out.r * 0.97,
            g: out.g * 0.97,
            b: out.b * 0.97,
            a: out.a,
        };
    }
    out
}

/// The quick settings.
pub fn quick_panel(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(with_alpha(PANEL, 0.96))),
        border: Border {
            radius: 20.0.into(),
            width: 1.0,
            color: OUTLINE,
        },
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.5),
            offset: iced::Vector::new(0.0, 12.0),
            blur_radius: 32.0,
        },
        ..container::Style::default()
    }
}

/// A switch in them, and its icon.
pub fn quick_tile_button(
    on: bool,
    accent: Color,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let base = if on { accent } else { RAISED_2 };
        let bg = match status {
            button::Status::Hovered => lighten(base, 0.06),
            button::Status::Pressed => lighten(base, 0.12),
            _ => base,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: TEXT,
            border: rounded(11.0),
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

pub fn quick_badge(on: bool) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(with_alpha(
            TEXT,
            if on { 0.22 } else { 0.10 },
        ))),
        border: rounded(14.0),
        ..container::Style::default()
    }
}

/// A key of a shortcut: lit while it waits for a new one.
pub fn key_button(
    waiting: bool,
    accent: Color,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let mut s = raised_button(RAISED_2)(theme, status);
        if waiting {
            s.background = Some(Background::Color(with_alpha(accent, 0.2)));
            s.border.width = 1.0;
            s.border.color = accent;
        }
        s
    }
}

/// The volume popover.
pub fn popover(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(CONTROL_ON_ART)),
        border: Border {
            radius: 24.0.into(),
            width: 0.0,
            color: Color::TRANSPARENT,
        },
        ..container::Style::default()
    }
}

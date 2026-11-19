//! Design tokens (colors, fonts) and shared widget styles.

use iced::font::Weight;
use iced::widget::{button, container};
use iced::{Background, Border, Color, Font, Shadow, Theme, color};

// Surfaces use the eight grays in assets/ui.ase, darkest first:
// 131316 1C1C21 26262C 2F3037 393A41 4B4C52 5B5C62 6A6B70.
pub const BG: Color = color!(0x131316);
pub const PANEL: Color = color!(0x1C1C21);
pub const CONTROL: Color = color!(0x2F3037);
pub const CONTROL_SUNKEN: Color = color!(0x26262C);
pub const SEGMENT_SELECTED: Color = color!(0x4B4C52);
pub const TRACK: Color = color!(0x393A41);
pub const ACCENT: Color = color!(0x4B7BF5);
pub const BAR: Color = color!(0xE4E4E7);
pub const SEEK_HANDLE: Color = color!(0xD9D9DE);
pub const TEXT: Color = color!(0xF5F5F7);
pub const TEXT_2: Color = color!(0xA0A0A8);
pub const TEXT_3: Color = color!(0x8E8E96);
pub const ICON_OFF: Color = color!(0x6A6B70);
pub const ZERO_TICK: Color = color!(0x5B5C62);
pub const CUSTOM_RING: Color = color!(0x4B4C52);
/// Higher-contrast track-info text over album-art / ambient backgrounds.
pub const TEXT_2_ON_ART: Color = color!(0xD0D0D6);
pub const TEXT_3_ON_ART: Color = color!(0xC4C4CC);
/// Inactive header buttons over a dynamic background.
pub const CONTROL_ON_ART: Color = Color::from_rgba8(0x2F, 0x30, 0x37, 0.85);
/// Appearance panel: translucent so the moving background shows through.
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

/// Header button: accent fill when active; translucent over dynamic backgrounds.
/// `alpha` fades it for auto-hide.
pub fn header_button(
    active: bool,
    accent: Color,
    over_art: bool,
    radius: f32,
    alpha: f32,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let base = match (active, over_art) {
            (true, _) => accent,
            (false, true) => CONTROL_ON_ART,
            (false, false) => CONTROL,
        };
        let bg = match status {
            button::Status::Hovered => lighten(base, 0.06),
            button::Status::Pressed => lighten(base, 0.12),
            _ => base,
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

pub fn segment(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let (bg, fg) = match (selected, status) {
            (_, button::Status::Disabled) => (
                selected.then_some(with_alpha(SEGMENT_SELECTED, 0.45)),
                with_alpha(TEXT_2, 0.45),
            ),
            (true, _) => (Some(SEGMENT_SELECTED), TEXT),
            (false, button::Status::Hovered) => (Some(with_alpha(TEXT, 0.04)), TEXT),
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

/// Segmented-control trough; the ring sits in its 3px padding when focused.
pub fn trough_ring(focused: bool) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let mut s = trough(theme);
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

/// A preset row: highlighted when it matches the current settings.
pub fn preset_row(current: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let base = if current {
            Color::from_rgba8(75, 123, 245, 0.18)
        } else {
            CONTROL_SUNKEN
        };
        let bg = match status {
            button::Status::Hovered => lighten(base, 0.05),
            button::Status::Pressed => lighten(base, 0.1),
            _ => base,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: TEXT,
            border: rounded(12.0),
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

/// Destructive button; `armed` once the first click asked for confirmation.
pub fn danger_button(armed: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let red = color!(0xFF6B57);
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

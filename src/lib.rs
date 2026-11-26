//! Undavi: a desktop music visualizer with synced lyrics.
//!
//! The app itself starts with [`run`]. The modules below are the reusable,
//! UI-free parts, documented as a library so they can be tested and built on:
//!
//! | Module | What it does |
//! |---|---|
//! | [`audio`] | Captures system audio (or any input) and turns it into spectra. |
//! | [`spectrum`] | Turns spectra into animated bar heights, beats and a noise gate. |
//! | [`settings`] | The settings model, presets, and loading/saving them. |
//! | [`lyrics`] | Parses LRC and Lyricsfile lyrics and fetches them from LRCLIB. |
//! | [`media`] | What the music player is playing, and controlling it. |
//!
//! The data flows one way, once per frame:
//!
//! ```text
//! audio::Capture ─▶ audio::Analyzer ─▶ spectrum::Gate ─▶ spectrum::Processor ─▶ bars
//!                                          └──────────▶ spectrum::BeatGlow ──▶ glow, pulse
//! ```
//!
//! See `docs/architecture.md` in the repository for the whole picture.

#![warn(missing_docs)]

pub mod audio;
pub mod lyrics;
pub mod media;
pub mod settings;
pub mod spectrum;

// The app: state, views and drawing. Internal, not part of the library API.
mod about;
mod app;
mod art;
mod backdrop;
mod background;
mod controls;
mod focus;
mod icons;
mod karaoke;
mod layout;
mod meter;
mod monitor;
mod platform;
mod shaders;
mod theme;
mod update;
mod view;
mod visualizer;

use iced::{Size, window};

/// Matches the `.desktop` file and icon names, so Wayland compositors and taskbars
/// can find the app's icon.
#[cfg(target_os = "linux")]
const APP_ID: &str = "undavi";

/// The window icon, for X11 and Windows (Wayland uses the `.desktop` file's icon).
fn window_icon() -> Option<window::Icon> {
    let png = include_bytes!("../assets/icons/png/undavi-256.png");
    let img = image::load_from_memory_with_format(png, image::ImageFormat::Png)
        .ok()?
        .into_rgba8();
    let (w, h) = img.dimensions();
    window::icon::from_rgba(img.into_raw(), w, h).ok()
}

/// Print to the terminal Undavi was started from, if it was. (Windows doesn't connect
/// an app without a console window of its own to it.)
#[cfg(windows)]
pub fn attach_console() {
    use windows::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};

    // SAFETY: a plain call. It fails without a terminal, which is fine.
    let _ = unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
}

/// Draw with DirectX 12 on Windows, where the GPU has it, unless `WGPU_BACKEND` says
/// otherwise. wgpu would pick Vulkan first, and with NVIDIA's Vulkan the desktop could
/// show through the window, or a white patch cover it, for a few seconds when it went
/// full screen.
#[cfg(windows)]
fn prefer_direct3d() {
    use iced::wgpu;

    if std::env::var_os("WGPU_BACKEND").is_some() {
        return;
    }
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::DX12,
        ..Default::default()
    });
    // (Windows always has a DirectX 12 that runs on the processor.)
    let has_gpu = instance
        .enumerate_adapters(wgpu::Backends::DX12)
        .iter()
        .any(|a| a.get_info().device_type != wgpu::DeviceType::Cpu);
    if has_gpu {
        // SAFETY: changing the environment is always safe on Windows (see `set_var`).
        unsafe { std::env::set_var("WGPU_BACKEND", "dx12") };
    }
}

/// Open the Undavi window and run until it's closed.
///
/// # Errors
///
/// Fails if no window or graphics device could be created.
pub fn run() -> iced::Result {
    #[cfg(windows)]
    prefer_direct3d();
    let mut app = iced::application(app::App::new, app::App::update, app::App::view)
        .title(app::App::title)
        .subscription(app::App::subscription)
        .style(theme::app_style)
        .theme(iced::Theme::Dark)
        .default_font(theme::INTER)
        .antialiasing(true)
        .window(window::Settings {
            size: Size::new(1440.0, 900.0),
            // The layout adapts down to about this (see layout.rs).
            min_size: Some(Size::new(480.0, 520.0)),
            icon: window_icon(),
            #[cfg(target_os = "linux")]
            platform_specific: window::settings::PlatformSpecific {
                application_id: APP_ID.into(),
                ..Default::default()
            },
            ..window::Settings::default()
        });
    for font in theme::FONTS {
        app = app.font(font);
    }
    app.run()
}

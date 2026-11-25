//! What the current platform and session support, so the UI can leave out
//! controls that wouldn't do anything, and the system's reduced-motion setting.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Detected once at startup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Platform {
    /// Now-playing info and transport controls (see `media`). Without it there's no
    /// track, cover or lyrics, so those options are hidden too.
    pub media: bool,
    /// Keeping the window above others. Wayland has no way for an app to ask.
    pub always_on_top: bool,
    /// Installing new releases: there are release builds for x86-64 Linux and Windows.
    pub updates: bool,
}

impl Platform {
    pub fn detect() -> Self {
        Self {
            media: cfg!(any(target_os = "linux", windows)),
            always_on_top: !on_wayland(),
            updates: cfg!(all(
                any(target_os = "linux", windows),
                target_arch = "x86_64"
            )),
        }
    }

    /// Everything on, for tests and snapshots that should look the same anywhere.
    #[cfg(test)]
    pub fn full() -> Self {
        Self {
            media: true,
            always_on_top: true,
            updates: true,
        }
    }
}

/// Whether the window will be a Wayland one. winit (0.30) uses Wayland whenever
/// `WAYLAND_DISPLAY` or `WAYLAND_SOCKET` is set and non-empty.
fn on_wayland() -> bool {
    let set = |name| std::env::var_os(name).is_some_and(|v| !v.is_empty());
    cfg!(target_os = "linux") && (set("WAYLAND_DISPLAY") || set("WAYLAND_SOCKET"))
}

/// The system's reduced-motion setting, as last read. (Linux and Windows so far; off
/// everywhere else.)
#[derive(Clone, Debug, Default)]
pub struct ReducedMotion(Arc<AtomicBool>);

impl ReducedMotion {
    pub fn on(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    /// Read it again, on a thread of its own (the desktop can take a moment to
    /// answer).
    pub fn refresh(&self) {
        // Tests look the same on any desktop.
        #[cfg(all(any(target_os = "linux", windows), not(test)))]
        {
            let flag = self.0.clone();
            let _ = std::thread::Builder::new()
                .name("undavi-motion".into())
                .spawn(move || flag.store(reduced_motion().unwrap_or(false), Ordering::Relaxed));
        }
    }
}

/// Ask the desktop portal: the accessibility setting where it's supported, or
/// whether animations are turned off. `None` without an answer.
#[cfg(all(target_os = "linux", not(test)))]
fn reduced_motion() -> Option<bool> {
    use dbus::arg::{RefArg, Variant};
    use dbus::blocking::Connection;

    /// The number (or 0/1 for a boolean) inside any number of variants.
    fn number(value: &dyn RefArg) -> Option<i64> {
        value
            .as_i64()
            .or_else(|| value.as_iter()?.next().and_then(number))
    }

    let connection = Connection::new_session().ok()?;
    let portal = connection.with_proxy(
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        std::time::Duration::from_millis(500),
    );
    let read = |namespace: &str, key: &str| -> Option<i64> {
        let (value,): (Variant<Box<dyn RefArg>>,) = portal
            .method_call("org.freedesktop.portal.Settings", "Read", (namespace, key))
            .ok()?;
        number(&value.0)
    };
    if read("org.freedesktop.appearance", "reduced-motion") == Some(1) {
        return Some(true);
    }
    read("org.gnome.desktop.interface", "enable-animations").map(|on| on == 0)
}

/// Whether "Animation effects" is off (Settings, Accessibility, Visual effects).
#[cfg(all(windows, not(test)))]
fn reduced_motion() -> Option<bool> {
    use windows::Win32::UI::WindowsAndMessaging::{
        SPI_GETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
    };
    use windows::core::BOOL;

    let mut animations = BOOL(1);
    // SAFETY: this setting is written to a BOOL.
    unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some((&raw mut animations).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS::default(),
        )
    }
    .ok()?;
    Some(!animations.as_bool())
}

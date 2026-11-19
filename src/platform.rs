//! What the current platform and session support, so the UI can leave out
//! controls that wouldn't do anything.

/// Detected once at startup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Platform {
    /// Now-playing info and transport controls (MPRIS; Linux only so far). Without
    /// it there's no track, cover or lyrics, so those options are hidden too.
    pub media: bool,
    /// Keeping the window above others. Wayland has no way for an app to ask.
    pub always_on_top: bool,
}

impl Platform {
    pub fn detect() -> Self {
        Self {
            media: cfg!(target_os = "linux"),
            always_on_top: !on_wayland(),
        }
    }

    /// Everything on, for tests and snapshots that should look the same anywhere.
    #[cfg(test)]
    pub fn full() -> Self {
        Self {
            media: true,
            always_on_top: true,
        }
    }
}

/// Whether the window will be a Wayland one. winit (0.30) uses Wayland whenever
/// `WAYLAND_DISPLAY` or `WAYLAND_SOCKET` is set and non-empty.
fn on_wayland() -> bool {
    let set = |name| std::env::var_os(name).is_some_and(|v| !v.is_empty());
    cfg!(target_os = "linux") && (set("WAYLAND_DISPLAY") || set("WAYLAND_SOCKET"))
}

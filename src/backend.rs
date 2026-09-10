use std::env;

/// Which compositor protocol is actually running underneath.
///
/// Application code should rarely need to branch on this directly — it
/// exists so [`crate::connect`] can pick the right [`crate::Compositor`]
/// implementation, and so callers can report *why* a given operation was
/// unsupported (e.g. moving a window is not possible on plain Wayland).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// Hyprland specifically, detected via `HYPRLAND_INSTANCE_SIGNATURE`.
    /// Hyprland is a Wayland compositor, but it also exposes its own
    /// `hyprctl` socket IPC, which is the only way this crate can offer
    /// exact window positioning/sizing on Wayland at all — the standard
    /// protocols this crate otherwise speaks don't allow it.
    Hyprland,
    /// niri specifically, detected via `NIRI_SOCKET`. Like Hyprland, niri
    /// is a Wayland compositor with its own JSON IPC (`niri msg`), used
    /// here instead of standard protocols for the same reason: it can do
    /// things (exact floating-window position/size) those can't.
    Niri,
    /// Any other Wayland compositor (Sway, Wayfire, river, dwl, KDE,
    /// GNOME, ...), reached through standard extension protocols.
    Wayland,
    X11,
}

impl Backend {
    /// Detect the backend from the standard environment variables.
    ///
    /// Compositor-specific IPC signals (`HYPRLAND_INSTANCE_SIGNATURE`,
    /// `NIRI_SOCKET`) take precedence over plain `WAYLAND_DISPLAY`, since
    /// those compositors set both; `WAYLAND_DISPLAY` in turn takes
    /// precedence over `DISPLAY`, matching how most toolkits choose a
    /// backend when several are present (e.g. an XWayland session).
    pub fn detect() -> Option<Self> {
        if env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() {
            Some(Backend::Hyprland)
        } else if env::var_os("NIRI_SOCKET").is_some() {
            Some(Backend::Niri)
        } else if env::var_os("WAYLAND_DISPLAY").is_some() {
            Some(Backend::Wayland)
        } else if env::var_os("DISPLAY").is_some() {
            Some(Backend::X11)
        } else {
            None
        }
    }
}

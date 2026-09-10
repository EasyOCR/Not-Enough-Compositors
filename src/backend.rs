use std::env;

/// Which compositor protocol is actually running underneath.
///
/// Application code should rarely need to branch on this directly — it
/// exists so [`crate::connect`] can pick the right [`crate::Compositor`]
/// implementation, and so callers can report *why* a given operation was
/// unsupported (e.g. moving a window is not possible on Wayland).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Wayland,
    X11,
}

impl Backend {
    /// Detect the backend from the standard environment variables.
    ///
    /// `WAYLAND_DISPLAY` takes precedence, matching how most toolkits
    /// choose a backend when both are present (e.g. an XWayland session).
    pub fn detect() -> Option<Self> {
        if env::var_os("WAYLAND_DISPLAY").is_some() {
            Some(Backend::Wayland)
        } else if env::var_os("DISPLAY").is_some() {
            Some(Backend::X11)
        } else {
            None
        }
    }
}

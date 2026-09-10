/// Backend-specific handle to a window. Opaque to callers; obtained from
/// [`crate::Compositor::list_windows`] and passed back into the other
/// `Compositor` methods.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WindowId(pub(crate) Backing);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum Backing {
    #[cfg(feature = "x11")]
    X11(u32),
    #[cfg(feature = "wayland")]
    Wayland(crate::wayland::WaylandBacking),
    /// A Hyprland client "address" (e.g. `0x5578...`), as reported by
    /// `hyprctl clients -j` and reused verbatim in dispatch commands.
    #[cfg(feature = "hyprland")]
    Hyprland(String),
    /// A niri window id, as reported by `Request::Windows` and reused
    /// verbatim in `Action` requests.
    #[cfg(feature = "niri")]
    Niri(u64),
}

/// A window's on-screen position and size, in the compositor's own
/// coordinate space. `None` on backends that don't expose it (plain
/// Wayland does not tell foreign clients window geometry).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Geometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Everything Not Enough Compositors knows about one window, gathered
/// through whichever backend is active.
#[derive(Debug, Clone)]
pub struct WindowInfo {
    pub id: WindowId,
    pub title: String,
    /// `WM_CLASS` on X11, `app_id` on Wayland — the application identifier,
    /// not a per-window title.
    pub app_id: String,
    pub geometry: Option<Geometry>,
}

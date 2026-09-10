/// Errors returned by [`crate::connect`] and [`crate::Compositor`] methods.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Neither `WAYLAND_DISPLAY` nor `DISPLAY` is set, so there is no
    /// compositor to connect to.
    #[error("no display server found (WAYLAND_DISPLAY and DISPLAY are both unset)")]
    NoDisplay,

    /// The detected backend was compiled out via Cargo features.
    #[error("support for the {0:?} backend was not compiled in (enable the matching feature)")]
    BackendDisabled(crate::Backend),

    /// Could not open a connection to the compositor at all.
    #[error("failed to connect to the compositor: {0}")]
    ConnectionFailed(String),

    /// The operation is not exposed by this compositor's protocol at all.
    ///
    /// This is the normal, expected outcome for things like moving or
    /// resizing another app's window under plain Wayland: the protocol
    /// deliberately does not let one client reposition another's surface.
    /// Callers that need the operation to always succeed should treat this
    /// as "not on this backend" rather than a bug.
    #[error("{operation} is not supported on {backend:?}: {reason}")]
    Unsupported {
        backend: crate::Backend,
        operation: &'static str,
        reason: &'static str,
    },

    /// The window id no longer refers to a live window.
    #[error("window not found")]
    WindowGone,

    /// A lower-level protocol error talking to the compositor.
    #[error("compositor protocol error: {0}")]
    Protocol(String),
}

pub type Result<T> = std::result::Result<T, Error>;

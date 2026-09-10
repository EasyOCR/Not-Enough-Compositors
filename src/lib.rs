//! Not Enough Compositors: one API for controlling windows across compositors.
//!
//! The problem this solves: an app that needs to list, focus, move, or
//! close *another* app's window has to talk to the compositor, and every
//! compositor speaks a different protocol for that — X11's EWMH hints, or
//! Wayland's (compositor-specific, often absent) foreign-toplevel
//! extensions. Without this crate, supporting both means writing and
//! maintaining two integrations. With it, you call [`connect`], get back
//! a [`Compositor`], and write your logic once.
//!
//! ```no_run
//! fn main() -> Result<(), nec::Error> {
//!     let mut compositor = nec::connect()?;
//!     for window in compositor.list_windows()? {
//!         println!("{}: {}", window.app_id, window.title);
//!     }
//!     Ok(())
//! }
//! ```
//!
//! ## Honest limits
//!
//! Wayland's security model deliberately does not let one app reposition
//! or resize another's window, and window listing/focus/close only work
//! at all on compositors implementing an extension protocol for it
//! (`org_kde_plasma_window_management`, `wlr-foreign-toplevel-management`,
//! or `ext-foreign-toplevel-list-v1` — see the crate source for which is
//! used where). Calls that aren't possible on the active backend return
//! [`Error::Unsupported`] rather than silently doing nothing, so calling
//! code can detect and handle the gap instead of assuming success.
//! Hyprland is the one exception: its own `hyprctl` IPC allows exact
//! move/resize where nothing else on Wayland can.

mod backend;
mod error;
mod window;

#[cfg(feature = "hyprland")]
mod hyprland;
#[cfg(feature = "wayland")]
mod wayland;
#[cfg(feature = "x11")]
mod x11;

pub use backend::Backend;
pub use error::{Error, Result};
pub use window::{Geometry, WindowId, WindowInfo};

/// One API for window control, backed by whichever compositor is actually
/// running. Obtain an implementation with [`connect`].
pub trait Compositor {
    /// Which backend this implementation is talking to.
    fn backend(&self) -> Backend;

    /// Enumerate the windows the compositor is willing to disclose.
    ///
    /// On Wayland this requires the compositor to implement
    /// `wlr-foreign-toplevel-management`; compositors that don't (GNOME,
    /// KDE) will surface that as [`Error::Unsupported`], not an empty list.
    fn list_windows(&mut self) -> Result<Vec<WindowInfo>>;

    /// Bring a window to the foreground and give it input focus.
    fn focus_window(&mut self, id: &WindowId) -> Result<()>;

    /// Ask a window to close, the same way clicking its titlebar close
    /// button would.
    fn close_window(&mut self, id: &WindowId) -> Result<()>;

    /// Move a window to `(x, y)` in the compositor's coordinate space.
    ///
    /// Returns [`Error::Unsupported`] on plain Wayland: the protocol has
    /// no operation for one client to reposition another's surface.
    fn move_window(&mut self, id: &WindowId, x: i32, y: i32) -> Result<()>;

    /// Resize a window to `width` x `height`.
    ///
    /// Returns [`Error::Unsupported`] on plain Wayland, for the same
    /// reason as [`Compositor::move_window`].
    fn resize_window(&mut self, id: &WindowId, width: u32, height: u32) -> Result<()>;
}

/// Detect the running compositor and connect to it.
///
/// This is the crate's main entry point: callers write against the
/// returned [`Compositor`] trait object and never need to know or check
/// which backend ended up behind it, beyond handling
/// [`Error::Unsupported`] for operations a given backend can't do.
pub fn connect() -> Result<Box<dyn Compositor>> {
    match Backend::detect().ok_or(Error::NoDisplay)? {
        Backend::Hyprland => connect_hyprland(),

        #[cfg(feature = "wayland")]
        Backend::Wayland => Ok(Box::new(wayland::WaylandCompositor::connect()?)),
        #[cfg(not(feature = "wayland"))]
        Backend::Wayland => Err(Error::BackendDisabled(Backend::Wayland)),

        #[cfg(feature = "x11")]
        Backend::X11 => Ok(Box::new(x11::X11Compositor::connect()?)),
        #[cfg(not(feature = "x11"))]
        Backend::X11 => Err(Error::BackendDisabled(Backend::X11)),
    }
}

/// Hyprland also implements `wlr-foreign-toplevel-management`, so if its
/// dedicated IPC backend is disabled or fails to reach the socket, fall
/// back to the generic Wayland backend rather than failing outright —
/// callers lose exact move/resize but keep list/focus/close.
#[allow(clippy::needless_return)]
fn connect_hyprland() -> Result<Box<dyn Compositor>> {
    #[cfg(feature = "hyprland")]
    {
        match hyprland::HyprlandCompositor::connect() {
            Ok(compositor) => return Ok(Box::new(compositor)),
            #[allow(unused_variables)]
            Err(hyprland_err) => {
                #[cfg(feature = "wayland")]
                {
                    return Ok(Box::new(wayland::WaylandCompositor::connect()?));
                }
                #[cfg(not(feature = "wayland"))]
                {
                    return Err(hyprland_err);
                }
            }
        }
    }

    #[cfg(not(feature = "hyprland"))]
    {
        #[cfg(feature = "wayland")]
        {
            return Ok(Box::new(wayland::WaylandCompositor::connect()?));
        }
        #[cfg(not(feature = "wayland"))]
        {
            return Err(Error::BackendDisabled(Backend::Hyprland));
        }
    }
}

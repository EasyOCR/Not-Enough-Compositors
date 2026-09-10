//! niri backend, built on niri's own JSON IPC (`niri msg` under the hood)
//! rather than a Wayland protocol.
//!
//! Like Hyprland, niri already speaks enough standard Wayland extension
//! surface to be listable through the generic backend in some cases, but
//! this dedicated backend exists for the same reason the Hyprland one
//! does: niri's IPC can set a **floating** window's exact position and
//! size (`Action::MoveFloatingWindow`/`SetWindowWidth`/`SetWindowHeight`
//! with `SetFixed`), which no standard Wayland protocol allows on any
//! compositor.
//!
//! One real limitation, inherent to niri's design rather than a gap here:
//! niri is a scrollable-tiling compositor, and most windows live in its
//! tiling layout, not the floating one. `move_window`/`resize_window`
//! only make sense for floating windows — calling them on a tiled window
//! is accepted by the IPC but may not do anything visible, since a tiled
//! window's position is a function of its column, not free placement.
//! niri's IPC gives no way to detect "floating vs tiled" failure from the
//! call itself, so this backend can't turn that into an `Unsupported`
//! error the way it does for protocol-level gaps elsewhere.
//!
//! Implemented against niri's documented IPC (`niri-ipc` crate) but not
//! exercised against a live niri session — niri, like Hyprland, needs a
//! GPU-backed compositor build this environment couldn't provide. Treat
//! it as needing real-world confirmation before depending on it.
//!
//! `niri-ipc` is GPL-3.0-or-later, unlike the rest of this crate's
//! dependencies — see the `niri` feature's comment in `Cargo.toml`.

use crate::{Backend, Compositor, Error, Result, WindowId, WindowInfo};
use niri_ipc::socket::Socket;
use niri_ipc::{Action, PositionChange, Request, Response, SizeChange};

pub struct NiriCompositor {
    socket: Socket,
}

impl NiriCompositor {
    pub(crate) fn connect() -> Result<Self> {
        let socket = Socket::connect()
            .map_err(|e| Error::ConnectionFailed(format!("connecting to niri socket: {e}")))?;
        Ok(NiriCompositor { socket })
    }

    fn request(&mut self, request: Request) -> Result<Response> {
        self.socket
            .send(request)
            .map_err(|e| Error::Protocol(format!("niri IPC I/O error: {e}")))?
            .map_err(Error::Protocol)
    }

    fn dispatch(&mut self, action: Action) -> Result<()> {
        match self.request(Request::Action(action))? {
            Response::Handled => Ok(()),
            other => Err(Error::Protocol(format!(
                "unexpected reply from niri action request: {other:?}"
            ))),
        }
    }

    fn id(id: &WindowId) -> Result<u64> {
        match &id.0 {
            crate::window::Backing::Niri(id) => Ok(*id),
            #[allow(unreachable_patterns)]
            _ => Err(Error::WindowGone),
        }
    }
}

impl Compositor for NiriCompositor {
    fn backend(&self) -> Backend {
        Backend::Niri
    }

    fn list_windows(&mut self) -> Result<Vec<WindowInfo>> {
        match self.request(Request::Windows)? {
            Response::Windows(windows) => Ok(windows
                .into_iter()
                .map(|w| WindowInfo {
                    id: WindowId(crate::window::Backing::Niri(w.id)),
                    title: w.title.unwrap_or_default(),
                    app_id: w.app_id.unwrap_or_default(),
                    // niri reports window/tile size but not absolute
                    // screen-space position, so this can't be filled in
                    // with the same semantics as the other backends.
                    geometry: None,
                })
                .collect()),
            other => Err(Error::Protocol(format!(
                "unexpected reply from niri windows request: {other:?}"
            ))),
        }
    }

    fn focus_window(&mut self, id: &WindowId) -> Result<()> {
        let id = Self::id(id)?;
        self.dispatch(Action::FocusWindow { id })
    }

    fn close_window(&mut self, id: &WindowId) -> Result<()> {
        let id = Self::id(id)?;
        self.dispatch(Action::CloseWindow { id: Some(id) })
    }

    fn move_window(&mut self, id: &WindowId, x: i32, y: i32) -> Result<()> {
        let id = Self::id(id)?;
        self.dispatch(Action::MoveFloatingWindow {
            id: Some(id),
            x: PositionChange::SetFixed(x as f64),
            y: PositionChange::SetFixed(y as f64),
        })
    }

    fn resize_window(&mut self, id: &WindowId, width: u32, height: u32) -> Result<()> {
        let id = Self::id(id)?;
        self.dispatch(Action::SetWindowWidth {
            id: Some(id),
            change: SizeChange::SetFixed(width as i32),
        })?;
        self.dispatch(Action::SetWindowHeight {
            id: Some(id),
            change: SizeChange::SetFixed(height as i32),
        })
    }
}

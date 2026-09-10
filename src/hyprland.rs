//! Hyprland backend, built on `hyprctl`'s own socket IPC rather than a
//! Wayland protocol.
//!
//! Hyprland already speaks `wlr-foreign-toplevel-management` like other
//! wlroots-based compositors, so the generic [`crate::wayland`] backend
//! would work on it too — but this backend exists because Hyprland's IPC
//! additionally supports **exact** window positioning and sizing
//! (`movewindowpixel exact`/`resizewindowpixel exact`), which no standard
//! Wayland protocol allows on any compositor. Where `move_window` and
//! `resize_window` return [`Error::Unsupported`] everywhere else on
//! Wayland, they actually work here.
//!
//! This is implemented against Hyprland's documented IPC (one command per
//! connection to `$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/.socket.sock`,
//! plain text in, plain text or JSON out) but has not been exercised
//! against a live Hyprland session — Hyprland requires a GPU-backed
//! wlroots build this environment couldn't provide. Treat it as needing
//! real-world confirmation before depending on it.

use crate::{Backend, Compositor, Error, Geometry, Result, WindowId, WindowInfo};
use std::env;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

#[derive(serde::Deserialize)]
struct HyprClient {
    address: String,
    at: (i32, i32),
    size: (u32, u32),
    title: String,
    class: String,
}

pub struct HyprlandCompositor {
    socket_path: PathBuf,
}

impl HyprlandCompositor {
    pub(crate) fn connect() -> Result<Self> {
        let signature = env::var("HYPRLAND_INSTANCE_SIGNATURE")
            .map_err(|_| Error::ConnectionFailed("HYPRLAND_INSTANCE_SIGNATURE is not set".into()))?;
        let runtime_dir = env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .ok_or_else(|| Error::ConnectionFailed("XDG_RUNTIME_DIR is not set".into()))?;
        let socket_path = runtime_dir.join("hypr").join(&signature).join(".socket.sock");
        if !socket_path.exists() {
            return Err(Error::ConnectionFailed(format!(
                "hyprctl socket not found at {}",
                socket_path.display()
            )));
        }
        Ok(HyprlandCompositor { socket_path })
    }

    /// Send one command over a fresh connection and return the raw reply.
    /// Hyprland's IPC is one command per connection: it replies, then
    /// closes its end, so reading to EOF gives the complete response.
    fn command(&self, cmd: &str) -> Result<String> {
        let mut stream = UnixStream::connect(&self.socket_path)
            .map_err(|e| Error::Protocol(format!("connecting to hyprctl socket: {e}")))?;
        stream
            .write_all(cmd.as_bytes())
            .map_err(|e| Error::Protocol(format!("writing to hyprctl socket: {e}")))?;
        stream
            .shutdown(std::net::Shutdown::Write)
            .map_err(|e| Error::Protocol(format!("shutting down hyprctl socket: {e}")))?;
        let mut reply = String::new();
        stream
            .read_to_string(&mut reply)
            .map_err(|e| Error::Protocol(format!("reading from hyprctl socket: {e}")))?;
        Ok(reply)
    }

    /// Send a `dispatch` command and treat anything other than `ok` as failure.
    fn dispatch(&self, cmd: &str) -> Result<()> {
        let reply = self.command(&format!("dispatch {cmd}"))?;
        if reply.trim() == "ok" {
            Ok(())
        } else {
            Err(Error::Protocol(format!("hyprctl dispatch {cmd:?} failed: {reply}")))
        }
    }

    fn address(id: &WindowId) -> Result<&str> {
        match &id.0 {
            crate::window::Backing::Hyprland(address) => Ok(address),
            #[allow(unreachable_patterns)]
            _ => Err(Error::WindowGone),
        }
    }
}

impl Compositor for HyprlandCompositor {
    fn backend(&self) -> Backend {
        Backend::Hyprland
    }

    fn list_windows(&mut self) -> Result<Vec<WindowInfo>> {
        let reply = self.command("j/clients")?;
        let clients: Vec<HyprClient> = serde_json::from_str(&reply)
            .map_err(|e| Error::Protocol(format!("parsing hyprctl clients JSON: {e}")))?;
        Ok(clients
            .into_iter()
            .map(|c| WindowInfo {
                id: WindowId(crate::window::Backing::Hyprland(c.address)),
                title: c.title,
                app_id: c.class,
                geometry: Some(Geometry {
                    x: c.at.0,
                    y: c.at.1,
                    width: c.size.0,
                    height: c.size.1,
                }),
            })
            .collect())
    }

    fn focus_window(&mut self, id: &WindowId) -> Result<()> {
        let address = Self::address(id)?;
        self.dispatch(&format!("focuswindow address:{address}"))
    }

    fn close_window(&mut self, id: &WindowId) -> Result<()> {
        let address = Self::address(id)?;
        self.dispatch(&format!("closewindow address:{address}"))
    }

    fn move_window(&mut self, id: &WindowId, x: i32, y: i32) -> Result<()> {
        let address = Self::address(id)?;
        self.dispatch(&format!("movewindowpixel exact {x} {y},address:{address}"))
    }

    fn resize_window(&mut self, id: &WindowId, width: u32, height: u32) -> Result<()> {
        let address = Self::address(id)?;
        self.dispatch(&format!(
            "resizewindowpixel exact {width} {height},address:{address}"
        ))
    }
}

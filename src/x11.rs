//! X11 backend, implemented against the EWMH hints most window managers
//! (and no others, notably some minimal WMs) publish on the root window:
//! `_NET_CLIENT_LIST`, `_NET_ACTIVE_WINDOW`, `_NET_CLOSE_WINDOW`, and
//! `_NET_MOVERESIZE_WINDOW`.

use crate::window::Backing;
use crate::{Backend, Compositor, Error, Geometry, Result, WindowId, WindowInfo};
use x11rb::connection::Connection as _;
use x11rb::protocol::xproto::{AtomEnum, ClientMessageEvent, ConnectionExt, EventMask, Window};
use x11rb::rust_connection::RustConnection;

pub struct X11Compositor {
    conn: RustConnection,
    root: Window,
    atoms: Atoms,
}

struct Atoms {
    net_client_list: u32,
    net_wm_name: u32,
    net_active_window: u32,
    net_close_window: u32,
    net_moveresize_window: u32,
    utf8_string: u32,
}

impl X11Compositor {
    pub(crate) fn connect() -> Result<Self> {
        let (conn, screen_num) =
            x11rb::connect(None).map_err(|e| Error::ConnectionFailed(e.to_string()))?;
        let root = conn.setup().roots[screen_num].root;

        let atoms = Atoms {
            net_client_list: intern(&conn, "_NET_CLIENT_LIST")?,
            net_wm_name: intern(&conn, "_NET_WM_NAME")?,
            net_active_window: intern(&conn, "_NET_ACTIVE_WINDOW")?,
            net_close_window: intern(&conn, "_NET_CLOSE_WINDOW")?,
            net_moveresize_window: intern(&conn, "_NET_MOVERESIZE_WINDOW")?,
            utf8_string: intern(&conn, "UTF8_STRING")?,
        };

        Ok(X11Compositor { conn, root, atoms })
    }

    fn xid(id: &WindowId) -> Result<Window> {
        match &id.0 {
            Backing::X11(xid) => Ok(*xid),
            #[allow(unreachable_patterns)]
            _ => Err(Error::WindowGone),
        }
    }

    fn send_root_client_message(
        &self,
        message_type: u32,
        data: [u32; 5],
        window: Window,
    ) -> Result<()> {
        let event = ClientMessageEvent::new(32, window, message_type, data);
        self.conn
            .send_event(
                false,
                self.root,
                EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
                event,
            )
            .map_err(|e| Error::Protocol(e.to_string()))?;
        self.conn
            .flush()
            .map_err(|e| Error::Protocol(e.to_string()))?;
        Ok(())
    }
}

fn intern(conn: &RustConnection, name: &str) -> Result<u32> {
    Ok(conn
        .intern_atom(false, name.as_bytes())
        .map_err(|e| Error::Protocol(e.to_string()))?
        .reply()
        .map_err(|e| Error::Protocol(e.to_string()))?
        .atom)
}

impl Compositor for X11Compositor {
    fn backend(&self) -> Backend {
        Backend::X11
    }

    fn list_windows(&mut self) -> Result<Vec<WindowInfo>> {
        let client_list = self
            .conn
            .get_property(
                false,
                self.root,
                self.atoms.net_client_list,
                AtomEnum::WINDOW,
                0,
                u32::MAX,
            )
            .map_err(|e| Error::Protocol(e.to_string()))?
            .reply()
            .map_err(|e| Error::Protocol(e.to_string()))?;

        let windows: Vec<Window> = client_list
            .value32()
            .map(|v| v.collect())
            .unwrap_or_default();

        let mut infos = Vec::with_capacity(windows.len());
        for window in windows {
            let title = self.window_title(window).unwrap_or_default();
            let class = self.window_class(window).unwrap_or_default();
            let geometry = self.window_geometry(window).ok();

            infos.push(WindowInfo {
                id: WindowId(Backing::X11(window)),
                title,
                app_id: class,
                geometry,
            });
        }
        Ok(infos)
    }

    fn focus_window(&mut self, id: &WindowId) -> Result<()> {
        let window = Self::xid(id)?;
        // _NET_ACTIVE_WINDOW: data.l = [source indication, timestamp, requestor's currently active window]
        self.send_root_client_message(self.atoms.net_active_window, [1, 0, 0, 0, 0], window)
    }

    fn close_window(&mut self, id: &WindowId) -> Result<()> {
        let window = Self::xid(id)?;
        // _NET_CLOSE_WINDOW: data.l = [timestamp, source indication]
        self.send_root_client_message(self.atoms.net_close_window, [0, 1, 0, 0, 0], window)
    }

    fn move_window(&mut self, id: &WindowId, x: i32, y: i32) -> Result<()> {
        let window = Self::xid(id)?;
        self.send_root_client_message(
            self.atoms.net_moveresize_window,
            moveresize_data(MoveResizePresent::XY, x as u32, y as u32),
            window,
        )
    }

    fn resize_window(&mut self, id: &WindowId, width: u32, height: u32) -> Result<()> {
        let window = Self::xid(id)?;
        self.send_root_client_message(
            self.atoms.net_moveresize_window,
            moveresize_data(MoveResizePresent::WidthHeight, width, height),
            window,
        )
    }
}

/// Which pair of `_NET_MOVERESIZE_WINDOW` fields a request is setting.
enum MoveResizePresent {
    XY,
    WidthHeight,
}

/// Build the `data.l` payload for a `_NET_MOVERESIZE_WINDOW` client message.
///
/// Per the EWMH spec, `data.l[0]` is `gravity | (present-flags << 8) | (source << 12)`,
/// where the present-flags bits are 1=x, 2=y, 4=width, 8=height, and the
/// remaining fields carry whichever two values `present` selects.
fn moveresize_data(present: MoveResizePresent, first: u32, second: u32) -> [u32; 5] {
    const SOURCE_APPLICATION: u32 = 1 << 12;
    match present {
        MoveResizePresent::XY => {
            const X_PRESENT: u32 = 1;
            const Y_PRESENT: u32 = 2;
            let flags = ((X_PRESENT | Y_PRESENT) << 8) | SOURCE_APPLICATION;
            [flags, first, second, 0, 0]
        }
        MoveResizePresent::WidthHeight => {
            const WIDTH_PRESENT: u32 = 4;
            const HEIGHT_PRESENT: u32 = 8;
            let flags = ((WIDTH_PRESENT | HEIGHT_PRESENT) << 8) | SOURCE_APPLICATION;
            [flags, 0, 0, first, second]
        }
    }
}

/// Extract the class (second field) from a raw `WM_CLASS` property value,
/// which is two NUL-terminated strings back to back: instance, then class.
fn parse_wm_class(raw: &[u8]) -> String {
    let raw = String::from_utf8_lossy(raw);
    raw.split('\u{0}').nth(1).unwrap_or_default().to_string()
}

impl X11Compositor {
    fn window_title(&self, window: Window) -> Result<String> {
        let reply = self
            .conn
            .get_property(
                false,
                window,
                self.atoms.net_wm_name,
                self.atoms.utf8_string,
                0,
                u32::MAX,
            )
            .map_err(|e| Error::Protocol(e.to_string()))?
            .reply()
            .map_err(|e| Error::Protocol(e.to_string()))?;
        if !reply.value.is_empty() {
            return Ok(String::from_utf8_lossy(&reply.value).into_owned());
        }

        // Fall back to the legacy WM_NAME for apps that don't set the EWMH hint.
        let legacy = self
            .conn
            .get_property(
                false,
                window,
                AtomEnum::WM_NAME,
                AtomEnum::STRING,
                0,
                u32::MAX,
            )
            .map_err(|e| Error::Protocol(e.to_string()))?
            .reply()
            .map_err(|e| Error::Protocol(e.to_string()))?;
        Ok(String::from_utf8_lossy(&legacy.value).into_owned())
    }

    fn window_class(&self, window: Window) -> Result<String> {
        let reply = self
            .conn
            .get_property(
                false,
                window,
                AtomEnum::WM_CLASS,
                AtomEnum::STRING,
                0,
                u32::MAX,
            )
            .map_err(|e| Error::Protocol(e.to_string()))?
            .reply()
            .map_err(|e| Error::Protocol(e.to_string()))?;
        Ok(parse_wm_class(&reply.value))
    }

    fn window_geometry(&self, window: Window) -> Result<Geometry> {
        let reply = self
            .conn
            .get_geometry(window)
            .map_err(|e| Error::Protocol(e.to_string()))?
            .reply()
            .map_err(|e| Error::Protocol(e.to_string()))?;
        Ok(Geometry {
            x: reply.x as i32,
            y: reply.y as i32,
            width: reply.width as u32,
            height: reply.height as u32,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_wm_class_second_field() {
        assert_eq!(parse_wm_class(b"xterm\0XTerm\0"), "XTerm");
    }

    #[test]
    fn wm_class_missing_class_field_is_empty() {
        assert_eq!(parse_wm_class(b"just-instance\0"), "");
    }

    #[test]
    fn wm_class_empty_input_is_empty() {
        assert_eq!(parse_wm_class(b""), "");
    }

    #[test]
    fn moveresize_xy_sets_only_x_and_y_flags() {
        let data = moveresize_data(MoveResizePresent::XY, 50, 60);
        let flags = data[0];
        assert_eq!(flags & 0b1111_00000000, 0b0011_00000000, "x and y bits set");
        assert_eq!(data[1], 50);
        assert_eq!(data[2], 60);
        assert_eq!(data[3], 0);
        assert_eq!(data[4], 0);
    }

    #[test]
    fn moveresize_width_height_sets_only_size_flags() {
        let data = moveresize_data(MoveResizePresent::WidthHeight, 640, 480);
        let flags = data[0];
        assert_eq!(
            flags & 0b1111_00000000,
            0b1100_00000000,
            "width and height bits set"
        );
        assert_eq!(data[1], 0);
        assert_eq!(data[2], 0);
        assert_eq!(data[3], 640);
        assert_eq!(data[4], 480);
    }

    #[test]
    fn moveresize_always_marks_source_application() {
        const SOURCE_APPLICATION: u32 = 1 << 12;
        assert_ne!(
            moveresize_data(MoveResizePresent::XY, 0, 0)[0] & SOURCE_APPLICATION,
            0
        );
        assert_ne!(
            moveresize_data(MoveResizePresent::WidthHeight, 0, 0)[0] & SOURCE_APPLICATION,
            0
        );
    }
}

//! Wayland backend, built on the wlroots `foreign-toplevel-management`
//! extension. This is the only widely-deployed Wayland protocol that lets
//! one client see and act on another's windows at all — it is implemented
//! by wlroots-based compositors (Sway, and others built on wlroots), but
//! not by GNOME's Mutter or KDE's KWin, which expose no equivalent. On
//! those, [`WaylandCompositor::connect`] still succeeds (the display
//! connection itself works fine) but [`Compositor::list_windows`] and
//! friends return [`Error::Unsupported`], since there is nothing this
//! crate can do about a protocol the compositor simply doesn't speak.
//!
//! Moving and resizing another client's surface has no protocol at all,
//! on any Wayland compositor — that is a deliberate part of Wayland's
//! security model, not a gap in this crate. Those two methods always
//! return [`Error::Unsupported`] here.

use crate::window::Backing;
use crate::{Backend, Compositor, Error, Result, WindowId, WindowInfo};
use wayland_client::protocol::{wl_registry, wl_seat::WlSeat};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle};
use wayland_protocols_wlr::foreign_toplevel::v1::client::{
    zwlr_foreign_toplevel_handle_v1::{self, ZwlrForeignToplevelHandleV1},
    zwlr_foreign_toplevel_manager_v1::{self, ZwlrForeignToplevelManagerV1},
};

struct ToplevelEntry {
    handle: ZwlrForeignToplevelHandleV1,
    title: String,
    app_id: String,
    closed: bool,
}

#[derive(Default)]
struct State {
    manager: Option<ZwlrForeignToplevelManagerV1>,
    seat: Option<WlSeat>,
    toplevels: Vec<ToplevelEntry>,
}

pub struct WaylandCompositor {
    conn: Connection,
    queue: EventQueue<State>,
    state: State,
}

impl WaylandCompositor {
    pub(crate) fn connect() -> Result<Self> {
        let conn = Connection::connect_to_env()
            .map_err(|e| Error::ConnectionFailed(e.to_string()))?;
        let mut queue = conn.new_event_queue::<State>();
        let qh = queue.handle();
        let display = conn.display();
        display.get_registry(&qh, ());

        let mut state = State::default();
        // Round 1: receive the registry's globals and bind the ones we need.
        queue
            .roundtrip(&mut state)
            .map_err(|e| Error::Protocol(e.to_string()))?;
        // Round 2: flush those bind requests and receive the immediate
        // toplevel snapshot (title/app_id/state/done) the manager sends.
        queue
            .roundtrip(&mut state)
            .map_err(|e| Error::Protocol(e.to_string()))?;

        Ok(WaylandCompositor { conn, queue, state })
    }

    fn require_manager(&self) -> Result<()> {
        if self.state.manager.is_none() {
            return Err(Error::Unsupported {
                backend: Backend::Wayland,
                operation: "window listing/control",
                reason: "compositor does not implement wlr-foreign-toplevel-management (only wlroots-based compositors like Sway do)",
            });
        }
        Ok(())
    }

    fn find_handle(&self, id: &WindowId) -> Result<&ZwlrForeignToplevelHandleV1> {
        let object_id = match &id.0 {
            Backing::Wayland(object_id) => object_id,
            #[allow(unreachable_patterns)]
            _ => return Err(Error::WindowGone),
        };
        self.state
            .toplevels
            .iter()
            .find(|entry| !entry.closed && &entry.handle.id() == object_id)
            .map(|entry| &entry.handle)
            .ok_or(Error::WindowGone)
    }

    fn refresh(&mut self) -> Result<()> {
        self.queue
            .roundtrip(&mut self.state)
            .map_err(|e| Error::Protocol(e.to_string()))?;
        self.state.toplevels.retain(|entry| !entry.closed);
        Ok(())
    }
}

impl Compositor for WaylandCompositor {
    fn backend(&self) -> Backend {
        Backend::Wayland
    }

    fn list_windows(&mut self) -> Result<Vec<WindowInfo>> {
        self.require_manager()?;
        self.refresh()?;
        Ok(self
            .state
            .toplevels
            .iter()
            .map(|entry| WindowInfo {
                id: WindowId(Backing::Wayland(entry.handle.id())),
                title: entry.title.clone(),
                app_id: entry.app_id.clone(),
                // Wayland does not disclose foreign window geometry to clients.
                geometry: None,
            })
            .collect())
    }

    fn focus_window(&mut self, id: &WindowId) -> Result<()> {
        self.require_manager()?;
        let seat = self.state.seat.as_ref().ok_or(Error::Unsupported {
            backend: Backend::Wayland,
            operation: "focus_window",
            reason: "no wl_seat was advertised by the compositor",
        })?;
        let handle = self.find_handle(id)?;
        handle.activate(seat);
        self.conn.flush().map_err(|e| Error::Protocol(e.to_string()))?;
        Ok(())
    }

    fn close_window(&mut self, id: &WindowId) -> Result<()> {
        self.require_manager()?;
        let handle = self.find_handle(id)?;
        handle.close();
        self.conn.flush().map_err(|e| Error::Protocol(e.to_string()))?;
        Ok(())
    }

    fn move_window(&mut self, _id: &WindowId, _x: i32, _y: i32) -> Result<()> {
        Err(Error::Unsupported {
            backend: Backend::Wayland,
            operation: "move_window",
            reason: "Wayland has no protocol for one client to reposition another's surface",
        })
    }

    fn resize_window(&mut self, _id: &WindowId, _width: u32, _height: u32) -> Result<()> {
        Err(Error::Unsupported {
            backend: Backend::Wayland,
            operation: "resize_window",
            reason: "Wayland has no protocol for one client to resize another's surface",
        })
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _data: &(),
        _conn: &Connection,
        qh: &QueueHandle<State>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            match interface.as_str() {
                "zwlr_foreign_toplevel_manager_v1" => {
                    state.manager = Some(registry.bind(name, version.min(3), qh, ()));
                }
                "wl_seat" => {
                    state.seat = Some(registry.bind(name, version.min(1), qh, ()));
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<ZwlrForeignToplevelManagerV1, ()> for State {
    fn event(
        state: &mut Self,
        _manager: &ZwlrForeignToplevelManagerV1,
        event: zwlr_foreign_toplevel_manager_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<State>,
    ) {
        if let zwlr_foreign_toplevel_manager_v1::Event::Toplevel { toplevel } = event {
            state.toplevels.push(ToplevelEntry {
                handle: toplevel,
                title: String::new(),
                app_id: String::new(),
                closed: false,
            });
        }
    }

    wayland_client::event_created_child!(State, ZwlrForeignToplevelManagerV1, [
        zwlr_foreign_toplevel_manager_v1::EVT_TOPLEVEL_OPCODE => (ZwlrForeignToplevelHandleV1, ()),
    ]);
}

impl Dispatch<ZwlrForeignToplevelHandleV1, ()> for State {
    fn event(
        state: &mut Self,
        handle: &ZwlrForeignToplevelHandleV1,
        event: zwlr_foreign_toplevel_handle_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<State>,
    ) {
        let Some(entry) = state
            .toplevels
            .iter_mut()
            .find(|entry| entry.handle.id() == handle.id())
        else {
            return;
        };
        match event {
            zwlr_foreign_toplevel_handle_v1::Event::Title { title } => entry.title = title,
            zwlr_foreign_toplevel_handle_v1::Event::AppId { app_id } => entry.app_id = app_id,
            zwlr_foreign_toplevel_handle_v1::Event::Closed => entry.closed = true,
            _ => {}
        }
    }
}

impl Dispatch<WlSeat, ()> for State {
    fn event(
        _state: &mut Self,
        _seat: &WlSeat,
        _event: <WlSeat as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<State>,
    ) {
    }
}

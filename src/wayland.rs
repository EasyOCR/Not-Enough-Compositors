//! Wayland backend.
//!
//! Wayland deliberately gives clients no built-in way to see or act on
//! each other's windows, so this module speaks whichever of the following
//! extension protocols the running compositor actually advertises, in
//! order of preference:
//!
//! 1. **`org_kde_plasma_window_management`** (KWin's own protocol) — full
//!    list/focus/close, plus geometry. Available on KDE Plasma, *unless*
//!    another client (typically `plasmashell`'s own taskbar) has already
//!    bound it: the protocol allows only one client at a time, and there
//!    is no way to detect that in advance short of trying.
//! 2. **`wlr-foreign-toplevel-management`** — list/focus/close. Available
//!    on wlroots-based compositors (Sway and similar).
//! 3. **`ext-foreign-toplevel-list-v1`** — the newer, standardized
//!    protocol some non-wlroots compositors (recent Mutter, recent KWin)
//!    implement. It is intentionally list-only: no activate, no close.
//!
//! If none of the three is advertised — stock GNOME/Mutter today — window
//! operations return [`Error::Unsupported`]; the display connection itself
//! still succeeds, since that much always works regardless of compositor.
//!
//! Moving or resizing another client's surface to an exact position has no
//! protocol on any of the three: Wayland's security model doesn't allow
//! it, and Plasma's `request_move`/`request_resize` only start an
//! interactive, pointer-grab-driven drag (the same thing Alt+drag does),
//! not a programmatic "put it at (x, y)". So `move_window`/`resize_window`
//! always return [`Error::Unsupported`] on this backend.

use crate::{Backend, Compositor, Error, Result, WindowId, WindowInfo};
use wayland_client::backend::ObjectId;
use wayland_client::protocol::{wl_registry, wl_seat::WlSeat};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle};
use wayland_protocols::ext::foreign_toplevel_list::v1::client::{
    ext_foreign_toplevel_handle_v1::{self, ExtForeignToplevelHandleV1},
    ext_foreign_toplevel_list_v1::{self, ExtForeignToplevelListV1},
};
use wayland_protocols_plasma::plasma_window_management::client::{
    org_kde_plasma_window::{self, OrgKdePlasmaWindow},
    org_kde_plasma_window_management::{self, OrgKdePlasmaWindowManagement},
};
use wayland_protocols_wlr::foreign_toplevel::v1::client::{
    zwlr_foreign_toplevel_handle_v1::{self, ZwlrForeignToplevelHandleV1},
    zwlr_foreign_toplevel_manager_v1::{self, ZwlrForeignToplevelManagerV1},
};

/// Which extension protocol identifies a given [`WindowId`] on Wayland.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum WaylandBacking {
    Plasma(ObjectId),
    Wlr(ObjectId),
    Ext(ObjectId),
}

/// The lowest `org_kde_plasma_window_management` version this backend uses:
/// `window_with_uuid` and `get_window_by_uuid` were added at version 13.
const PLASMA_MIN_VERSION: u32 = 13;
const PLASMA_ACTIVE: u32 = 0x1;

struct PlasmaEntry {
    handle: OrgKdePlasmaWindow,
    title: String,
    app_id: String,
    closed: bool,
}

struct WlrEntry {
    handle: ZwlrForeignToplevelHandleV1,
    title: String,
    app_id: String,
    closed: bool,
}

struct ExtEntry {
    handle: ExtForeignToplevelHandleV1,
    title: String,
    app_id: String,
    closed: bool,
}

#[derive(Default)]
struct State {
    plasma_manager: Option<OrgKdePlasmaWindowManagement>,
    wlr_manager: Option<ZwlrForeignToplevelManagerV1>,
    ext_manager: Option<ExtForeignToplevelListV1>,
    seat: Option<WlSeat>,
    plasma_windows: Vec<PlasmaEntry>,
    wlr_toplevels: Vec<WlrEntry>,
    ext_toplevels: Vec<ExtEntry>,
}

/// Which protocol is actually driving window listing/control right now.
/// Decided once at connect time by preferring the richest one available.
enum Source {
    Plasma,
    Wlr,
    Ext,
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
        // toplevel snapshot each manager sends right after being bound.
        queue
            .roundtrip(&mut state)
            .map_err(|e| Error::Protocol(e.to_string()))?;
        // Round 3: the plasma path needs an extra hop — window_with_uuid
        // (round 2) triggers our get_window_by_uuid call, whose resulting
        // title/app_id events only arrive after another roundtrip.
        queue
            .roundtrip(&mut state)
            .map_err(|e| Error::Protocol(e.to_string()))?;

        Ok(WaylandCompositor { conn, queue, state })
    }

    fn source(&self) -> Option<Source> {
        if self.state.plasma_manager.is_some() {
            Some(Source::Plasma)
        } else if self.state.wlr_manager.is_some() {
            Some(Source::Wlr)
        } else if self.state.ext_manager.is_some() {
            Some(Source::Ext)
        } else {
            None
        }
    }

    fn require_source(&self) -> Result<Source> {
        self.source().ok_or(Error::Unsupported {
            backend: Backend::Wayland,
            operation: "window listing/control",
            reason: "compositor advertises none of org_kde_plasma_window_management, \
                      wlr-foreign-toplevel-management, or ext-foreign-toplevel-list-v1 \
                      (stock GNOME/Mutter has none of these)",
        })
    }

    fn refresh(&mut self) -> Result<()> {
        self.queue
            .roundtrip(&mut self.state)
            .map_err(|e| Error::Protocol(e.to_string()))?;
        self.state.plasma_windows.retain(|e| !e.closed);
        self.state.wlr_toplevels.retain(|e| !e.closed);
        self.state.ext_toplevels.retain(|e| !e.closed);
        Ok(())
    }

    fn find_plasma(&self, oid: &ObjectId) -> Result<&OrgKdePlasmaWindow> {
        self.state
            .plasma_windows
            .iter()
            .find(|e| !e.closed && &e.handle.id() == oid)
            .map(|e| &e.handle)
            .ok_or(Error::WindowGone)
    }

    fn find_wlr(&self, oid: &ObjectId) -> Result<&ZwlrForeignToplevelHandleV1> {
        self.state
            .wlr_toplevels
            .iter()
            .find(|e| !e.closed && &e.handle.id() == oid)
            .map(|e| &e.handle)
            .ok_or(Error::WindowGone)
    }
}

impl Compositor for WaylandCompositor {
    fn backend(&self) -> Backend {
        Backend::Wayland
    }

    fn list_windows(&mut self) -> Result<Vec<WindowInfo>> {
        let source = self.require_source()?;
        self.refresh()?;
        let infos = match source {
            Source::Plasma => self
                .state
                .plasma_windows
                .iter()
                .map(|e| WindowInfo {
                    id: WindowId(crate::window::Backing::Wayland(WaylandBacking::Plasma(
                        e.handle.id(),
                    ))),
                    title: e.title.clone(),
                    app_id: e.app_id.clone(),
                    geometry: None,
                })
                .collect(),
            Source::Wlr => self
                .state
                .wlr_toplevels
                .iter()
                .map(|e| WindowInfo {
                    id: WindowId(crate::window::Backing::Wayland(WaylandBacking::Wlr(
                        e.handle.id(),
                    ))),
                    title: e.title.clone(),
                    app_id: e.app_id.clone(),
                    geometry: None,
                })
                .collect(),
            Source::Ext => self
                .state
                .ext_toplevels
                .iter()
                .map(|e| WindowInfo {
                    id: WindowId(crate::window::Backing::Wayland(WaylandBacking::Ext(
                        e.handle.id(),
                    ))),
                    title: e.title.clone(),
                    app_id: e.app_id.clone(),
                    geometry: None,
                })
                .collect(),
        };
        Ok(infos)
    }

    fn focus_window(&mut self, id: &WindowId) -> Result<()> {
        #[allow(irrefutable_let_patterns)]
        let crate::window::Backing::Wayland(backing) = &id.0 else {
            return Err(Error::WindowGone);
        };
        match backing {
            WaylandBacking::Plasma(oid) => {
                let handle = self.find_plasma(oid)?;
                handle.set_state(PLASMA_ACTIVE, PLASMA_ACTIVE);
            }
            WaylandBacking::Wlr(oid) => {
                let seat = self.state.seat.as_ref().ok_or(Error::Unsupported {
                    backend: Backend::Wayland,
                    operation: "focus_window",
                    reason: "no wl_seat was advertised by the compositor",
                })?;
                let handle = self.find_wlr(oid)?;
                handle.activate(seat);
            }
            WaylandBacking::Ext(_) => {
                return Err(Error::Unsupported {
                    backend: Backend::Wayland,
                    operation: "focus_window",
                    reason: "ext-foreign-toplevel-list-v1 is list-only; this compositor has no activation protocol",
                });
            }
        }
        self.conn.flush().map_err(|e| Error::Protocol(e.to_string()))?;
        Ok(())
    }

    fn close_window(&mut self, id: &WindowId) -> Result<()> {
        #[allow(irrefutable_let_patterns)]
        let crate::window::Backing::Wayland(backing) = &id.0 else {
            return Err(Error::WindowGone);
        };
        match backing {
            WaylandBacking::Plasma(oid) => self.find_plasma(oid)?.close(),
            WaylandBacking::Wlr(oid) => self.find_wlr(oid)?.close(),
            WaylandBacking::Ext(_) => {
                return Err(Error::Unsupported {
                    backend: Backend::Wayland,
                    operation: "close_window",
                    reason: "ext-foreign-toplevel-list-v1 is list-only; this compositor has no close protocol",
                });
            }
        }
        self.conn.flush().map_err(|e| Error::Protocol(e.to_string()))?;
        Ok(())
    }

    fn move_window(&mut self, _id: &WindowId, _x: i32, _y: i32) -> Result<()> {
        Err(Error::Unsupported {
            backend: Backend::Wayland,
            operation: "move_window",
            reason: "no Wayland protocol lets one client place another's surface at an exact position",
        })
    }

    fn resize_window(&mut self, _id: &WindowId, _width: u32, _height: u32) -> Result<()> {
        Err(Error::Unsupported {
            backend: Backend::Wayland,
            operation: "resize_window",
            reason: "no Wayland protocol lets one client set another's surface to an exact size",
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
                "org_kde_plasma_window_management" if version >= PLASMA_MIN_VERSION => {
                    state.plasma_manager =
                        Some(registry.bind(name, version.min(18), qh, ()));
                }
                "zwlr_foreign_toplevel_manager_v1" => {
                    state.wlr_manager = Some(registry.bind(name, version.min(3), qh, ()));
                }
                "ext_foreign_toplevel_list_v1" => {
                    state.ext_manager = Some(registry.bind(name, version.min(1), qh, ()));
                }
                "wl_seat" => {
                    state.seat = Some(registry.bind(name, version.min(1), qh, ()));
                }
                _ => {}
            }
        }
    }
}

// --- org_kde_plasma_window_management ---------------------------------

impl Dispatch<OrgKdePlasmaWindowManagement, ()> for State {
    fn event(
        state: &mut Self,
        manager: &OrgKdePlasmaWindowManagement,
        event: org_kde_plasma_window_management::Event,
        _data: &(),
        _conn: &Connection,
        qh: &QueueHandle<State>,
    ) {
        if let org_kde_plasma_window_management::Event::WindowWithUuid { uuid, .. } = event {
            let handle = manager.get_window_by_uuid(uuid, qh, ());
            state.plasma_windows.push(PlasmaEntry {
                handle,
                title: String::new(),
                app_id: String::new(),
                closed: false,
            });
        }
    }
}

impl Dispatch<OrgKdePlasmaWindow, ()> for State {
    fn event(
        state: &mut Self,
        handle: &OrgKdePlasmaWindow,
        event: org_kde_plasma_window::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<State>,
    ) {
        let Some(entry) = state
            .plasma_windows
            .iter_mut()
            .find(|e| e.handle.id() == handle.id())
        else {
            return;
        };
        match event {
            org_kde_plasma_window::Event::TitleChanged { title } => entry.title = title,
            org_kde_plasma_window::Event::AppIdChanged { app_id } => entry.app_id = app_id,
            org_kde_plasma_window::Event::Unmapped => entry.closed = true,
            _ => {}
        }
    }
}

// --- wlr-foreign-toplevel-management ------------------------------------

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
            state.wlr_toplevels.push(WlrEntry {
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
            .wlr_toplevels
            .iter_mut()
            .find(|e| e.handle.id() == handle.id())
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

// --- ext-foreign-toplevel-list-v1 ---------------------------------------

impl Dispatch<ExtForeignToplevelListV1, ()> for State {
    fn event(
        state: &mut Self,
        _manager: &ExtForeignToplevelListV1,
        event: ext_foreign_toplevel_list_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<State>,
    ) {
        if let ext_foreign_toplevel_list_v1::Event::Toplevel { toplevel } = event {
            state.ext_toplevels.push(ExtEntry {
                handle: toplevel,
                title: String::new(),
                app_id: String::new(),
                closed: false,
            });
        }
    }

    wayland_client::event_created_child!(State, ExtForeignToplevelListV1, [
        ext_foreign_toplevel_list_v1::EVT_TOPLEVEL_OPCODE => (ExtForeignToplevelHandleV1, ()),
    ]);
}

impl Dispatch<ExtForeignToplevelHandleV1, ()> for State {
    fn event(
        state: &mut Self,
        handle: &ExtForeignToplevelHandleV1,
        event: ext_foreign_toplevel_handle_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<State>,
    ) {
        let Some(entry) = state
            .ext_toplevels
            .iter_mut()
            .find(|e| e.handle.id() == handle.id())
        else {
            return;
        };
        match event {
            ext_foreign_toplevel_handle_v1::Event::Title { title } => entry.title = title,
            ext_foreign_toplevel_handle_v1::Event::AppId { app_id } => entry.app_id = app_id,
            ext_foreign_toplevel_handle_v1::Event::Closed => entry.closed = true,
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

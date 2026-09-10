# EasyCompositing

One API for window control that works the same whether the app you're
looking at is running under X11 or a wlroots-based Wayland compositor
(Sway and similar). EasyCompositing is not a Windows/Linux compatibility
layer — it exists so a developer who wants to list, focus, close, move,
or resize windows across compositor boundaries writes that logic **once**,
instead of maintaining a separate integration per compositor.

```rust
let mut compositor = easycompositing::connect()?;
for window in compositor.list_windows()? {
    println!("{}: {}", window.app_id, window.title);
}
compositor.focus_window(&window.id)?;
```

`connect()` detects the running backend (`WAYLAND_DISPLAY` / `DISPLAY`)
and hands back a `Box<dyn Compositor>` — calling code never branches on
which compositor it's talking to.

## What's implemented

- **X11**, via the EWMH hints most window managers publish
  (`_NET_CLIENT_LIST`, `_NET_ACTIVE_WINDOW`, `_NET_CLOSE_WINDOW`,
  `_NET_MOVERESIZE_WINDOW`): full list/focus/close/move/resize.
- **Wayland**, via the wlroots `foreign-toplevel-management` protocol:
  list/focus/close. This is the only widely deployed Wayland protocol
  that lets one client see or act on another's windows at all — it's
  implemented by wlroots-based compositors (Sway and similar), **not**
  by GNOME's Mutter or KDE's KWin, which expose no equivalent. On those,
  `connect()` still succeeds, but window operations return
  `Error::Unsupported`.

## What's deliberately not implemented

Moving or resizing another client's window **has no Wayland protocol at
all**, on any compositor — that's a security-model decision by Wayland
itself, not a gap in this crate. `move_window`/`resize_window` always
return `Error::Unsupported` on the Wayland backend, so callers can detect
and handle it rather than assume the call silently no-oped.

## Building

```sh
cargo build                                    # both backends (default)
cargo build --no-default-features --features x11
cargo build --no-default-features --features wayland
cargo run --example list_windows               # try it against your session
```

Verified end-to-end against a live Xvfb + Openbox + xterm session: window
enumeration, title/class lookup, geometry, focus, move, and resize via
EWMH all round-trip correctly.

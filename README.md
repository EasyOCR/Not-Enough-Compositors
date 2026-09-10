# EasyCompositing

One API for window control that works the same across X11 and the major
Wayland compositors. EasyCompositing is not a Windows/Linux compatibility
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
- **Wayland**: no single protocol covers every compositor, so this backend
  speaks whichever of three extensions is actually advertised, preferring
  the richest one available:
  1. **`org_kde_plasma_window_management`** (KWin's own protocol) — full
     list/focus/close, on KDE Plasma. *Caveat:* the protocol only allows
     one client to bind it at a time, and `plasmashell`'s own taskbar
     usually already holds it — there is no way to detect that in advance
     short of trying.
  2. **`wlr-foreign-toplevel-management`** — list/focus/close, on
     wlroots-based compositors (Sway and similar).
  3. **`ext-foreign-toplevel-list-v1`** — the newer, standardized
     protocol some non-wlroots compositors (recent Mutter, recent KWin)
     implement. Intentionally list-only: `focus_window`/`close_window`
     return `Error::Unsupported` when this is the only one available.

  If a compositor advertises none of the three — stock GNOME/Mutter,
  as of this writing — the display connection still succeeds, but window
  operations return `Error::Unsupported` rather than silently no-oping.

## What's deliberately not implemented

Moving or resizing another client's window to an exact position **has no
protocol on any Wayland compositor** — that's a security-model decision
by Wayland itself, not a gap in this crate. (KDE's `request_move` /
`request_resize` only start an interactive, pointer-grab-driven drag —
the same thing Alt+drag does — not a programmatic "put it at (x, y)".)
`move_window`/`resize_window` always return `Error::Unsupported` on the
Wayland backend, so callers can detect and handle it rather than assume
the call silently no-oped.

## Building

```sh
cargo build                                    # both backends (default)
cargo build --no-default-features --features x11
cargo build --no-default-features --features wayland
cargo run --example list_windows               # try it against your session
```

## Verification

- **X11**: verified end-to-end against a live Xvfb + Openbox + xterm
  session — window enumeration, title/class lookup, geometry, focus,
  move, and resize via EWMH all round-trip correctly.
- **Wayland (wlr path)**: verified end-to-end against a headless Sway +
  foot session — listing, title/app_id, and focus all round-trip
  correctly (confirmed via `WAYLAND_DEBUG=1` wire tracing); move/resize
  correctly report `Error::Unsupported`.
- **Wayland (Plasma and ext-foreign-toplevel-list paths)**: implemented
  against the published protocol specs, but not yet verified against a
  live KDE or GNOME session (none available in the environment this was
  built in). Treat these two as needing real-world confirmation before
  depending on them.

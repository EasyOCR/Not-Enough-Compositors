# Not Enough Compositors

One API for window control that works the same across X11, Hyprland,
niri, and the other major Wayland compositors. Not Enough Compositors is
not a Windows/Linux compatibility layer — it exists so a developer who
wants to list, focus, close, move, or resize windows across compositor
boundaries writes that logic **once**, instead of maintaining a separate
integration per compositor.

```rust
let mut compositor = nec::connect()?;
for window in compositor.list_windows()? {
    println!("{}: {}", window.app_id, window.title);
}
compositor.focus_window(&window.id)?;
```

`connect()` detects the running backend (`HYPRLAND_INSTANCE_SIGNATURE` /
`NIRI_SOCKET` / `WAYLAND_DISPLAY` / `DISPLAY`) and hands back a
`Box<dyn Compositor>` — calling code never branches on which compositor
it's talking to.

## What's implemented

- **X11**, via the EWMH hints most window managers publish
  (`_NET_CLIENT_LIST`, `_NET_ACTIVE_WINDOW`, `_NET_CLOSE_WINDOW`,
  `_NET_MOVERESIZE_WINDOW`): full list/focus/close/move/resize.
- **Hyprland**, via `hyprctl`'s own socket IPC rather than a Wayland
  protocol: full list/focus/close/move/resize, including **exact**
  positioning and sizing — something no standard Wayland protocol exposes
  to another client at all. If the IPC socket can't be reached, this
  falls back to the generic Wayland path below (Hyprland implements
  `wlr-foreign-toplevel-management` too), trading away exact move/resize
  for list/focus/close.
- **niri**, via its own JSON IPC (the same one `niri msg` uses), behind
  the **opt-in** `niri` feature (not part of `default` — see below): full
  list/focus/close, plus exact position/size for **floating** windows.
  niri is a scrollable-tiling compositor, so most windows live in its
  tiling layout, where "move to (x, y)" doesn't apply the same way — the
  IPC call still succeeds on a tiled window, it just may not visibly do
  anything, since there's no way to detect that case from the IPC
  response alone. Falls back to the generic Wayland path if the IPC
  socket can't be reached, same as Hyprland.
  **License note:** niri's official IPC crate (`niri-ipc`) is
  GPL-3.0-or-later, unlike everything else this crate depends on
  (MIT/Apache-2.0). That's exactly why this feature isn't in `default` —
  enable it explicitly (`--features niri`) if that's acceptable for your
  project.
- **Other Wayland compositors**: no single protocol covers all of them,
  so this backend speaks whichever of three extensions is actually
  advertised, preferring the richest one available:
  1. **`org_kde_plasma_window_management`** (KWin's own protocol) — full
     list/focus/close, on KDE Plasma. *Caveat:* the protocol only allows
     one client to bind it at a time, and `plasmashell`'s own taskbar
     usually already holds it — there is no way to detect that in advance
     short of trying.
  2. **`wlr-foreign-toplevel-management`** — list/focus/close, on
     wlroots-based compositors: Sway, Wayfire, river, dwl, Hyprland, and
     similar all implement this the same way, so they all work here with
     no compositor-specific code. This most likely also covers
     **COSMIC** (`cosmic-comp`), which implements the same protocol for
     panel/taskbar compatibility, though that hasn't been directly
     confirmed against a live COSMIC session.
  3. **`ext-foreign-toplevel-list-v1`** — the newer, standardized
     protocol some non-wlroots compositors (recent Mutter, recent KWin)
     implement. Intentionally list-only: `focus_window`/`close_window`
     return `Error::Unsupported` when this is the only one available.

  If a compositor advertises none of the three — stock GNOME/Mutter,
  as of this writing — the display connection still succeeds, but window
  operations return `Error::Unsupported` rather than silently no-oping.
  Reference/niche compositors like Weston or Mir-based shells generally
  fall into this bucket too; that's expected, not a bug.

## What's deliberately not implemented

Moving or resizing another client's window to an exact position **has no
protocol on plain Wayland** — that's a security-model decision by
Wayland itself, not a gap in this crate. (KDE's `request_move` /
`request_resize` only start an interactive, pointer-grab-driven drag —
the same thing Alt+drag does — not a programmatic "put it at (x, y)".)
`move_window`/`resize_window` return `Error::Unsupported` on every
Wayland path except Hyprland's and niri's own IPCs, so callers can detect
and handle the gap rather than assume the call silently no-oped.

## Building

```sh
cargo build                                    # default: x11 + wayland + hyprland
cargo build --features niri                    # add niri (GPL-3.0-or-later dependency)
cargo build --no-default-features --features x11
cargo build --no-default-features --features wayland
cargo build --no-default-features --features hyprland
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
- **Wayland (Plasma and ext-foreign-toplevel-list paths), Hyprland's IPC
  backend, and niri's IPC backend**: implemented against the published
  protocol/IPC specs, but not yet verified against a live KDE, GNOME,
  Hyprland, or niri session — none were available in the environment this
  was built in (Hyprland and niri both need a GPU-backed compositor
  build). Treat these as needing real-world confirmation before depending
  on them.
- **COSMIC**: not tested at all; expected to work via the wlr path based
  on its published protocol support, unconfirmed.

## License

MIT (see [`LICENSE`](LICENSE)) — free to use, modify, and redistribute,
including commercially, as long as the copyright notice and license text
are kept with any copy. The optional `niri` feature is the one exception:
enabling it pulls in `niri-ipc`, which is GPL-3.0-or-later — see that
feature's note above before turning it on in a project with different
licensing needs.

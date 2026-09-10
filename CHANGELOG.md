# Changelog

All notable changes to this project are documented here. Format loosely
follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

Not yet published to crates.io — everything below describes the current
state of `main`/`InDev`, tracked as `0.1.0` in `Cargo.toml`.

### Added

- Core `Compositor` trait: `list_windows`, `focus_window`, `close_window`,
  `move_window`, `resize_window`, plus `connect()` to auto-detect the
  running compositor and hand back the right implementation.
- **X11 backend** (`x11` feature, default on) via EWMH hints
  (`_NET_CLIENT_LIST`, `_NET_ACTIVE_WINDOW`, `_NET_CLOSE_WINDOW`,
  `_NET_MOVERESIZE_WINDOW`): full list/focus/close/move/resize.
- **Wayland backend** (`wayland` feature, default on), speaking whichever
  of three extension protocols a compositor advertises, richest first:
  `org_kde_plasma_window_management` (KDE Plasma), then
  `wlr-foreign-toplevel-management` (Sway, Wayfire, river, dwl, and other
  wlroots-based compositors), then `ext-foreign-toplevel-list-v1`
  (list-only; recent Mutter/KWin).
- **Hyprland backend** (`hyprland` feature, default on) via `hyprctl`'s
  own socket IPC: full list/focus/close/move/resize, including exact
  positioning/sizing that no standard Wayland protocol allows. Falls back
  to the generic Wayland backend if the IPC socket can't be reached.
- **niri backend** (`niri` feature, **opt-in**, not part of default) via
  niri's own JSON IPC (`niri-ipc` crate): full list/focus/close, plus
  exact position/size for floating windows. Not in `default` because
  `niri-ipc` is GPL-3.0-or-later, unlike the rest of this crate's
  dependencies (MIT/Apache-2.0). Falls back to the generic Wayland
  backend if disabled or the IPC socket can't be reached.
- `MIT` `LICENSE` file.
- CI: rustfmt, clippy (`-D warnings`) across every feature combination,
  build + test, and a doc-link check, all via GitHub Actions.
- Unit tests for the X11 `WM_CLASS` parsing and `_NET_MOVERESIZE_WINDOW`
  flag construction, Hyprland's JSON client parsing and dispatch-reply
  handling, and `Backend::detect`'s environment-variable precedence.

### Known gaps

- Stock GNOME (Mutter) implements none of the three Wayland protocols
  this crate speaks, so window operations there return
  `Error::Unsupported` (the display connection itself still succeeds).
- COSMIC (`cosmic-comp`) is expected to work via the wlr path based on
  its published protocol support but hasn't been directly confirmed.
- The Plasma and ext-foreign-toplevel-list Wayland paths, and both the
  Hyprland and niri IPC backends, are implemented against their
  documented protocols/IPCs but have not been exercised against a live
  KDE, GNOME, Hyprland, or niri session — none were available in the
  environment this was built in. The X11 backend and the generic
  wlr-foreign-toplevel-management path *have* been verified live (Xvfb +
  Openbox + xterm, and headless Sway + foot, respectively).

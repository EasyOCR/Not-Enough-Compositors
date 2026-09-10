//! `cargo run --example list_windows` — lists windows via whichever
//! compositor backend is detected, then (on X11 only, since Wayland can't)
//! nudges the first one to prove move/resize round-trip through EWMH.

fn main() -> Result<(), not_enough_compositors::Error> {
    let mut compositor = not_enough_compositors::connect()?;
    println!("backend: {:?}", compositor.backend());

    let windows = compositor.list_windows()?;
    for w in &windows {
        println!("{:?} app_id={:?} title={:?} geometry={:?}", w.id, w.app_id, w.title, w.geometry);
    }

    if let Some(first) = windows.first() {
        println!("focusing {:?}", first.id);
        compositor.focus_window(&first.id)?;

        match compositor.move_window(&first.id, 50, 60) {
            Ok(()) => println!("moved window to (50, 60)"),
            Err(e) => println!("move_window: {e}"),
        }
        match compositor.resize_window(&first.id, 640, 480) {
            Ok(()) => println!("resized window to 640x480"),
            Err(e) => println!("resize_window: {e}"),
        }
    }

    Ok(())
}

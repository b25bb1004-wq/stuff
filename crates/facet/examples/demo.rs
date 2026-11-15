//! Demo: open, move, tile and close windows, printing how many screen rects each frame changed.
use facet::{Compositor, Layout};

fn main() {
    let mut comp = Compositor::new(640, 400);
    let a = comp.open("terminal", 320, 200, 0);
    let b = comp.open("notes", 200, 150, 0);
    println!("opened windows {a} and {b} on a 640x400 screen");
    println!("stacking order (bottom to top): {:?}", comp.order());
    println!("focused window: {:?}", comp.focused());

    let first = comp.frame(0);
    println!("frame at 0 ms: {} changed rects", first.len());
    let idle = comp.frame(10);
    println!("frame at 10 ms with no edits: {} changed rects", idle.len());

    comp.move_to(a, 40, 30, 10);
    let mid = comp.frame(100);
    println!(
        "frame at 100 ms, window {a} easing: {} changed rects",
        mid.len()
    );
    let end = comp.frame(200);
    println!(
        "frame at 200 ms, ease finished: {} changed rects",
        end.len()
    );
    if let Some(r) = comp.rect(a) {
        println!(
            "window {a} final rect: x={} y={} w={} h={}",
            r.x, r.y, r.w, r.h
        );
    }

    comp.set_layout(Layout::Tiling, 200);
    let tiled = comp.frame(400);
    println!(
        "tiling layout, frame at 400 ms: {} changed rects",
        tiled.len()
    );
    if let Some(r) = comp.rect(b) {
        println!(
            "window {b} tiled rect: x={} y={} w={} h={}",
            r.x, r.y, r.w, r.h
        );
    }

    let closed = comp.close(b, 500);
    println!("closed window {b}: {closed}");
    println!("focused after close: {:?}", comp.focused());
    let last = comp.frame(600);
    println!("frame at 600 ms after close: {} changed rects", last.len());
}

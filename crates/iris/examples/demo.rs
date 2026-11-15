//! Demo: scripted desktop session with Files, Editor, Terminal, and tiling layout.

use indium::{Event, Key};
use iris::{App, Desktop, Editor, Files, Layout, Routed, Terminal};
use pigment::Rgba;

fn fnv1a_pixels(pixels: &[Rgba]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for p in pixels {
        for b in [p.0, p.1, p.2, p.3] {
            hash ^= u64::from(b);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
    }
    hash
}

fn main() {
    let mut desktop = Desktop::new(640, 400);
    if let Err(e) = desktop.fs().borrow_mut().create("/notes.txt") {
        eprintln!("create failed: {e:?}");
        return;
    }

    let files_id = desktop.launch(Box::new(Files::new(desktop.fs(), "/")), 280, 240, 0);
    let _ = desktop.frame(0);
    let _ = desktop.input(Event::Key(Key::Enter), false, 10);
    let edit_id = desktop.focused().unwrap_or(files_id);

    for (i, ch) in "hello".chars().enumerate() {
        let _ = desktop.input(Event::Text(ch), false, 20 + i as u64);
    }
    let title_unsaved = desktop.app(edit_id).map_or("", |a| a.title()).to_owned();
    let _ = desktop.frame(30);
    let h1 = fnv1a_pixels(&desktop.screen().pixels);

    let _ = desktop.input(Event::Text('\x13'), false, 40);
    let title_saved = desktop.app(edit_id).map_or("", |a| a.title()).to_owned();
    let _ = desktop.frame(40);
    let mut buf = [0u8; 32];
    let saved_len = desktop
        .fs()
        .borrow_mut()
        .read("/notes.txt", 0, &mut buf)
        .unwrap_or(0);
    let saved_text = String::from_utf8_lossy(&buf[..saved_len]).into_owned();

    let _ = desktop.close(edit_id, 50);
    let _ = desktop.frame(50);

    let routed = desktop.input(Event::Key(Key::Enter), true, 60);
    assert_eq!(routed, Routed::Handled);
    for (i, ch) in "echo hi".chars().enumerate() {
        let _ = desktop.input(Event::Text(ch), false, 70 + i as u64);
    }
    let _ = desktop.input(Event::Key(Key::Enter), false, 80);
    let _ = desktop.frame(90);
    let h2 = fnv1a_pixels(&desktop.screen().pixels);

    let _ = desktop.launch(
        Box::new(Editor::new(desktop.fs(), "/notes.txt")),
        280,
        240,
        100,
    );
    desktop.set_layout(Layout::Tiling, 100);
    let _ = desktop.frame(150);
    let _ = desktop.frame(300);
    let h3 = fnv1a_pixels(&desktop.screen().pixels);

    let mut term = Terminal::new();
    for ch in "echo hi".chars() {
        let _ = term.event(Event::Text(ch), 0);
    }
    let _ = term.event(Event::Key(Key::Enter), 1);

    println!("editor title unsaved: {title_unsaved}");
    println!("editor title saved: {title_saved}");
    println!("saved /notes.txt: {saved_text:?}");
    println!("terminal lines: {:?}", term.lines());
    println!("windows tiled: {:?}", desktop.order());
    println!("frame hash 1 (unsaved editor): {h1:#018x}");
    println!("frame hash 2 (terminal echo):  {h2:#018x}");
    println!("frame hash 3 (tiled 3 apps):   {h3:#018x}");
}

//! Walks through the slate pieces: buffer, undo tree, regex, highlighting and the modal app.
use slate::{
    ansi, decode_keys, highlight, parse_keys, App, Buffer, Cursor, Editor, Mode, Regex, Style,
};

fn main() {
    // 1. Piece-table buffer: edits never copy the original text.
    let mut buf = Buffer::new("let x = 1\nlet y = 2\n");
    buf.insert(0, "// demo\n");
    buf.delete(0, 8);
    println!(
        "buffer: {} bytes, {} lines, {} pieces",
        buf.len(),
        buf.line_count(),
        buf.pieces()
    );
    println!("buffer line 1: {:?}", buf.slice(0, 9));

    // 2. Multi-cursor editor with an undo tree.
    let mut ed = Editor::new("ab\ncd");
    ed.set_cursors(vec![
        Cursor { anchor: 0, head: 0 },
        Cursor { anchor: 3, head: 3 },
    ]);
    ed.type_text(">");
    println!(
        "editor cursors after typing at two places: {}",
        ed.cursors().len()
    );
    let undone = ed.undo();
    println!("editor undo applied: {undone}, branches: {}", ed.branches());

    // 3. Linear-time regex (no backtracking).
    match Regex::new(r"[a-z]+(\d+)") {
        Ok(re) => {
            let spans = re.find_iter("foo12 bar7 baz");
            println!("regex matches: {}", spans.len());
            for (start, end) in spans {
                println!(
                    "  match {start}..{end}: {:?}",
                    &"foo12 bar7 baz"[start..end]
                );
            }
        }
        Err(e) => println!("regex error at {}: {}", e.pos, e.msg),
    }
    match Regex::new("(a") {
        Ok(_) => println!("unexpected: '(a' compiled"),
        Err(e) => println!("regex rejects '(a' at byte {}: {}", e.pos, e.msg),
    }

    // 4. Mica syntax highlighting.
    let styles = highlight("demo.mica", "fn main() {}");
    let keywords = styles.iter().filter(|s| **s == Style::Keyword).count();
    println!(
        "highlight: {} cells, {} keyword cells",
        styles.len(),
        keywords
    );

    // 5. Modal app driven by key notation.
    let mut app = App::with_text("demo.mica", "hello", 20, 4);
    println!("start mode: {:?}", app.mode());
    for key in parse_keys("A world<Esc>") {
        app.handle(key);
    }
    println!("text: {:?}, mode: {:?}", app.text(), app.mode());
    println!("mode is normal: {}", app.mode() == Mode::Normal);

    // 6. Terminal bytes to keys, then one rendered frame.
    let keys = decode_keys(b"ix\x1b");
    println!("decoded keys: {}", keys.len());
    let screen = app.render();
    println!(
        "screen: {} rows, cursor at {:?}",
        screen.rows.len(),
        screen.cursor
    );
    let frame = ansi(&screen);
    println!("frame bytes: {}", frame.len());
    println!(
        "first row: {:?}",
        screen.rows.first().map(|r| r.trim_end()).unwrap_or("")
    );
}

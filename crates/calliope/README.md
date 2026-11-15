# calliope

calliope is a small modal text editor written as a library. It has the parts an editor needs: a
piece-table text buffer, a multi-cursor editor with a branching undo tree, a regex engine that
runs in linear time, Logos syntax highlighting, and an app layer that turns keys into edits and
renders a terminal frame. It exists so that the editing logic can be tested without a terminal,
and so that the same code can drive a real screen later.

## Features

- Piece-table `Buffer`: edits are recorded as pieces over an original and an append-only add string.
- `Editor` with several cursors that are normalised and merged; typing, backspace, undo and redo.
- Undo is a tree: `branches()` counts the branches and `redo_branch(i)` switches to one.
- Linear-time regex (`Regex`): a Thompson NFA run as a Pike VM, so there is no backtracking.
  It follows Python `re` ASCII semantics for the supported syntax.
- `highlight` colours Logos source into one `Style` per character cell.
- `App` has Normal, Insert, Command and Search modes, a modified flag and a quit request.
- `App::render` returns a `Screen` (rows, per-cell styles and cursor), and `ansi` turns it into a
  full-screen escape sequence redraw.
- Key input: `parse_keys` reads test notation such as `<Esc>`, and `decode_keys` reads raw terminal bytes.
- `App::open` loads a file from disk; `App::with_text` takes the text from memory.

## Public API

| Item | What it does |
| --- | --- |
| `Buffer::new(text)`, `insert`, `delete`, `slice`, `text` | Piece-table text |
| `Buffer::line_count`, `line_start`, `line_of`, `pieces` | Line lookup and piece count |
| `Editor::new(text)`, `cursors`, `set_cursors` | Multi-cursor editing |
| `Editor::type_text`, `backspace`, `undo`, `redo` | Edits and history |
| `Editor::branches`, `redo_branch` | Undo tree branches |
| `Regex::new(pattern)`, `is_match`, `find_at`, `find_iter` | Linear-time regex |
| `highlight(name, text) -> Vec<Style>` | Logos syntax colours |
| `App::new`, `with_text`, `open`, `handle`, `render` | Modal app and frame |
| `App::text`, `cursor`, `mode`, `modified`, `quit_requested`, `resize` | App state |
| `parse_keys`, `decode_keys`, `Key` | Keyboard input |
| `ansi(&Screen) -> String`, `Screen` | Terminal redraw |

## Demo

```
cargo run -p calliope --example demo
```

Real output:

```
buffer: 20 bytes, 3 lines, 1 pieces
buffer line 1: "let x = 1"
editor cursors after typing at two places: 2
editor undo applied: true, branches: 1
regex matches: 2
  match 0..5: "foo12"
  match 6..10: "bar7"
regex rejects '(a' at byte 0: unbalanced parenthesis
highlight: 12 cells, 2 keyword cells
start mode: Normal
text: "hello world", mode: Normal
mode is normal: true
decoded keys: 3
screen: 4 rows, cursor at (0, 10)
frame bytes: 128
first row: "hello world"
```

## Tests

```
cargo test -p calliope
```

The gate for this crate is `bash tools/check.sh calliope`.

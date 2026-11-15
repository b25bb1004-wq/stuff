# iris

iris is a software window compositor. It keeps a list of windows, lays them out
either freely (floating) or as a master-and-stack tiling, animates moves and
layout changes, and draws only the screen regions that changed since the last
frame. It is a library: the caller owns the screen canvas and decides when to
call `frame`. Its only workspace dependencies are `pigment` and `indium`.

Redrawing the whole screen on every tick wastes work. iris records what changed
(window moves, resizes, raises, focus changes and client damage) and returns only
those rects, so a caller can push small updates.

## Features

- Floating and tiling layouts, switched at runtime with `set_layout`.
- Windows get a 1 px border and an 18 px title bar; the frame rect includes them.
- `move_to` and layout changes animate over 180 ms with an ease-out cubic curve.
- Damage tracking: `frame` returns the changed screen rects and nothing else.
- Client content is written through `surface_mut`, or marked with `damage` for
  partial updates.
- Input routing with `input`: compositor shortcuts, title-bar drag, and delivery
  to a window with coordinates moved into its content area.
- Stacking order: a new window goes on top and takes focus; `raise` moves a window up.

## Public API

| Item | Purpose |
| --- | --- |
| `Compositor::new(w, h)` | Create a screen filled with the desktop colour. |
| `open(title, w, h, now_ms)` | Open a window on top and focus it; returns a `WinId`. |
| `close(id, now_ms)` | Close a window; returns `true` if it existed. |
| `move_to(id, x, y, now_ms)` | Move a window, animating when floating. |
| `resize(id, w, h, now_ms)` | Resize a window's client surface. |
| `set_layout(layout, now_ms)` | Switch between `Layout::Floating` and `Layout::Tiling`. |
| `surface_mut(id)` | Borrow the client canvas and mark the window fully damaged. |
| `damage(id, rect)` | Mark a part of the client content as changed. |
| `raise(id)`, `focused()`, `order()` | Stacking and focus queries. |
| `rect(id)` | Current animated frame rect of a window. |
| `input(event, super_held, now_ms)` | Route an input event; returns a `Routed` value. |
| `frame(now_ms)` | Advance animations, composite damage, return changed rects. |
| `screen()` | Borrow the composited screen canvas. |

## Demo

```
cargo run -p iris --example demo
```

Output (program stdout):

```
opened windows 1 and 2 on a 640x400 screen
stacking order (bottom to top): [1, 2]
focused window: Some(2)
frame at 0 ms: 1 changed rects
frame at 10 ms with no edits: 0 changed rects
frame at 100 ms, window 1 easing: 5 changed rects
frame at 200 ms, ease finished: 4 changed rects
window 1 final rect: x=40 y=30 w=322 h=220
tiling layout, frame at 400 ms: 1 changed rects
window 2 tiled rect: x=320 y=0 w=320 h=400
closed window 2: true
focused after close: Some(1)
frame at 600 ms after close: 3 changed rects
```

The final rect is 322 by 220 because a 320 by 200 client area gets the border
(2 px in width) and the title bar plus borders (20 px in height) added around it.

## Tests

```
cargo test -p iris
```

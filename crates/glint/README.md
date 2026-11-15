# glint

Glint is a small widget toolkit for Strata. It keeps a tree of widgets, lays them out with a flex
model, routes mouse and keyboard input, and draws the result in software onto a pigment canvas.
It also holds the state machines behind list, scroll, tab, menu and text field widgets, which
can be tested without drawing anything. It depends only on pigment.

## Features

- Widget tree with `Box`, `Label`, `Button` and `Field` nodes; `add`, `remove`, `children`, `text`, `set_text`.
- Flex layout with direction, justify, align, gap, padding, grow, fixed width and height, and minimum size.
- Mouse hit testing, Tab and Shift+Tab focus movement, Escape to clear focus.
- Click or Space activates a button; Enter on a field submits it.
- Text field editing: typed characters, Backspace, and a `Changed` action on every edit.
- Software rendering with a light or dark `Theme`, with a configurable text scale.
- Widget state without drawing: `ListState`, `ScrollState`, `TabsState`, `MenuState`, `FieldState`.
- Integer easing, tweening and a damped spring in `anim`. No floats.

## Public API

| Item | What it does |
|------|--------------|
| `Ui::new(theme)` | Create a tree with a root `Box` at node `0`. |
| `Ui::add(parent, kind, style)` | Add a child under a live `Box`; returns `None` otherwise. |
| `Ui::layout(width, height)` | Compute rectangles for every live node. |
| `Ui::event(event)` | Handle an `Event`; returns the `Action`s in order. |
| `Ui::render(&mut canvas)` | Draw the tree with the theme. |
| `Ui::rect`, `Ui::hit`, `Ui::focus` | Query geometry, hit testing and focus. |
| `widgets::{ListState, ScrollState, TabsState, MenuState, FieldState}` | Pure widget state machines. |
| `anim::{ease, tween, Spring}` | Easing, tweening and spring motion. |

## Demo

    cargo run -p glint --example demo

Real output:

```
nodes: root=0 label=Some(2) field=Some(3) button=Some(4)
column rect: x=0 y=0 w=200 h=72
button rect: x=0 y=48 w=200 h=24
click actions: [Focused(Some(4)), Clicked(4)]
focus after Tab: Some(3)
typed actions: [Changed(3)]
field text: Some("Ada!")
canvas 200x120, corner pixel: Some(Rgba(245, 245, 245, 255))
list selected=Some(2) offset=0
tween 0->100 at 50 of 100 ms (OutQuad): 75
ease OutCubic at t=500 of 1000: 875
spring after 200 steps: pos=152 vel=225 settled=false
```

The demo is in `examples/demo.rs`. It uses no clock and no randomness, so the output is the same on every run.

## Tests

    cargo test -p glint

The test files cover animation, events, layout and the widget state machines. The gate is
`bash tools/check.sh glint` from the repository root.

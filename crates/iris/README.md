# iris

iris is a software window compositor and desktop shell. It manages windows in
floating or master-and-stack tiling layouts, animates moves and layout changes,
tracks minimal screen damage, and hosts interactive applications on a shared
in-memory `hades` filesystem.

## Features

- Floating and master-and-stack tiling layouts (`Layout::Floating`, `Layout::Tiling`).
- 1 px border and 18 px title bar chrome around each client surface.
- 180 ms ease-out cubic animations on `move_to` and layout switches.
- Tight screen-space damage rects from `frame(now_ms)`.
- Application protocol (`App`, `Effect::{None, Close, Open}`) and `Desktop`
  window/session manager with `Super+Enter` terminal launch.
- Fixed 8x16 monospace text grid rendered with the same bitmap font as `indium`.
- Built-in apps:
  - `Terminal`: `krypton::exec::Shell` with `$ ` prompt, 200-line scrollback, and `exit` close.
  - `Editor`: `calliope::Editor` buffer with typing, arrows (`\u{2190}`-`\u{2193}`), Backspace, `Ctrl+S` (`\x13`) save to `hades::Fs`, and `*` unsaved title indicator.
  - `Files`: directory browser on the shared `hades::Fs` (`MemDisk`), Up/Down selection, Backspace to parent, and Enter to enter directories or open files in `Editor`.

## Public API

| Item | Purpose |
| --- | --- |
| `Compositor` | Low-level window compositor (`open`, `close`, `move_to`, `resize`, `set_layout`, `input`, `frame`, `screen`). |
| `App`, `Effect` | Application trait (`title`, `event`, `paint`) and window lifecycle effects (`None`, `Close`, `Open`). |
| `Desktop` | High-level desktop hosting `Compositor`, `App`s, `SharedFs`, and `Super+Enter` terminal shortcut. |
| `Terminal` | Terminal emulator backed by `krypton::exec::Shell`. |
| `Editor` | Text editor backed by `calliope::Editor` and `SharedFs`. |
| `Files` | Directory browser over `SharedFs` opening `Editor` windows. |

## Demo

```
cargo run -p iris --example demo
```

Runs a scripted session (Files -> `/notes.txt` -> Editor -> `Ctrl+S` -> Terminal `echo hi` -> 3-window tiling) and prints the text state and the 3 FNV-1a 64-bit golden frame hashes (`0x20197a7eca68442d`, `0xa3fd4fe5d730988f`, `0x7cb528c4ebff2e47`).

## Tests

```
cargo test -p iris
```

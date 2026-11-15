# Apps and tools: every system ships as its own app and its own command-line tool

v1, written by the leader on 2026-11-16 (D7) because the operator said: "everything u have built should exists as
seperate app and respective tool". Workers follow this file exactly; the leader changes it.

## The rule
Each of the 25 systems has:
1. **An app**: a standalone, installable web app at `/<id>/` in the public bundle. It opens in its own window,
   works offline and has its own name and icon. The site's section for the system gets an "Open app" button.
   `/apps/` is a launcher with all 25 apps in a grid.
2. **A tool**: a command-line program named after the system (`sphinx`, `hades`, ...), built by
   `cargo build --release`. It runs on Linux, Windows and macOS unless the system itself is OS-bound (pandora is
   Linux-only, titan, lithium and zeus run on our Silicon emulator).

## Tool conventions (every tool)
- Lives in its own crate as `src/bin/<id>.rs`. If a dependency cycle forbids that, the leader names the crate.
  Std only, hand-written argument parsing, no shared CLI crate.
- `<id> --help` prints usage with every subcommand and one example each; `<id> --version` prints `<id> 0.1.0`.
- Files are paths; `-` means stdin or stdout. Output is plain text, one fact per line, stable enough to grep.
  `--json` is not required.
- Errors go to stderr as `<id>: <message>`. Exit codes: 0 ok, 1 the operation failed (bad input, check failed),
  2 usage error. Never panic on user input.
- No network access except the tools whose job is network (mercury `get`/`serve`, xenon, argo `serve`, aurum
  `node`, iridium client), and those bind to `127.0.0.1` unless `--listen` says otherwise.
- Tests: `tests/cli.rs` runs the binary through `env!("CARGO_BIN_EXE_<id>")` with temp files: `--help`,
  `--version`, every subcommand on a good input, one bad input per subcommand (exit 1 and a `<id>:` message),
  an unknown subcommand (exit 2).

## App conventions (every app)
- Files in `crates/site/apps/<id>/` (`index.html`, `app.js`, optional `app.css`), plus the shared
  `crates/site/apps/_kit/` (`kit.js`, `kit.css`). The pack step writes `out/<id>/` with `manifest.webmanifest`,
  `sw.js` and icons generated per app. Demo code runs through the existing `site.wasm` registry at the bundle root.
- Window: a 40 px title bar (icon, `Monolith <Name>`, Install button when installable, theme toggle, About),
  then the app filling the rest. About panel: one paragraph, the source folder, and the tool's `--help` text.
  In window-controls-overlay mode the title bar sits in the OS title bar (`env(titlebar-area-*)`).
- Manifest: `name` `Monolith <Name>`, `short_name` `<Name>`, `display: standalone`, `display_override`
  `["window-controls-overlay","standalone"]`, `start_url` and `scope` `./`, colours `#0e1013`, icons 192, 512
  and maskable 512.
- Icon: drawn by pigment at pack time: `--bg` rounded square, the system's initial in `--accent` from a built-in
  5x7 bitmap font scaled up, and one thin `--line` ring. Same look for all 25 so they read as one family.
- Service worker: caches its own files plus `/site.wasm` (and `/titan.elf` where used) under a cache name with a
  content hash; cache-first, network fallback, old caches deleted.
- Tokens, motion, 360 px, keyboard and reduced-motion rules from `docs/FRONT.md`. No requests to other hosts.
- State the user makes (files, databases, disks, repos, wallets) persists in localStorage or IndexedDB under
  `monolith.<id>.*`, with Export (download) and Import (file picker) in the app's menu.

## The 25 (app at `/<id>/`, tool `<id>`)
| id | App | Tool subcommands | Tool state |
|---|---|---|---|
| titan | full-window terminal; disk persists | `boot [--disk IMG] [--script FILE]` (boots on Silicon; bin in `neon`) | new |
| iridium | SQL console: editor, results grid, table list, saved queries | `sql DB.idb [QUERY or -]`, `connect ADDR` (keep `iridium-cli`) | extend |
| elysium | bytecode inspector: compile, verify, step, stack and heap view | `verify`, `dis`, `run` on `.mbc` files | new |
| mercury | HTTP workbench: compose a request, see parse and response | `parse FILE`, `serve DIR [--listen]`, `get http://URL` | new |
| helios | the original game, full window | `run GAME [--ticks N]` prints world hash, `replay FILE` | new |
| cobalt | compiler explorer: source, IR, optimised IR, bytecode panes | `ir`, `opt`, `build -o OUT.mbc` | new (keep `logos` runner) |
| logos | playground: files, run, tests, props | `check`, `fmt`, `run`, `test` (bin in `cobalt`) | extend |
| odyssey | small browser: address bar over bundled pages, render to canvas, DOM tree | `tokens`, `tree`, `select SEL`, `render FILE.html --out PNG` | new |
| aurum | wallet and block explorer on an in-browser chain | `keygen`, `tx`, `verify CHAIN`, `merkle`, `node` (keep deploy's `aurum-node`) | new |
| sphinx | crypto lab: hash, HMAC, keys, sign, verify, seal, open | same verbs | new |
| silicon | RISC-V machine: load ELF, run, step, registers, memory, UART | `run ELF [--steps N] [--trace]` | new |
| hades | disk explorer: tree, upload, download, fsck | `mkfs`, `ls`, `cat`, `put`, `rm`, `mkdir`, `fsck` on an image | new |
| pandora | container spec editor and validator, recorded sessions (runtime is Linux-only) | keep | done |
| argo | package browser and solver: index, manifest, lock graph | `solve`, `lock`, `install`, `serve` | new |
| krypton | shell playground: parse tree, expansion, run on a virtual fs | keep | done |
| iris | the desktop compositor in a canvas | `replay EVENTS --out PNG` | new |
| indium | widget gallery rendered by indium, keyboard navigation | `render LAYOUT --out PNG` | new |
| calliope | text editor: multi-cursor, undo tree, regex search | `grep REGEX FILE...`, `edit SCRIPT FILE` | new |
| athena | the IDE (T72, T73, T85) | `check`, `fmt`, `test` | new |
| chronos | repository viewer: commit graph, tree, diff on an in-browser repo | `init`, `add`, `commit`, `log`, `branch`, `checkout`, `cat-file` on disk | new |
| xenon | transport simulator: loss and delay sliders, throughput graph | keep | done |
| lithium | OS console: task interleave view | `run [--ticks N]` on Silicon | new |
| kairos | scheduler visualiser: Gantt chart per CPU | `sim FILE` | new |
| midas | allocator visualiser: buddy blocks and slab classes | `sim FILE` | new |
| zeus | hypervisor console: two guests side by side | `run [--guests N]` on Silicon (host tool; the riscv `zeus` bin stays) | new |

## Order
Tools first, in batches of about five crates per task, never sharing a crate with a running task (T86a-e). Apps
after T75b: T87 builds the kit, the launcher and the apps whose demo already exists; T88 adds the rest with T75c.

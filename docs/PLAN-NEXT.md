# Monolith, next: features only this stack can do, and one motion language

Written by the architect on 2026-11-16, after `docs/REVIEW.md`. It covers three things, in this order:
simplify the front end to one kit, give every page one motion system, then build seven features that exist
only because the CPU, kernels, hypervisor, database, compiler, VM and browser engine are all ours and all run
in one wasm file. Tasks start at T90. Leader decisions are listed separately in section 6.

## 1. Rules for this plan
1. Zero dependencies applies to JavaScript and CSS too. No library, no CDN, no web font. The Web Animations
   API, View Transitions, `IntersectionObserver`, `requestAnimationFrame` and canvas are platform features.
2. No fake motion. Every moving thing on screen shows real state that the wasm produced (bytes, blocks,
   instructions, packets). If there is no data behind it, it does not move.
3. One crate per task, and the rules in `briefs/_RULES.md` hold. Tasks that run before T75c, T74a2 and T74b
   finish are additive only: new modules and new functions, no change to an existing signature.
4. Nothing here touches `crates/site` before T75c is done, `crates/cobalt` or `crates/neon` (its gate builds
   cobalt) before T74a2 is done, or `crates/athena` before T74b is done.
5. A task is done when its gate passes and the leader's own check passes, as before.

## 2. Order
| Wave | Can start | Tasks | Result |
|---|---|---|---|
| 0 | today | T90 to T94 | cores gain the APIs the features need (indium, silicon, zeus, midas) |
| 1 | after T74a2 | T95 to T98, T119, T120 | the shared web kit, motion engine, rewind and descent cores |
| 2 | after T75c and T74b | T99 to T104 | site and Athena on the kit, service workers fixed, motion on every page |
| 3 | after T104 | T105 to T111 | a signature moment in each app |
| 4 | after T104 | T112 to T118 | the seven features |

`crates/site` takes 18 of the 31 tasks, one after another (about 42 hours). In waves 3 and 4 the "may touch"
lists mostly do not overlap (one app folder and one demo file each), so the leader may run two of them at once
with separate build directories if it wants the time back. T115 (`pack.rs`) and T118 (kit, `wasm.rs`) touch
shared files and run alone.

## 3. Features (seven kept)
Each is feasible with crates that exist. "Needs" names the API; new API is a task in section 5.

**F1 Descent (flagship).** Type one command in Titan and watch it go down the stack: the line as Krypton parsed
it, the system calls Titan served, the blocks Hades read, the instructions Silicon retired, the bytes that came
back on the UART. Five layers, each with a live counter, drawn top to bottom. Only possible when you own every
layer. Needs: `silicon::probe` (new, T91: trap counts by cause, the last 64 `ecall` numbers, UART and block
counters), `neon` wasm export (T98), the existing krypton demo for the parse. UI: T112.

**F2 Rewind the OS.** A scrubber under the Titan terminal: drag left and the whole machine goes back to that
instruction, then type something different. Works because Silicon is deterministic (the timer ticks once per
step) and `Machine` is `Clone`. Needs: `silicon::machine::Snapshot` with 4 KiB page deltas (new, T92), a
checkpoint list plus an input log keyed by `instret` in `neon` (T97). UI: T113.

**F3 Pull the plug.** In the Hades and Iridium apps, run some writes, then drag a slider to the write where the
power fails. The app cuts the disk at that write, remounts, and shows what survived and that the check passes.
The leader checks already do this at every write; this makes it visible. Needs: `hades::disk::CrashDisk`,
`iridium::crash::CrashStorage` (both exist). UI: T114.

**F4 Break out of the guest.** Two guests side by side under Zeus. Press "read the neighbour's memory": the spy
guest tries, the guest page fault is caught, and the trap record (cause, guest address) appears in the
hypervisor log while the other guest keeps printing. Needs: `zeus::build_machine` and the spy fixture (exist),
a small stepping wrapper `zeus::scene` (new, T93). This is also the Zeus app, which does not exist yet. UI: T115.

**F5 Race.** One Logos program, compiled twice by Cobalt: to MBC run by Elysium, and to RV64 run by Silicon.
Both run side by side with their outputs and instruction counts, and a line says whether the outputs match.
Needs: `cobalt::run::run_main` and `cobalt::rv64::run` (exist), Elysium fuel used and `cpu.instret`. Honest
label: in the browser the RV64 side runs on bare Silicon; the run inside Titan stays a native leader check
(quality bar 7). UI: T116.

**F6 Wire.** The Odyssey app loads a page from a Mercury server that lives in the same wasm, over a simulated
Xenon link. A loss slider drops packets; you see them in flight, lost, resent, and the page still renders.
Needs: the Xenon simulation used by the xenon demo (`Lossy`, `crates/xenon/src/udp.rs:21`), Mercury request
handling, Odyssey parse, layout and paint. `odyssey::fetch::get` uses host sockets, so the demo builds the
request itself. UI: T117.

**F7 Proof links.** A Share button in every app puts the input in the URL fragment with the SHA-256 of the
output, computed by our own Sphinx. Opening the link runs it again and shows "same result" or "different".
Helios adds its world hash. Determinism becomes something a visitor can test on their own machine. Needs: one
wasm export for the digest of the last output. UI: T118.

Cut, with the reason: *Monolith renders Monolith* (a page cannot read Chrome's own pixels, so there is no real
diff to show); *live chain with a gas meter* and the *avalanche heatmap* are kept, but as the signature moments
of Aurum and Sphinx (section 4.5), not as features.

## 4. Motion system

### 4.1 The idea
Monolith is cut stone: heavy, exact, layered. So things move a short distance and stop firmly; nothing bounces
unless it is live data arriving. Two axes carry meaning everywhere. **Vertical is the stack**: down is toward
the hardware, up is toward the person (Descent goes down, output rises). **Horizontal is time**: history,
replay, scrubbing and logs run left to right.

The web UI and the GUI inside the machine move on the same curves. Indium already has an integer damped spring
(`crates/indium/src/anim.rs`). The pack step samples it and writes CSS `linear(...)` easings and a JS table, so
a DOM panel and an Iris window open with the identical curve, generated from Rust and tested there (T90, T96).

### 4.2 Tokens (in `tokens.css`; the only values allowed)
| Token | Value | Use |
|---|---|---|
| `--t-press` | 80 ms | press and release feedback |
| `--t-fast` | 120 ms | hover, small state change, every exit of a small element |
| `--t-med` | 240 ms | tabs, list items, tooltips |
| `--t-large` | 400 ms | panels, dialogs, view transitions |
| `--t-slow` | 600 ms | site reveals |
| `--t-hero` | 1200 ms | hard cap for a whole signature sequence, stagger included |
| `--ease-out` | `cubic-bezier(.2,.7,.2,1)` (today's `--ease`, kept as alias) | anything entering |
| `--ease-in` | `cubic-bezier(.4,0,1,.6)` | anything leaving |
| `--ease-move` | `cubic-bezier(.6,0,.2,1)` | moving while staying on screen |
| `--spring-firm`, `--spring-firm-t` | generated; no overshoot, settled by 280 ms | panels, tabs, cursor, focus ring |
| `--spring-live`, `--spring-live-t` | generated; 4 to 8 percent overshoot, settled by 450 ms | data arriving, windows, signature moments |
| `--spring-heavy`, `--spring-heavy-t` | generated; no overshoot, settled by 700 ms | large slabs, the site layer stack |
| `--d-1` to `--d-4` | 4, 8, 16, 32 px | travel: controls, list items, reveals, panels |
| `--s-enter`, `--s-press` | 0.96, 0.98 | scale at the start of an enter, during a press |
| `--stagger` | 30 ms, at most 8 steps | lists and grids; items after the eighth arrive with the eighth |

Browsers without `linear()` get `--ease-out` with the same duration (an `@supports` block in `tokens.css`).

### 4.3 The kit (`crates/neon/web/kit/`, served as `/_kit/`)
`tokens.css` (colours, type, space, motion), `wasm.js` (the one loader), `term.js` (the one Titan terminal) and
`motion.js`, a global `Motion` like today's `Kit`:
- `Motion.enter(el, opts)`, `Motion.exit(el, opts)`, `Motion.stagger(list, opts)`: Web Animations on `opacity`
  and `transform` only; each returns a handle with `finish()`. A new call on the same element starts from the
  current value, so motion can always be interrupted.
- `Motion.flip(el, mutate)`: measure, change the layout, play the difference as a transform.
- `Motion.swap(update, name)`: a same-document view transition, or a plain call to `update` where unsupported.
- `Motion.count(el, to)`: number roll in fixed-width digits. `Motion.tween(ms, curve, onFrame)`: for canvas.
- `Motion.canvas(canvas, draw)`: one shared frame loop for every canvas and for the terminal pump; it pauses
  canvases that are off screen or in a hidden tab, caps device pixel ratio at 2, and lowers `quality` when the
  draw time average goes over budget.
- `Motion.hero(id, play)`: the signature moment; one at a time, once per session unless the visitor asks again.
- `Motion.reduced()`: true under `prefers-reduced-motion` or the kit's own toggle.

### 4.4 Choreography
- Enter: fade in and travel one distance token along the axis that carries the meaning, `--ease-out` or a spring.
- Exit: one duration token shorter than the enter, `--ease-in`, half the distance, never staggered.
- Hand-off: when the same thing exists before and after (launcher icon to title bar icon, a tab's panel, the
  debugger's current line), it moves; it does not fade out and in. Cross-page hand-offs use
  `@view-transition { navigation: auto }` with a `view-transition-name`; elsewhere the page just loads.
- One hero per view. While it plays, nothing else starts. Any key press, click or scroll finishes it at once.
- Motion never delays work: the wasm call starts in the same frame as the press animation, not after it.
- Nothing loops forever except a state that is truly running (a cursor, a live counter).

### 4.5 Signature moments (one per app, plus the site, the launcher and Athena)
| Where | Moment | Real data behind it |
|---|---|---|
| site | Core sample: the 25 layers stack up from the bottom on load (`heavy`, staggered); the active layer lights as you scroll; "Open app" carries the section icon into the app | section list, scroll position |
| launcher | Icons arrive in build order; a click carries the icon into the app's title bar | app list |
| athena | Run line: a hairline crosses the top edge with one tick per compile stage; the debugger's current-line bar slides to the next line | stage results, debugger position |
| titan | Cold boot: an amber scan line follows the UART output down the screen while an instruction odometer rolls | pump output, `instret` |
| lithium | Two lanes: tasks A and B as two tracks, a marker swings at each context switch | the A/B transcript |
| zeus | Two worlds: two consoles with a wall between; a blocked read hits the wall and a trap record drops into the log | guest fault record |
| silicon | Register grid: a cell pulses when written; a tick runs along the disassembly | register writes per step |
| kairos | Run queue: task bars on worker lanes; a stolen task hops lanes on an arc | scheduler trace |
| midas | Buddy split: the arena bar halves on alloc and joins on free; a double free flashes `--bad` | block list |
| hades | Journal then home: a write lights journal blocks, then home blocks, then a checksum tick | block write order |
| iridium | Log strip: each statement appends a WAL segment; touched B+tree pages light in a page map | WAL records, page ids |
| chronos | Commit graph: commits drop into lanes, a merge draws its joining curve, HEAD slides | commit graph |
| aurum | Block forge: a block lands on the chain (`heavy`); a tampered block turns `--bad` and greys what follows; a fuel bar per contract call | verify result, `fuel_used` |
| argo | Solve: dependency nodes settle as versions are picked; a conflict flashes and backtracks | solver steps |
| sphinx | Avalanche: flip one input bit and the output cells that changed turn over in a ripple; Medusa labelled experimental | two digests |
| pandora | Seal: one frame per namespace closes around the process in setup order | setup steps |
| logos | Check wave: an underline sweeps the source through lex, parse and check; errors land on their span | spans per stage |
| cobalt | Lowering: pick a source line and bands sweep to its IR and its RV64 instructions | line map |
| elysium | Stack machine: operand slabs push and pop (`live`); a GC sweep wipes the heap grid | step trace |
| krypton | Pipeline: the stages of a pipe joined by a line, bytes travel along it | parsed pipeline, byte counts |
| calliope | Rope: the rope's leaves as a strip under the text, splitting and joining on edits; the cursor morphs with the mode | leaf lengths, mode |
| mercury | Round trip: a request slab crosses to the server, the status lands in its colour | request, response |
| xenon | Packets in flight: packets cross a timeline, lost ones fall away, resends in accent | simulation events |
| odyssey | Layout first: box outlines appear in tree order, then the painted page fades in over them | layout boxes, pixels |
| helios | Ghost trail: scrubbing a replay leaves a fading trail; the world hash stamps when a replay matches | replay frames, hash |
| indium | Spring lab: drag stiffness and damping; see the integer spring and the CSS curve from the same numbers | `indium::motion` samples |
| iris | Same curve twice: a window opens in the framebuffer while a DOM panel opens beside it, both on `live` | iris frames |
| chisel | Bit fields: the 32 bits of an assembled instruction light field by field | encoder fields |

### 4.6 Performance budget
| Budget | Limit |
|---|---|
| Frame | 16.7 ms; under 2 percent dropped frames during motion, under 10 percent with 4x CPU throttle |
| Script per frame during motion | 4 ms at the 95th percentile; canvas draw 6 ms |
| Wasm slice per frame while motion runs | 8 ms; `term.js` tunes steps per slice to hold it |
| Long tasks | none over 50 ms during motion (one, up to 120 ms, allowed under 4x throttle at wasm compile) |
| DOM animation properties | `opacity` and `transform` only; no `will-change` at rest |
| At once | 1 hero, 2 animating canvases, 24 animating elements |
| Canvas | backing store at most 1280 by 720, device pixel ratio capped at 2 |
| Off screen or hidden tab | every loop paused |
| Layout shift from motion | zero: space is reserved before anything enters |
| Size (raw, not compressed) | `motion.js` 10 KB, `tokens.css` 4 KB, each app's `sig.js` 6 KB, no JS file over 64 KB |
| Wasm growth | `site.wasm` grows by at most 200 KB across all of section 5 |

### 4.7 Reduced motion
`tokens.css` holds one switch: under `prefers-reduced-motion: reduce` every transition and animation lasts
0.01 ms and runs once, and view-transition groups do not animate. Every `Motion` call applies the end state in
the same frame and resolves. A canvas draws its final frame once. Signature moments show their end picture.
The information is the same; only the travel is gone.

### 4.8 How each piece is checked
1. **Bundle lint in Rust** (`neon::webkit::lint`, called from `crates/site/tests/motion_lint.rs` and Athena's
   bundle test): no `http://` or `https://`; a `transition` or `@keyframes` names only `opacity` and
   `transform`; every duration is a token (no bare `ms` or `s` outside `tokens.css` and the generated table);
   the reduced-motion switch is present and every page links `tokens.css`; no `will-change`; no
   `setInterval(`, and `requestAnimationFrame(` only inside the kit; the size budgets of 4.6.
2. **Self-test page** (`/_kit/selftest.html`): prints one `PASS <name>` or `FAIL <name> <why>` line per check
   and sets the title to `SELFTEST PASS n/n`. It checks the tokens, that each spring table ends at 1 and stays
   inside its overshoot bound, that every `Motion` call under forced reduced motion ends within one frame, that
   running animations touch only `opacity` and `transform` (`document.getAnimations()`), the frame and draw
   times of each registered signature over 120 frames, the run time of every app example, and long tasks.
3. **Leader's Chrome trace**: 4x CPU throttle, record the site scroll from top to bottom and each signature
   moment; pass when the limits of 4.6 hold. Then 360 px, light theme, reduced motion, server stopped.
4. **Rust tests for the curves** (T90): the three springs settle, respect their overshoot bounds, and produce
   the same samples on every run.

## 5. Tasks
Every task follows `briefs/_RULES.md`. "Gate" means `bash tools/check.sh <crate>` ends with `CHECK PASS`.
Lane is `hydrogen-high` unless a line says otherwise; `argon` is named only as the fallback for a hard core.

### Wave 0 (start today; additive only)
**T90 indium: curves from the integer spring.** 90 min. Needs: none. May touch: `crates/indium/`.
- Build: `src/motion.rs`: presets `FIRM`, `LIVE`, `HEAVY` (stiffness, damping, duration), `sample(preset, n)`
  giving `n` fixed-point points from `Spring::step`, and `css_linear(points)` giving a `linear(...)` string of at
  most 32 stops.
- Accept: `tests/motion.rs`: each preset settles by its duration, `FIRM` and `HEAVY` never pass the target,
  `LIVE` peaks between 4 and 8 percent, output is identical across runs; gate. Leader: paste one string into
  Chrome and see the curve.

**T91 silicon: probe.** 90 min. Needs: none. May touch: `crates/silicon/`. Fallback lane: argon.
- Build: `src/probe.rs`: `Probe` with trap counts by cause, a ring of the last 64 `ecall` records (`a7`,
  `instret`), UART bytes in and out, block reads and writes; `Machine::probe()` and `probe_reset()`. Counters
  only on trap and device paths, nothing per instruction.
- Accept: tests for each counter; the 110 riscv-tests still pass; the report gives emulator speed before and
  after, within 3 percent; gate.

**T92 silicon: snapshots.** 120 min. Needs: T91. May touch: `crates/silicon/`. Fallback lane: argon.
- Build: `machine::Snapshot`: CPU and device state plus the RAM and disk pages (4 KiB) that differ from a
  base snapshot; `Machine::snapshot(base)` and `Machine::restore(chain)`.
- Accept: property test: for random programs, run N steps, snapshot, run M more, restore, run M again, and the
  CPU state and UART output are equal; a snapshot of an idle machine after boot is under 1 MiB; gate.

**T93 zeus: scene.** 120 min. Needs: none. May touch: `crates/zeus/`.
- Build: `scene::Scene::new(zeus_elf, guests)`, `step(max)`, `console(guest)`, `last_fault()` (guest, cause,
  guest address), built on `build_machine` and `split_console`; the spy guest source (`tests/fixtures/spy.s`)
  is embedded so other crates can build the same scene.
- Accept: test: Lithium and the spy run, the fault record shows cause 21 or 23 for the spy, Lithium's output
  is unchanged by the spy; gate.

**T94 midas: safe simulator.** 90 min. Needs: none. May touch: `crates/midas/`.
- Build: `sim::Sim::new(bytes)` owning its arena, `alloc(size)`, `free(handle)` returning `Err` on a double or
  foreign free, `blocks()` listing offset, order and state. The real buddy and slab code runs underneath; all
  `unsafe` stays inside midas with `// SAFETY:` lines.
- Accept: property test against a shadow model; double free is `Err`, never a panic; gate.

### Wave 1 (after T74a2)
**T95 neon: the web kit, part 1.** 120 min. Needs: T74a2. May touch: `crates/neon/`.
- Build: `web/kit/{tokens.css,wasm.js,term.js}` (one loader that takes the wasm URL, one terminal with the
  pump, colour, type and space tokens from `docs/FRONT.md`); `src/webkit.rs`: `FILES`, `run_cargo`, content
  hash, the service worker template of T100, and `lint`. Delete `web/index.html`, `web/neon.js`,
  `web/style.css`, `examples/pack.rs` and the page half of `src/pack.rs` (the wasm build half stays).
- Accept: `tests/webkit.rs`: `FILES` has the three files, `lint` rejects a sample with a URL and one with
  `transition: width`; `tests/wasm_build.rs` still checks the wasm; `ls crates/neon/web/neon.js` fails; gate.

**T96 neon: motion engine.** 150 min. Needs: T90, T95. May touch: `crates/neon/` (host-only path dependency on
`indium` allowed).
- Build: `web/kit/motion.js` (section 4.3), the motion tokens of 4.2 in `tokens.css`, `webkit::motion_css()`
  and `motion_table_js()` generated from `indium::motion`, `web/kit/selftest.html` (section 4.8, item 2).
- Accept: `lint` passes on the kit; a test checks the generated CSS has the three springs and the reduced
  switch; `motion.js` is under 10 KB. Leader: the self-test page prints only `PASS` lines in Chrome.

**T97 neon: rewind core.** 120 min. Needs: T92, T95. May touch: `crates/neon/`.
- Build: in `session.rs`: a checkpoint every 2 million instructions (page deltas, total capped at 48 MiB,
  oldest thinned first), an input log keyed by `instret`, `Titan::seek(instret)`; wasm exports `titan_seek`,
  `titan_instret`, `titan_checkpoints`.
- Accept: property test: for random scripts, `seek(n)` then replay gives the same transcript and `instret` as
  the straight run; typing after a seek cuts the old future; gate.

**T98 neon: descent exports.** 90 min. Needs: T91, T97. May touch: `crates/neon/`.
- Build: `Titan::probe()` and a wasm export `titan_probe` that writes the counters as `name value` lines.
- Accept: test: after `ls`, `ecall` count and block reads are above zero and UART bytes out equal the output
  length; the native and wasm transcripts stay equal; gate.

**T119 argo: no panic in `pkg`.** 90 min. Needs: T74a2. May touch: `crates/argo/`.
- Build: `encode` and `sign` return `Result<_, PkgError>`; all six `assert!` and `expect` sites become errors.
- Accept: a test per bad value returns `Err`; `grep -nE "assert!|expect\(" crates/argo/src/pkg.rs` hits only
  the test module; gate.

**T120 chisel: no panic in `elf::write`.** 90 min. Needs: T74a2. May touch: `crates/chisel/` (the leader
changes the one call in `crates/cobalt/src/rv64/mod.rs`).
- Build: `write` returns `Result<Vec<u8>, ElfError>`.
- Accept: tests for too many segments, `mem_size` below the data length and address overflow return `Err`; gate.

### Wave 2 (after T75c and T74b)
**T99 site: adopt the kit.** 150 min. Needs: T75c, T95. May touch: `crates/site/`.
- Build: `web/site.js`, `apps/_kit/kit.js` and `apps/titan/app.js` use `/_kit/wasm.js` and `/_kit/term.js`;
  `web/style.css` and `apps/_kit/kit.css` keep only their own rules and link `tokens.css`; `pack` writes
  `neon::webkit::FILES` to `out/_kit/`; "Strata" becomes "Monolith" in the two UI strings.
- Accept: `grep -rlE "instantiateStreaming|function pumpFrame" crates/site/web crates/site/apps` prints
  nothing; `tests/apps_bundle.rs` and `tests/wasm_build.rs` pass; gate. Leader: site, Titan app and three demo
  apps behave as before.

**T100 site: service workers.** 120 min. Needs: T99, T74b. May touch: `crates/site/`.
- Build: `pack` uses the shared template: a core cache named by the hash of `site.wasm`, `titan.elf` and
  `_kit/*`, a per-app cache for the app's own files (including `app.css` and `sig.js` when present), lookups
  only in those two opened caches; the wasm exports `site_abi()` and the kit reloads once on a mismatch.
- Accept: bundle test: no `sw.js` contains `caches.match(`, every listed file exists, `site.wasm` is listed
  once per worker under the core list. Leader: the Chrome steps of `docs/REVIEW.md` B1 and B2.

**T101 athena: adopt the kit and the worker.** 120 min. Needs: T74b, T95. May touch: `crates/athena/`.
- Build: `web/athena.js` uses the kit loader and terminal, `web/style.css` links `tokens.css`, `pack` uses the
  shared `run_cargo` and worker template (cache prefix `athena-`); delete `web/sw.js`.
- Accept: Athena's bundle test calls `lint`; no `caches.match(` in the bundle; gate. Leader: open `/titan/`,
  then `/athena/`, stop the server, both still load.

**T102 athena: motion.** 120 min. Needs: T96, T101. May touch: `crates/athena/`.
- Build: panels, tabs and the palette on `Motion` with `firm`; the Run line and the sliding current-line bar
  (section 4.5); nothing else moves.
- Accept: `lint` passes; Athena's self-test entries print `PASS`. Leader: trace under 4x throttle.

**T103 site: page and launcher motion.** 150 min. Needs: T96, T99. May touch: `crates/site/`.
- Build: `tests/motion_lint.rs`; the site's Core sample hero, section reveals with stagger, the launcher
  arrival, cross-page icon hand-off; site and kit CSS use only tokens.
- Accept: `motion_lint` passes on the packed bundle; self-test `PASS`. Leader: scroll trace under 4x throttle,
  reduced motion shows the end state, 360 px.

**T104 site: app shell motion and the signature slot.** 150 min. Needs: T103. May touch: `crates/site/`.
- Build: `demo-app.js` on `Motion` (run press, output enter, history, tabs, dialogs); a run starts in the same
  frame as the press and is timed; `Demo` gains `viz: Option<fn(&str) -> Result<String, String>>` (all `None`)
  with a wasm export `site_viz` returning `name value` lines; `pack` ships `apps/<id>/sig.js` when it exists;
  the kit loads it and registers it with `Motion.hero` and the self-test.
- Accept: `tests/demos.rs` checks that every `viz` output parses; self-test prints `PASS run-time <id>` for
  every example; gate.

### Wave 3: signature moments (after T104)
Same shape for all seven. 150 min each. May touch: `crates/site/apps/<id>/` and `crates/site/src/demos/<id>.rs`
for the listed apps only. Build: the `viz` function and `sig.js` for each app as described in section 4.5.
Accept: `motion_lint` passes, `tests/demos.rs` covers each new `viz`, each `sig.js` is under 6 KB, the
self-test prints `PASS sig <id>` with frame times inside the budget, reduced motion shows the end picture.
Leader: Chrome trace of each moment under 4x throttle.

| Task | Apps | Extra needs |
|---|---|---|
| T105 | titan, silicon, lithium | T98 |
| T106 | kairos, midas (new app), chisel | T94 |
| T107 | hades, iridium, chronos | none |
| T108 | aurum, argo, sphinx, pandora | none |
| T109 | logos, cobalt, elysium, krypton | T74a2 |
| T110 | mercury, xenon, odyssey, helios | none |
| T111 | indium, iris, calliope | T90 |

### Wave 4: features (after T104)
**T112 site: Descent.** 150 min. Needs: T98, T105. May touch: `crates/site/apps/titan/`,
`crates/site/tests/descent.rs`.
- Build: a "Descent" panel in the Titan app: five stacked layers with counters that update from `titan_probe`
  after each command, the Krypton parse of the typed line on top, a pulse that travels down and back up.
- Accept: test: the probe lines parse and the counters grow after `ls`. Leader: `echo hi` and `ls` show
  different block counts; reduced motion shows the numbers without the pulse.

**T113 site: Rewind.** 120 min. Needs: T97, T112. May touch: `crates/site/apps/titan/`.
- Build: a scrubber under the terminal (horizontal, keyboard reachable, labelled with the instruction count)
  that calls `titan_seek` and redraws the screen; "back to now".
- Accept: Leader: run three commands, scrub before the second, type a different one, the transcript is right;
  memory stays under the 48 MiB cap in the Chrome task manager.

**T114 site: Pull the plug.** 120 min. Needs: T107. May touch: `crates/site/apps/{hades,iridium}/`,
`crates/site/src/demos/{hades,iridium}.rs`.
- Build: a "power fails at write N" slider; the demo runs the script on `CrashDisk` or `CrashStorage`, cuts at
  N, remounts and reports what survived and the check result; the block map plays journal, cut, recovery.
- Accept: test: for every N of the default script the remount succeeds and the check passes. Leader: three
  values of N in each app.

**T115 site: Zeus app and the break-out.** 150 min. Needs: T93, T104. May touch: `crates/site/apps/zeus/`,
`crates/site/src/demos/zeus.rs`, `crates/site/src/pack.rs` (ship the Zeus image).
- Build: the Zeus app (two consoles, hypervisor log), the "read the neighbour's memory" button, the Two worlds
  signature; the launcher card stops saying "coming soon".
- Accept: `tests/apps_bundle.rs` counts the new app; test: the demo reports the fault and Lithium's output is
  intact. Leader: press the button twice, the second guest never stops.

**T116 site: Race.** 120 min. Needs: T74a2, T109. May touch: `crates/site/apps/cobalt/`,
`crates/site/src/demos/race.rs`.
- Build: a Race tab: one Logos source, two lanes (Elysium, Silicon) with outputs, instruction counts filling
  along the time axis, and "outputs match" or the first differing line.
- Accept: test: the three example programs match on both back ends; a program with a runtime error reports
  the same error on both. Leader: run all three.

**T117 site: Wire.** 150 min. Needs: T110. May touch: `crates/site/apps/odyssey/`, `crates/site/src/demos/wire.rs`.
- Build: an in-wasm Mercury serving two small pages, a Xenon link with a loss slider, Odyssey rendering what
  arrives; the event list drives the Packets in flight drawing.
- Accept: test: at 0, 10 and 30 percent loss with a fixed seed the page bytes are identical and the resend
  count rises. Leader: move the slider, the page still renders.

**T118 site: Proof links.** 90 min. Needs: T104. May touch: `crates/site/apps/_kit/`, `crates/site/src/wasm.rs`.
- Build: `Kit.share()`: input and output digest in the URL fragment (input up to 4 KiB, else Export); on load
  the app runs the input and shows "same result" or "different"; wasm export `site_digest`.
- Accept: test: the digest equals `sphinx::sha256` of the output. Leader: copy a link from one app into a new
  window and into a second browser.

## 6. Leader tasks (not for workers)
| Id | Task | When |
|---|---|---|
| L1 | `docs/REVIEW.md` D1: scrub local paths, decide on old history, add `README.md`, fix bar 10 in `LOOP.md` | before the next push |
| L2 | Edit `docs/SPEC.md` rule 1 and `docs/PLAN.md` section 8: no animation libraries | before T96 |
| L3 | Rewrite the two stale SPEC columns; remove `machine/` and `web/`; delete stale `target/w-*` folders | any time |
| L4 | Write briefs T90 to T94 from section 5 and dispatch them; they do not collide with running work | today |
| L5 | Replace the motion and token part of `docs/FRONT.md` with a pointer to sections 4.2 to 4.7 here | with T96 |
| L6 | Add `clippy::expect_used` and `clippy::panic` to the workspace lints, one crate at a time | after T119, T120 |
| L7 | Decide A3 (service binaries), A6 (tool conformance test), A7 (wasm profile) of the review | before T83 |
| L8 | Fold the checks of section 4.8 into T76, and make T76 the last task before T83 | after wave 4 |

## 7. Size of the work
- 31 worker tasks, T90 to T120: 3,930 minutes, about 65 hours of worker time.
- By wave: wave 0 is 510 minutes, wave 1 is 660, wave 2 is 810, wave 3 is 1,050, wave 4 is 900.
- If time runs short, stop after any wave: each one leaves the bundle shippable. Waves 0 to 2 remove the
  duplication, fix the service workers and give every page the motion language. Wave 3 is the polish the
  operator asked for. Wave 4 is the part nobody else can show.
- Smaller option, if wanted: skip T92, T97 and T113 (Rewind) and T117 (Wire); that saves 510 minutes and keeps
  Descent, Pull the plug, the break-out, Race and Proof links.

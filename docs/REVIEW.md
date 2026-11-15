# Monolith review: the code against its requirements

Written by the architect on 2026-11-16. Scope: the whole workspace (32 crates, about 208,000 lines of Rust
counted with `find crates -name "*.rs" | xargs cat | wc -l`, 1,816 `#[test]` functions) against `docs/SPEC.md`,
`docs/APPS.md`, `docs/FRONT.md` and the quality bar in `LOOP.md`. The follow-up work is in `docs/PLAN-NEXT.md`.

## Verdict in five lines
1. The systems are real and well proven. The proof method (official vectors, crash injection at every write,
   independent models, a leader check per task) is the best part of the project and should not change.
2. The complexity that is not needed sits almost entirely in the browser front end and in the packaging, not in
   the 25 cores: the same loader, terminal and tokens were written again for each page.
3. The worst defect is in the service workers: opening Athena deletes the offline cache of every other app, and
   apps can be served another app's old `site.wasm`.
4. The repository is public by the operator's decision (2026-11-15), but the quality bar still says private,
   and the tree tracks process files and local user paths. That needs a clean-up before the next push.
5. Rule 4 ("untrusted input never panics") is enforced by review only. The gate lints `unwrap` but not
   `expect`, `panic!` or `assert!`, and a few library functions still panic on caller data.

## How this was checked
Sampled reading with `grep`, `sed -n` and `head`, not a full read of every crate. Each item below names the
command that shows it. Nothing was run in a browser for this review, so the service worker findings are marked
"found by reading, not reproduced in Chrome". Line numbers are from the working tree on 2026-11-16; `crates/site`
and `crates/cobalt` had uncommitted worker edits at the time, so their numbers can drift by a few lines.

## A. Complexity that is not needed (ranked by what it costs every future task)

### A1. One web kit instead of four copies (highest value)
- Today: the wasm loader exists in 4 files (`crates/neon/web/neon.js:141`, `crates/site/web/site.js:56`,
  `crates/athena/web/athena.js:861`, `crates/site/apps/_kit/kit.js:82`). The Titan terminal pump (`appendTerm`,
  `pumpFrame`, `startPump`) exists in 4 files (`neon.js:83`, `site.js:290`, `crates/site/apps/titan/app.js:149`,
  `athena.js:818`). The design tokens are defined in 4 CSS files (`crates/site/web/style.css`,
  `crates/site/apps/_kit/kit.css`, `crates/athena/web/style.css`, `crates/neon/web/style.css`).
- Cut or merge: one kit in `crates/neon/web/kit/` (`tokens.css`, `wasm.js`, `term.js`, later `motion.js`),
  exposed to both packers through a small `neon::webkit` module. `neon` is the only crate both `site` and
  `athena` already depend on, and the workspace member list is explicit, so a new crate would need a root edit.
- Why it matters now: the motion system would otherwise be written four times as well.
- Done when each of these prints `1` (each prints `4` today):
  `grep -rl "instantiateStreaming" crates/*/web crates/site/apps | wc -l`,
  `grep -rl "function pumpFrame" crates/*/web crates/site/apps | wc -l`,
  `grep -l -- "--accent:" crates/*/web/*.css crates/*/web/kit/*.css crates/site/apps/_kit/*.css | wc -l`.
- Smaller option: dedupe inside `crates/site` only and leave Athena its own copy (2 copies instead of 4).

### A2. One pack pipeline and one service worker template
- Today: three packers, each with its own `run_cargo` (`crates/site/src/pack.rs:464`, `crates/athena/src/pack.rs`,
  `crates/neon/src/pack.rs`). App service workers are generated (`crates/site/src/pack.rs:348`); Athena has a
  hand-written one (`crates/athena/web/sw.js`) that behaves differently (see B1).
- Cut or merge: move `run_cargo`, the content hash and the service worker template into `neon::webkit`; Athena
  uses the generated worker. Delete the standalone neon page (`crates/neon/web/index.html`, `neon.js`,
  `style.css`, `crates/neon/examples/pack.rs`, and the page half of `crates/neon/src/pack.rs`): `apps/titan`,
  `apps/logos` and Athena replaced it, and the site bundle does not ship it.
- Done when `grep -rl "fn run_cargo" crates/*/src | wc -l` prints `1` (prints `3` today)
  and `ls crates/neon/web/neon.js` fails.

### A3. Two binaries for the same service
- Today: `crates/deploy/src/bin/{iridium-server,aurum-node,argo-registry,monolith-web}.rs` next to the tools'
  own `argo serve` (`crates/argo/src/bin/argo.rs:538`), `mercury serve` (`crates/mercury/src/bin/mercury.rs:101`)
  and `aurum node`; `crates/iridium/src/bin/` holds both `iridium-cli.rs` and `iridium.rs` (which has `connect`).
- Cut or merge: container images run the tools; delete the four deploy binaries and `iridium-cli.rs`. Iridium
  needs a `serve` subcommand first. `docs/APPS.md` currently says to keep some of these, so this is a leader
  decision (D3), not a bug.
- Done when `ls crates/deploy/src/bin` shows only `healthcheck.rs` and `bash tools/check.sh deploy` passes.

### A4. Process files inside the product repository
- Today, tracked in git: 229 files under `briefs/`, 148 under `reports/`, 19 under `comms/`, plus `LOOP.md`
  (402 lines), `AGENTS.md` and `GEMINI.md` (`git ls-files | awk -F/ '{print $1}' | sort | uniq -c`).
  There is no root `README.md` (`ls README.md` fails); `docs/BOOK.md` is the real entry point.
- Cut or merge: keep the loop files in a private place or one archive folder, and give the repository a short
  `README.md` that points to `docs/BOOK.md`, the site and `tools/check.sh`. See B6 for the privacy side.
- Done when `ls README.md` succeeds and the leader has ruled on D1.

### A5. Names left over from the rename of 2026-11-15
- `docs/SPEC.md:36-62`: the "why the name" column still explains the old geology names with the new words
  pasted in ("A iridium is one year's layer of sediment", "Hades splits into flat pages"), and the "Where"
  column says `machine/titan`, `machine/lithium`, `machine/zeus`, `web/athena` while the code is in `crates/`.
- Root `machine/` and `web/` are empty directories.
- Old names in code: `CAIRNPKG1` and `CAIRNSIG1` (package format magic, `crates/argo/src/pkg.rs` and
  `crates/argo/src/bin/argo.rs`), `varve_err_to_io` (`crates/iridium/src/net/server.rs`), "Hello from quartz"
  (`crates/silicon/tests/machine_leader.rs:28`), "Strata" as the layer diagram title
  (`crates/site/web/index.html:38`) and "Strata Render" (`crates/site/apps/odyssey/app.js:39`).
- 57 build directories `target/w-*`, about half of them for crate names that no longer exist.
- Done when this prints nothing:
  `grep -rniE "cairn|varve|quartz|spelunk|drillcore|strata" crates docs/SPEC.md --include=*.rs --include=*.js --include=*.html --include=*.md`
  (allow the word in `tools/rename-map.txt` only). No package has been published, so renaming the magic is free
  today and a format break later.

### A6. Hand-written argument parsing in 25 tools
- Today: 33 files under `crates/*/src/bin/`, 10,775 lines in total. `docs/APPS.md` forbids a shared CLI crate.
  The cost showed up as drift (the `docs/TOOLS.md` pass found tools without `--help` or with other exit codes).
- Keep the rule, add one check instead of an abstraction: a single conformance test that runs every tool with
  `--help`, `--version` and an unknown subcommand and asserts exit codes 0, 0 and 2.
- Done when that test exists and passes in `bash tools/check.sh`.

### A7. Release profile shared by native and wasm
- Today: `opt-level = 3`, thin LTO, `panic = "abort"` for everything (`Cargo.toml:44-47`). `site.wasm` is
  2,039,026 bytes and `athena.wasm` 673,895 bytes (`ls -l target/site-web/site.wasm target/athena-web/athena.wasm`).
- Try a `[profile.wasm]` that inherits release with `opt-level = "s"`; keep it only if Titan boot time in the
  browser gets no worse than 15 percent. Measure first: this is a guess until the numbers exist.

### A8. Small stale items
- `LOOP.md:39` still lists "indium spring stall at low stiffness" as open; it is fixed and covered by
  `spring_low_stiffness_reaches_target` (`crates/indium/tests/soak.rs:579`).
- `docs/FRONT.md` repeats the token table that lives in CSS; after A1 it should point to `tokens.css`.

## B. Defects and risks

### B1. Opening Athena deletes every other app's offline cache (high; found by reading, not reproduced in Chrome)
- Where: `crates/athena/web/sw.js:13-26`. On `activate` it deletes every cache whose name is not its own
  (`keys.filter((key) => key !== CACHE_NAME)`). Cache storage is shared by the whole origin, and the apps keep
  theirs under `monolith-<id>-<hash>`.
- Failure: a visitor installs Titan, later opens `/athena/` once. Athena's worker activates and removes the Titan
  cache. Titan's worker is still registered and does not reinstall (its `sw.js` bytes did not change), and its
  fetch handler only reads from caches. From then on Titan does not start offline.
- Check after the fix: `grep -n 'startsWith' crates/athena/web/sw.js` (or the generated worker) shows a prefix
  filter; in Chrome: open `/titan/`, open `/athena/`, stop the server, reload `/titan/`, it still boots.

### B2. Apps can be served another app's old wasm, and each app stores its own 2 MB copy (high; found by reading)
- Where: `crates/site/src/pack.rs:362` adds `../site.wasm` to every app's cache; `pack.rs:435` and `:440` answer
  with the global `caches.match(req)`, which searches every cache of the origin in creation order.
  `crates/athena/web/sw.js:38` does the same.
- Failure: after a release, app A updates and gets a new `kit.js`. App B was not opened, so its older cache
  still holds the previous `site.wasm` and was created earlier. A's request for `../site.wasm` can be answered
  from B's cache: new JavaScript against an old wasm ABI. Separately, 17 or more apps each keep a 2 MB copy.
- Fix: one shared core cache named by the hash of `site.wasm`, `titan.elf` and the kit; a per-app cache for the
  app's own files; look up in those two opened caches only; an ABI number exported by the wasm that the kit
  compares, with one reload on mismatch.
- Check: `grep -c "caches.match(" crates/site/src/pack.rs crates/athena/web/sw.js` gives 0 for both (4 hits in
  total today); a bundle test asserts no generated `sw.js` contains the string `caches.match(`.

### B3. Library functions that panic on caller data (medium; reach from the tools not shown)
- `crates/argo/src/pkg.rs:69` and `:74` (`assert!` on an invalid or duplicate path), `:82`, `:83`, `:93`, `:94`
  (`expect` on size conversions) inside the public `encode`, which `sign` (`:196`) calls.
- `crates/chisel/src/elf.rs:81`, `:85` (`assert!`), `:89` (`expect`) inside the public `write`, called by
  `crates/cobalt/src/rv64/mod.rs:67`.
- Failure: a caller hands over a `Package` with a path like `../x`, or an ELF `Image` whose `mem_size` is smaller
  than its data. With `panic = "abort"` the process (or the wasm instance) dies instead of returning the
  crate's error. This breaks rule 4 of `briefs/_RULES.md` and SPEC rule 4.
- Reach: `grep -n "sign(\|encode(" crates/argo/src/bin/argo.rs` finds no call, and the chisel tool does not call
  `elf::write` directly, so no tool input was shown to reach these lines. `argo.rs` handles `CAIRNSIG1` in two
  places of its own, which may be a second encoder worth a look.
- Check after the fix: `grep -nE "assert!|expect\(" crates/argo/src/pkg.rs crates/chisel/src/elf.rs` shows hits
  only below the `#[cfg(test)]` line, plus a test that feeds each bad value and gets `Err`.

### B4. The gate does not enforce the no-panic rule (medium)
- Where: the workspace lints are `missing_docs` and `clippy::unwrap_used` only (`Cargo.toml:39-42`).
- Failure: B3 passed the gate. Any worker can add `expect`, `panic!` or `unreachable!` to library code and
  `tools/check.sh` stays green.
- Check: `cargo clippy --workspace --lib -- -W clippy::expect_used -W clippy::panic 2>&1 | grep -c "^warning"`
  gives the size of the job; the fix is done when those two lints are in `Cargo.toml` and the gate passes.
  `indexing_slicing` is too noisy for this code; the fuzz soaks cover it better.

### B5. Demos run to completion on the main thread (medium; not measured)
- Where: `crates/site/src/wasm.rs:113` calls `(demo.run)(input)` in one go; `crates/site/apps/_kit/kit.js:143`
  calls it synchronously from the click handler. Input may be up to 64 KiB (`demos/mod.rs:26`).
- Failure: a large input to a slow demo freezes the tab, which breaks the `docs/FRONT.md` rule "long work never
  blocks input" and will show as dropped frames once motion exists. If a demo panics, the wasm traps; the kit
  keeps using the same instance (`kit.js:174-184`), so memory held at the trap is never freed.
- Check: the self-test page planned in `docs/PLAN-NEXT.md` times every example of every app and prints
  `PASS run-time <id>` when a run is under 50 ms or is sliced.

### B6. Public repository hygiene against quality bar 10 (high; mostly a clean-up, not a code bug)
- `gh repo view --json visibility` prints `PUBLIC`, and `AGENTS.md:32` records that as the operator's decision
  of 2026-11-15. `LOOP.md:59` still says "pushes go only to the private repo": the two files disagree.
- The operator's first name was removed from `AGENTS.md` and `docs/PLAN.md` on 2026-11-16 (commit `148059b`).
  Still tracked: local user paths in `docs/PLAN.md` (3 lines), in 21 files under `briefs/` and 2 under
  `reports/`, and model lane names with usage notes (`LOOP.md`, `docs/PLAN.md`). No key or token was found in
  the sampled files; that is not a full secret scan.
- Failure: anyone can read the old commits. Removing a line today does not remove it from history; only a
  history rewrite or a fresh repository does.
- Check: `git grep -cE "Users[\\\\/]|/mnt/c/" -- . ":!crates" ":!docs/REVIEW.md"` prints nothing, and
  `grep -n "private repo" LOOP.md` prints nothing.

### B7. The zero-dependency rule has a written exception the operator has now withdrawn (low)
- `docs/SPEC.md:11` ("The web front end may load animation libraries") and `docs/PLAN.md:219`.
- Check: `grep -n -i "animation libraries" docs/SPEC.md docs/PLAN.md` prints nothing.

### B8. Latent: an app's optional `app.css` would not be cached (low)
- `docs/APPS.md` allows `app.css`; the asset list in `crates/site/src/pack.rs:349-365` does not include it. No
  app ships one today (`ls crates/site/apps/*/app.css` fails), so nothing is broken yet.

## C. What is solid (sampled)
| Claim | Evidence |
|---|---|
| Zero outside code | `grep -c '^source = ' Cargo.lock` prints 0; the gate fails otherwise (`tools/check.sh`) |
| CPU is correct | 110 of 110 official riscv-tests (`LOOP.md`, T13 rows) |
| Storage survives power loss | `CrashDisk` (`crates/hades/src/disk.rs:78`) and `CrashStorage` (`crates/iridium/src/crash.rs:12`) fail at every write in the leader checks |
| Guests are isolated | `isolation_spy_next_to_lithium` (`crates/zeus/tests/guests.rs:234`) |
| The emulator is deterministic | the timer advances one tick per step (`crates/silicon/src/machine/clint.rs`), `Machine` is `Clone` |
| Security review found real bugs | Ed25519 small-order keys, forged transaction signatures, display size overflow (`LOOP.md`, T80 rows) |
| Memory safety by default | 30 of 32 crates have `forbid(unsafe_code)` in `lib.rs` |
| The browser bundle is self-contained | no `http://` or `https://` in the bundle (`crates/site/tests/apps_bundle.rs`), only local fetches |
| Motion is already disciplined | no `@keyframes`, transitions only on `opacity` and `transform`, a reduced-motion block in each page CSS |
| Independent leader checks | LLVM corpus for chisel, Python vectors for medusa, shadow models, fresh seeds (`LOOP.md` log) |

The architecture choice behind all of this is right: every system has a portable core, so the same code runs
natively, on our CPU and in a browser tab. The plan in `docs/PLAN-NEXT.md` builds on exactly that.

## D. Decisions for the leader (not worker tasks)
| Id | Decision | Suggested answer |
|---|---|---|
| D1 | Public repository clean-up (B6, A4) | Scrub local paths, decide on old history, move loop files out, add `README.md`, fix bar 10 |
| D2 | Zero dependencies for JavaScript too (B7) | Yes: edit `docs/SPEC.md` rule 1 and `docs/PLAN.md` section 8 |
| D3 | Duplicate service binaries (A3) | Images run the tools; delete the copies after iridium has `serve` |
| D4 | Rename `CAIRNPKG1` / `CAIRNSIG1` now (A5) | Yes, before any package is published |
| D5 | Add `expect_used` and `panic` lints (B4) | Yes, after the B3 fixes, one crate at a time |
| D6 | Rewrite the two SPEC table columns, remove `machine/` and `web/` (A5) | Yes, 20 minutes of leader time |
| D7 | Wasm size profile (A7) | Measure, then decide |

## E. Not checked in this review
- No browser run, no performance trace, no full secret scan, no read of the 25 cores beyond samples.
- The reach of B3 from real tool input, and the run time of each demo (B5), are open until measured.
- `crates/site` and `crates/cobalt` were being edited by running tasks (T75c, T74a2, T74b); findings there
  should be confirmed again after those land.

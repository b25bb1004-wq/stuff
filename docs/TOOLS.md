# Monolith command-line tools

Each of the 25 systems has a command-line tool named after it (zeus's tool is `zeusctl`).
The tools are std-only Rust, and they take files as paths.
Build one with `cargo build --release -p <crate> --bin <name>`, then run `target/release/<name>`.
`-` means stdin or stdout. Output is plain text, one fact per line.
Exit codes: 0 ok, 1 operation failed, 2 usage error. Errors go to stderr prefixed with the tool name.
The sections list each tool's `--help` text, shortened; the notes mark the tools that deviate from these rules.

## Table

| Tool | Crate | Purpose | Subcommands |
|---|---|---|---|
| titan | neon | Boots the Titan kernel on the Silicon emulator | `boot` |
| iridium | iridium | SQL database engine CLI | `sql`, `connect` |
| elysium | elysium | Verifies, disassembles and runs MBC1 bytecode | `verify`, `dis`, `run` |
| mercury | mercury | HTTP/1.1 parser, static file server and client | `parse`, `serve`, `get` |
| helios | helios | Deterministic game runner and input replay | `run`, `record`, `replay` |
| cobalt | cobalt | Logos IR, optimiser and Elysium bytecode compiler | `ir`, `opt`, `build` |
| logos | cobalt | Logos runner: run a program or its tests | `run`, `test` |
| odyssey | odyssey | Small HTML tokeniser, DOM viewer and PNG renderer | `tokens`, `tree`, `select`, `render` |
| aurum | aurum | Chain, Merkle and transfer CLI | `keygen`, `tx`, `merkle`, `verify`, `demo` |
| sphinx | sphinx | Crypto lab: hashes, HMAC, Ed25519 signatures, sealed files | `hash`, `hmac`, `keygen`, `sign`, `verify`, `seal`, `open` |
| silicon | chisel | RISC-V machine: load and run an RV64 ELF | `run` |
| hades | hades | Filesystem image tool | `mkfs`, `ls`, `cat`, `put`, `rm`, `mkdir`, `fsck` |
| pandora | pandora | Linux container runtime (`run` is Linux only) | `pull`, `run`, `ps`, `kill`, `rm` |
| argo | argo | Package solver, lock file, installer and HTTP index server | `solve`, `lock`, `install`, `serve` |
| krypton | krypton | POSIX shell for Monolith | none (`-c`, script file or interactive) |
| iris | iris | Desktop compositor: replay events to a PNG frame | `replay` |
| indium | indium | Widget layout renderer and focus inspector | `render` |
| calliope | calliope | Text tools: regex search and scripted edits | `grep`, `edit` |
| athena | athena | Logos checker, formatter and test runner | `check`, `fmt`, `test` |
| chronos | chronos | Git-like repository tool | `init`, `add`, `commit`, `log`, `branch`, `checkout`, `cat-file`, `status` |
| xenon | xenon | UDP echo and encrypted file transfer | `keygen`, `echo-server`, `echo`, `recv`, `send` |
| lithium | lithium | Boots the Lithium RV64 OS image, or exports its binary | `run`, `image` |
| kairos | kairos | Scheduler simulator: per-CPU timeline and CPU share | `sim` |
| midas | midas | Allocator simulator: heap usage and invariant checks | `sim` |
| zeusctl | zeus | Boots guests under the zeus hypervisor on Silicon | `run` |

---

## titan

Build: `cargo build --release -p neon --bin titan`

- `boot [--kernel PATH] [--script FILE]`: boots the Titan kernel on Silicon. Interactive by default (stdin lines go to the shell). With `--script FILE` (or `-`), it feeds the lines and exits.
- `--version`, `--help`: as usual.

Example: `titan boot --script commands.txt`

## iridium

Build: `cargo build --release -p iridium --bin iridium`

- `sql DB [QUERY]`: open or create DB and run QUERY. With no QUERY or `-`, statements come from stdin.
- `connect ADDR`: connect to an iridium server at host:port and run statements from stdin.

Example: `iridium sql app.idb "SELECT 1 + 2 AS sum"`

## elysium

Build: `cargo build --release -p elysium --bin elysium`

- `verify FILE.mbc`: verify an MBC1 bytecode module (`-` for stdin).
- `dis FILE.mbc`: disassemble an MBC1 module.
- `run FILE.mbc [--fuel N]`: run the module's entry function.

Example: `elysium run prog.mbc --fuel 10000`

## mercury

Build: `cargo build --release -p mercury --bin mercury`

- `parse FILE`: parse a raw HTTP/1.x request or response (`-` for stdin).
- `serve DIR [--listen 127.0.0.1:8080]`: serve static files from DIR until Ctrl+C.
- `get http://HOST[:PORT]/PATH`: fetch a plain HTTP URL.

Example: `mercury serve ./public --listen 127.0.0.1:8080`

## helios

Build: `cargo build --release -p helios --bin helios`

- `run GAME.logos [--ticks N] [--seed S]`: run a Logos game headless, printing the world hash every 60 ticks and at the end. Defaults are 300 ticks and seed 0.
- `record GAME.logos INPUTS -o REPLAY [--ticks N] [--seed S]`: record text key-state inputs (`<frame> <keys>` lines) into a TREC replay file and print the final hash.
- `replay GAME.logos REPLAY [--ticks N] [--seed S]`: replay a TREC file and print the final hash.

Example: `helios run game.logos --ticks 180 --seed 42`

## cobalt

Build: `cargo build --release -p cobalt --bin cobalt`

- `ir FILE.logos`: print typed Logos IR.
- `opt FILE.logos`: optimise the IR and print it with stats.
- `build FILE.logos -o OUT.mbc`: compile and verify an Elysium bytecode module (`-` as OUT.mbc writes stdout).

Example: `cobalt build hello.logos -o hello.mbc`

## logos

Build: `cargo build --release -p cobalt --bin logos` (the `logos` binary is defined in the cobalt crate)

- `run FILE [--fuel N]`: compile and run a Logos program. Input for `io.read_line` is read from stdin.
- `test FILE [--seed N] [--cases N]`: run the program's tests.

Example: `logos test game.logos --cases 200`

Notes: the built binary has no `--help` or `--version`; an unknown subcommand prints `unknown subcommand` and exits 2. APPS.md lists `check` and `fmt` for logos, but the source at `crates/cobalt/src/main.rs` implements only `run` and `test`; those two are what is described here.

## odyssey

Build: `cargo build --release -p odyssey --bin odyssey`

- `tokens FILE.html`: print HTML tokens, one per line.
- `tree FILE.html`: print the indented DOM tree.
- `select SEL FILE.html`: print matching elements with their path.
- `render FILE.html [--width N] --out PNG`: render the page to a PNG (default width 800). `--out -` writes PNG bytes to stdout.

Example: `odyssey render page.html --width 800 --out page.png`

## aurum

Build: `cargo build --release -p aurum --bin aurum`

- `keygen -o KEYFILE`: generate an ed25519 keyfile (hex seed) and print its address.
- `tx KEYFILE TO AMOUNT NONCE -o TX`: sign a transfer and write it as a hex file.
- `merkle [--proof N] FILE...`: compute the RFC 6962 SHA-256 Merkle root, with an optional proof for leaf N.
- `verify CHAIN`: validate every block of CHAIN.
- `demo -o CHAIN`: build a deterministic 3-block demo chain.

Example: `aurum verify demo.chain`

Notes: the `node` subcommand that APPS.md lists for aurum is not in this binary's help; the node is built separately as `aurum-node` in the deploy crate.

## sphinx

Build: `cargo build --release -p sphinx --bin sphinx`

- `hash ALG FILE`: hash FILE (`-` for stdin) with sha256 or sha512.
- `hmac KEYHEX FILE`: HMAC-SHA-256 of FILE with a hex key.
- `keygen -o KEYFILE`: generate a 32-byte Ed25519 seed file (mode 600 on Unix).
- `sign KEYFILE FILE`: sign FILE with a seed file.
- `verify PUBHEX SIGHEX FILE`: verify a signature (exit 0 ok, 1 failed).
- `seal KEYFILE FILE [-o OUT]`: encrypt FILE with a 32-byte key file (ChaCha20-Poly1305).
- `open KEYFILE FILE [-o OUT]`: decrypt FILE with the same key file.

Example: `sphinx sign id.key msg.txt`

## silicon

Build: `cargo build --release -p chisel --bin silicon` (the silicon binary is defined in the chisel crate)

- `run ELF [--steps N] [--ram MiB] [--trace]`: load an RV64 ELF and run it on Silicon. `-` reads the ELF from stdin.

Example: `silicon run program.elf --steps 50000 --ram 32 --trace`

## hades

Build: `cargo build --release -p hades --bin hades`

- `mkfs IMG [--blocks N]`: create an empty image (default 1024 blocks).
- `ls IMG PATH`: list directory entries at PATH.
- `cat IMG PATH`: write a file's contents to stdout.
- `put IMG SRC PATH`: copy host file SRC (or `-` for stdin) to PATH.
- `rm IMG PATH`: remove the file at PATH.
- `mkdir IMG PATH`: create a directory at PATH.
- `fsck IMG`: check filesystem consistency.

Example: `hades put disk.img hello.txt /notes/hello.txt`

## pandora

Build: `cargo build --release -p pandora --bin pandora`. Linux only for `run`: off Linux, `run` returns Unsupported (the spec and cgroup code build everywhere).

- `pull NAME[@REQ] [--registry ADDR] [--trust HEX]`: fetch an image, with an optional registry address and trusted key.
- `run [--rootfs DIR | --image IMG] [--name NAME] [--hostname H] [--memory M] [--pids N] [--cpu C] [--env K=V]... [--cgroup-base DIR] -- CMD...`: start a command in new namespaces. `--memory`, `--pids` and `--cpu` create a cgroup v2 group; `--cgroup-base` names the delegated subtree.
- `ps`: print a table with `ID IMAGE STATUS` columns.
- `kill ID [SIGNAL]`: send a signal to a container.
- `rm ID`: remove a container.
- Global: `--store DIR` (default `~/.pandora`) comes before the subcommand.

Example: `pandora run --rootfs /srv/box --hostname box --env PATH=/bin --pids 64 -- /bin/sh -c 'echo $$'`

Notes: there is no `--help` or `--version` flag in the source; a bad call prints the usage line. Exit codes follow the container: the container's own code on `run`, 128 plus the signal if it was killed, 125 when pandora itself fails, 126 when namespace setup fails, 127 when the command cannot run. So pandora does not use the 1 and 2 convention.

## argo

Build: `cargo build --release -p argo --bin argo`

- `solve INDEX MANIFEST`: resolve dependencies and print `name version` lines.
- `lock INDEX MANIFEST -o LOCK`: resolve dependencies and write a lock file.
- `install LOCK STORE_DIR --from DIR [--key HEX_OR_FILE]`: install locked packages into STORE_DIR from a local package directory.
- `serve INDEX_DIR [--listen ADDR] [--key HEX_OR_FILE]`: serve signed packages over HTTP (default `127.0.0.1:7500`).

Example: `argo lock index.txt Cargo.argo -o argo.lock`

## krypton

Build: `cargo build --release -p krypton --bin krypton` (the `std` feature is on by default)

- `krypton -c SCRIPT [ARG0 [ARGS...]]`: run SCRIPT with optional `$0` and positional arguments.
- `krypton FILE [ARGS...]`: run a script file with positional arguments.
- `krypton`: interactive shell, reading stdin.
- `krypton --help`: show help.
- `krypton --version`: print `krypton 0.1.0`. Bad options exit 2; a missing script file exits 1 with the `krypton: ` prefix; a script's own exit status passes through (T86t).

Example: `krypton -c 'echo hello'`

Notes: there is no `--version`. Terminal job control (Ctrl-Z, `fg`, `bg`) is out of scope.

## iris

Build: `cargo build --release -p iris --bin iris`

- `replay EVENTS --out FRAME.png [--width W --height H]`: replay desktop events, write the final PNG frame, and print windows and damage. `--out -` writes the PNG to stdout (stats go to stderr). Width and height are 1 to 8192, default 800 by 600.

The events file has one command per line: `open terminal|editor|files`, `close`, `move`, `focus`, `tile`, `float`, `key NAME`, `text STRING`, `wait MS`. `#` lines and blank lines are ignored, and `-` reads stdin.

Example: `iris replay events.txt --out frame.png --width 800 --height 600`

## indium

Build: `cargo build --release -p indium --bin indium`

- `render LAYOUT --out UI.png [--width W] [--height H] [--focus N]`: parse an indented layout file, print the focus order, and write a PNG. `-` as LAYOUT reads stdin; `-` as `--out` writes PNG bytes to stdout. `--focus N` draws the focus ring on the N-th focusable widget (0-based).

The layout file has one widget per line, indented by spaces: `column`, `row`, `text "..."`, `button "..."`, and `list N`. Blank lines and `#` lines are ignored.

Example: `indium render layout.txt --out ui.png --width 480 --height 320 --focus 0`

## calliope

Build: `cargo build --release -p calliope --bin calliope`

- `grep REGEX FILE...`: print `file:line:col: text` for each match. Exit 0 on a match, 1 on no match or I/O error, 2 on a bad regex or usage.
- `edit SCRIPT FILE`: apply the script's edit lines to FILE and print the resulting buffer. Script commands: `goto LINE COL`, `insert TEXT`, `delete N`, `undo`, `redo`, `cursor add LINE COL`, `save`.

Example: `calliope grep "fn [a-z]+" src/main.rs`

## athena

Build: `cargo build --release -p athena --bin athena`

- `check FILE.logos`: parse and type-check Logos source (`-` for stdin).
- `fmt FILE.logos [--write]`: format Logos source. It refuses files that contain comments.
- `test FILE.logos`: run unit tests and property tests.

Example: `athena fmt main.logos --write`

## chronos

Build: `cargo build --release -p chronos --bin chronos`. The usage line also shows a global `-C DIR` option.

- `init [DIR]`: create a repository under DIR/.chronos/ (default `.`).
- `add PATH...`: stage files into the index.
- `commit -m MSG`: record a commit from the staged index.
- `log`: show commits reachable from HEAD, newest first.
- `branch [NAME]`: list branches, or create one at HEAD.
- `checkout NAME`: switch to a branch or revision and update working files.
- `cat-file ID`: print an object (blob, tree or commit).
- `status`: show staged changes relative to HEAD.

Example: `chronos commit -m "initial commit"`

## xenon

Build: `cargo build --release -p xenon --bin xenon`

- `keygen`: print a `secret:` and a `public:` hex line (x25519 keys).
- `echo-server ADDR SECRET_HEX`: run a UDP echo server on ADDR.
- `echo ADDR PUBKEY_HEX TEXT`: send TEXT to an echo server, which replies.
- `recv ADDR SECRET_HEX OUT_FILE`: receive a file and write it to OUT_FILE.
- `send ADDR PUBKEY_HEX FILE`: send FILE to a receiver.

Example: `xenon echo 127.0.0.1:9000 <pubkey-hex> hello`

Notes: follows the tool conventions (T86s): `--help`, `--version`, usage errors exit 2, runtime errors exit 1, `xenon: ` prefix.

## lithium

Build: `cargo build --release -p lithium --bin lithium`

- `run [--steps N]`: boot Lithium on Silicon, print console output, exit, and the step count (default 1000000 steps).
- `image -o OUT.bin`: write the assembled kernel image (`-` for stdout).

Example: `lithium run --steps 100000`

## kairos

Build: `cargo build --release -p kairos --bin kairos`

- `sim FILE`: simulate the tasks in FILE (`-` for stdin) and print a per-CPU timeline in ticks, plus each task's CPU share against its weight. FILE lines are `cpus N` and `task NAME WEIGHT WORK`.

Example: `kairos sim workload.txt`

## midas

Build: `cargo build --release -p midas --bin midas`

- `sim FILE`: run an allocator trace (`-` for stdin) with `heap BYTES`, `alloc SIZE` and `free N` lines. It prints memory usage and an invariant check for each step.

Example: `midas sim trace.txt`

## zeusctl

Build: `cargo build --release -p zeus --bin zeusctl`

- `run [--guest NAME[:MiB]]... [--zeus PATH] [--titan PATH] [--steps N]`: boot zeus with one to three guests and print the console output of each source. Guests are `lithium` (default 4 MiB) or `titan` (default 16 MiB), with an even size from 2 to 16 MiB. `--steps` caps the Silicon steps (default 20000000).

Example: `zeusctl run --guest lithium:4 --guest titan:16 --steps 80000000`

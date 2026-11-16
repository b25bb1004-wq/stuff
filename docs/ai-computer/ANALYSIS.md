# Monolith as a computer for AI: where it stands (2026-11-16)

Operator, 2026-11-16: make everything functional and dynamic, with a strong UI, features people long for,
and "compatible for AI ... all of this collectively makes an environment, a computer for AI itself".
Written by the leader from the code, the installed tools and today's simulated-user run (reports/sim/).

## Measured today
- 27 command-line tools install with one script. Four simulated users did 233 real tasks: 172 worked, 61 broke.
  Round-one and most round-two fixes are committed (iridium two-writer data loss, chronos staging, krypton
  scripts, installed athena, calliope line endings, mercury caching headers).
- Half-done, uncommitted: chronos merge/stash/tag/show, iridium REAL and AVG.
- Not tested by anyone yet: the interactive Calliope editor in a real terminal, the Athena IDE in a browser.

## What an AI agent can use today
| Product | Real today | For an agent today | Biggest gap |
|---|---|---|---|
| Krypton (shell, 14.6k lines) | scripts, pipes, real programs, aliases, traps | text in, text out | no structured output, no dry run, no arrays or `[[ ]]` |
| Chronos (version control, 10.6k) | add, commit, branch, switch, status, diff, git import/export | text only | merge and stash unfinished; no machine output |
| Calliope (editor, 9.6k) | terminal editor, `grep`, scripted `edit` | `edit` scripts are close to agent-friendly | one file per session; edits are addressed by line and column only |
| Athena (IDE, 5.5k + web) | local folder API with token, web UI | the HTTP API is close to agent-ready | no agent view, no diff or approval flow |
| Iridium (database, 16.6k) | SQL subset, WAL, server, single-writer lock | SQL is a good agent language | no REAL or AVG yet, output is text tables |
| Mercury (web server, 7.4k) | static serve, HTTP/1.1 client | `get` for plain http only | no TLS, so no https at all |
| Odyssey (browser, 12.7k) | HTML and CSS parse, layout, paint to PNG, `select` | reads local HTML files | cannot open a URL from the CLI, no TLS, no JavaScript, no forms |
| Sphinx (crypto, 5.1k) | ChaCha20-Poly1305, X25519, Ed25519, SHA-2, HKDF | library | no RSA, no P-256, no X.509: TLS cannot verify certificates yet |
| Argo, Xenon, Aurum, Pandora | package solver, UDP transport, chain, containers (Linux) | text CLIs | not wired to each other |
| Titan, Lithium, Zeus, Silicon | RISC-V OS, kernel, hypervisor in our emulator | `titan boot --script` | not on real hardware; x86-64 UEFI port not started |
| Iris, Indium, Helios, Pigment | compositor, widgets, game engine, pixels | render to PNG | no live desktop on the host |
| Hades, Midas, Kairos, Elysium, Logos, Cobalt | file system image tool, allocator, scheduler, VM, language, compiler | text CLIs | teaching tools; not part of a daily flow |

Zero of the 27 tools has `--json` or any machine-readable output. docs/APPS.md says "`--json` is not required".
That one line is the main thing standing between Monolith and an AI-native computer.

## What a computer for AI needs that nothing here provides yet
1. One agent interface for every tool: a self-describing manifest (`<tool> --describe`), JSON results with stable
   fields, errors that say what to do next, paging so output never floods a context window.
2. One door: a `monolith` program that speaks MCP (and plain JSON over stdin) and exposes every tool, so any
   agent gets the whole computer in one connection.
3. Permissions by capability, deny by default: which folders, which hosts, which programs, per agent session.
4. Undo for everything an agent does: each action snapshotted (Chronos for files, the WAL for Iridium), one command back.
5. A record: every agent action logged with its inputs, effects and cost, queryable in Iridium.
6. Memory and sessions that survive restarts.
7. An agent browser: URL in, a compact page tree with element references out, then `click`, `fill`, `submit`.
8. A human window on all of it: what the agents are doing right now, with approve, pause and rewind.

## Contradictions and risks in the brief
- "Has everything www has" needs TLS, certificate checking and a JavaScript engine. TLS 1.3 from scratch is
  weeks of careful work and stays "unaudited"; a JavaScript engine from scratch is months. Zero external crates
  makes both harder. The operator chooses.
- "Instead of waiting to pass through Cloudflare": the browser never solves or dodges bot checks. The honest
  routes are a signed agent identity (Web Bot Auth, which uses Ed25519, and Sphinx already has Ed25519),
  official APIs, feeds and sitemaps, and stopping politely at a challenge.
- "Separate products" (said about the site) against "one environment" (said now): both hold if the products
  stay separate and one thin layer joins them.
- "Release": the repo is public but nothing is tagged. Tag, GitHub release and site deploy are the operator's call.
- Bootable OS on this PC and AI-native tools are two large tracks. The order matters.

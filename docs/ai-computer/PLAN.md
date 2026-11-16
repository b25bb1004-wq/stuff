# Monolith: a computer for AI. Master plan (2026-11-16)

Read with ANALYSIS.md (where we stand), PRIOR-ART.md (who did what) and ACI.md (the tool interface).
Each phase gets its own task plan in `docs/ai-computer/plans/` when it starts; phase 1's is written.

## What the operator decided (question rounds, 2026-11-16)
| Question | Answer |
|---|---|
| Who is it for | A local AI model gets its own personal computer made of the Monolith tools |
| Where first | Tools on Windows now, then the same thing inside the Titan OS |
| "In the cloud" | Portable: one folder plus one program, this PC first, any server later |
| What a computer is | Both a fenced workspace on the host and a Titan virtual machine, one set of commands |
| Tools | All 27 tools get the AI interface. Plain commands, no MCP needed (MCP is an optional adapter) |
| Driver | Built in (`monolith run` talks to a local model server) and open to outside agents |
| Permissions | Free inside its own computer; asks before touching real files, posting or logging in |
| Browser | Own engine with simple commands (open, snapshot, click @e1, fill); installed Chrome as a stopgap for script-heavy pages until our own JavaScript engine exists |
| https | TLS 1.3 written by us on Sphinx, labelled unaudited |
| Bot checks | Signed agent identity (Web Bot Auth) plus official APIs and feeds. Never solve or dodge a check |
| Signature features | Rewind anything, fork a computer, replay and explain, lasting memory, and a learning loop |
| Look | A live cockpit window and rich terminal views |
| Release | v0.1 when the simulated users pass; published only after the operator's yes |
| Pace | Waves of 3 Haiku workers at medium effort, report per phase |
| Parallel (2026-11-16) | Both: one AI working beside the operator, and several AIs at once, each with its own computer. `monolith new` and `fork` must be cheap and computers must not share state |
| Model (2026-11-16) | Not chosen yet. Build model-agnostic first (any OpenAI-style chat endpoint on 127.0.0.1); pick the model once the base works |
| Purpose (2026-11-16) | A separate AI layer that runs beside the human one; its computer is its memory (files, journal, lessons), and at night it learns from the day (phase 3b) |

## The picture
```
 local model (llama-server, Ollama)        outside agent (Claude Code, any other)
            |  chat over 127.0.0.1                     |  plain commands
            v                                          v
   monolith run  ------------------>  monolith exec NAME -- <tool> --json ...
                                               |
        +--------------------------------------+-----------------------------+
        | one AI computer = ~/.monolith/computers/NAME/                      |
        |   root/      its files          (Krypton, Calliope, Hades ...)     |
        |   journal    every step         (Iridium)                          |
        |   snapshots  state per step     (Chronos)   -> rewind, fork, why   |
        |   memory     facts, lessons     (Iridium)   -> learning loop       |
        |   web/       cookies, identity  (Odyssey, Mercury, Sphinx)         |
        +--------------------------------------------------------------------+
                                               ^
                       cockpit (browser window) and `monolith top` (terminal)
```
The 25 products stay separate products. `monolith` is the one thin layer that joins them.

## Phases (three lanes run side by side, one worker each)
| Phase | Lane | Delivers | Gate | Workers |
|---|---|---|---|---|
| 0 Close out | fixes | chronos merge, stash, show, tag, branch -d, empty-repo export; iridium REAL and AVG | crate gates, leader repro | 2 |
| 1 Agent interface | tools | `crates/aci`; `--describe` and `--json` on all 27 tools | `tools/aci-check.sh`; two simulated agents do 30 tasks each from manifests alone, 90% or better | 12 |
| 2 The computer | tools | `monolith new, list, exec, log, rewind, fork, replay, why, rm`; fenced Krypton; fast Chronos snapshots | rewind restores files and database rows byte for byte; fork of 1 GB under 5 s | 8 |
| 3 Driver and learning | tools | `monolith run` against a local model; memory; plan, act, check, reflect; practice suites | a local model finishes 10 scripted tasks; pass rate on a suite rises after lessons, measured | 8 |
| 3b Sleep | tools | nightly cycle above: sort day, corrected rows, practice in a fork, LoRA, exam, promote or keep, morning report | pass rate on a held-out suite rises over 3 nights with no drop on old skills; a bad adapter is never promoted | 6 |
| 4 Web | web | https fetch, TLS 1.3, certificate checks against the OS root store; Odyssey `open, snapshot, click, fill, submit, text, links`; cookies; robots.txt; Web Bot Auth signing; Chrome stopgap | RFC 8448 and RFC 8032 vectors; 30 real sites read; signature verified by a second implementation | 16 |
| 5 Cockpit | look | live window: computers as tiles, timeline, file changes, approve, pause, rewind; terminal `monolith top` | screenshots desktop, mobile, reduced motion; every control exercised on a live session | 8 |
| 6 Release v0.1 | fixes | tag, notes, Windows bundle, honest list of limits | all simulated users and agents pass; operator says yes | 3 |
| 7 Titan computer | os | the same commands with `kind = vm` (Titan on Silicon); then the x86-64 boot track | same suite passes inside the VM | later |

Lanes: phases 1, 2, 3 run in order on the tools lane; phase 4 runs beside them on the web lane from wave 2;
the third worker in each wave takes fixes, then the cockpit. Rough total 60 to 90 worker tasks, about 10 to
14 million tokens across many sessions. Phase 1 is about 2.5 to 3 million.

## The learning loop (operator: "teach it to learn and think")
What gets built: each computer keeps lessons, skills and episodes in its memory database. The driver works
in four steps (plan, act, check, reflect), writes a lesson after every task and a skill after every failure,
and reads the matching ones before the next task. `monolith train NAME --suite S` runs a fixed set of checked
tasks so improvement is a number, not a feeling. Later, with the operator's decision and GPU, the journal can
be exported as training data to fine-tune a local model.
Honest label: this makes a model do better on its own computer through memory and practice. It does not
change how the model reasons and it is not general intelligence.

## Phase 3b: sleep (operator, 2026-11-16: "when things are done, apps are closed, based on everything it did
today, stored and failures, it can train itself on it at night, learn new tasks like human")
Grounded in the operator's Faith sleep run (2026-10-28, Qwen3.5-2B): held-out recall 0.43 with 3 question
templates, 0.88 with 8; only verified answers became rows. Nightly cycle per computer:
1. Start only when idle (no session, GPU free); a STOP file cancels.
2. Successes from the day's journal (checked: tests, `ok:true`, operator approval) become rows.
3. Failures become corrected rows (later success, saved lesson, or teacher model), kept only if the same
   checker accepts them. Never train on raw mistakes or on unchecked self-samples.
4. Practice in a fork: retry today's failures and variants; only passing attempts become rows.
5. Replay older rows and write each lesson in many phrasings, against forgetting.
6. Train a LoRA on the RTX 5070 (2B to 7B models; no full fine-tune on 12 GB).
7. Exam: practice suite, memory probes, old-skill checks. Promote the new adapter only if it scores higher
   and nothing old drops; keep every adapter so a bad night can be rewound.
8. Morning report: learned, scores before and after, still failing.
Facts go to memory (editable, dated); skills go to weights. The trainer is PyTorch in WSL (the Faith code),
called by Monolith: the one part not written from scratch, pending the operator's yes.

## Limits stated up front
- The host workspace is a fence, not a wall: the door checks paths and offers only Monolith tools, but Windows
  gives no kernel isolation here. Strong isolation is the Titan VM (phase 7) or Pandora on Linux.
- TLS written here is unaudited. Read the web with it; never send passwords or payment details through it.
- Chrome stopgap pages are not rendered by our engine and are marked as such in every result.
- Local models on a 12 GB GPU follow simple commands far better than long tool schemas; the command set is
  kept small for that reason.
- Chronos took 19.6 s to add a 5 MB file in the simulated-user run. Rewind needs that fixed first (phase 2).

## What needs the operator
- A domain to publish the browser's public key, and registering it with Cloudflare (phase 4, late).
- The address of the local model server to test against (phase 3).
- Yes or no on the v0.1 tag and release notes (phase 6).
- Yes or no on the two downloads for the x86-64 boot track (phase 7).

## Loop rules
Quality loop as in LOOP.md: brief per task, Haiku at medium effort, cap 3, at most 2 retries, leader reruns the
gate and checks evidence by hand before any commit, stop at a usage warning and write state to disk.

# Harness and teaching plan: a local Qwen learns to work on its Monolith computer (2026-11-16)

Operator, 2026-11-16 (asleep while this runs): "download a qwen model, maybe a 27B, and teach it to use this
effectively, maintain memory do tasks ... everything from scratch ... baseline, teach, repeat ... first study,
do research how this works and then work ... dont worry about self training". Later: "first plan, then use
claude mods and orchestrator mode, so that u dont run out of usage".

**Goal:** a from-scratch harness, `monolith`, that lets a local open-source model do real tasks with the
Monolith tools on its own computer, remember across sessions, and get measurably better by being taught
through its harness and memory. No weight training (phase 3b stays parked).

**Spec:** docs/ai-computer/PLAN.md (phases 2 and 3 merged here, narrowed to what the goal needs),
docs/ai-computer/ACI.md (tool interface).

## What the study found (searches 2026-11-16, practitioner posts and preprints, not a review)
1. Few actions beat many tools for small models; one generic "run a command" action plus file actions.
2. Malformed output is a top failure for local models; constrain the decoder (llama-server turns a JSON
   schema in `response_format` into a grammar) so an invalid action cannot be produced.
3. Do not append everything to the history; keep recent steps whole and older ones as one line each.
4. Errors must teach: on a failed command, show the relevant part of the tool's manifest.
5. Memory helps only if it is verified and retrieved for the model (Reflexion, skill libraries); the
   operator's Faith result agrees: only checked rows, never unchecked self-samples.
6. SWE-agent: a purpose-built file write/read action is worth more than a raw shell.
7. Qwen3.8-27B: thinks by default; non-thinking sampling temp 0.7, top_p 0.8, top_k 20 (model cards).

## Model and serving
- File: `Qwen3.8-27B-UD-IQ4_XS.gguf`, 14.35 GB, from `unsloth/Qwen3.8-27B-GGUF` on Hugging Face (the most
  used GGUF build of the official Qwen/Qwen3.8-27B). Downloaded in WSL to `/home/dev/models` with range
  requests and checked against the SHA-256 Hugging Face publishes (`~/pdl1.py`, log `~/monolith-ai/dl.log`).
- Server: the llama.cpp already built in WSL (`~/Projects/llama.cpp`, CUDA), `llama-server` on
  127.0.0.1:8080, about half the layers on the RTX 5070 (12 GB, about 8.5 GB free), rest on CPU.
  Expected 4 to 7 tokens/s (the PC's own notes: dense 32B at 5). Measured before any exam.
- Plan B if under 4 tokens/s: `Qwen3.6-35B-A3B` (mixture of experts, 16 to 37 tokens/s on this card
  earlier). Not downloaded unless needed.
- The harness is model-agnostic: any OpenAI-style `/v1/chat/completions` on a loopback address.

## Design: crate `crates/monolith`, binary `monolith`, std only (depends on `aci` for JSON)
One AI computer = `$MONOLITH_HOME/computers/NAME/` (`MONOLITH_HOME` defaults to `~/.monolith`):
`root/` its files, `journal.jsonl` every step, `memory/` lessons and facts.

| File | Owns |
|---|---|
| `types.rs` (leader) | shared types: `Action`, `Observation`, `Level`, `RunConfig`, `Outcome`, `Error` |
| `computer.rs` | create, list, remove, open; path fence; `exec` of a Monolith tool with cwd `root/`, timeout, output cap |
| `journal.rs` | append and read `journal.jsonl` |
| `llm.rs` | chat completion over `TcpStream` to a loopback address only; JSON schema; token counts |
| `memory.rs` | lessons and facts as JSON lines; add, list, keyword search with top k |
| `suite.rs` | task file parser and check evaluator |
| `actions.rs` | execute one `Action` on a computer and render the observation the model reads |
| `agent.rs` | the loop: prompt, call model, act, record, stop; levels 0 to 2; reflection |
| `eval.rs` | run a suite (model or reference solutions), resumable results, report |
| `main.rs` (leader skeleton, workers add arms) | CLI plus `--describe` and `--json` |

**Action protocol:** each model turn is one JSON object, enforced by schema:
`{"thought": str, "action": "run"|"help"|"write"|"read"|"remember"|"recall"|"done", "arg": str, "content": str}`.
`run` takes one Monolith command line (`chronos status`); the harness splits it without a shell, adds
`--json`, runs it inside `root/` and shows a compact view of the envelope. `help` shows a tool's commands
from its manifest. `write`/`read` are file actions inside `root/`. `remember`/`recall` use memory.
`done` ends the task with an answer.

**Levels (so each teaching step can be measured alone):**
- 0 baseline: tool names with one-line summaries; actions run, write, read, done; raw results; no memory.
- 1 interface teaching: `help` cards, a failed command gets the matching manifest card attached, long
  output shaped and capped, history compaction, one worked example in the prompt.
- 2 memory: `remember`/`recall`; the top lessons for the task are put in the prompt; after a failed task
  the model reflects, retries once, and the lesson is kept only if the retry passes the checks
  (verified); after a passed task the working command sequence is kept as a skill.

**The fence:** only Monolith binaries run; cwd is `root/`; paths that leave `root/` are refused; the model
server must be a loopback address. This is a fence, not a wall (PLAN.md limits).

## The exam (leader owns the tasks; checks are mechanical)
Suites in `suites/*.tasks`, each task: prompt, optional setup commands, checks
(`CMD => contains TEXT`, `=> lacks TEXT`, `=> exit N`, `answer contains TEXT`), and a reference solution as
action lines. `monolith eval --selftest` proves every task both ways: checks fail before the solution and
pass after it. A task that cannot do both is not used.
- `train.tasks` 24 tasks, `heldout.tasks` 12 tasks (same tools, different goals; 3 on tools absent from
  train), `memory.tasks` 4 chains of 2 sessions on one computer where session 2 needs a fact from session 1.
- The model never sees checks or solutions.

## Baseline, teach, repeat
| Run | Level | Suites | Purpose |
|---|---|---|---|
| R0 | 0 | all 44 sessions | baseline |
| R1 | 1 | all | what interface teaching alone buys |
| R2a | 2 | train | the model builds verified lessons and skills |
| R2b | 2, memory frozen | heldout, memory | does what it learned transfer; does memory work |
| R3.. | best | failures only | leader reads failure classes, changes the harness, reruns |
Report per run: pass rate with counts, steps and tokens per task, invalid-action rate, failure classes.
Honest reading rules: train and held-out reported apart; one seed unless time allows three; small
counts get counts, not percentages alone; any harness change made after seeing held-out failures is
labelled as such.

## Global constraints
- Zero external crates; `bash tools/check.sh monolith` must end `CHECK PASS monolith`.
- Loopback only; no secrets in the repo; text mode of existing tools unchanged.
- Workers: Haiku at medium effort, at most 3 at once, each in its own git worktree, one file set each,
  at most 2 retries. The leader reruns every gate and checks evidence by hand before a commit.
- GPU: one model server at a time; check memory before launch; stop it by PID.

## Review focus (most likely to bite)
1. A command line with quotes or JSON inside it: the splitter must keep it intact (tested with nested quotes).
2. A tool that hangs or floods output: timeout kills the child, output capped, the loop continues.
3. The model server drops or answers malformed JSON: the run records an error for that task and goes on.
4. A path that escapes `root/` (`..`, absolute, drive letter): refused before anything runs.
5. A resumed eval must not rerun finished tasks or mix results from another level.

## Tasks
| Id | Wave | Deliverable | Gate |
|---|---|---|---|
| H0 | leader | crate skeleton, shared types, pinned signatures, board, this plan | compiles, gate passes with stubs |
| A1 | 1 | computer.rs, journal.rs, CLI new, list, rm, exec, log | crate gate; fence and timeout tests |
| A2 | 1 | llm.rs | crate gate; fake-server tests (length and chunked bodies, errors, loopback refusal) |
| A3 | 1 | memory.rs, suite.rs | crate gate; parser and check tests |
| B1 | 2 | actions.rs, agent.rs, CLI run | crate gate; scripted fake model finishes a real task end to end |
| B2 | 2 | suites: train, heldout, memory | every solution replayed by hand with the installed tools |
| C1 | 3 | eval.rs, CLI eval, report; installers list `monolith` | crate gate; selftest green on all suites |
| G1 | leader | start server, measure speed, smoke test | tokens/s recorded |
| R0-R3 | leader + 1 analyst worker | runs above | results files and report |

## Budget
Haiku workers: 6 build tasks at about 170k = 1.0M, fix rounds 0.3M, 2 failure-analysis workers 0.3M:
**about 1.6M tokens**, stop and write state at any usage warning. GPU: about 6 to 8 hours of exams if the
model gives 5 tokens/s; runs are resumable and write results to disk.

## What needs the operator
Nothing to start. Later: a yes before the plan B model download if the 27B is too slow, and before any release.

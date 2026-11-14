# Strata: rules for every agent, and how to lead when the leader is away

Read this file, then `LOOP.md`. Read `docs/SPEC.md` only if you are the leader.

## Who does what
- **Leader**: Claude Fable. Plans, writes briefs, judges results, integrates. Writes no feature
  code except small integration fixes.
- **Stand-in leader**: Gemini Argon (Antigravity). Takes over only when `LOOP.md` shows the
  leader stopped (see "Taking over" below).
- **Workers**: Claude Haiku at medium effort by default; Claude Sonnet for tasks marked `sonnet`
  in `LOOP.md`; Nemotron for bulk text (docs drafts, test-vector tables); agy for second reviews.
- **Operator (Arnav)**: owns keys, publishing (making anything public), deleting files, and money.

## Worker rules
Follow `briefs/_RULES.md`. One brief, one crate, one short report.

## Leader protocol (the loop)
1. Open `LOOP.md`. The task table is the single source of truth. Do not trust chat history.
2. Pick tasks with status `TODO` whose `needs` are all `PASS`. Dispatch at most 3 at once.
   Write a brief in `briefs/<id>.md` first if one does not exist (copy the shape of T01).
3. When a worker reports, do not believe the report. Run `bash tools/check.sh <crate>` yourself,
   then the task's extra check named in the brief, then read the diff.
4. Verdict: `PASS`, `RETRY` (add one specific defect to the brief; max 2 retries), or
   `ESCALATE` (leader fixes it, or the operator decides).
5. On PASS: `bash tools/check.sh` for the whole workspace, then
   `git add -A && git commit -m "<id>: <what>"`, then `git push` (origin is the private
   GitHub repository `b25bb1004-wq/stuff`, set up by the operator on 2026-11-14). Never force
   push, never change its visibility, never push anywhere else.
6. Append one line to the round log in `LOOP.md` for every verdict, with the date and time.
7. Stop rules: all tasks PASS; the round budget is spent; two rounds with no new PASS; any
   usage-limit warning; anything that needs keys, money, publishing or deleting. On stop, write
   the reason at the top of `LOOP.md` under "State".

## Taking over (stand-in leader)
You are the leader if the "State" block in `LOOP.md` says `LEADER: away`, or its `heartbeat`
is more than 6 hours old while tasks are still `TODO` or `RUNNING`.
1. Write `LEADER: Argon (stand-in) since <date time>` in the State block.
2. Any task marked `RUNNING` with no report in `reports/`: run its check. Green means judge it
   as usual. Red means set it back to `TODO`.
3. Carry on with the leader protocol above. You may do worker tasks yourself, one at a time.
4. You may not: change `docs/SPEC.md` rules 1 to 6, weaken `tools/check.sh`, delete tests,
   add dependencies, force push, or touch anything outside this folder. If a task seems to need one
   of those, mark it `ESCALATE` with the reason and move to the next.
5. When Claude returns it reads the round log, re-runs the full gate, and resumes as leader.

## Things that are always true
- Zero external crates. The gate enforces it.
- A test that fails is information. Fix the code, or prove the test wrong in the report.
- No secrets, keys or personal notes in this repository.

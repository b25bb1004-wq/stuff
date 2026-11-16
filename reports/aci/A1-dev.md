A1-dev: 30 tasks, 26 passed, 4 failed
| id | tool | command you ran | expected | actual (exit, first 120 chars of stdout) | what the manifest failed to tell me |
|---|---|---|---|---|---|
| 5 | calliope | `calliope --json edit edit4.txt proj/notes.txt`; script `goto 2 1 / delete 3 / insert two changed / save` (tries 1-3 failed) | line 2 = "two changed", saved | exit 0, saved:true, text "one\ntwo changed\nthree\n" on try 4; try 1 "unknown command", try 2 "goto expects LINE COL", try 3 saved:false | No script grammar anywhere in the manifest; nothing says save is a script line |
| 13 | calliope + krypton | edit script `goto 1 1 / delete 48 / insert ONE-resolved`; then `insert \n` and `newline` | line 1 "ONE-resolved", line 2 "two changed" | exit 0, text "ONE-resolvedtwo changed" (line break lost); `newline` unknown; `insert \n` changed:false. Fixed by krypton printf | No way to put a line break in an edit script; grammar not documented |
| 16 | chronos | `rm notes.txt` (shell), then `chronos --json restore notes.txt`, `checkout main`, `checkout HEAD` | file restored, status clean | restore exit 2 "only supports --staged"; checkout exit 0 but file still deleted (detached). Restored via cat-file blob + python write, status clean | No command restores a deleted work-tree file from HEAD |
| 28 | krypton | `krypton --json -c "printf '%20000s' ''"` | truncated flag and full byte size | exit 0, stdout 4096 chars, no truncated or bytes field; full 20000 not reported. First try (`/dev/zero \| tr`) hung 120 s | "capped" given without field names; no truncation field documented for krypton |
| 1 | chronos | `chronos --json init work1/proj` | repo created | ok, path ".../proj/.chronos" | repo metadata lives in proj/.chronos (not stated) |
| 2 | krypton | `krypton --json -c 'printf ... > notes.txt'` | 3 lines | exit 0; od shows one\ntwo\nthree\n | |
| 3 | chronos | `add -A`; `commit -m first` | commit id recorded | id 3a15f584...927e4, count 1 | |
| 4 | chronos | `chronos --json status` | clean | clean:true | |
| 6 | chronos | `chronos --json status` after edit | notes.txt unstaged modified | unstaged [{notes.txt, modified}] | |
| 7 | chronos | `chronos --json diff` | - two / + two changed | hunk lines "-two", "+two changed" | |
| 8 | chronos | `commit -m second`; `log --limit 1 --offset 1` | 2 commits, older fetched | total 2, returned "first" | |
| 9 | chronos | `branch feature`; `switch feature` | on feature | branch feature | |
| 10 | chronos | add feature.txt; commit | commit on feature | id b7b97666..0681, branch feature | |
| 11 | chronos | `switch main`; `merge feature` | fast_forward | status fast_forward | |
| 12 | chronos | b1 edit, main edit, `merge b1` | error code conflict, path | ok:false code conflict, exit 1; path only inside message text | Conflicted path is not a field, only in message text |
| 14 | chronos | `tag v1`; `tag` | v1 listed | items [v1] total 1 | |
| 15 | chronos | append line; `stash`; `stash list`; `stash pop` | change back | list total 1; line "stash me" back | |
| 17 | chronos | `chronos --json show v1` | message and files | "resolve conflict"; notes.txt modified with hunks | |
| 18 | calliope | `calliope --json grep "two\|feature" notes.txt feature.txt` | hits with path, line, col | 2 items: notes.txt:2:1, feature.txt:1:1 | Does not recurse; shell glob used for file list |
| 19 | calliope | `grep "^zzq[0-9]+$" notes.txt feature.txt` | total 0 | total 0, exit 1 | Exit code for no match not documented |
| 20 | cobalt | `cobalt --json check good.lg` | clean | count 0, clean:true, exit 0 | |
| 21 | cobalt | `cobalt --json check bad.lg` | diagnostic line and message | 3:5 "binary operator Add type mismatch: Bool vs Int", exit 1 | Manifest says compile error is code invalid; envelope was ok:true with diagnostics |
| 22 | logos | `logos --json run good.lg` | run result | exit 0, stdout empty | main's return value (3) not reported, so it cannot be compared to task 24 |
| 23 | cobalt | `cobalt --json build good.lg -o good.mbc` | path and bytes | path good.mbc, bytes 48 (matches file size) | |
| 24 | elysium | `elysium --json run good.mbc` | match task 22 | result "3" (1+2); logos gave no value to compare | Depends on task 22 gap |
| 25 | logos | `logos --json test tests.lg` | passed and failed counts | passed 1, failed 1, exit 1 | |
| 26 | athena | `athena --json check bad.lg` | diagnostic line | line 3, col 5 | |
| 27 | athena | `athena --json fmt --write messy_copy.lg` | changed true, new text | changed:true; new text shown via cat (fn add(a: int, b: int) -> int { ... }) | Envelope has no text; need cat to see result |
| 29 | krypton | `krypton --json -c 'exit 3'` | exit 3, ok | exit 3, ok:true | |
| 30 | krypton | `krypton --json syntax_err.sh` (first tried `krypton --json file ...`, not_found) | error code | ok:false code "invalid", "syntax error at line 1, column 8" | Manifest lists a `file` subcommand that does not exist; the example form is the right one |

Three manifest fixes that would help most: (1) document the calliope edit script grammar, including save and how to insert a line break; (2) add a way to restore a deleted work-tree file from a revision in chronos; (3) state krypton's truncation fields (truncated, bytes) and the syntax-error code, and fix cobalt check's documented ok/code behaviour.

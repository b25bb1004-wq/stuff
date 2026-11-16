A2-data: 30 tasks, 29 passed, 1 failed
| id | tool | command you ran | expected | actual (exit, first 120 chars of stdout) | what the manifest failed to tell me |
|---|---|---|---|---|---|
| 29 | titan | titan --json boot --script titan.txt | stdout contains "hi from titan" | exit 1, not_found: cannot read kernel 'target/titan-kernel/riscv64gc-unknown-none-elf/release/kernel' | Default --kernel is a repo build output; no kernel was where the default points and the manifest says nothing about where one comes from. I also listed the tool install folder once to look for a kernel, which went beyond the manifest; I did not boot anything from it. |
| 1 | iridium | iridium --json sql shop.db "CREATE ...; INSERT 3 rows; SELECT AVG(price)" | avg 20.25 | exit 0, avg_price 20.25 | Multi-statement QUERY works; the manifest does not say so. |
| 2 | iridium | insert 250 rows via stdin "-", then SELECT with --offset 200 --limit 50 | 50 rows, ids 201-250, more true | exit 0, 50 rows ids 201-250, total 253, more true, next_offset 250 | Paging is by tool flags, not SQL OFFSET; the manifest says so only in the flag text. |
| 3 | iridium | INSERT duplicate id 1 | error code | exit 1, code invalid (UNIQUE constraint failed) | Error codes are not listed anywhere in the manifest. |
| 4 | iridium | SELECT * FROM nosuch | error code | exit 1, code not_found (no such table) | Same: codes not documented. |
| 5 | hades | hades --json mkfs disk.img --blocks 256; mkdir /docs | ok | exit 0, path /docs. Git Bash first turned /docs into C:/Program Files/Git/docs (invalid path); fixed with MSYS_NO_PATHCONV=1 | Manifest does not warn that image paths get rewritten by Git Bash on Windows. |
| 6 | hades | hades --json put disk.img a.txt /docs/a.txt; ls /docs | a.txt file, size 20 | exit 0, items [a.txt file 20], total 1 | None. |
| 7 | hades | hades --json cat disk.img /docs/a.txt; cat --out back.txt; cmp | text matches host file | exit 0, text "hello from the host\n"; cmp SAME | None. |
| 8 | hades | put 20000-byte big.txt; cat --json; cat --out; cmp | truncated true, bytes 20000, identical | exit 0, truncated true, bytes 20000, text capped at 16384; cmp IDENTICAL | None. |
| 9 | hades | hades --json fsck disk.img | clean true, count 0 | exit 0, clean true, count 0 | None. |
| 10 | hades | hades --json mkfs tiny.img --blocks 4 | error code | exit 2, code invalid: need at least 68 blocks | Minimum block count (68) is not in the manifest; the hint gave it. |
| 11 | sphinx | sphinx --json hash sha256 big.txt | hash equals hashlib | exit 0, 091024f2...c27cebb, equal to python hashlib | None. |
| 12 | sphinx | keygen -o id.key; sign id.key msg.txt; verify pub sig msg.txt | valid true | exit 0, valid true | None. |
| 13 | sphinx | verify the same signature on a copy with one byte flipped | valid false, ok true | exit 1, ok true, valid false | Task wording said "valid and ok"; manifest says a wrong signature is valid false with exit 1, which is what I got. |
| 14 | sphinx | seal box.key a.txt -o secret.enc; open with box.key; open with other.key | plaintext matches; wrong key error | open with box.key exit 0, cmp MATCH; wrong key exit 1, code invalid (decryption failed) | None. |
| 15 | sphinx | sphinx --json hmac 4a656665 a.txt | mac equals hmac in python | exit 0, mac 7c7931e7...f10ca08, equal to python | None. |
| 16 | mercury | mercury --json serve www --listen 127.0.0.1:0 (background, stdout to file) | bound address on first line | exit 0, listen 127.0.0.1:56868 | None. |
| 17 | mercury | mercury --json get http://127.0.0.1:56868/hello.txt | status 200, Content-Length 12, body | exit 0, status 200, Content-Length 12, text "hello world\n" | None. |
| 18 | mercury | get /nope.txt; then stopped server by PID 55788 (taskkill //PID //F) | status 404, ok true | exit 0, status 404, ok true; server gone after taskkill | None. |
| 19 | mercury | mercury --json get https://127.0.0.1/hello.txt | error code and hint | exit 1, code unsupported, hint "https is not built yet; use an http:// URL" | None; the hint is useful. |
| 20 | mercury | mercury --json parse req.http | kind, method, path | exit 0, kind request, method GET, path /index.html | None. |
| 21 | odyssey | odyssey --json select "a" shop.html --limit 2 | first page hrefs, total, next_offset | exit 0, hrefs /a and /b, total 3, next_offset 2 | "link element" is ambiguous; I used a (anchor). Manifest does not say which tag "link" means. |
| 22 | odyssey | odyssey --json select "div >>> p" shop.html | error code | exit 1, code invalid: invalid selector | None. |
| 23 | odyssey | odyssey --json render shop.html --width 800 --out shop.png | PNG, width and height | exit 0, width 800, height 186, PNG signature checked in python | None. |
| 24 | odyssey | odyssey --json tree shop.html --limit 200 | depth and name of li nodes | exit 0, 3 li nodes, all at depth 3 (total 20 nodes) | None. |
| 25 | aurum | aurum --json demo -o demo.chain; aurum --json verify demo.chain | height and hash; both agree | exit 0, height 3, tip hash 7d65b736...af90a67 in both commands | None. |
| 26 | xenon | xenon --json keygen | secret_key and public_key | exit 0, fields secret_key and public_key (hex) | None. |
| 27 | bench | bench --json bench --only xenon (fastest of the suites I timed: 542 ms; mercury, iridium and aurum were slower) | one result with unit | exit 0, bulk_transfer 81.66 MB/s | "Fastest suite" is not defined in the manifest; I measured wall time. |
| 28 | lithium | lithium --json run --steps 100000 | stop reason; last stdout line | exit 0, stop exit (not step_limit), steps 22754, last line "lithium: halt" | Manifest does not say that the kernel usually exits early; with stop exit, the step count is below --steps. |
| 30 | pandora | pandora --json ps | container list on Windows | exit 0 but ok false, code unsupported: "pandora needs Linux" | Manifest does not say Linux-only; a failure returned exit 0. |

Three manifest fixes that would help an agent most:
1. Every tool: list its error codes and exit codes (invalid, not_found, unsupported) and state when ok:false comes with exit 0 (pandora).
2. titan: say where the kernel comes from and what to do when the default --kernel path is missing (no kernel was present).
3. Platform notes: pandora is Linux-only, and hades image paths need MSYS_NO_PATHCONV=1 under Git Bash on Windows; add these to the manifest, along with hades's 68-block minimum.

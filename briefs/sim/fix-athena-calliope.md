# FIX athena + calliope (from SIM p3, reports/sim/p3-editor-ide.md)
Files you may touch: crates/athena/**, crates/calliope/**, tools/install.sh, tools/install.ps1, docs/DAILY.md (Packaging section only).
Gates: `bash tools/check.sh athena` and `bash tools/check.sh calliope` must each end CHECK PASS.
Zero external crates. Do not weaken, skip or delete tests. Add a test per item where it can be tested.
Report: reports/sim/fix-athena-calliope.md, 10 lines max. Budget about 150k. Do not invoke skills.

1. A10 (BLOCKS): an installed `athena serve FOLDER` fails "no web app in target/athena-web". Required: the
   installers also build the web pack (`cargo run -q --release -p athena --example pack`, see how it writes
   target/athena-web) and copy it to ~/.monolith/share/athena-web (ps1: same under %USERPROFILE%); uninstall
   removes it. athena looks for its web dir in this order: --web DIR, then <exe dir>/../share/athena-web, then
   target/athena-web relative to cwd. Error text lists the places it looked.
2. E5 (WRONG): `calliope edit` script inserting "\n" into a CRLF file writes a bare LF. Required: detect the file's
   line ending on load (majority of line ends, CRLF if any tie) and write inserted newlines in that style on save
   and on stdout. LF files unchanged. Check the interactive Session save path uses the same rule.
3. W7: PUT /api/fs/write?path=dir/ (trailing slash or empty last segment) creates a file. Required: 400
   "not a file path".
4. W3b: create kind=dir with missing parents fails os error 3. Required: create parents (like mkdir -p) for create
   of a dir and for write of a file whose parent folders are missing. Error bodies: plain words
   ("folder not found: X"), not raw OS text, for not-found cases.
5. G1: `calliope grep ... | head -1` prints "failed to write stdout ... os error 232" and exits 1. Required: a
   closed pipe (BrokenPipe, or Windows os error 232) ends quietly with exit 0, same as chronos does
   (see crates/chronos/src/bin/chronos.rs stdout_err).
6. N1/C1: `calliope FILE` without a terminal says "unknown subcommand". Required: when the argument is an existing
   file or contains a path separator or a dot, say "calliope: the editor needs a terminal (stdin is not a
   terminal)" exit 2. Unknown words still get "unknown subcommand".
7. E10: errors from `calliope edit` scripts name the script line: "script line N: line 99 out of range".
8. P3: `athena serve nosuchfolder` says "athena: not a folder: nosuchfolder".

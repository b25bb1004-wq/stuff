# FIX misc-1: mercury, hades, tool help (from SIM p4, reports/sim/p4-server-data.md)
Files: crates/mercury/**, crates/hades/**, crates/midas/**, crates/kairos/**, crates/argo/** only.
Gates: `bash tools/check.sh <crate>` ends CHECK PASS for each crate you touch. Zero external crates.
Do not weaken, skip or delete tests; add a test per item. Budget about 150k. No skills.
Report: reports/sim/fix-misc-1.md, 10 lines max.
Compare with `python -m http.server` where it applies.
1. Row 10: mercury static responses carry `Last-Modified` (RFC 1123 GMT from the file mtime), and a GET/HEAD with
   `If-Modified-Since` at or after that time gets 304 with no body.
2. Row 11: a path that names a file plus a trailing slash (`/index.html/`) is 404.
3. Row 3: a raw (not percent-encoded) UTF-8 request path is served like its percent-encoded form.
4. Rows 5/6: 404 responses have a small text/plain body "404 not found: /path\n" (path escaped of control chars).
5. Row 48: `hades mkfs img.hd --blocks 64` fails "no space left on device". Find the minimum the layout needs;
   either make 64 work or refuse below the real minimum with "hades: mkfs: need at least N blocks" exit 2.
6. Row 43: `midas --help`, `kairos --help`, `argo --help` begin with one line "<tool> - <what it does>" before
   "Usage:" (the installer MANIFEST takes the first such line). Row 46: midas and kairos --help show the input
   file format with a 3-line example that runs as given (test it).

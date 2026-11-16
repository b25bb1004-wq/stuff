# Persona p4-server-data: a person who serves sites with mercury and keeps data in iridium
Read briefs/sim/COMMON.md first. Persona name for paths: p4-server-data.
Mercury (`mercury serve DIR --listen 127.0.0.1:PORT`), compare with `python -m http.server`:
- a static site with index.html, css, js, svg, png, wasm, json, a unicode file name, a 50 MB file: status codes, Content-Type, Content-Length, body byte-exact (cmp).
- 404, directory without index, HEAD, Range requests, `If-Modified-Since`, path escapes (/../, %2e%2e, backslash, absolute), 100 parallel curls (`xargs -P 20`), a slow client that sends half a request (does the server keep serving others?), port already in use.
Iridium (`iridium sql DB [QUERY]`; see `iridium --help`):
- create tables, insert, select with where/order/limit, update, delete, joins, aggregates, NULLs, text with quotes and unicode, a bad query (clear error, DB unharmed).
- persistence: close and reopen, data still there; 10 000 inserts (time it); kill the process in the middle of a big insert, reopen, is the DB still readable and consistent?
- two processes writing the same DB at once.
Other installed tools, quick sanity: for each of argo, aurum, xenon, sphinx, odyssey, helios, iris, indium, midas, kairos, hades, elysium, lithium, zeusctl, pandora, bench, titan: run --help (exit 0?) and then the first example from its help. Report any that crash, hang or do nothing useful.
Installer: with HOME=$SIM/work/p4-server-data/home, run `bash C:/Users/ADMIN/strata/tools/install.sh` twice (update in place) and then `--uninstall`; nothing outside that HOME may change.

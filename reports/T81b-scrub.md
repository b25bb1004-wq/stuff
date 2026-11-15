T81b-scrub (docs-only, no commit): BOOK.md and PLAN.md corrected; LOOP.md line 59 fixed to public.
Files changed (9): docs/BOOK.md, docs/PLAN.md, LOOP.md (line 59 only), reports/T50b.md, reports/T75d.md, reports/T79g.md, reports/T80-chronos.md, reports/T89b.md, reports/T91.md.
Paths replaced (13): 6 file:/// links to repo-relative crates/ paths, 3 gate target dirs to <target-dir>, 3 PLAN.md paths (vault, repo root, agy CLI home) to placeholders, 1 username in a quoted chronos test payload to <user>.
Item 4 hits: AGENTS.md:32 and LOOP.md:59 carry the public GitHub account handle of the repo owner (not a credential; left as is). No email addresses, tokens or personal names found in docs/, reports/ or AGENTS.md. docs/REVIEW.md:169 is a check command, not a path; unchanged.
BOOK.md: titan, zeus, athena corrected at lines 41, 50, 51, 63-65, 411-414 (section 5 intro), 418, 424, 433-435 (athena T73 built, T74 WIP). Crate READMEs and lib.rs read; titan README says only host-side modules are in the demo.
Gap: README.md does not exist at the repo root, so item 4 had nothing to scan there.
Conflict: reports/T81b.md already exists (T81b indium/lithium README report, leader PASS 2026-11-15); not overwritten. This report is in reports/T81b-scrub.md instead.
Not committed; working tree also holds other agents' modified files (Cargo.lock, crates/athena, crates/silicon) which I did not touch.

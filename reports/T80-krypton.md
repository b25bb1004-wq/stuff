# T80 security review: krypton (shell)
Method: code reading only, nothing executed. MOVING TARGET: krypton is being edited by another worker right now; this reflects the tree at review time.
Coverage: grep-level plus sampled reads: expand.rs caps (14, 35, 123, 182), arith.rs depth (249-250), parser.rs depth (17, 233-236, 411-414, 428-438), glob.rs glob_match (241-275), exec/printf.rs:280, word.rs:346. Not read in full: exec/mod.rs, exec/external.rs, exec/redir.rs, exec/builtins.rs.
HIGH: none. MEDIUM: none. LOW: none found in sampled code.
No finding: printf.rs:280 and word.rs:346 unreachable arms are reached only from outer matches that list the same characters; glob_match is the single-backtrack greedy form, with no exponential blowup; expansion output is capped at 2 MiB (expand.rs:14); parser and expansion nesting is capped at 128 (parser.rs:17, expand.rs:182).

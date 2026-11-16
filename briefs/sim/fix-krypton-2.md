# FIX krypton-2: daily builtins (from SIM p2, reports/sim/p2-shell.md)
Files: crates/krypton/** only. Gate: `bash tools/check.sh krypton` ends CHECK PASS krypton. Zero external crates.
Do not weaken, skip or delete tests; add tests in crates/krypton/tests/sim_fixes2.rs. Budget about 150k. No skills.
Report: reports/sim/fix-krypton-2.md, 10 lines max. Expected = bash.
1. R19: `alias name='text'`, `alias` (list, `alias name='text'` form), `unalias name`, `unalias -a`. Expansion of
   the first word of a simple command, recursive with a guard, trailing-space rule optional. Works in -c,
   scripts and the interactive shell, so ~/.kryptonrc aliases apply interactively.
2. R5: printf supports %d %i %u %x %X %o %e %f %g %c %s %b %% with flags `-+ 0#`, width and .precision, and `*`
   width; a bad number prints an error and exits 1 like bash ("printf: abc: invalid number").
3. R6: $'...' ANSI-C quoting: \n \t \r \ \' \" \a \b \e \f \v \0NNN \xHH \uHHHH.
4. R7: `kill -0 $$` succeeds for the running shell (on Windows check the process by PID with the existing
   process code; do not add unsafe code unless it already exists for this, with a SAFETY line).
5. `umask` (print 0022, accept a value and store it; on Windows it only stores) and `hash -r` (no-op, exit 0).

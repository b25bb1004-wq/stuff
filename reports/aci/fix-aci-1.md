# fix-aci-1 report
- F1 titan: default kernel now repo path, else `<exe>/../share/titan/kernel`; not_found names both; manifest/help updated. install.sh/install.ps1 build the kernel (warn-only on failure), copy to share/titan, remove it on --uninstall. New tests in crates/neon/tests/aci.rs (boot outside repo; no-kernel error names both).
- F1 hand check (HOME=scratch fix1, empty cwd): `"stdout":"...$ hi from titan\n$ ","stop":"halt"` (exit 0; install log shows no kernel warning).
- F2 logos: run JSON gains `result` (main's value as elysium renders it; null for unit). run_main_value added in cobalt run.rs, fmt copied from elysium (elysium is off-limits). Test: logos_run_json_result_is_main_return_value.
- F3 cobalt --describe: check `returns`/`summary` now describe diagnostics, count, clean, ok true, exit 1 when not clean.
- F4 hades: JSON error hint "Git Bash rewrote the image path; set MSYS_NO_PATHCONV=1 or write it as //docs" for in-image Windows-looking paths (text mode untouched); summary adds path and 68-block notes. Test: json_git_bash_rewritten_path_gets_a_hint.
- F5 pandora: summary starts "Linux only: on other systems every command answers code unsupported."
- Gates: CHECK PASS neon / CHECK PASS cobalt / CHECK PASS hades / CHECK PASS pandora / CHECK PASS titan
- Not committed. Other workers' files (chronos, calliope, krypton) untouched by me.

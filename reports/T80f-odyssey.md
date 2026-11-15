# T80f odyssey fix: H1 and H2 closed, gate PASS
H1: select/matcher.rs descendant/sibling walks now memoise failed (combinator, node) pairs per element and stop at memoised failures; select/parser.rs rejects selectors over 32 compounds or 1024 bytes (rule dropped by cascade).
H2: sibling positions computed once per cascade and per select() via sibling_table(); Selector::matches still scans for single calls.
Tests added (tests/select_tests.rs): selector_size_caps, descendant_chain_on_deep_tree_is_linear (2000-deep, 30x `*`), descendant_failure_on_deep_tree_is_linear (2000-deep, failing 32-compound), nth_child_on_wide_list_is_linear (ul with 5000 li, :nth-child rule).
Timings, debug: 0.01 s / 0.06 s / 0.03 s; release: 0.00 s / 0.01 s / 0.00 s (limit 1 s each).
cargo fmt -p odyssey run; `bash tools/check.sh odyssey` ends CHECK PASS odyssey (fmt, clippy -D warnings, all tests, zero-dep). Note: check.sh overrides CARGO_TARGET_DIR to target/w-odyssey, so the h-odyssey prefix had no effect.
Not done: next/subsequent-sibling walks are still O(siblings) per step; cascade still runs every selector on every element; the 1024-byte cap applies to the whole selector list text. Nothing committed; no other crates, check.sh, or existing tests touched.

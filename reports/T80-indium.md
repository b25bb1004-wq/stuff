# T80 security review: indium (read-only; nothing executed)
Read: widgets/field.rs insert, backspace and delete (20-58), motion.rs sample (46-56), lib.rs forbid(unsafe_code). Grep of src for process, network and unsafe APIs: none in src (only bin/indium.rs file read and write).
HIGH: none. MEDIUM: none. LOW: none.
No finding (sampled): field.rs keeps the cursor in characters and converts with byte_at before each String insert and remove, so edits stay on char boundaries; layout stacks are bounded by the node tree.
Conditional note: motion.rs:56 sizes Vec::with_capacity from preset duration_ms. Safe while presets are built-in constants; if bin/indium.rs lets a user pass a duration, a very large value would abort on allocation. The CLI argument path was not checked.
Not reviewed in depth: layout.rs, events.rs, widgets.rs, bin/indium.rs.

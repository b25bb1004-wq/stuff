# T80 security review: iris (read-only; sampled by grep and a few reads; nothing executed)
Read: grep of src for unsafe, process, network and fs APIs; apps/edit.rs truncate call (84); apps/files.rs set_len (92); apps/term.rs history drain (68); lib.rs forbid(unsafe_code).
HIGH: none. MEDIUM: none. LOW: none.
No finding (sampled): no Command, socket or unsafe use in src; the file apps work on the in-memory SharedFs (apps/edit.rs writes through fs.truncate with a length taken from the buffer); term.rs drains history with a computed excess. The CLI reads and writes only the paths it is given.
Not reviewed: window.rs, desktop.rs, layout.rs, damage.rs, SharedFs internals and apps/files.rs path handling, apps/term.rs command semantics.

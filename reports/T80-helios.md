# T80 security review: helios (read-only; nothing executed)
Read: audio.rs tone() (20-40), script.rs ScriptScene::new and per-frame fuel (227, 266, 303), draws push (174), render (342-360), scene.rs parse (180-187), lib.rs forbid(unsafe_code).
HIGH: none. MEDIUM: none. LOW: none.
No finding (sampled): tone() caps samples at 4,800,000 (audio.rs:20, 30) before allocating; scene.rs:180-187 checks that the byte length equals 8 + 12*count before with_capacity(count); scripts run under a per-frame fuel budget (script.rs:227, 303); draw rectangles are clipped by fill_rect; the CLI reads and writes only the paths it is given.
Not reviewed in depth: ecs/mod.rs, phys/world.rs, the rest of script.rs (the language compiler lives in the logos crate, out of scope), bin/helios.rs argument handling.

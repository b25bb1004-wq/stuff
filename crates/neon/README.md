# neon

Zero-external-crate WebAssembly build and browser glue for **Titan** (RISC-V kernel on Silicon + Hades disk) and the **Logos** playground (compiled by Cobalt, executed on Elysium).

## Wasm ABI (`wasm32-unknown-unknown`)

All pointers live in `neon`'s exported linear `memory`. Multi-byte inputs are allocated by JS with `neon_alloc`, written into `memory.buffer`, passed to the call, and freed with `neon_free`. Variable-length outputs (and `titan_boot` errors) are written into an internal last-output buffer and read immediately via `neon_last_ptr()` and the returned byte length (or `neon_last_len()`).

| Export | Signature | Description |
|---|---|---|
| `neon_alloc` | `(len: usize) -> *mut u8` | Allocate `len` bytes in wasm linear memory |
| `neon_free` | `(ptr: *mut u8, len: usize)` | Free a buffer returned by `neon_alloc` |
| `neon_last_ptr` | `() -> *const u8` | Pointer to the last output/error buffer |
| `neon_last_len` | `() -> usize` | Byte length of the last output/error buffer |
| `titan_boot` | `(ptr: *const u8, len: usize) -> i32` | Boot Titan ELF; `0` on ok, non-zero with error in last buffer |
| `titan_send` | `(ptr: *const u8, len: usize)` | Queue UTF-8 line + `\n` onto UART RX |
| `titan_pump` | `(max_steps: u64) -> i32` | Run up to `max_steps`; `0` waiting, `1` busy, `2 + code` exited |
| `titan_output` | `() -> usize` | Drain pending UART TX into last buffer and return byte length |
| `logos_run` | `(ptr: *const u8, len: usize, fuel: u64) -> usize` | Run Logos `main` with `fuel`; output/diagnostic/trap in last buffer |

## Bundling

```sh
cargo run -p neon --example pack
```

Writes `index.html`, `neon.js`, `style.css`, `neon.wasm`, and `titan.elf` to `target/neon-web`.

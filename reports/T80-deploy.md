# T80 deploy (servers): security review
Sampled: services.rs peer setup (105-135), fresh_peer_seed (318-327), demo key derivation (310-317), probe.rs chunked-body parser (196-230), web.rs entry (18-60), README.md:48 (demo keys documented as public), unsafe scan (none; bin files use forbid(unsafe_code)).
- MEDIUM crates/deploy/src/services.rs:318-327 fresh_peer_seed = SHA-256("monolith-demo-seed-" || id || nanos). It seeds the xenon Peer RNG (services.rs:113, 123), so the handshake ephemerals are predictable from the start-up time to within nanoseconds. Input: a peer started at a known second. README.md:48 marks the keys as public demo keys, so the impact is limited to demo transport confidentiality, but the code has no real RNG. Fix: seed from the OS CSPRNG, or refuse to run without one.
- No other finding. probe.rs bounds chunk size against MAX_HTTP_BODY before slicing (probe.rs:208-227). web.rs delegates request parsing to the mercury HTTP stack.
Count: high 0, medium 1, low 0.

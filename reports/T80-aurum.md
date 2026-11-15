# T80 aurum (blockchain): security review
Sampled: tx.rs verify (86-88), state.rs apply (79-89, checked math), chain.rs header verify (158), block.rs decode (107-120), tx decode (MAX_DATA check, tx.rs:104-108), CLI chain-file count limits (bin/aurum.rs:438-445), node/net.rs write loop (66-100). Signatures and nonces are checked; decoded counts and lengths are bounded.
- LOW crates/aurum/src/bin/aurum.rs:335-356 generate_seed on win32: no /dev/urandom, so the seed is SHA-256 over SipHash RandomState outputs plus clock and PID. The RandomState keys are OS-seeded (about 128 bits), but the output is not a CSPRNG. Input: aurum keygen -o key on Windows. Fix: use the OS CSPRNG and fail closed.
- No other finding in the sampled paths. Not read: the rest of node/mod.rs gossip logic, contract.rs VM, and merkle proofs.
Count: high 0, medium 0, low 1.

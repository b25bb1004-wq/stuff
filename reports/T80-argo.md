# T80 argo (package manager, signatures): security review
Sampled: pkg.rs validate_path (49-66), decode (120-190), open (206-230), client.rs install verification (318-345), write_package_to_disk (236-265), HTTP body cap (client.rs:108-145), manifest name check (manifest.rs:18-26). open() verifies the signature and trusted key before decode; name, version, and dependency checks run before any write.
- LOW crates/argo/src/pkg.rs:49-66 validate_path accepts Windows reserved names (CON, NUL, COM1, AUX) and trailing dots or spaces. Input: a signed package containing file "bin/CON". After verification, client.rs:259-261 writes to the device on win32. Fix: reject reserved names case-insensitively, with or without an extension.
- LOW crates/argo/src/pkg.rs:158-185 duplicate detection is case-sensitive. Input: "lib/A.txt" and "lib/a.txt" both pass; on case-insensitive filesystems the second write overwrites the first. Fix: compare paths under ASCII case folding.
- LOW crates/argo/src/pkg.rs:69 encode() asserts validate_path and panics on an invalid path. It is local packaging only, not untrusted input. Fix: return Result.
Count: high 0, medium 0, low 3.

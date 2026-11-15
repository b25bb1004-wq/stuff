# T80 pandora (containers): security review
Sampled: sys.rs unsafe FFI blocks (all 17 carry SAFETY notes and check return codes), fork and pipe handshake read loop (sys.rs:316-345), image.rs validate_rel_path (59-75), symlink rejection in collect_pkg_files (233-262), ensure_safe_parent (290-325), collect_subgraph visited check (126-150), spec.rs parse_size (112-127, checked_mul, ASCII-only suffix slicing).
- No finding in the sampled code. Not read: the rest of image.rs install and pull logic, cgroup.rs, state.rs.
Count: high 0, medium 0, low 0.

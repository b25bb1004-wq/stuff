# T80 security review: zeus (hypervisor)
Method: code reading only, nothing executed. Sampled: lib.rs host build_machine and load_guest (bounds and slot math), bin/zeus/boot.rs parse_boot_table and build_gstage (boot.rs:165-248), unsafe sites from grep in bin/zeus/*.rs and zeus.rs. Not reviewed in full: guest.rs trap and CSR handling, emulate.rs.
HIGH: none. MEDIUM: none.
LOW
- bin/zeus/boot.rs:241 (build_gstage): every guest 2 MiB page is mapped with 0xdf (R, W, X, U, A, D). A guest can write and execute its own RAM (no W^X). No cross-guest or host access follows from this. Fix: if W^X is wanted, map code segments read-execute only.
No finding: guest slots are 16 MiB apart and each guest is at most 16 MiB (lib.rs SLOT_BASE, SLOT_STRIDE, MAX_GUEST_SIZE), so G-stage maps cannot overlap; the in-guest boot table is re-validated (boot.rs:204-210); host ELF and raw images are range-checked to guest RAM before load (lib.rs:153-156, 172-178); lib.rs is forbid(unsafe_code) (lib.rs:3).

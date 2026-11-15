//! Build script for `zeus`: passes `zeus.ld` when linking the `riscv64*` binary.

use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=zeus.ld");
    let target = env::var("TARGET").unwrap_or_default();
    if target.starts_with("riscv64") {
        let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
        let ld_path = manifest_dir.join("zeus.ld");
        println!("cargo:rustc-link-arg-bins=-T{}", ld_path.display());
    }
}

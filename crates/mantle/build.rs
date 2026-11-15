//! Build script for `mantle`: passes `kernel.ld` when targeting `riscv64*`.

use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=kernel.ld");
    let target = env::var("TARGET").unwrap_or_default();
    if target.starts_with("riscv64") {
        let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
        let ld_path = PathBuf::from(manifest_dir).join("kernel.ld");
        println!("cargo:rustc-link-arg-bins=-T{}", ld_path.display());
    }
}

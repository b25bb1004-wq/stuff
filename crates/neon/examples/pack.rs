//! Build and bundle the neon web assets into `target/neon-web`.

use std::path::Path;

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("cannot locate workspace root");
    let out = root.join("target").join("neon-web");
    if let Err(err) = neon::pack::pack(&out) {
        eprintln!("pack failed: {err}");
        std::process::exit(1);
    }
    println!("{}", out.display());
}

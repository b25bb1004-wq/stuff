//! Pack the Monolith site web bundle into `target/site-web`.

use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let Some(root) = manifest_dir.parent().and_then(Path::parent) else {
        eprintln!("cannot locate workspace root");
        return ExitCode::FAILURE;
    };
    let out = root.join("target").join("site-web");
    match site::pack::pack(&out) {
        Ok(()) => {
            println!("packed Monolith site to {}", out.display());
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("pack failed: {err}");
            ExitCode::FAILURE
        }
    }
}

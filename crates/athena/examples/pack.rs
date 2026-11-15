//! Example that packs the Athena web bundle into `target/athena-web`.

use std::path::Path;

fn main() -> Result<(), String> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| String::from("cannot locate workspace root"))?;
    let out = root.join("target").join("athena-web");
    athena::pack::pack(&out)?;
    println!("{}", out.display());
    Ok(())
}

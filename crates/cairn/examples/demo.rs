//! Runnable demonstration of the cairn package manager library.

use cairn::lock;
use cairn::manifest::Manifest;
use cairn::pkg::{self, Package};
use cairn::solve::{self, Index};
use cairn::version::{Req, Version};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Parse semantic versions and Cargo-style requirements.
    let v100 = Version::parse("1.0.0")?;
    let v120 = Version::parse("1.2.0")?;
    let v200 = Version::parse("2.0.0")?;
    let req = Req::parse("^1.0")?;
    println!(
        "req {req}: 1.0.0={}, 1.2.0={}, 2.0.0={}",
        req.matches(&v100),
        req.matches(&v120),
        req.matches(&v200)
    );

    // 2. Parse a package manifest.
    let manifest_text = "\
[package]
name = \"app\"
version = \"0.1.0\"

[dependencies]
http = \"^1.0\"
";
    let manifest = Manifest::parse(manifest_text)?;
    println!(
        "manifest: {} {} ({} dep)",
        manifest.name,
        manifest.version,
        manifest.dependencies.len()
    );

    // 3. Build a package index and solve dependencies.
    let mut index = Index::new();
    index.add("log", Version::parse("0.4.0")?, vec![]);
    index.add("log", Version::parse("0.4.2")?, vec![]);
    index.add(
        "http",
        Version::parse("1.0.0")?,
        vec![("log".to_string(), Req::parse("^0.4")?)],
    );
    index.add(
        "http",
        Version::parse("1.2.0")?,
        vec![("log".to_string(), Req::parse("^0.4.2")?)],
    );

    let solution = solve::solve(&index, &manifest.dependencies, None)?;
    println!("solved {} packages:", solution.len());
    for (name, ver) in &solution {
        println!("  {name} = {ver}");
    }

    // 4. Write and re-read a deterministic lock file, then re-solve with lock preference.
    let lock_text = lock::write(&solution);
    let parsed_lock = lock::read(&lock_text)?;
    let locked_solution = solve::solve(&index, &manifest.dependencies, Some(&parsed_lock))?;
    println!("lock roundtrip ok: {}", locked_solution == solution);

    // 5. Encode and decode a canonical package archive.
    let pkg = Package {
        manifest,
        files: vec![
            ("src/main.rs".to_string(), b"fn main() {}\n".to_vec()),
            ("README.txt".to_string(), b"hello cairn\n".to_vec()),
        ],
    };
    let body = pkg::encode(&pkg);
    let decoded = pkg::decode(&body)?;
    println!(
        "package encoded {} bytes, decoded {} files",
        body.len(),
        decoded.files.len()
    );

    Ok(())
}

//! Deterministic demonstration of chronos repository, branching, merge, and diff APIs.

use chronos::{merge3, to_hex, unified, MemStore, Repo, Signature};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sig = Signature {
        name: "Alice".to_string(),
        email: "alice@example.com".to_string(),
        time: 1_700_000_000,
        tz_minutes: 0,
    };

    let mut repo = Repo::init(MemStore::new());
    repo.add("README.txt", b"hello chronos\n", false)?;
    repo.add("src/app.txt", b"line 1\nline 2\n", false)?;
    println!("staged changes before commit: {}", repo.status().len());

    let c1 = repo.commit("initial commit", &sig)?;
    println!(
        "committed {} on branch {:?}",
        &to_hex(&c1)[..12],
        repo.current_branch()
    );

    repo.branch("feature", &c1)?;
    repo.checkout("feature")?;
    repo.add("src/app.txt", b"line 1\nline 2\nline 3\n", false)?;
    let c2 = repo.commit("add line 3 on feature", &sig)?;
    println!("feature commit: {}", &to_hex(&c2)[..12]);

    repo.checkout("main")?;
    repo.add("README.txt", b"hello chronos\nv2\n", false)?;
    let c3 = repo.commit("update README on main", &sig)?;
    println!("main commit: {}", &to_hex(&c3)[..12]);

    let merge_id = repo
        .merge("feature", "merge feature into main", &sig)
        .map_err(|e| format!("{e:?}"))?;
    let history = repo.log(&merge_id)?;
    let tree_files = repo.files(&merge_id)?;
    println!(
        "merged into {}: {} commits in log, {} files in tree",
        &to_hex(&merge_id)[..12],
        history.len(),
        tree_files.len()
    );

    let patch = unified(
        "src/app.txt",
        b"line 1\nline 2\n",
        b"line 1\nline 2\nline 3\n",
        1,
    );
    let patch_text = String::from_utf8_lossy(&patch);
    println!("unified diff lines: {}", patch_text.lines().count());

    let merged = merge3(b"a\nb\n", b"a\nb\nc\n", b"x\na\nb\n");
    println!(
        "merge3 conflicts: {}, bytes: {}",
        merged.conflicts,
        merged.text.len()
    );

    Ok(())
}

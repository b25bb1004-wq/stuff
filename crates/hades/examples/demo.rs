//! Demo: format a disk, make files and directories, then cut the power in the middle of a write
//! and check that the file system recovers. Deterministic: no clock, no randomness, no I/O.

use hades::{fsck, mkfs, BlockDevice, CrashDisk, Fs, MemDisk, MkfsOptions};

fn main() {
    // A 256-block (1 MiB) RAM disk with the default inode count and a 64-block journal.
    let mut disk = MemDisk::new(256);
    mkfs(&mut disk, MkfsOptions::default()).expect("mkfs");
    let mut fs = Fs::mount(disk).expect("mount");

    let free = fs.statfs().expect("statfs");
    println!(
        "formatted: {} blocks, {} free, {} free inodes",
        free.total_blocks, free.free_blocks, free.free_inodes
    );

    fs.mkdir("/notes").expect("mkdir");
    fs.create("/notes/hello.txt").expect("create");
    let text = b"hello from hades\n";
    let n = fs.write("/notes/hello.txt", 0, text).expect("write");
    println!("wrote {n} bytes to /notes/hello.txt");

    let mut buf = [0u8; 64];
    let got = fs.read("/notes/hello.txt", 0, &mut buf).expect("read");
    println!(
        "read back {got} bytes: {}",
        std::str::from_utf8(&buf[..got]).expect("utf8").trim_end()
    );

    fs.rename("/notes/hello.txt", "/notes/greeting.txt")
        .expect("rename");
    for entry in fs.list("/notes").expect("list") {
        println!(
            "list /notes: {} is {:?} (inode {})",
            entry.name, entry.kind, entry.inode
        );
    }
    let st = fs.stat("/notes/greeting.txt").expect("stat");
    println!(
        "stat: {:?}, {} bytes, {} link(s)",
        st.kind, st.size, st.links
    );

    // Power cut: only the first 3 block writes of the next session reach the disk.
    let disk = fs.unmount().expect("unmount");
    let mut fs = Fs::mount(CrashDisk::new(disk, 3)).expect("mount");
    let longer = b"this line is longer and is written after the power cut\n";
    fs.write("/notes/greeting.txt", 0, longer).expect("write");
    let crash = fs.unmount().expect("unmount");
    println!(
        "power cut: {} block writes attempted, 3 kept",
        crash.writes()
    );

    let mut inner = crash.into_inner();
    let before = fsck(&mut inner, false).expect("fsck");
    println!("fsck before replay: {} problem(s)", before.errors.len());
    for problem in &before.errors {
        println!("  {problem}");
    }

    // Mounting replays a complete journal transaction; after that the check must be clean.
    let mut fs = Fs::mount(inner).expect("mount after crash");
    let mut out = [0u8; 128];
    let got = fs.read("/notes/greeting.txt", 0, &mut out).expect("read");
    let shown = std::str::from_utf8(&out[..got]).expect("utf8");
    println!(
        "after recovery: {} bytes, first line {:?}",
        got,
        shown.lines().next().unwrap_or("")
    );
    let mut inner = fs.unmount().expect("unmount");
    let after = fsck(&mut inner, false).expect("fsck");
    println!("fsck after recovery: {} problem(s)", after.errors.len());
    println!(
        "block size {} bytes, device {} blocks",
        hades::BLOCK,
        inner.block_count()
    );
}

# hades

hades is the file system of Monolith OS. It stores files and directories on a device of 4096-byte
blocks, using version 1 of the on-disk format. It offers path-based operations, a consistency
checker, and a write-ahead journal.

The reason for the crate is safety on power loss. Each public operation runs in one private
transaction and commits through the journal, so after a power cut the device holds either the
state before the operation or the state after it. The `fsck` function checks a device for
consistency and can repair it. The crate has no dependencies and no `unsafe` code.

## Features

- Format a device with `mkfs`, with an inode count and a journal size you can set.
- Mount and unmount. Mounting replays a complete journal transaction before use.
- Create files and directories, read and write at an offset, truncate, rename, and remove.
- Remove empty directories, list a directory sorted by name, and read file metadata.
- Free-space report with `statfs`.
- Names up to 255 bytes. Errors are typed and leave the device unchanged.
- `fsck` reports problems, and with `repair` set it replays the journal and fixes what it safely can.
- `MemDisk` is a RAM device. `CrashDisk` simulates a power cut after a set number of writes.
- Builds without `std` outside of tests (`no_std` with `alloc`).

## Public API

| Item | What it does |
| --- | --- |
| `mkfs(dev, MkfsOptions)` | Write an empty file system to a device |
| `Fs::mount(dev)`, `Fs::unmount()` | Attach to a device; unmount flushes and returns it |
| `create`, `mkdir`, `write`, `read` | Make entries and move data at an offset |
| `truncate`, `remove`, `rmdir`, `rename` | Change size, delete, and move entries |
| `list`, `stat`, `statfs` | Read directories, metadata, and free space |
| `fsck(dev, repair)` | Check a device and return a `Report` |
| `BlockDevice` | Trait a device implements: block count, read, write, flush |
| `MemDisk`, `CrashDisk` | RAM device, and a device that drops writes after a budget |
| `Error`, `Kind`, `Stat`, `DirEntry`, `FsStat` | Result types and metadata |
| `BLOCK` | Block size, 4096 bytes |

## Demo

The demo formats a 1 MiB RAM disk, makes a few files, cuts the power in the middle of a write
with `CrashDisk`, and then checks that the file system recovers. It is deterministic and does
not use the clock or the network.

```
cargo run -p hades --example demo
```

Output:

```
formatted: 256 blocks, 188 free, 62 free inodes
wrote 17 bytes to /notes/hello.txt
read back 17 bytes: hello from hades
list /notes: greeting.txt is File (inode 3)
stat: File, 17 bytes, 1 link(s)
power cut: 6 block writes attempted, 3 kept
fsck before replay: 1 problem(s)
  journal holds a committed transaction that is not replayed
after recovery: 55 bytes, first line "this line is longer and is written after the power cut"
fsck after recovery: 0 problem(s)
block size 4096 bytes, device 256 blocks
```

## Tests

```
cargo test -p hades
```

The tests are in `crates/hades/src/tests*.rs`: operation, crash, consistency-check, and property
tests.

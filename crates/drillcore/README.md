# drillcore

`drillcore` is a zero-dependency Git SHA-256 compatible object store, staging index, commit graph, diff/merge engine, and HTTP remote protocol written in Rust. It provides `no_std` (with `alloc`) core repository operations over pluggable storage backends, plus optional `std` filesystem storage and HTTP remote sync so tools can embed version control without shelling out to `git`.

## Features

- SHA-256 object hashing and verification (`hash_object`, `to_hex`, `from_hex`) for `Blob`, `Tree`, and `Commit` objects
- Git-compatible binary tree and text commit encoding and decoding (`encode_tree`, `decode_tree`, `encode_commit`, `decode_commit`)
- Pluggable `Store` trait with in-memory `MemStore` and `std`-gated filesystem `DirStore`
- Staging index and repository lifecycle: `init`, `open`, `add`, `rm`, `staged`, `status`, `commit`, `files`, and `log`
- Branching, revision resolution (`HEAD`, branch names, hex prefixes, `~n`, `^`), and `checkout`
- Myers `diff`, `lines`, and `unified` diff formatting
- 3-way line merge (`merge3`) and repository `merge` with fast-forward and conflict reporting
- `std`-gated HTTP remote protocol (`remote::serve`, `clone`, `fetch`, `pull`, `push`, `encode_pack`, `decode_pack`)

## Public API

| Item | Role |
| --- | --- |
| `Repo<S>` | High-level repository operations over a `Store` |
| `Store`, `MemStore`, `DirStore` | Flat key-value storage trait, in-memory map, and filesystem backend |
| `Id`, `Kind`, `Error` | 32-byte SHA-256 id, object kind, and repository error type |
| `Commit`, `TreeEntry`, `Mode`, `Signature` | Decoded commit/tree structures and binary/text codecs |
| `Change` | Staged index change relative to `HEAD` (`Added`, `Modified`, `Deleted`) |
| `diff`, `lines`, `unified`, `Edit` | Myers diff algorithm and unified diff formatter |
| `merge3`, `Merged`, `MergeError` | 3-way line merge and repository merge error types |
| `remote` | HTTP remote `clone`, `fetch`, `pull`, `push`, `serve`, and pack codecs |

## Demo

Run the deterministic example:

```sh
cargo run -p drillcore --example demo
```

```text
staged changes before commit: 2
committed a2cd9260a303 on branch Some("main")
feature commit: dc0ba18a876d
main commit: acf002a282a6
merged into d20f60279a6f: 4 commits in log, 2 files in tree
unified diff lines: 5
merge3 conflicts: 0, bytes: 8
```

## Tests

Run the crate test suite:

```sh
cargo test -p drillcore
```

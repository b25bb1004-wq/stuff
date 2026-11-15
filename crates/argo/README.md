# argo

`argo` is a package manager library for semantic versions, Cargo-style version requirements, package manifests, deterministic dependency resolution, lock files, signed binary package archives, and an HTTP/1.1 registry and local installer. It is written with `#![forbid(unsafe_code)]` and uses internal crates (`sphinx` for Ed25519 signatures and `mercury` for HTTP/1.1) with no third-party dependencies.

## Features

- Semantic versions (`MAJOR.MINOR.PATCH`) and Cargo-style requirements (bare, `^`, `~`, `=`, `>`, `>=`, `<`, `<=`, and `*` wildcards).
- `[package]` and `[dependencies]` manifest parser and canonical formatter (`Manifest`).
- Iterative backtracking dependency solver (`solve::solve`) with optional lock preference.
- Deterministic `# argo lock v1` lock file reader and writer (`lock::read`, `lock::write`).
- Canonical binary package format (`CAIRNPKG1\n`) with path validation and Ed25519 signing/verification (`CAIRNSIG1\n`).
- In-memory HTTP/1.1 package registry (`Registry`) supporting `PUT /pkg/<name>/<ver>`, `GET /pkg/<name>/<ver>`, and `GET /index`.
- Local package client (`install`, `remove`, `installed`) with signature verification, dependency checks on removal, and `argo.lock` updates.

## Public API

| Module | Key Items | Purpose |
| --- | --- | --- |
| `version` | `Version`, `Req`, `ParseError` | Parse and match semantic versions and requirements |
| `manifest` | `Manifest` | Parse and format `[package]` and `[dependencies]` |
| `solve` | `Index`, `Solution`, `SolveError`, `solve` | Resolve dependency graphs against an index |
| `lock` | `read`, `write` | Read and write `# argo lock v1` lock files |
| `pkg` | `Package`, `PkgError`, `encode`, `decode`, `sign`, `open` | Encode, decode, sign, and verify package blobs |
| `registry` | `Registry` | Serve signed packages and an index over HTTP/1.1 |
| `client` | `InstallError`, `install`, `remove`, `installed` | Install and remove verified packages on disk |

## Demo

Run the example:

```sh
cargo run -p argo --example demo
```

```text
req ^1.0: 1.0.0=true, 1.2.0=true, 2.0.0=false
manifest: app 0.1.0 (1 dep)
solved 2 packages:
  http = 1.2.0
  log = 0.4.2
lock roundtrip ok: true
package encoded 146 bytes, decoded 2 files
```

## Tests

Run the unit and integration tests:

```sh
cargo test -p argo
```

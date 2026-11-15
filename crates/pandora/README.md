# Pandora

A small Linux container runtime with no external crates. `pandora run` starts a command in new user, mount,
PID, UTS, IPC and network namespaces, switches its root to a host directory with `pivot_root`, mounts `/proc`,
and gives it only the environment you pass. With `--memory`, `--pids` or `--cpu` it first creates a cgroup v2
group and puts the container in it.

```
pandora run --rootfs /srv/box --hostname box --env PATH=/bin --pids 64 -- /bin/sh -c 'echo $$'   # prints 1
```

- Runs unprivileged where the kernel allows user namespaces (tested on WSL2, kernel 6.18). The user is root
  inside and maps to your own uid outside.
- cgroup limits need write access to the cgroup v2 tree (root, or a delegated subtree passed with
  `--cgroup-base`). Without it, pandora refuses to start the container instead of running it unlimited.
- Exit code: the container's own, 128 + signal if it was killed, 125 when pandora fails, 126 when namespace
  setup fails inside the child, 127 when the command cannot be executed.
- `unsafe` lives only in `src/sys.rs` (raw libc calls), each block with a `// SAFETY:` note. Off Linux,
  `run` returns `Unsupported`; spec and cgroup code build and test everywhere.

Tests: `tests/spec_cgroup.rs` (all platforms) and `tests/linux_run.rs` (real containers: PID 1, hostname,
root listing, environment, exit codes, `/proc` isolation, cgroup refusal). Run the Linux ones in WSL with
`cargo test -p pandora`.

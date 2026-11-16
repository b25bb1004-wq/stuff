//! `pandora`: Linux container runtime CLI (`pull`, `run`, `ps`, `kill`, `rm`).
//! Exits with the container's exit code on foreground `run`, 0 on management commands, or 125 on error.

use std::fs;
use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{exit, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use argo::version::Req;
use pandora::state::{self, Status};
use pandora::{image, parse_size, Spec};

const USAGE: &str = "usage: pandora [--store DIR] <pull|run|ps|kill|rm> ...";

fn fail(msg: &str) -> ! {
    eprintln!("pandora: {msg}");
    exit(125)
}

fn default_store() -> io::Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is not set; pass --store"))?;
    Ok(PathBuf::from(home).join(".pandora"))
}

fn resolve_store(opt: Option<PathBuf>) -> PathBuf {
    let raw = opt.unwrap_or_else(|| default_store().unwrap_or_else(|e| fail(&e.to_string())));
    abs_path(&raw).unwrap_or_else(|e| fail(&e.to_string()))
}

fn abs_path(p: &Path) -> io::Result<PathBuf> {
    if p.is_absolute() {
        Ok(p.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(p))
    }
}

fn parse_hex32(s: &str) -> Result<[u8; 32], String> {
    let b = s.as_bytes();
    if b.len() != 64 {
        return Err(format!(
            "expected 64 hex chars for --trust, got {}",
            b.len()
        ));
    }
    let hex_val = |c: u8| -> Option<u8> {
        match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            b'A'..=b'F' => Some(c - b'A' + 10),
            _ => None,
        }
    };
    let mut out = [0u8; 32];
    for i in 0..32 {
        let hi = hex_val(b[i * 2]).ok_or_else(|| "invalid hex in --trust".to_string())?;
        let lo = hex_val(b[i * 2 + 1]).ok_or_else(|| "invalid hex in --trust".to_string())?;
        out[i] = (hi << 4) | lo;
    }
    Ok(out)
}

fn next_val<I: Iterator<Item = String>>(args: &mut I, flag: &str) -> String {
    args.next()
        .unwrap_or_else(|| fail(&format!("{flag} requires a value")))
}

fn cmd_pull<I: Iterator<Item = String>>(mut args: I, mut store: Option<PathBuf>) -> ! {
    let mut registry: Option<SocketAddr> = None;
    let mut trusted: Vec<[u8; 32]> = Vec::new();
    let mut target: Option<String> = None;

    while let Some(a) = args.next() {
        match a.as_str() {
            "--store" => store = Some(PathBuf::from(next_val(&mut args, "--store"))),
            "--registry" => {
                let v = next_val(&mut args, "--registry");
                registry = Some(
                    v.parse::<SocketAddr>()
                        .unwrap_or_else(|e| fail(&format!("bad --registry {v:?}: {e}"))),
                );
            }
            "--trust" => {
                let v = next_val(&mut args, "--trust");
                trusted.push(parse_hex32(&v).unwrap_or_else(|e| fail(&e)));
            }
            s if s.starts_with('-') => fail(&format!("unknown pull flag: {s}")),
            pos => {
                if target.is_some() {
                    fail("pull takes a single NAME[@REQ] argument");
                }
                target = Some(pos.to_string());
            }
        }
    }

    let reg = registry.unwrap_or_else(|| fail("pull requires --registry HOST:PORT"));
    if trusted.is_empty() {
        fail("pull requires at least one --trust HEX64");
    }
    let spec_str = target.unwrap_or_else(|| fail("pull requires NAME[@REQ]"));
    let (name, req_str) = spec_str.split_once('@').unwrap_or((&spec_str, "*"));
    let req = Req::parse(req_str).unwrap_or_else(|e| fail(&format!("bad version req: {e}")));
    let store_path = resolve_store(store);

    if let Err(e) = image::pull(reg, name, &req, &store_path, &trusted) {
        fail(&e.to_string());
    }
    exit(0);
}

struct RunOpts {
    store: Option<PathBuf>,
    image: Option<String>,
    name: Option<String>,
    detached: bool,
    rootfs_given: bool,
    spec: Spec,
    cgroup_base: PathBuf,
    supervise_id: Option<String>,
}

fn parse_run_opts<I: Iterator<Item = String>>(
    mut args: I,
    store: Option<PathBuf>,
    is_supervise: bool,
) -> RunOpts {
    let mut opts = RunOpts {
        store,
        image: None,
        name: None,
        detached: false,
        rootfs_given: false,
        spec: Spec::new("", Vec::new()),
        cgroup_base: PathBuf::from("/sys/fs/cgroup"),
        supervise_id: None,
    };

    while let Some(a) = args.next() {
        if a == "--" {
            opts.spec.argv = args.by_ref().collect();
            break;
        }
        match a.as_str() {
            "--store" => opts.store = Some(PathBuf::from(next_val(&mut args, "--store"))),
            "--image" if !is_supervise => opts.image = Some(next_val(&mut args, "--image")),
            "--name" if !is_supervise => opts.name = Some(next_val(&mut args, "--name")),
            "-d" if !is_supervise => opts.detached = true,
            "--rootfs" => {
                opts.rootfs_given = true;
                opts.spec.rootfs = PathBuf::from(next_val(&mut args, "--rootfs"));
            }
            "--hostname" => opts.spec.hostname = next_val(&mut args, "--hostname"),
            "--memory" => {
                let v = next_val(&mut args, "--memory");
                opts.spec.limits.memory_max =
                    parse_size(&v).unwrap_or_else(|e| fail(&e.to_string()));
            }
            "--pids" => {
                let v = next_val(&mut args, "--pids");
                opts.spec.limits.pids_max = parse_size(&v).unwrap_or_else(|e| fail(&e.to_string()));
            }
            "--cpu" => {
                let v = next_val(&mut args, "--cpu");
                let (q, p) = v
                    .split_once('/')
                    .unwrap_or_else(|| fail("--cpu needs QUOTA/PERIOD"));
                let num = |s: &str| {
                    s.parse::<u64>()
                        .unwrap_or_else(|_| fail("--cpu needs numbers"))
                };
                opts.spec.limits.cpu_max = Some((num(q), num(p)));
            }
            "--env" => {
                let v = next_val(&mut args, "--env");
                let (k, val) = v.split_once('=').unwrap_or_else(|| fail("--env needs K=V"));
                opts.spec.env.push((k.to_string(), val.to_string()));
            }
            "--cgroup-base" => {
                opts.cgroup_base = PathBuf::from(next_val(&mut args, "--cgroup-base"));
            }
            pos if is_supervise && !pos.starts_with('-') && opts.supervise_id.is_none() => {
                opts.supervise_id = Some(pos.to_string());
            }
            _ => fail(&format!("unexpected argument: {a}")),
        }
    }
    opts
}

fn spawn_supervisor(store: &Path, id: &str, opts: &RunOpts) -> ! {
    let exe = std::env::current_exe().unwrap_or_else(|e| fail(&e.to_string()));
    let mut cmd = Command::new(exe);
    cmd.arg("__supervise").arg("--store").arg(store);
    if opts.image.is_none() {
        cmd.arg("--rootfs").arg(&opts.spec.rootfs);
    }
    cmd.arg("--hostname").arg(&opts.spec.hostname);
    if let Some(m) = opts.spec.limits.memory_max {
        cmd.arg("--memory").arg(m.to_string());
    }
    if let Some(p) = opts.spec.limits.pids_max {
        cmd.arg("--pids").arg(p.to_string());
    }
    if let Some((q, p)) = opts.spec.limits.cpu_max {
        cmd.arg("--cpu").arg(format!("{q}/{p}"));
    }
    for (k, v) in &opts.spec.env {
        cmd.arg("--env").arg(format!("{k}={v}"));
    }
    cmd.arg("--cgroup-base").arg(&opts.cgroup_base);
    cmd.arg(id).arg("--").args(&opts.spec.argv);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    let mut child = cmd.spawn().unwrap_or_else(|e| fail(&e.to_string()));
    let err_file = state::container_dir(store, id).join("error");
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(st) = state::read(store, id) {
            if st.status != Status::Created {
                println!("{id}");
                exit(0);
            }
        }
        if let Ok(Some(_)) = child.try_wait() {
            let msg = fs::read_to_string(&err_file)
                .unwrap_or_else(|_| "detached container failed to start".to_string());
            let _ = fs::remove_file(&err_file);
            fail(msg.trim());
        }
        if Instant::now() >= deadline {
            fail("timed out waiting for container to start");
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn run_with_state(
    store_path: &Path,
    st: &mut state::ContainerState,
    spec: &Spec,
    cgroup_base: &Path,
    record_error: bool,
) -> ! {
    let res = pandora::run_with_pid(spec, cgroup_base, |pid| {
        st.status = Status::Running;
        st.pid = Some(pid);
        state::write(store_path, st)
    });
    match res {
        Ok(code) => {
            st.status = Status::Exited;
            st.exit = Some(code);
            let _ = state::write(store_path, st);
            exit(code);
        }
        Err(e) => {
            if record_error && st.status == Status::Created {
                let _ = fs::write(
                    state::container_dir(store_path, &st.id).join("error"),
                    e.to_string(),
                );
            }
            st.status = Status::Exited;
            st.exit = Some(125);
            let _ = state::write(store_path, st);
            fail(&e.to_string());
        }
    }
}

fn cmd_run<I: Iterator<Item = String>>(args: I, store: Option<PathBuf>) -> ! {
    let mut opts = parse_run_opts(args, store, false);
    if opts.image.is_some() && opts.rootfs_given {
        fail("cannot pass both --image and --rootfs");
    }

    let mut check = opts.spec.clone();
    if opts.image.is_some() {
        check.rootfs = PathBuf::from("/pending");
    }
    if let Err(e) = check.validate() {
        fail(&e.to_string());
    }

    if opts.image.is_some() || opts.detached || opts.name.is_some() {
        let store_path = resolve_store(opts.store.take());
        let id = opts.name.take().unwrap_or_else(state::generate_id);
        let img_label = opts.image.as_deref().unwrap_or("-");
        let mut st =
            state::create(&store_path, &id, img_label).unwrap_or_else(|e| fail(&e.to_string()));
        if let Some(ref img) = opts.image {
            let rootfs = state::rootfs_dir(&store_path, &id);
            if let Err(e) = image::build_rootfs(&store_path, img, &rootfs) {
                let _ = fs::remove_dir_all(state::container_dir(&store_path, &id));
                fail(&e.to_string());
            }
            opts.spec.rootfs = fs::canonicalize(&rootfs).unwrap_or(rootfs);
        } else if let Ok(canon) = fs::canonicalize(&opts.spec.rootfs) {
            opts.spec.rootfs = canon;
        }
        if opts.detached {
            spawn_supervisor(&store_path, &id, &opts);
        }
        run_with_state(&store_path, &mut st, &opts.spec, &opts.cgroup_base, false);
    }

    match pandora::run(&opts.spec, &opts.cgroup_base) {
        Ok(code) => exit(code),
        Err(e) => fail(&e.to_string()),
    }
}

fn cmd_supervise<I: Iterator<Item = String>>(args: I, store: Option<PathBuf>) -> ! {
    let mut opts = parse_run_opts(args, store, true);
    let store_path = resolve_store(opts.store.take());
    let id = opts
        .supervise_id
        .take()
        .unwrap_or_else(|| fail("__supervise requires ID"));
    let mut st = state::read(&store_path, &id).unwrap_or_else(|e| fail(&e.to_string()));
    if !opts.rootfs_given {
        let rootfs = state::rootfs_dir(&store_path, &id);
        opts.spec.rootfs = fs::canonicalize(&rootfs).unwrap_or(rootfs);
    }
    if let Err(e) = opts.spec.validate() {
        let _ = fs::write(
            state::container_dir(&store_path, &id).join("error"),
            e.to_string(),
        );
        fail(&e.to_string());
    }
    run_with_state(&store_path, &mut st, &opts.spec, &opts.cgroup_base, true);
}

fn cmd_ps<I: Iterator<Item = String>>(mut args: I, mut store: Option<PathBuf>) -> ! {
    while let Some(a) = args.next() {
        match a.as_str() {
            "--store" => store = Some(PathBuf::from(next_val(&mut args, "--store"))),
            _ => fail(&format!("unexpected ps argument: {a}")),
        }
    }
    let store_path = resolve_store(store);
    let list = state::list(&store_path).unwrap_or_else(|e| fail(&e.to_string()));
    println!("ID  IMAGE  STATUS");
    for c in list {
        println!("{}  {}  {}", c.id, c.image, c.ps_status());
    }
    exit(0);
}

fn cmd_kill<I: Iterator<Item = String>>(mut args: I, mut store: Option<PathBuf>) -> ! {
    let mut id: Option<String> = None;
    let mut sig_str: Option<String> = None;

    while let Some(a) = args.next() {
        match a.as_str() {
            "--store" => store = Some(PathBuf::from(next_val(&mut args, "--store"))),
            s if s.starts_with('-') => fail(&format!("unknown kill flag: {s}")),
            pos if id.is_none() => id = Some(pos.to_string()),
            pos if sig_str.is_none() => sig_str = Some(pos.to_string()),
            _ => fail("usage: pandora kill [--store DIR] ID [SIGNAL]"),
        }
    }

    let id = id.unwrap_or_else(|| fail("kill requires container ID"));
    let sig: i32 = match sig_str {
        Some(s) => {
            if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
                fail(&format!("signal must be numeric: {s:?}"));
            }
            let n: i32 = s
                .parse()
                .unwrap_or_else(|_| fail(&format!("bad signal number: {s:?}")));
            if n <= 0 {
                fail("signal must be positive");
            }
            n
        }
        None => 9,
    };

    let store_path = resolve_store(store);
    let st = state::read(&store_path, &id).unwrap_or_else(|e| fail(&e.to_string()));
    if st.status != Status::Running {
        fail(&format!("container '{id}' is not running"));
    }
    let pid = st
        .pid
        .unwrap_or_else(|| fail(&format!("container '{id}' has no recorded pid")));
    if let Err(e) = pandora::kill(pid, sig) {
        fail(&e.to_string());
    }
    exit(0);
}

fn cmd_rm<I: Iterator<Item = String>>(mut args: I, mut store: Option<PathBuf>) -> ! {
    let mut id: Option<String> = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--store" => store = Some(PathBuf::from(next_val(&mut args, "--store"))),
            s if s.starts_with('-') => fail(&format!("unknown rm flag: {s}")),
            pos if id.is_none() => id = Some(pos.to_string()),
            _ => fail("usage: pandora rm [--store DIR] ID"),
        }
    }
    let id = id.unwrap_or_else(|| fail("rm requires container ID"));
    let store_path = resolve_store(store);
    if let Err(e) = state::remove(&store_path, &id) {
        fail(&e.to_string());
    }
    exit(0);
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut store: Option<PathBuf> = None;
    let sub = loop {
        match args.next() {
            Some(a) if a == "--store" => {
                store = Some(PathBuf::from(next_val(&mut args, "--store")));
            }
            Some(a) if a == "--help" || a == "-h" => {
                println!("{USAGE}");
                exit(0)
            }
            Some(a) => break a,
            None => fail(USAGE),
        }
    };
    match sub.as_str() {
        "pull" => cmd_pull(args, store),
        "run" => cmd_run(args, store),
        "__supervise" => cmd_supervise(args, store),
        "ps" => cmd_ps(args, store),
        "kill" => cmd_kill(args, store),
        "rm" => cmd_rm(args, store),
        _ => fail(USAGE),
    }
}

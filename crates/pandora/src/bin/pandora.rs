//! `pandora run --rootfs DIR [--hostname H] [--memory 64M] [--pids N] [--cpu QUOTA/PERIOD] [--env K=V]...
//! [--cgroup-base DIR] -- CMD [ARGS...]`. Exits with the container's exit code, or 125 when pandora itself fails.

use std::path::PathBuf;
use std::process::exit;

use pandora::{parse_size, Spec};

const USAGE: &str = "usage: pandora run --rootfs DIR [--hostname H] [--memory SIZE] [--pids N] \
[--cpu QUOTA/PERIOD] [--env K=V]... [--cgroup-base DIR] -- CMD [ARGS...]";

fn fail(msg: &str) -> ! {
    eprintln!("pandora: {msg}");
    exit(125)
}

fn main() {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() != Some("run") {
        fail(USAGE);
    }
    let mut spec = Spec::new("", Vec::new());
    let mut base = PathBuf::from("/sys/fs/cgroup");
    while let Some(a) = args.next() {
        if a == "--" {
            spec.argv = args.by_ref().collect();
            break;
        }
        let v = args.next().unwrap_or_else(|| fail(USAGE));
        match a.as_str() {
            "--rootfs" => spec.rootfs = PathBuf::from(v),
            "--hostname" => spec.hostname = v,
            "--memory" => {
                spec.limits.memory_max = parse_size(&v).unwrap_or_else(|e| fail(&e.to_string()))
            }
            "--pids" => {
                spec.limits.pids_max = parse_size(&v).unwrap_or_else(|e| fail(&e.to_string()))
            }
            "--cpu" => {
                let (q, p) = v
                    .split_once('/')
                    .unwrap_or_else(|| fail("--cpu needs QUOTA/PERIOD"));
                let num = |s: &str| {
                    s.parse::<u64>()
                        .unwrap_or_else(|_| fail("--cpu needs numbers"))
                };
                spec.limits.cpu_max = Some((num(q), num(p)));
            }
            "--env" => {
                let (k, val) = v.split_once('=').unwrap_or_else(|| fail("--env needs K=V"));
                spec.env.push((k.to_string(), val.to_string()));
            }
            "--cgroup-base" => base = PathBuf::from(v),
            _ => fail(USAGE),
        }
    }
    if let Err(e) = spec.validate() {
        fail(&e.to_string());
    }
    match pandora::run(&spec, &base) {
        Ok(code) => exit(code),
        Err(e) => fail(&e.to_string()),
    }
}

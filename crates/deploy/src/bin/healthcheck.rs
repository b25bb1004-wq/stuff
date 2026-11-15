//! Container healthcheck probe binary (TCP and HTTP).

#![forbid(unsafe_code)]

use std::process;
use std::time::Duration;

use deploy::cli::healthcheck::{parse_args, Target};
use deploy::probe;

const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let target = match parse_args(&args) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{e}");
            process::exit(2);
        }
    };

    let res = match target {
        Target::Tcp { addr } => probe::tcp(&addr, PROBE_TIMEOUT),
        Target::Http { addr, path } => probe::http(&addr, &path, PROBE_TIMEOUT).map(|_| ()),
    };

    if let Err(e) = res {
        eprintln!("{e}");
        process::exit(1);
    }
}

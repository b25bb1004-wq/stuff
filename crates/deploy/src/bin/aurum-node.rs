//! Aurum validator node binary.

#![forbid(unsafe_code)]

use std::process;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use deploy::cli::aurum::parse_args;
use deploy::{run_aurum, shutdown, AurumConfig};

fn main() {
    shutdown::install();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let opts = match parse_args(&args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{e}");
            process::exit(2);
        }
    };

    let cfg = AurumConfig {
        id: opts.id,
        listen: opts.listen,
        health: opts.health,
        peers: opts.peers,
        validators: opts.validators,
    };

    let stop = AtomicBool::new(false);
    let finished = AtomicBool::new(false);
    let result = thread::scope(|s| {
        let handle = s.spawn(|| {
            let res = run_aurum(&cfg, &stop, |addr| {
                println!("listening on {addr}");
            });
            finished.store(true, Ordering::Release);
            res
        });

        while !finished.load(Ordering::Acquire) {
            if shutdown::requested() {
                stop.store(true, Ordering::Release);
                break;
            }
            thread::sleep(Duration::from_millis(25));
        }

        handle.join()
    });

    match result {
        Ok(Ok(())) => println!("stopped"),
        Ok(Err(e)) => {
            eprintln!("{e}");
            process::exit(1);
        }
        Err(_) => {
            eprintln!("service thread panicked");
            process::exit(1);
        }
    }
}

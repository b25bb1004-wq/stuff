//! Pure command-line flag parsers for all deploy binaries.

#![forbid(unsafe_code)]

use std::path::PathBuf;

/// CLI parsing for `iridium-server`.
pub mod iridium {
    use super::*;

    /// Parsed options for `iridium-server`.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Options {
        /// TCP listen address.
        pub listen: String,
        /// Directory for database files.
        pub data: PathBuf,
    }

    /// Parse `iridium-server --listen ADDR --data DIR` arguments (excluding argv[0]).
    pub fn parse_args(args: &[String]) -> Result<Options, String> {
        let mut listen: Option<String> = None;
        let mut data: Option<PathBuf> = None;
        let mut i = 0usize;
        while i < args.len() {
            match args[i].as_str() {
                "--listen" => {
                    let val = next_val(args, &mut i, "--listen")?;
                    if listen.is_some() {
                        return Err("duplicate --listen".to_string());
                    }
                    listen = Some(val.to_string());
                }
                "--data" => {
                    let val = next_val(args, &mut i, "--data")?;
                    if data.is_some() {
                        return Err("duplicate --data".to_string());
                    }
                    data = Some(PathBuf::from(val));
                }
                other => return Err(format!("unknown argument: {other}")),
            }
            i += 1;
        }
        Ok(Options {
            listen: listen.ok_or_else(|| "missing --listen".to_string())?,
            data: data.ok_or_else(|| "missing --data".to_string())?,
        })
    }
}

/// CLI parsing for `argo-registry`.
pub mod registry {
    use super::*;

    /// Parsed options for `argo-registry`.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Options {
        /// HTTP listen address.
        pub listen: String,
        /// Trusted publisher Ed25519 public keys.
        pub publishers: Vec<[u8; 32]>,
    }

    /// Parse `argo-registry --listen ADDR [--publisher HEX64]...` arguments (excluding argv[0]).
    pub fn parse_args(args: &[String]) -> Result<Options, String> {
        let mut listen: Option<String> = None;
        let mut publishers: Vec<[u8; 32]> = Vec::new();
        let mut i = 0usize;
        while i < args.len() {
            match args[i].as_str() {
                "--listen" => {
                    let val = next_val(args, &mut i, "--listen")?;
                    if listen.is_some() {
                        return Err("duplicate --listen".to_string());
                    }
                    listen = Some(val.to_string());
                }
                "--publisher" => {
                    let val = next_val(args, &mut i, "--publisher")?;
                    publishers.push(decode_hex32(val)?);
                }
                other => return Err(format!("unknown argument: {other}")),
            }
            i += 1;
        }
        Ok(Options {
            listen: listen.ok_or_else(|| "missing --listen".to_string())?,
            publishers,
        })
    }
}

/// CLI parsing for `aurum-node`.
pub mod aurum {
    use super::*;

    /// Parsed options for `aurum-node`.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Options {
        /// Validator identifier.
        pub id: u8,
        /// UDP listen address.
        pub listen: String,
        /// HTTP health listen address.
        pub health: String,
        /// Total validator count.
        pub validators: u8,
        /// Remote peers as `(id, host:port)`.
        pub peers: Vec<(u8, String)>,
    }

    /// Parse `aurum-node --id N --listen ADDR --health ADDR --validators N [--peer ID@HOST:PORT]...`.
    pub fn parse_args(args: &[String]) -> Result<Options, String> {
        let mut id: Option<u8> = None;
        let mut listen: Option<String> = None;
        let mut health: Option<String> = None;
        let mut validators: Option<u8> = None;
        let mut peers: Vec<(u8, String)> = Vec::new();

        let mut i = 0usize;
        while i < args.len() {
            match args[i].as_str() {
                "--id" => {
                    let val = next_val(args, &mut i, "--id")?;
                    if id.is_some() {
                        return Err("duplicate --id".to_string());
                    }
                    let parsed: u8 = val.parse().map_err(|_| format!("invalid --id: {val}"))?;
                    id = Some(parsed);
                }
                "--listen" => {
                    let val = next_val(args, &mut i, "--listen")?;
                    if listen.is_some() {
                        return Err("duplicate --listen".to_string());
                    }
                    listen = Some(val.to_string());
                }
                "--health" => {
                    let val = next_val(args, &mut i, "--health")?;
                    if health.is_some() {
                        return Err("duplicate --health".to_string());
                    }
                    health = Some(val.to_string());
                }
                "--validators" => {
                    let val = next_val(args, &mut i, "--validators")?;
                    if validators.is_some() {
                        return Err("duplicate --validators".to_string());
                    }
                    let parsed: u8 = val
                        .parse()
                        .map_err(|_| format!("invalid --validators: {val}"))?;
                    validators = Some(parsed);
                }
                "--peer" => {
                    let val = next_val(args, &mut i, "--peer")?;
                    peers.push(parse_peer_spec(val)?);
                }
                other => return Err(format!("unknown argument: {other}")),
            }
            i += 1;
        }

        let id = id.ok_or_else(|| "missing --id".to_string())?;
        let validators = validators.ok_or_else(|| "missing --validators".to_string())?;
        if validators == 0 {
            return Err("--validators must be >= 1".to_string());
        }
        if id == 0 || id > validators {
            return Err(format!("--id {id} must be in 1..={validators}"));
        }

        Ok(Options {
            id,
            listen: listen.ok_or_else(|| "missing --listen".to_string())?,
            health: health.ok_or_else(|| "missing --health".to_string())?,
            validators,
            peers,
        })
    }

    fn parse_peer_spec(spec: &str) -> Result<(u8, String), String> {
        let (id_str, addr) = spec
            .split_once('@')
            .ok_or_else(|| format!("invalid --peer (expected ID@HOST:PORT): {spec}"))?;
        let pid: u8 = id_str
            .parse()
            .map_err(|_| format!("invalid peer id in --peer: {spec}"))?;
        if pid == 0 {
            return Err(format!("peer id must be >= 1: {spec}"));
        }
        let (host, port_str) = addr
            .rsplit_once(':')
            .ok_or_else(|| format!("invalid peer address (expected HOST:PORT): {spec}"))?;
        if host.is_empty() || port_str.parse::<u16>().is_err() {
            return Err(format!("invalid peer address (expected HOST:PORT): {spec}"));
        }
        Ok((pid, addr.to_string()))
    }
}

/// CLI parsing for `healthcheck`.
pub mod healthcheck {
    /// Parsed healthcheck probe target.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum Target {
        /// TCP connectivity probe.
        Tcp {
            /// Target `HOST:PORT`.
            addr: String,
        },
        /// HTTP GET probe expecting status 200.
        Http {
            /// Target `HOST:PORT`.
            addr: String,
            /// Request path (e.g. `"/health"`).
            path: String,
        },
    }

    /// Parse `healthcheck tcp ADDR` or `healthcheck http ADDR PATH` (excluding argv[0]).
    pub fn parse_args(args: &[String]) -> Result<Target, String> {
        match args {
            [mode, addr] if mode == "tcp" => {
                if addr.is_empty() || addr.starts_with('-') {
                    return Err(format!("invalid addr: {addr}"));
                }
                Ok(Target::Tcp { addr: addr.clone() })
            }
            [mode, addr, path] if mode == "http" => {
                if addr.is_empty() || addr.starts_with('-') {
                    return Err(format!("invalid addr: {addr}"));
                }
                if path.is_empty() {
                    return Err("empty http path".to_string());
                }
                Ok(Target::Http {
                    addr: addr.clone(),
                    path: path.clone(),
                })
            }
            _ => Err("usage: healthcheck tcp ADDR | healthcheck http ADDR PATH".to_string()),
        }
    }
}

/// CLI parsing for `monolith-web`.
pub mod web {
    use super::*;

    const DEFAULT_LISTEN: &str = "0.0.0.0:8080";
    const DEFAULT_ROOT: &str = "/www";

    /// Parsed options for `monolith-web`.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Options {
        /// HTTP listen address.
        pub listen: String,
        /// Directory containing the packed site and Athena bundle.
        pub root: PathBuf,
    }

    /// Parse `monolith-web [--listen ADDR] [--root DIR]` arguments (excluding argv[0]).
    pub fn parse_args(args: &[String]) -> Result<Options, String> {
        let mut listen: Option<String> = None;
        let mut root: Option<PathBuf> = None;
        let mut i = 0usize;
        while i < args.len() {
            match args[i].as_str() {
                "--listen" => {
                    let val = next_val(args, &mut i, "--listen")?;
                    if listen.is_some() {
                        return Err("duplicate --listen".to_string());
                    }
                    listen = Some(val.to_string());
                }
                "--root" => {
                    let val = next_val(args, &mut i, "--root")?;
                    if root.is_some() {
                        return Err("duplicate --root".to_string());
                    }
                    root = Some(PathBuf::from(val));
                }
                other => return Err(format!("unknown argument: {other}")),
            }
            i += 1;
        }
        Ok(Options {
            listen: listen.unwrap_or_else(|| DEFAULT_LISTEN.to_string()),
            root: root.unwrap_or_else(|| PathBuf::from(DEFAULT_ROOT)),
        })
    }
}

fn next_val<'a>(args: &'a [String], idx: &mut usize, flag: &str) -> Result<&'a str, String> {
    let next = idx
        .checked_add(1)
        .ok_or_else(|| format!("missing value for {flag}"))?;
    let val = args
        .get(next)
        .ok_or_else(|| format!("missing value for {flag}"))?;
    if val.is_empty() {
        return Err(format!("empty value for {flag}"));
    }
    *idx = next;
    Ok(val.as_str())
}

fn decode_hex32(s: &str) -> Result<[u8; 32], String> {
    if s.len() != 64 {
        return Err(format!("expected 64 hex chars, got {}", s.len()));
    }
    let bytes = s.as_bytes();
    let mut out = [0u8; 32];
    for (i, slot) in out.iter_mut().enumerate() {
        let hi = hex_val(bytes[i * 2])?;
        let lo = hex_val(bytes[i * 2 + 1])?;
        *slot = (hi << 4) | lo;
    }
    Ok(out)
}

fn hex_val(b: u8) -> Result<u8, String> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err("invalid hex character".to_string()),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn prop_hex32_roundtrip_and_cli_parsers_never_panic() {
        assay::forall(300, 0x77_00_02, |rng| {
            let mut key = [0u8; 32];
            rng.fill(&mut key);
            let encoded = assay::to_hex(&key);
            assert_eq!(decode_hex32(&encoded).unwrap(), key);

            let count = rng.below(8);
            let mut args = Vec::with_capacity(count);
            for _ in 0..count {
                let raw = rng.bytes(24);
                let s: String = raw
                    .into_iter()
                    .map(|b| char::from(b'!' + (b % 90)))
                    .collect();
                args.push(s);
            }
            let _ = iridium::parse_args(&args);
            let _ = registry::parse_args(&args);
            let _ = aurum::parse_args(&args);
            let _ = healthcheck::parse_args(&args);
            let _ = web::parse_args(&args);
        });
    }

    #[test]
    fn web_cli_defaults_and_overrides() {
        let def = web::parse_args(&[]).unwrap();
        assert_eq!(def.listen, "0.0.0.0:8080");
        assert_eq!(def.root, PathBuf::from("/www"));

        let custom = web::parse_args(&[
            "--listen".to_string(),
            "127.0.0.1:9090".to_string(),
            "--root".to_string(),
            "/tmp/bundle".to_string(),
        ])
        .unwrap();
        assert_eq!(custom.listen, "127.0.0.1:9090");
        assert_eq!(custom.root, PathBuf::from("/tmp/bundle"));

        assert!(web::parse_args(&["--listen".to_string()]).is_err());
        assert!(web::parse_args(&[
            "--listen".to_string(),
            "a".to_string(),
            "--listen".to_string(),
            "b".to_string(),
        ])
        .is_err());
        assert!(web::parse_args(&["--bogus".to_string()]).is_err());
    }
}

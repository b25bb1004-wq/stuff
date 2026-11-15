//! Xenon encrypted reliable transport over an in-memory lossy link demo.

use std::collections::VecDeque;
use std::fmt::Write as _;

use xenon::{Config, Connection, Server};

use super::{check_input, Demo, Output};

const MAX_LINES: usize = 64;
const MAX_MESSAGES: usize = 32;
const MAX_DELAY_MS: u64 = 2_000;
const MAX_STEPS: usize = 20_000;
const MAX_SIM_US: u64 = 30_000_000;

const SERVER_SECRET: [u8; 32] = [0x11; 32];
const SERVER_SEED: [u8; 32] = [0x22; 32];
const CLIENT_SEED: [u8; 32] = [0x33; 32];
const LINK_RNG_SEED: u64 = 1;

const DEFAULT_SCRIPT: &str = "\
loss 20
delay 20
send hello xenon
send reliable over udp
send encrypted frames
send out of order recovery
send verified stream";

/// Registered Xenon transport demo.
pub const DEMO: Demo = Demo {
    id: "xenon",
    title: "Xenon Transport",
    default_input: DEFAULT_SCRIPT,
    run,
};

#[derive(Clone, Copy)]
enum Dir {
    C2S,
    S2C,
}

struct InFlight {
    deliver_at: u64,
    dir: Dir,
    pkt: Vec<u8>,
}

struct Link {
    loss_pct: u32,
    delay_us: u64,
    rng: u64,
    sent: u64,
    dropped: u64,
    queue: VecDeque<InFlight>,
}

impl Link {
    fn new(loss_pct: u32, delay_ms: u64) -> Self {
        Self {
            loss_pct,
            delay_us: delay_ms.saturating_mul(1_000),
            rng: LINK_RNG_SEED,
            sent: 0,
            dropped: 0,
            queue: VecDeque::new(),
        }
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.rng.wrapping_add(0x9e37_79b9_7f4a_7c15);
        self.rng = x;
        x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        x ^ (x >> 31)
    }

    fn send(&mut self, dir: Dir, pkt: Vec<u8>, now: u64) {
        self.sent = self.sent.saturating_add(1);
        if self.loss_pct > 0 && ((self.next_u64() >> 32) % 100) < u64::from(self.loss_pct) {
            self.dropped = self.dropped.saturating_add(1);
            return;
        }
        let deliver_at = now.saturating_add(self.delay_us);
        let pos = self
            .queue
            .iter()
            .position(|item| item.deliver_at > deliver_at)
            .unwrap_or(self.queue.len());
        self.queue.insert(
            pos,
            InFlight {
                deliver_at,
                dir,
                pkt,
            },
        );
    }

    fn next_arrival(&self) -> Option<u64> {
        self.queue.front().map(|p| p.deliver_at)
    }
}

/// Simulate encrypted reliable stream delivery over an in-memory lossy link.
pub fn run(input: &str) -> Result<Output, String> {
    check_input(input)?;
    let (loss_pct, delay_ms, messages) = parse_script(input)?;

    let cfg = Config::default();
    let mut server = Server::new(SERVER_SECRET, SERVER_SEED, cfg);
    let mut client = Connection::connect(server.public_key(), CLIENT_SEED, cfg, 0);
    let mut link = Link::new(loss_pct, delay_ms);

    let mut now: u64 = 0;
    let mut server_cid: Option<u64> = None;
    let mut next_msg: usize = 0;
    let mut recv_bufs: Vec<(u64, Vec<u8>, bool)> = Vec::new();
    let mut delivered: Vec<(u64, String)> = Vec::new();

    for _ in 0..MAX_STEPS {
        while let Some(pkt) = client.poll_transmit(now) {
            link.send(Dir::C2S, pkt, now);
        }
        if let Some(cid) = server_cid {
            if let Some(sc) = server.connection(cid) {
                while let Some(pkt) = sc.poll_transmit(now) {
                    link.send(Dir::S2C, pkt, now);
                }
            }
        }

        if let Some(err) = client.error() {
            return Err(format!("client transport failed: {err}"));
        }

        if client.is_connected() && next_msg < messages.len() {
            while let Some(msg) = messages.get(next_msg) {
                let sid = client
                    .open_stream()
                    .map_err(|e| format!("open_stream failed: {e}"))?;
                let bytes = msg.as_bytes();
                let written = client
                    .write(sid, bytes)
                    .map_err(|e| format!("write failed: {e}"))?;
                if written != bytes.len() {
                    return Err(String::from("stream write truncated"));
                }
                client
                    .finish(sid)
                    .map_err(|e| format!("finish failed: {e}"))?;
                next_msg += 1;
                while let Some(pkt) = client.poll_transmit(now) {
                    link.send(Dir::C2S, pkt, now);
                }
            }
        }

        if let Some(cid) = server_cid {
            if let Some(sc) = server.connection(cid) {
                while let Some(sid) = sc.accept_stream() {
                    recv_bufs.push((sid, Vec::new(), false));
                }
                for (sid, buf, done) in &mut recv_bufs {
                    if *done {
                        continue;
                    }
                    let mut tmp = [0u8; 512];
                    loop {
                        let n = sc.read(*sid, &mut tmp).unwrap_or(0);
                        if n == 0 {
                            break;
                        }
                        buf.extend_from_slice(&tmp[..n]);
                    }
                    if sc.stream_finished(*sid) {
                        *done = true;
                    }
                }
            }
        }

        if recv_bufs.len() == messages.len() && recv_bufs.iter().all(|(_, _, d)| *d) {
            for (sid, buf, _) in &recv_bufs {
                delivered.push((*sid, String::from_utf8_lossy(buf).into_owned()));
            }
            break;
        }

        let srv_timeout = server_cid
            .and_then(|cid| server.connection(cid))
            .and_then(|c| c.timeout());
        let next_t = [link.next_arrival(), client.timeout(), srv_timeout]
            .into_iter()
            .flatten()
            .min();
        let Some(t) = next_t else {
            return Err(String::from("simulation stalled with no pending timers"));
        };
        if t > MAX_SIM_US {
            return Err(String::from("simulation exceeded 30s virtual time limit"));
        }
        now = t.max(now);

        client.on_timeout(now);
        if let Some(cid) = server_cid {
            if let Some(sc) = server.connection(cid) {
                sc.on_timeout(now);
            }
        }

        while link.queue.front().is_some_and(|p| p.deliver_at <= now) {
            if let Some(item) = link.queue.pop_front() {
                match item.dir {
                    Dir::C2S => {
                        if let Some(cid) = server.handle(&item.pkt, now) {
                            if server.accept() == Some(cid) || server_cid.is_none() {
                                server_cid = Some(cid);
                            }
                        }
                    }
                    Dir::S2C => {
                        client.handle(&item.pkt, now);
                    }
                }
            }
        }
    }

    if delivered.len() != messages.len() {
        return Err(format!(
            "transfer incomplete: delivered {} of {} messages",
            delivered.len(),
            messages.len()
        ));
    }

    let c_stats = client.stats();
    let s_stats = server_cid
        .and_then(|cid| server.connection(cid))
        .map(|c| c.stats())
        .unwrap_or_default();
    let retransmits = c_stats.lost_packets.saturating_add(s_stats.lost_packets);

    let mut out = String::new();
    let _ = writeln!(
        out,
        "link: loss={loss_pct}% delay={delay_ms}ms elapsed={}ms",
        now / 1_000
    );
    out.push_str("=== delivered in order ===\n");
    for (sid, text) in &delivered {
        let _ = writeln!(out, "  [stream {sid}] {text}");
    }
    out.push_str("=== stats ===\n");
    let _ = writeln!(out, "retransmits: {retransmits}");
    let _ = writeln!(
        out,
        "link_packets: sent={} dropped={}",
        link.sent, link.dropped
    );
    let _ = writeln!(
        out,
        "client: sent={} lost={} cwnd={} srtt={}us",
        c_stats.sent_packets, c_stats.lost_packets, c_stats.cwnd, c_stats.srtt_us
    );
    let _ = writeln!(
        out,
        "server: sent={} lost={} cwnd={} srtt={}us",
        s_stats.sent_packets, s_stats.lost_packets, s_stats.cwnd, s_stats.srtt_us
    );
    Ok(Output::Text(out))
}

fn parse_script(input: &str) -> Result<(u32, u64, Vec<String>), String> {
    let mut loss_pct: u32 = 20;
    let mut delay_ms: u64 = 20;
    let mut messages = Vec::new();
    let mut lines_seen: usize = 0;

    for (idx, raw) in input.lines().enumerate() {
        let line_no = idx + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        lines_seen += 1;
        if lines_seen > MAX_LINES {
            return Err(format!("line {line_no}: exceeds {MAX_LINES} lines"));
        }

        if let Some(rest) = line.strip_prefix("loss") {
            if rest.is_empty() || !rest.starts_with(char::is_whitespace) {
                return Err(format!("line {line_no}: expected `loss P`"));
            }
            let p = rest
                .trim()
                .parse::<u32>()
                .map_err(|_| format!("line {line_no}: invalid loss `{}`", rest.trim()))?;
            if p > 100 {
                return Err(format!("line {line_no}: loss must be 0..=100, got {p}"));
            }
            loss_pct = p;
        } else if let Some(rest) = line.strip_prefix("delay") {
            if rest.is_empty() || !rest.starts_with(char::is_whitespace) {
                return Err(format!("line {line_no}: expected `delay N`"));
            }
            let d = rest
                .trim()
                .parse::<u64>()
                .map_err(|_| format!("line {line_no}: invalid delay `{}`", rest.trim()))?;
            if d > MAX_DELAY_MS {
                return Err(format!(
                    "line {line_no}: delay must be 0..={MAX_DELAY_MS} ms"
                ));
            }
            delay_ms = d;
        } else if let Some(rest) = line.strip_prefix("send") {
            if rest.is_empty() || !rest.starts_with(char::is_whitespace) {
                return Err(format!("line {line_no}: expected `send TEXT`"));
            }
            let text = rest.trim();
            if text.is_empty() {
                return Err(format!("line {line_no}: empty message in `send`"));
            }
            if messages.len() >= MAX_MESSAGES {
                return Err(format!("line {line_no}: exceeds {MAX_MESSAGES} messages"));
            }
            messages.push(text.to_owned());
        } else {
            return Err(format!(
                "line {line_no}: expected `loss P`, `delay N`, or `send TEXT`"
            ));
        }
    }

    if messages.is_empty() {
        return Err(String::from("expected at least one `send TEXT` line"));
    }
    Ok((loss_pct, delay_ms, messages))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn lossy_link_delivers_all_and_reports_retransmits() {
        let Output::Text(txt) = run(DEFAULT_SCRIPT).unwrap() else {
            panic!("expected text");
        };
        assert!(txt.contains("[stream 0] hello xenon"));
        assert!(txt.contains("[stream 2] reliable over udp"));
        assert!(txt.contains("[stream 4] encrypted frames"));
        let retx_line = txt
            .lines()
            .find(|l| l.starts_with("retransmits: "))
            .unwrap();
        let retx: u64 = retx_line
            .strip_prefix("retransmits: ")
            .unwrap()
            .parse()
            .unwrap();
        assert!(
            retx > 0,
            "expected retransmits > 0 under 20% loss, got:\n{txt}"
        );
    }
}

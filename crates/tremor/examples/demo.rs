//! Demo: a Tremor client and server in one process, passing datagrams by hand (no sockets, no clock).
use tremor::{Config, Connection, Server};

const SERVER_SECRET: [u8; 32] = [7u8; 32];
const SERVER_SEED: [u8; 32] = [9u8; 32];
const CLIENT_SEED: [u8; 32] = [3u8; 32];

fn main() {
    let config = Config::default();
    let mut server = Server::new(SERVER_SECRET, SERVER_SEED, config);
    let server_key = server.public_key();
    let mut client = Connection::connect(server_key, CLIENT_SEED, config, 0);
    let mut now: u64 = 0;

    // Pump datagrams between the two ends in both directions, one simulated millisecond per round.
    let mut rounds = 0u32;
    while !client.is_connected() && rounds < 100 {
        pump(&mut client, &mut server, now);
        now += 1_000;
        rounds += 1;
    }
    println!(
        "handshake connected: {} after {} rounds",
        client.is_connected(),
        rounds
    );
    // The server finishes its side one round after the client sees the handshake complete.
    for _ in 0..5 {
        pump(&mut client, &mut server, now);
        now += 1_000;
    }
    let Some(cid) = server.accept() else {
        println!("server did not accept a connection");
        return;
    };
    println!("server accepted connection {cid}");

    let Ok(sid) = client.open_stream() else {
        println!("client could not open a stream");
        return;
    };
    let message = b"hello over tremor";
    let written = client.write(sid, message).unwrap_or(0);
    let _ = client.finish(sid);
    println!(
        "client wrote {written} of {} bytes on stream {sid}",
        message.len()
    );

    let mut echoed = Vec::new();
    let mut buf = [0u8; 64];
    for _ in 0..100 {
        pump(&mut client, &mut server, now);
        now += 1_000;
        if let Some(conn) = server.connection(cid) {
            while let Some(id) = conn.accept_stream() {
                println!("server saw stream {id}");
                let mut got = Vec::new();
                loop {
                    match conn.read(id, &mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => got.extend_from_slice(&buf[..n]),
                    }
                }
                println!(
                    "server read {} bytes: {:?}",
                    got.len(),
                    String::from_utf8_lossy(&got)
                );
                let _ = conn.write(id, &got);
                let _ = conn.finish(id);
            }
        }
        pump(&mut client, &mut server, now);
        now += 1_000;
        while let Ok(n) = client.read(sid, &mut buf) {
            if n == 0 {
                break;
            }
            echoed.extend_from_slice(&buf[..n]);
        }
        if client.stream_finished(sid) {
            break;
        }
    }
    println!(
        "client read back {} bytes: {:?}",
        echoed.len(),
        String::from_utf8_lossy(&echoed)
    );
    println!("echo matches: {}", echoed == message);
    let stats = client.stats();
    println!(
        "client sent {} packets, cwnd {} bytes",
        stats.sent_packets, stats.cwnd
    );
}

/// Move every pending datagram from each side to the other once.
fn pump(client: &mut Connection, server: &mut Server, now: u64) {
    while let Some(d) = client.poll_transmit(now) {
        server.handle(&d, now);
    }
    for cid in server.cids() {
        if let Some(conn) = server.connection(cid) {
            while let Some(d) = conn.poll_transmit(now) {
                client.handle(&d, now);
            }
        }
    }
}

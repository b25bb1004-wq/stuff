//! Threaded HTTP/1.1 server on [`geyser::pool::Pool`] with keep-alive, routing, and static files.

mod conn;
mod router;
mod static_files;

use std::collections::HashMap;
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::string::{String, ToString};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::SeqCst};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use geyser::pool::Pool;

use crate::Limits;
pub use router::{Handler, Router};

/// Configuration for [`Server::start`].
#[derive(Debug, Clone)]
pub struct Config {
    /// Socket address to bind (e.g., `"127.0.0.1:0"`).
    pub addr: String,
    /// Number of worker threads in the `geyser` pool.
    pub threads: usize,
    /// HTTP/1.1 parser limits applied to each connection.
    pub limits: Limits,
    /// Per-read/write socket timeout in milliseconds.
    pub read_timeout_ms: u64,
    /// Maximum requests served on a single keep-alive connection.
    pub keep_alive_max: u32,
    /// Maximum concurrent open connections before replying `503`.
    pub max_connections: usize,
}

impl Default for Config {
    /// Default configuration: `"127.0.0.1:0"`, 8 threads, default limits, 5000 ms, 100, 1024.
    fn default() -> Self {
        Self {
            addr: "127.0.0.1:0".to_string(),
            threads: 8,
            limits: Limits::default(),
            read_timeout_ms: 5000,
            keep_alive_max: 100,
            max_connections: 1024,
        }
    }
}

/// Live connection and request counters reported by [`Server::stats`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ServerStats {
    /// Total TCP connections accepted by the listener.
    pub accepted: u64,
    /// Total HTTP requests parsed and dispatched.
    pub requests: u64,
    /// Currently open connection jobs.
    pub open: u64,
    /// Connections rejected with `503` because `max_connections` was reached.
    pub rejected: u64,
}

pub(crate) struct ActiveConn {
    pub(crate) stream: TcpStream,
    pub(crate) in_flight: Arc<AtomicBool>,
}

pub(crate) struct ServerInner {
    pub(crate) router: Router,
    pub(crate) limits: Limits,
    pub(crate) read_timeout: Duration,
    pub(crate) keep_alive_max: u32,
    pub(crate) max_connections: usize,
    pub(crate) stopping: AtomicBool,
    pub(crate) accepted: AtomicU64,
    pub(crate) requests: AtomicU64,
    pub(crate) open: AtomicU64,
    pub(crate) rejected: AtomicU64,
    pub(crate) next_conn_id: AtomicU64,
    pub(crate) active: Mutex<HashMap<u64, ActiveConn>>,
}

/// Running HTTP/1.1 server. Dropping it stops the server gracefully.
pub struct Server {
    local_addr: SocketAddr,
    inner: Arc<ServerInner>,
    pool: Option<Arc<Pool>>,
    acceptor: Option<JoinHandle<()>>,
}

impl Server {
    /// Bind `cfg.addr`, start the worker pool and acceptor thread, and return the running server.
    pub fn start(cfg: Config, router: Router) -> io::Result<Server> {
        let listener = TcpListener::bind(&cfg.addr)?;
        let local_addr = listener.local_addr()?;
        let pool = Arc::new(Pool::new(cfg.threads.max(1)));
        let inner = Arc::new(ServerInner {
            router,
            limits: cfg.limits,
            read_timeout: Duration::from_millis(cfg.read_timeout_ms.max(1)),
            keep_alive_max: cfg.keep_alive_max,
            max_connections: cfg.max_connections,
            stopping: AtomicBool::new(false),
            accepted: AtomicU64::new(0),
            requests: AtomicU64::new(0),
            open: AtomicU64::new(0),
            rejected: AtomicU64::new(0),
            next_conn_id: AtomicU64::new(1),
            active: Mutex::new(HashMap::new()),
        });

        let accept_inner = Arc::clone(&inner);
        let accept_pool = Arc::clone(&pool);
        let acceptor = thread::Builder::new()
            .name("outcrop-accept".to_string())
            .spawn(move || accept_loop(listener, accept_inner, accept_pool))?;

        Ok(Server {
            local_addr,
            inner,
            pool: Some(pool),
            acceptor: Some(acceptor),
        })
    }

    /// Return the bound local socket address.
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Snapshot the current server counters.
    pub fn stats(&self) -> ServerStats {
        ServerStats {
            accepted: self.inner.accepted.load(SeqCst),
            requests: self.inner.requests.load(SeqCst),
            open: self.inner.open.load(SeqCst),
            rejected: self.inner.rejected.load(SeqCst),
        }
    }

    /// Stop accepting, finish in-flight requests, close idle connections, and join all threads.
    pub fn stop(mut self) {
        self.stop_inner();
    }

    fn stop_inner(&mut self) {
        self.inner.stopping.store(true, SeqCst);
        if let Some(acceptor) = self.acceptor.take() {
            let wake_addr = connect_addr(self.local_addr);
            let _ = TcpStream::connect_timeout(&wake_addr, Duration::from_millis(200));
            let _ = acceptor.join();
        }
        {
            let active = lock(&self.inner.active);
            for conn in active.values() {
                if !conn.in_flight.load(SeqCst) {
                    let _ = conn.stream.shutdown(Shutdown::Both);
                }
            }
        }
        if let Some(pool) = self.pool.take() {
            if let Ok(owned) = Arc::try_unwrap(pool) {
                owned.shutdown();
            }
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop_inner();
    }
}

fn accept_loop(listener: TcpListener, inner: Arc<ServerInner>, pool: Arc<Pool>) {
    while !inner.stopping.load(SeqCst) {
        let (stream, _) = match listener.accept() {
            Ok(pair) => pair,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        };
        if inner.stopping.load(SeqCst) {
            break;
        }
        inner.accepted.fetch_add(1, SeqCst);
        let current_open = inner.open.load(SeqCst);
        if current_open >= inner.max_connections as u64 {
            inner.rejected.fetch_add(1, SeqCst);
            conn::reject_503(stream);
            continue;
        }
        inner.open.fetch_add(1, SeqCst);
        let conn_inner = Arc::clone(&inner);
        pool.spawn(move || conn::handle_connection(stream, conn_inner));
    }
}

fn connect_addr(addr: SocketAddr) -> SocketAddr {
    match addr.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => {
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), addr.port())
        }
        IpAddr::V6(ip) if ip.is_unspecified() => {
            SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), addr.port())
        }
        _ => addr,
    }
}

pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

//! Multi-threaded TCP server for a single Iridium database.

use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::net::codec::{encode_rows, read_frame, write_frame};
use crate::sql::{Db, Rows};
use crate::FileStorage;

const BUSY_WAIT: Duration = Duration::from_secs(2);

struct DbState {
    db: Option<Db<FileStorage>>,
    owner: Option<u64>,
    stopped: bool,
}

struct Shared {
    state: Mutex<DbState>,
    cvar: Condvar,
    stopping: AtomicBool,
    next_conn_id: AtomicU64,
    clients: Mutex<Vec<(u64, TcpStream)>>,
    client_threads: Mutex<Vec<JoinHandle<()>>>,
}

enum ExecOutcome {
    Reply(Result<Rows, String>),
    Busy,
    Stopped,
}

/// Multi-threaded TCP server exposing a single Iridium database.
pub struct Server {
    local_addr: SocketAddr,
    shared: Arc<Shared>,
    accept_thread: Option<JoinHandle<()>>,
}

impl Server {
    /// Open `main` and `log` files, bind `addr`, and start accepting connections.
    pub fn start(addr: SocketAddr, main: PathBuf, log: PathBuf) -> io::Result<Server> {
        let main_store = FileStorage::open(&main).map_err(varve_err_to_io)?;
        let log_store = FileStorage::open(&log).map_err(varve_err_to_io)?;
        let db = Db::open(main_store, log_store).map_err(varve_err_to_io)?;

        let listener = TcpListener::bind(addr)?;
        let local_addr = listener.local_addr()?;

        let shared = Arc::new(Shared {
            state: Mutex::new(DbState {
                db: Some(db),
                owner: None,
                stopped: false,
            }),
            cvar: Condvar::new(),
            stopping: AtomicBool::new(false),
            next_conn_id: AtomicU64::new(1),
            clients: Mutex::new(Vec::new()),
            client_threads: Mutex::new(Vec::new()),
        });

        let accept_shared = Arc::clone(&shared);
        let accept_thread = thread::spawn(move || accept_loop(listener, accept_shared));

        Ok(Server {
            local_addr,
            shared,
            accept_thread: Some(accept_thread),
        })
    }

    /// Return the local socket address the server is listening on.
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Stop accepting new clients, close active clients, roll back open transactions, and join threads.
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        let Some(accept_handle) = self.accept_thread.take() else {
            return;
        };

        self.shared.stopping.store(true, Ordering::Release);
        let _ = TcpStream::connect(wake_addr(self.local_addr));
        let _ = accept_handle.join();

        {
            let mut st = lock_unpoison(&self.shared.state);
            st.stopped = true;
            self.shared.cvar.notify_all();
        }

        {
            let clients = lock_unpoison(&self.shared.clients);
            for (_, stream) in clients.iter() {
                let _ = stream.shutdown(Shutdown::Both);
            }
        }

        let handles = {
            let mut threads = lock_unpoison(&self.shared.client_threads);
            std::mem::take(&mut *threads)
        };
        for handle in handles {
            let _ = handle.join();
        }

        let mut st = lock_unpoison(&self.shared.state);
        if let Some(mut db) = st.db.take() {
            if db.in_transaction() {
                let _ = db.execute("ROLLBACK");
            }
            let _ = db.close();
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn accept_loop(listener: TcpListener, shared: Arc<Shared>) {
    for incoming in listener.incoming() {
        if shared.stopping.load(Ordering::Acquire) {
            break;
        }
        let Ok(stream) = incoming else {
            continue;
        };
        if shared.stopping.load(Ordering::Acquire) {
            break;
        }
        let Ok(peer_clone) = stream.try_clone() else {
            continue;
        };
        let conn_id = shared.next_conn_id.fetch_add(1, Ordering::Relaxed);
        {
            let mut clients = lock_unpoison(&shared.clients);
            clients.push((conn_id, peer_clone));
        }
        let conn_shared = Arc::clone(&shared);
        let handle = thread::spawn(move || handle_client(conn_id, stream, conn_shared));
        let mut threads = lock_unpoison(&shared.client_threads);
        threads.push(handle);
    }
}

fn handle_client(conn_id: u64, mut stream: TcpStream, shared: Arc<Shared>) {
    while let Ok(Some(frame)) = read_frame(&mut stream) {
        let sql = match std::str::from_utf8(&frame) {
            Ok(s) => s,
            Err(_) => {
                if send_error(&mut stream, "invalid utf-8").is_err() {
                    break;
                }
                continue;
            }
        };

        match execute_for_conn(&shared, conn_id, sql) {
            ExecOutcome::Reply(Ok(rows)) => {
                let mut reply = Vec::new();
                reply.push(0x00);
                reply.extend_from_slice(&encode_rows(&rows));
                if write_frame(&mut stream, &reply).is_err() {
                    break;
                }
            }
            ExecOutcome::Reply(Err(msg)) => {
                if send_error(&mut stream, &msg).is_err() {
                    break;
                }
            }
            ExecOutcome::Busy => {
                if send_error(&mut stream, "busy").is_err() {
                    break;
                }
            }
            ExecOutcome::Stopped => break,
        }
    }

    cleanup_conn(&shared, conn_id);
}

fn execute_for_conn(shared: &Shared, conn_id: u64, sql: &str) -> ExecOutcome {
    let deadline = Instant::now() + BUSY_WAIT;
    let mut guard = lock_unpoison(&shared.state);
    loop {
        if guard.stopped {
            return ExecOutcome::Stopped;
        }
        if guard.owner.is_none() || guard.owner == Some(conn_id) {
            break;
        }
        let now = Instant::now();
        if now >= deadline {
            return ExecOutcome::Busy;
        }
        let remaining = deadline.saturating_duration_since(now);
        guard = wait_timeout_unpoison(&shared.cvar, guard, remaining);
    }

    let Some(db) = guard.db.as_mut() else {
        return ExecOutcome::Stopped;
    };

    let result = db.execute(sql).map_err(|e| e.to_string());
    let in_tx = db.in_transaction();
    if in_tx {
        guard.owner = Some(conn_id);
    } else if guard.owner == Some(conn_id) || guard.owner.is_none() {
        guard.owner = None;
        shared.cvar.notify_all();
    }
    ExecOutcome::Reply(result)
}

fn cleanup_conn(shared: &Shared, conn_id: u64) {
    {
        let mut st = lock_unpoison(&shared.state);
        if st.owner == Some(conn_id) {
            if let Some(db) = st.db.as_mut() {
                if db.in_transaction() {
                    let _ = db.execute("ROLLBACK");
                }
            }
            st.owner = None;
            shared.cvar.notify_all();
        }
    }
    let mut clients = lock_unpoison(&shared.clients);
    clients.retain(|(id, _)| *id != conn_id);
}

fn send_error(stream: &mut TcpStream, msg: &str) -> io::Result<()> {
    let mut reply = Vec::with_capacity(1 + msg.len());
    reply.push(0x01);
    reply.extend_from_slice(msg.as_bytes());
    write_frame(stream, &reply)
}

fn wake_addr(addr: SocketAddr) -> SocketAddr {
    match addr {
        SocketAddr::V4(v4) if v4.ip().is_unspecified() => {
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), v4.port())
        }
        SocketAddr::V6(v6) if v6.ip().is_unspecified() => {
            SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), v6.port())
        }
        other => other,
    }
}

fn varve_err_to_io(err: crate::Error) -> io::Error {
    io::Error::other(err.to_string())
}

fn lock_unpoison<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    match m.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn wait_timeout_unpoison<'a, T>(
    cv: &Condvar,
    guard: MutexGuard<'a, T>,
    dur: Duration,
) -> MutexGuard<'a, T> {
    match cv.wait_timeout(guard, dur) {
        Ok((g, _)) => g,
        Err(poisoned) => poisoned.into_inner().0,
    }
}

//! Security and hostile-input regression tests for Iridium.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::ops::Bound;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use iridium::net::{decode_rows, Client, Server};
use iridium::sql::{decode_record, encode_key, encode_record, Db, Value};
use iridium::{Error, MemStorage, Page, Pager, Tree, PAGE, USABLE};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

struct TempDb {
    dir: PathBuf,
    main: PathBuf,
    log: PathBuf,
}

impl TempDb {
    fn new(name: &str) -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "iridium_sec_test_{name}_{}_{}",
            std::process::id(),
            id
        ));
        fs::create_dir_all(&dir).unwrap();
        let main = dir.join("db.main");
        let log = dir.join("db.wal");
        Self { dir, main, log }
    }
}

impl Drop for TempDb {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

fn stamp_crc(page: &mut Page) {
    let c = crc32(&page[..USABLE]);
    page[USABLE..PAGE].copy_from_slice(&c.to_le_bytes());
}

fn make_page0(page_count: u64, free_head: u64, root: u64) -> Page {
    let mut page = [0u8; PAGE];
    page[0..8].copy_from_slice(b"IRIDI\0v1");
    page[8..12].copy_from_slice(&(PAGE as u32).to_le_bytes());
    page[12..20].copy_from_slice(&page_count.to_le_bytes());
    page[20..28].copy_from_slice(&free_head.to_le_bytes());
    page[28..36].copy_from_slice(&root.to_le_bytes());
    stamp_crc(&mut page);
    page
}

fn make_wal_header(salt: u64) -> [u8; 32] {
    let mut hdr = [0u8; 32];
    hdr[0..8].copy_from_slice(b"IRIDIWAL");
    hdr[8..12].copy_from_slice(&1u32.to_le_bytes());
    hdr[12..16].copy_from_slice(&(PAGE as u32).to_le_bytes());
    hdr[16..24].copy_from_slice(&salt.to_le_bytes());
    let c = crc32(&hdr[0..28]);
    hdr[28..32].copy_from_slice(&c.to_le_bytes());
    hdr
}

fn make_wal_frame(page_no: u64, commit: u64, salt: u64, page: &Page) -> Vec<u8> {
    let mut frame = vec![0u8; 32 + PAGE];
    frame[0..8].copy_from_slice(&page_no.to_le_bytes());
    frame[8..16].copy_from_slice(&commit.to_le_bytes());
    frame[16..24].copy_from_slice(&salt.to_le_bytes());
    let mut combined = Vec::with_capacity(28 + PAGE);
    combined.extend_from_slice(&frame[0..28]);
    combined.extend_from_slice(page);
    let c = crc32(&combined);
    frame[28..32].copy_from_slice(&c.to_le_bytes());
    frame[32..].copy_from_slice(page);
    frame
}

fn localhost_zero() -> SocketAddr {
    "127.0.0.1:0".parse().unwrap()
}

#[test]
fn server_stop_does_not_hang_with_idle_or_slowloris_client() {
    let tmp = TempDb::new("stop_hang");
    let srv = Server::start(localhost_zero(), tmp.main.clone(), tmp.log.clone()).unwrap();
    let addr = srv.local_addr();

    // Active idle client still open at stop time.
    let mut cli = Client::connect(addr).unwrap();
    let res = cli.execute("SELECT 1").unwrap().unwrap();
    assert_eq!(res.rows, vec![vec![Value::Int(1)]]);

    // Slowloris client that sent a partial frame header and keeps socket open.
    let mut slow = TcpStream::connect(addr).unwrap();
    slow.write_all(&[0x00, 0x01]).unwrap();
    slow.flush().unwrap();

    let start = Instant::now();
    srv.stop();
    assert!(
        start.elapsed() < Duration::from_secs(1),
        "Server::stop() took too long: {:?}",
        start.elapsed()
    );
    drop(cli);
    drop(slow);
}

#[test]
fn sql_chained_operators_respect_depth_limit() {
    let mut db = Db::open(MemStorage::new(), MemStorage::new()).unwrap();

    // Long left-associative chains must fail cleanly instead of overflowing the stack.
    let deep_add = format!("SELECT 1{}", " + 1".repeat(20_000));
    assert!(matches!(db.execute(&deep_add), Err(Error::Parse { .. })));

    let deep_or = format!("SELECT 1{}", " OR 1".repeat(20_000));
    assert!(matches!(db.execute(&deep_or), Err(Error::Parse { .. })));

    let deep_and = format!("SELECT 1{}", " AND 1".repeat(20_000));
    assert!(matches!(db.execute(&deep_and), Err(Error::Parse { .. })));

    let deep_is_null = format!("SELECT 1{}", " IS NULL".repeat(20_000));
    assert!(matches!(
        db.execute(&deep_is_null),
        Err(Error::Parse { .. })
    ));

    let deep_like = format!("SELECT 1{}", " LIKE 'a'".repeat(20_000));
    assert!(matches!(db.execute(&deep_like), Err(Error::Parse { .. })));

    let deep_concat = format!("SELECT 'a'{}", " || 'a'".repeat(20_000));
    assert!(matches!(db.execute(&deep_concat), Err(Error::Parse { .. })));

    let deep_parens = format!("SELECT {}1{}", "(".repeat(500), ")".repeat(500));
    assert!(matches!(db.execute(&deep_parens), Err(Error::Parse { .. })));

    // Normal shallow chain still works.
    let ok = format!("SELECT 1{}", " + 1".repeat(10));
    let rows = db.execute(&ok).unwrap();
    assert_eq!(rows.rows, vec![vec![Value::Int(11)]]);
}

#[test]
fn sql_like_long_mismatch_does_not_hang() {
    let mut db = Db::open(MemStorage::new(), MemStorage::new()).unwrap();
    let text = "a".repeat(100_000);
    let pat_suffix = format!("%{}b", "a".repeat(100_000));
    let q1 = format!("SELECT '{text}' LIKE '{pat_suffix}'");

    let start = Instant::now();
    let r1 = db.execute(&q1).unwrap();
    assert_eq!(r1.rows, vec![vec![Value::Int(0)]]);
    assert!(start.elapsed() < Duration::from_secs(1));

    let pat_mid = format!("%{}c%b", "a".repeat(50_000));
    let q2 = format!("SELECT '{text}b' LIKE '{pat_mid}'");
    let start2 = Instant::now();
    let r2 = db.execute(&q2).unwrap();
    assert_eq!(r2.rows, vec![vec![Value::Int(0)]]);
    assert!(start2.elapsed() < Duration::from_secs(1));

    let pat_wc = format!("%{}c%b", "a_".repeat(20_000));
    let q3 = format!("SELECT '{text}b' LIKE '{pat_wc}'");
    let start3 = Instant::now();
    let r3 = db.execute(&q3);
    assert!(matches!(r3, Err(Error::TooLarge) | Ok(_)));
    assert!(start3.elapsed() < Duration::from_secs(1));
}

#[test]
fn create_table_quoted_name_with_double_quote_reopens_cleanly() {
    let db_storage = {
        let mut db = Db::open(MemStorage::new(), MemStorage::new()).unwrap();
        db.execute("CREATE TABLE \"t\"\"1\" (\"c\"\"1\" INTEGER UNIQUE, v TEXT)")
            .unwrap();
        db.execute("INSERT INTO \"t\"\"1\" VALUES (7, 'ok')")
            .unwrap();
        db.close().unwrap()
    };

    let mut db2 = Db::open(db_storage.0, db_storage.1).unwrap();
    let rows = db2.execute("SELECT \"c\"\"1\", v FROM \"t\"\"1\"").unwrap();
    assert_eq!(
        rows.rows,
        vec![vec![Value::Int(7), Value::Text("ok".to_owned())]]
    );
}

#[test]
fn update_on_corrupt_short_record_returns_corrupt_not_panic() {
    let (main, log) = {
        let mut db = Db::open(MemStorage::new(), MemStorage::new()).unwrap();
        db.execute("CREATE TABLE t (id INTEGER PRIMARY KEY, a INTEGER, b TEXT)")
            .unwrap();
        db.execute("INSERT INTO t VALUES (1, 10, 'hi')").unwrap();
        db.close().unwrap()
    };

    // Corrupt the row record in table `t` so it only has 1 column instead of 3.
    let short_rec = encode_record(&[Value::Int(1)]);
    let key = encode_key(&[Value::Int(1)]);
    let (main, log) = {
        let mut pager = Pager::open(main, log).unwrap();
        let cat_root = pager.root();
        let mut table_root = None;
        Tree::open(cat_root)
            .scan(&mut pager, None, Bound::Unbounded, &mut |k, v| {
                if k == b"t" {
                    if let Ok(vals) = decode_record(v) {
                        if let Some(Value::Int(r)) = vals.get(2) {
                            table_root = Some(*r as u64);
                        }
                    }
                }
                true
            })
            .unwrap();
        pager.begin().unwrap();
        Tree::open(table_root.unwrap())
            .insert(&mut pager, &key, &short_rec)
            .unwrap();
        pager.commit().unwrap();
        pager.close().unwrap()
    };

    let mut db = Db::open(main, log).unwrap();
    let res = db.execute("UPDATE t SET b = 'x'");
    assert_eq!(res, Err(Error::Corrupt));
}

#[test]
fn corrupt_overflow_cycle_returns_corrupt() {
    let (mut main, log) = {
        let mut pager = Pager::open(MemStorage::new(), MemStorage::new()).unwrap();
        pager.begin().unwrap();
        let tree = Tree::create(&mut pager).unwrap();
        pager.set_root(tree.root()).unwrap();
        let big = vec![b'z'; 10_000];
        tree.insert(&mut pager, b"k", &big).unwrap();
        pager.commit().unwrap();
        pager.close().unwrap()
    };

    // Find the overflow page number from the leaf page and make it self-loop.
    let leaf_page_no = {
        let mut pager = Pager::open(main, log).unwrap();
        let root = pager.root();
        let leaf = pager.read(root).unwrap();
        // Overflow head is the last 8 bytes of the single leaf cell ending at USABLE.
        let ov_head = u64::from_le_bytes(leaf[USABLE - 8..USABLE].try_into().unwrap());
        let mut ov_page = pager.read(ov_head).unwrap();
        ov_page[16..24].copy_from_slice(&ov_head.to_le_bytes());
        stamp_crc(&mut ov_page);
        let (m, l) = pager.into_storage();
        main = m;
        let off = (ov_head as usize) * PAGE;
        let mut bytes = main.bytes().to_vec();
        bytes[off..off + PAGE].copy_from_slice(&ov_page);
        (MemStorage::from_bytes(bytes), l, root)
    };

    let mut pager = Pager::open(leaf_page_no.0, leaf_page_no.1).unwrap();
    pager.begin().unwrap();
    let res = Tree::open(leaf_page_no.2).remove(&mut pager, b"k");
    assert_eq!(res, Err(Error::Corrupt));
}

#[test]
fn corrupt_page_crc_and_wal_frame_crc_rejected() {
    // Corrupt page 0 CRC on existing DB.
    let (main, log) = {
        let mut pager = Pager::open(MemStorage::new(), MemStorage::new()).unwrap();
        pager.begin().unwrap();
        let p = pager.alloc().unwrap();
        pager.write(p, &[0x11u8; PAGE]).unwrap();
        pager.commit().unwrap();
        pager.close().unwrap()
    };
    let mut bad_main = main.bytes().to_vec();
    bad_main[50] ^= 0xFF;
    assert_eq!(
        Pager::open(MemStorage::from_bytes(bad_main), MemStorage::new()).unwrap_err(),
        Error::Corrupt
    );

    // Uncommitted / corrupted WAL frame tail is ignored during recovery.
    let p0 = make_page0(2, 0, 0);
    let mut main_bytes = p0.to_vec();
    let mut p1 = [0u8; PAGE];
    stamp_crc(&mut p1);
    main_bytes.extend_from_slice(&p1);

    let mut wal_bytes = make_wal_header(7).to_vec();
    let mut bad_frame = make_wal_frame(1, 2, 7, &p1);
    bad_frame[40] ^= 0x01; // flip payload bit -> WAL frame CRC mismatch
    wal_bytes.extend_from_slice(&bad_frame);

    let pager = Pager::open(
        MemStorage::from_bytes(main_bytes),
        MemStorage::from_bytes(wal_bytes),
    )
    .unwrap();
    assert_eq!(pager.log_frames(), 0);
    assert_eq!(pager.page_count(), 2);

    // WAL commit claiming out-of-range page_no >= committed.page_count is rejected.
    let mut wal2 = make_wal_header(7).to_vec();
    wal2.extend_from_slice(&make_wal_frame(5, 2, 7, &p1));
    assert_eq!(
        Pager::open(
            MemStorage::from_bytes(p0.to_vec()),
            MemStorage::from_bytes(wal2)
        )
        .unwrap_err(),
        Error::Corrupt
    );
    drop(log);
}

#[test]
fn corrupt_btree_cell_offsets_and_lengths_rejected() {
    // 1. Leaf page with out-of-bounds cell offset and valid CRC.
    let p0 = make_page0(2, 0, 1);
    let mut bad_leaf = [0u8; PAGE];
    bad_leaf[0] = 3; // KIND_LEAF
    bad_leaf[2..4].copy_from_slice(&1u16.to_le_bytes()); // cell_count = 1
    bad_leaf[4..6].copy_from_slice(&100u16.to_le_bytes()); // content_start = 100
    bad_leaf[16..18].copy_from_slice(&4091u16.to_le_bytes()); // off = 4091
    bad_leaf[4091] = 0; // klen = 0, truncated vlen
    stamp_crc(&mut bad_leaf);

    let mut main = p0.to_vec();
    main.extend_from_slice(&bad_leaf);
    let mut pager = Pager::open(MemStorage::from_bytes(main), MemStorage::new()).unwrap();
    assert_eq!(
        Tree::open(1).get(&mut pager, None, b"a"),
        Err(Error::Corrupt)
    );

    // 2. Leaf page with cell_count exceeding USABLE.
    let mut bad_count = [0u8; PAGE];
    bad_count[0] = 3;
    bad_count[2..4].copy_from_slice(&3000u16.to_le_bytes());
    bad_count[4..6].copy_from_slice(&3000u16.to_le_bytes());
    stamp_crc(&mut bad_count);
    let mut main2 = p0.to_vec();
    main2.extend_from_slice(&bad_count);
    let mut pager2 = Pager::open(MemStorage::from_bytes(main2), MemStorage::new()).unwrap();
    assert_eq!(
        Tree::open(1).get(&mut pager2, None, b"a"),
        Err(Error::Corrupt)
    );

    // 3. Leaf page with self-linking cycle (`link = 1`).
    let mut cycle_leaf = [0u8; PAGE];
    cycle_leaf[0] = 3;
    cycle_leaf[2..4].copy_from_slice(&1u16.to_le_bytes());
    cycle_leaf[4..6].copy_from_slice(&4088u16.to_le_bytes());
    cycle_leaf[8..16].copy_from_slice(&1u64.to_le_bytes()); // link = 1
    cycle_leaf[16..18].copy_from_slice(&4088u16.to_le_bytes());
    cycle_leaf[4088] = 1; // klen = 1
    cycle_leaf[4089] = 1; // vlen = 1
    cycle_leaf[4090] = b'a';
    cycle_leaf[4091] = b'b';
    stamp_crc(&mut cycle_leaf);
    let mut main3 = p0.to_vec();
    main3.extend_from_slice(&cycle_leaf);
    let mut pager3 = Pager::open(MemStorage::from_bytes(main3), MemStorage::new()).unwrap();
    assert_eq!(
        Tree::open(1).scan(&mut pager3, None, Bound::Unbounded, &mut |_, _| true),
        Err(Error::Corrupt)
    );
}

#[test]
fn hostile_record_and_wire_decoders_never_panic_or_oom() {
    // Huge column count claiming 2^32 - 1 on tiny buffer.
    assert_eq!(
        decode_record(&[0xff, 0xff, 0xff, 0xff, 0x0f]),
        Err(Error::Corrupt)
    );
    // Non-minimal varint.
    assert_eq!(decode_record(&[0x81, 0x00]), Err(Error::Corrupt));
    // Invalid UTF-8 in text value.
    assert_eq!(
        decode_record(&[0x01, 0x02, 0x02, 0xff, 0xfe]),
        Err(Error::Corrupt)
    );
    // Text length exceeding buffer.
    assert_eq!(
        decode_record(&[0x01, 0x02, 0x10, b'a']),
        Err(Error::Corrupt)
    );

    // Wire codec: huge col_count and row_count on tiny buffer.
    let mut huge_cols = vec![0u8; 16];
    huge_cols[8..12].copy_from_slice(&u32::MAX.to_be_bytes());
    assert_eq!(decode_rows(&huge_cols), None);

    let mut huge_rows = Vec::new();
    huge_rows.extend_from_slice(&0u64.to_be_bytes());
    huge_rows.extend_from_slice(&1u32.to_be_bytes());
    huge_rows.extend_from_slice(&1u32.to_be_bytes());
    huge_rows.push(b'c');
    huge_rows.extend_from_slice(&u32::MAX.to_be_bytes());
    assert_eq!(decode_rows(&huge_rows), None);
}

#[test]
fn net_oversize_frame_header_does_not_allocate() {
    let tmp = TempDb::new("oversize_hdr");
    let srv = Server::start(localhost_zero(), tmp.main.clone(), tmp.log.clone()).unwrap();
    let addr = srv.local_addr();

    {
        let mut raw = TcpStream::connect(addr).unwrap();
        raw.write_all(&u32::MAX.to_be_bytes()).unwrap();
        raw.flush().unwrap();
        let mut buf = [0u8; 8];
        let n = raw.read(&mut buf).unwrap();
        assert_eq!(n, 0);
    }

    let mut cli = Client::connect(addr).unwrap();
    let res = cli.execute("SELECT 42").unwrap().unwrap();
    assert_eq!(res.rows, vec![vec![Value::Int(42)]]);
    srv.stop();
}

#[test]
fn net_truncated_frames_roll_back_open_tx() {
    let tmp = TempDb::new("trunc_tx");
    let srv = Server::start(localhost_zero(), tmp.main.clone(), tmp.log.clone()).unwrap();
    let addr = srv.local_addr();

    {
        let mut cli = Client::connect(addr).unwrap();
        cli.execute("CREATE TABLE t (x INTEGER)").unwrap().unwrap();
        cli.execute("INSERT INTO t VALUES (1)").unwrap().unwrap();
    }

    {
        let mut raw = TcpStream::connect(addr).unwrap();
        let send = |s: &mut TcpStream, sql: &[u8]| {
            s.write_all(&(sql.len() as u32).to_be_bytes()).unwrap();
            s.write_all(sql).unwrap();
            s.flush().unwrap();
            let mut len_b = [0u8; 4];
            s.read_exact(&mut len_b).unwrap();
            let n = u32::from_be_bytes(len_b) as usize;
            let mut body = vec![0u8; n];
            s.read_exact(&mut body).unwrap();
        };
        send(&mut raw, b"BEGIN");
        send(&mut raw, b"INSERT INTO t VALUES (2)");
        // Send truncated frame body and drop.
        raw.write_all(&100u32.to_be_bytes()).unwrap();
        raw.write_all(b"COMMIT").unwrap();
        raw.flush().unwrap();
    }

    let mut check = Client::connect(addr).unwrap();
    let res = check.execute("SELECT x FROM t").unwrap().unwrap();
    assert_eq!(res.rows, vec![vec![Value::Int(1)]]);
    srv.stop();
}

#[test]
fn sql_lexer_and_parser_hostile_inputs_never_panic() {
    let mut db = Db::open(MemStorage::new(), MemStorage::new()).unwrap();
    let bad_inputs = [
        "",
        "   \t\n ",
        "SELECT 'unterminated",
        "SELECT \"unterminated",
        "SELECT \"\"",
        "SELECT /* unterminated",
        "SELECT x'abc'",
        "SELECT x'zzzz'",
        "SELECT 1.23",
        "SELECT 123abc",
        "SELECT \0",
        "SELECT \u{1F600}",
        "SELECT 1;",
    ];
    for sql in bad_inputs {
        let _ = db.execute(sql);
    }

    // Huge integer literal overflows cleanly instead of panicking.
    let huge_int = format!("SELECT {}", "9".repeat(100_000));
    assert_eq!(db.execute(&huge_int), Err(Error::Overflow));

    // Extreme integer boundary arithmetic and substr bounds never panic.
    assert_eq!(
        db.execute("SELECT -9223372036854775808 / -1"),
        Err(Error::Overflow)
    );
    assert_eq!(
        db.execute("SELECT -9223372036854775808 % -1").unwrap().rows,
        vec![vec![Value::Int(0)]]
    );
    assert_eq!(
        db.execute("SELECT abs(-9223372036854775808)"),
        Err(Error::Overflow)
    );
    assert_eq!(
        db.execute("SELECT substr('abc', -9223372036854775808, -9223372036854775808)")
            .unwrap()
            .rows,
        vec![vec![Value::Text(String::new())]]
    );
}

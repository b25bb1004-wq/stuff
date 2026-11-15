//! Integration tests for Logos smart contracts on the aurum chain.
#![allow(clippy::unwrap_used)]

use assay::Rng;
use aurum::{contract_address, merkle_root, Block, Chain, Receipt, Tx, DEPLOY, FUEL_PER_FEE};
use sphinx::{ed25519, sha256};

const COUNTER_SRC: &str = r#"
fn get(k: [byte]) -> [byte]? uses chain = extern "chain.get"
fn set(k: [byte], v: [byte]) uses chain = extern "chain.set"
fn emit(e: str) uses chain = extern "chain.emit"

fn dec_u64(bs: [byte]) -> int {
    var acc = 0
    var i = 0
    while i < 8 {
        acc = acc + int(bs[i]) * (1 << (i * 8))
        i = i + 1
    }
    acc
}

fn enc_u64(n: int) -> [byte] {
    var out: [byte] = []
    var i = 0
    while i < 8 {
        push(out, byte((n >> (i * 8)) & 255))
        i = i + 1
    }
    out
}

fn main() uses chain {
    let key = bytes("n")
    let cur = match get(key) {
        none => 0,
        some(bs) => dec_u64(bs),
    }
    let next = cur + 1
    set(key, enc_u64(next))
    emit(str(next))
}
"#;

const TOKEN_SRC: &str = r#"fn get(k: [byte]) -> [byte]? uses chain = extern "chain.get"
fn set(k: [byte], v: [byte]) uses chain = extern "chain.set"
fn caller() -> [byte] uses chain = extern "chain.caller"
fn input() -> [byte] uses chain = extern "chain.input"
fn emit(e: str) uses chain = extern "chain.emit"
fn dec(bs: [byte]) -> int {
    var a = 0
    var i = 0
    while i < 8 { a = a + int(bs[i]) * (1 << (i * 8)); i = i + 1 }
    a
}
fn enc(n: int) -> [byte] {
    var o: [byte] = []
    var i = 0
    while i < 8 { push(o, byte((n >> (i * 8)) & 255)); i = i + 1 }
    o
}
fn main() uses chain {
    let inp = input()
    let me = caller()
    if len(inp) == 0 { set(me, enc(1000)); emit("mint"); return }
    let to = inp[0..32]
    let amt = dec(inp[32..40])
    let sb = match get(me) { none => 0, some(b) => dec(b) }
    assert sb >= amt, "low"
    let rb = match get(to) { none => 0, some(b) => dec(b) }
    set(me, enc(sb - amt))
    set(to, enc(rb + amt))
    emit("xfer")
}"#;

fn seed_from(b: u8) -> [u8; 32] {
    let mut s = [0u8; 32];
    s[0] = b;
    s[31] = b.wrapping_mul(7).wrapping_add(1);
    s
}

fn setup_chain() -> (Chain, [u8; 32], [u8; 32], [u8; 32]) {
    let val_seed = seed_from(1);
    let alice_seed = seed_from(2);
    let bob_seed = seed_from(3);
    let val_addr = ed25519::public_key(&val_seed);
    let alice_addr = ed25519::public_key(&alice_seed);
    let bob_addr = ed25519::public_key(&bob_seed);
    let chain = Chain::new(
        &[(alice_addr, 1_000_000), (bob_addr, 1_000_000)],
        &[val_addr],
    );
    (chain, val_seed, alice_seed, bob_seed)
}

#[test]
fn counter_contract_increments_and_emits() {
    assert!(COUNTER_SRC.len() <= aurum::MAX_DATA);
    let (mut chain, val_seed, alice_seed, _) = setup_chain();
    let alice = ed25519::public_key(&alice_seed);

    let deploy_tx = Tx::signed(
        &alice_seed,
        DEPLOY,
        50,
        10,
        0,
        COUNTER_SRC.as_bytes().to_vec(),
    );
    let b1 = chain.propose(&val_seed, 1, vec![deploy_tx]).unwrap();
    chain.append(b1).unwrap();

    let expected_addr = contract_address(&alice, 0);
    assert_eq!(chain.code(&expected_addr), Some(COUNTER_SRC));
    assert_eq!(chain.account(&expected_addr), (50, 0));
    assert_eq!(
        chain.receipts(1),
        Some(
            &[Receipt {
                ok: true,
                fuel_used: 0,
                events: Vec::new(),
                contract: Some(expected_addr),
            }][..]
        )
    );
    assert_eq!(chain.receipts(0), None);

    let c1 = Tx::signed(&alice_seed, expected_addr, 0, 200, 1, Vec::new());
    let c2 = Tx::signed(&alice_seed, expected_addr, 0, 200, 2, Vec::new());
    let b2 = chain.propose(&val_seed, 2, vec![c1, c2]).unwrap();
    chain.append(b2).unwrap();

    assert_eq!(
        chain.storage(&expected_addr, b"n"),
        Some(&2u64.to_le_bytes()[..])
    );
    let r2 = chain.receipts(2).unwrap();
    assert_eq!(r2.len(), 2);
    assert!(r2[0].ok && r2[1].ok);
    assert!(r2[0].fuel_used > 0 && r2[1].fuel_used > 0);
    assert_eq!(r2[0].events, vec![String::from("1")]);
    assert_eq!(r2[1].events, vec![String::from("2")]);
}

#[test]
fn token_contract_mint_and_transfer() {
    assert!(TOKEN_SRC.len() <= aurum::MAX_DATA);
    let (mut chain, val_seed, alice_seed, bob_seed) = setup_chain();
    let alice = ed25519::public_key(&alice_seed);
    let bob = ed25519::public_key(&bob_seed);
    let token_addr = contract_address(&alice, 0);

    let deploy = Tx::signed(&alice_seed, DEPLOY, 0, 10, 0, TOKEN_SRC.as_bytes().to_vec());
    let mint = Tx::signed(&alice_seed, token_addr, 0, 500, 1, Vec::new());
    let mut xfer_input = Vec::with_capacity(40);
    xfer_input.extend_from_slice(&bob);
    xfer_input.extend_from_slice(&250u64.to_le_bytes());
    let xfer = Tx::signed(&alice_seed, token_addr, 0, 500, 2, xfer_input);

    let blk = chain
        .propose(&val_seed, 1, vec![deploy, mint, xfer])
        .unwrap();
    chain.append(blk).unwrap();

    let rs = chain.receipts(1).unwrap();
    assert_eq!(rs.len(), 3);
    assert!(rs.iter().all(|r| r.ok));
    assert_eq!(rs[1].events, vec![String::from("mint")]);
    assert_eq!(rs[2].events, vec![String::from("xfer")]);
    assert_eq!(
        chain.storage(&token_addr, &alice),
        Some(&750u64.to_le_bytes()[..])
    );
    assert_eq!(
        chain.storage(&token_addr, &bob),
        Some(&250u64.to_le_bytes()[..])
    );
}

#[test]
fn deploy_rejections_refund_amount_and_keep_fee() {
    let (mut chain, val_seed, alice_seed, _) = setup_chain();
    let alice = ed25519::public_key(&alice_seed);
    let val = ed25519::public_key(&val_seed);

    let io_src = br#"
fn print(s: str) uses io = extern "io.print"
fn main() uses io { print("hi") }
"#;
    let bad_parse = b"fn main( { broken";
    let no_main = b"fn helper() -> int { 1 }";

    let tx0 = Tx::signed(&alice_seed, DEPLOY, 100, 15, 0, io_src.to_vec());
    let tx1 = Tx::signed(&alice_seed, DEPLOY, 200, 25, 1, bad_parse.to_vec());
    let tx2 = Tx::signed(&alice_seed, DEPLOY, 300, 35, 2, no_main.to_vec());

    let blk = chain.propose(&val_seed, 1, vec![tx0, tx1, tx2]).unwrap();
    chain.append(blk).unwrap();

    for nonce in 0..3u64 {
        let addr = contract_address(&alice, nonce);
        assert_eq!(chain.code(&addr), None);
        assert_eq!(chain.account(&addr), (0, 0));
    }
    let rs = chain.receipts(1).unwrap();
    assert_eq!(rs.len(), 3);
    for r in rs {
        assert!(!r.ok);
        assert_eq!(r.contract, None);
        assert!(r.events.is_empty());
        assert_eq!(r.fuel_used, 0);
    }
    assert_eq!(chain.account(&alice), (1_000_000 - 15 - 25 - 35, 3));
    assert_eq!(chain.account(&val), (15 + 25 + 35, 0));
}

#[test]
fn out_of_fuel_and_trap_revert_storage_and_refund_amount() {
    let (mut chain, val_seed, alice_seed, _) = setup_chain();
    let alice = ed25519::public_key(&alice_seed);
    let val = ed25519::public_key(&val_seed);

    let src = r#"
fn set(k: [byte], v: [byte]) uses chain = extern "chain.set"
fn input() -> [byte] uses chain = extern "chain.input"
fn emit(e: str) uses chain = extern "chain.emit"

fn main() uses chain {
    set(bytes("k"), bytes("dirty"))
    emit("should_not_appear")
    let inp = input()
    if len(inp) == 1 {
        assert false, "boom"
    }
    if len(inp) == 2 {
        while true { }
    }
    set(bytes("k"), bytes("clean"))
}
"#;
    assert!(src.len() <= aurum::MAX_DATA);
    let addr = contract_address(&alice, 0);
    let deploy = Tx::signed(&alice_seed, DEPLOY, 10, 10, 0, src.as_bytes().to_vec());
    let trap_tx = Tx::signed(&alice_seed, addr, 500, 100, 1, vec![1]);
    let oof_tx = Tx::signed(&alice_seed, addr, 600, 1, 2, vec![2]);

    let blk = chain
        .propose(&val_seed, 1, vec![deploy, trap_tx, oof_tx])
        .unwrap();
    chain.append(blk).unwrap();

    let rs = chain.receipts(1).unwrap();
    assert!(rs[0].ok);
    assert!(!rs[1].ok);
    assert!(!rs[2].ok);
    assert!(rs[1].events.is_empty());
    assert!(rs[2].events.is_empty());
    assert!(rs[1].fuel_used > 0);
    assert!(rs[2].fuel_used <= FUEL_PER_FEE && rs[2].fuel_used > 0);
    assert_eq!(chain.storage(&addr, b"k"), None);
    assert_eq!(chain.account(&addr), (10, 0));
    assert_eq!(chain.account(&alice), (1_000_000 - 10 - 10 - 100 - 1, 3));
    assert_eq!(chain.account(&val), (10 + 100 + 1, 0));
}

#[test]
fn three_nodes_agree_via_block_encode_decode() {
    let (mut n1, val_seed, alice_seed, bob_seed) = setup_chain();
    let (mut n2, _, _, _) = setup_chain();
    let (mut n3, _, _, _) = setup_chain();

    let alice = ed25519::public_key(&alice_seed);
    let counter_addr = contract_address(&alice, 0);

    let txs1 = vec![Tx::signed(
        &alice_seed,
        DEPLOY,
        10,
        10,
        0,
        COUNTER_SRC.as_bytes().to_vec(),
    )];
    let b1 = n1.propose(&val_seed, 1, txs1).unwrap();
    let enc1 = b1.encode();
    n1.append(Block::decode(&enc1).unwrap()).unwrap();
    n2.append(Block::decode(&enc1).unwrap()).unwrap();
    n3.append(Block::decode(&enc1).unwrap()).unwrap();

    let txs2 = vec![
        Tx::signed(&alice_seed, counter_addr, 0, 200, 1, Vec::new()),
        Tx::signed(&bob_seed, counter_addr, 5, 200, 0, Vec::new()),
    ];
    let b2 = n2.propose(&val_seed, 2, txs2).unwrap();
    let enc2 = b2.encode();
    n1.append(Block::decode(&enc2).unwrap()).unwrap();
    n2.append(Block::decode(&enc2).unwrap()).unwrap();
    n3.append(Block::decode(&enc2).unwrap()).unwrap();

    assert_eq!(n1.state_root(), n2.state_root());
    assert_eq!(n2.state_root(), n3.state_root());
    assert_eq!(n1.receipts(1), n2.receipts(1));
    assert_eq!(n2.receipts(1), n3.receipts(1));
    assert_eq!(n1.receipts(2), n2.receipts(2));
    assert_eq!(n2.receipts(2), n3.receipts(2));
}

#[test]
fn chain_without_contracts_matches_t48_state_root() {
    let mut rng = Rng::new(42);
    let val_seed = seed_from(9);
    let a_seed = seed_from(10);
    let b_seed = seed_from(11);
    let val = ed25519::public_key(&val_seed);
    let a = ed25519::public_key(&a_seed);
    let b = ed25519::public_key(&b_seed);

    let mut chain = Chain::new(&[(a, 50_000), (b, 50_000)], &[val]);

    // Check genesis root against manual T48 48-byte account leaves.
    let mut accounts = [(a, 50_000u64, 0u64), (b, 50_000u64, 0u64)];
    accounts.sort_by_key(|x| x.0);
    let leaves: Vec<Vec<u8>> = accounts
        .iter()
        .map(|(addr, bal, non)| {
            let mut l = Vec::with_capacity(48);
            l.extend_from_slice(addr);
            l.extend_from_slice(&bal.to_le_bytes());
            l.extend_from_slice(&non.to_le_bytes());
            l
        })
        .collect();
    assert_eq!(chain.state_root(), merkle_root(&leaves));

    // Transfer to DEPLOY with empty data is a plain transfer, no contract created.
    let tx0 = Tx::signed(&a_seed, DEPLOY, 123, 7, 0, Vec::new());
    let amt = 100 + (rng.u64() % 500);
    let tx1 = Tx::signed(&b_seed, a, amt, 5, 0, b"hello".to_vec());
    let blk = chain.propose(&val_seed, 1, vec![tx0, tx1]).unwrap();
    chain.append(blk).unwrap();

    let (ab, an) = chain.account(&a);
    let (bb, bn) = chain.account(&b);
    let (db, dn) = chain.account(&DEPLOY);
    let (vb, vn) = chain.account(&val);
    let mut post = vec![(a, ab, an), (b, bb, bn), (DEPLOY, db, dn), (val, vb, vn)];
    post.retain(|x| (x.1, x.2) != (0, 0));
    post.sort_by_key(|x| x.0);
    let post_leaves: Vec<Vec<u8>> = post
        .iter()
        .map(|(addr, bal, non)| {
            let mut l = Vec::with_capacity(48);
            l.extend_from_slice(addr);
            l.extend_from_slice(&bal.to_le_bytes());
            l.extend_from_slice(&non.to_le_bytes());
            l
        })
        .collect();
    assert_eq!(chain.state_root(), merkle_root(&post_leaves));
    assert_eq!(chain.code(&DEPLOY), None);
}

#[test]
fn value_read_your_own_writes_and_contract_state_root() {
    let (mut chain, val_seed, alice_seed, bob_seed) = setup_chain();
    let alice = ed25519::public_key(&alice_seed);
    let bob = ed25519::public_key(&bob_seed);
    let val = ed25519::public_key(&val_seed);
    let src = r#"
fn get(k: [byte]) -> [byte]? uses chain = extern "chain.get"
fn set(k: [byte], v: [byte]) uses chain = extern "chain.set"
fn value() -> int uses chain = extern "chain.value"
fn emit(e: str) uses chain = extern "chain.emit"

fn main() uses chain {
    set(bytes("v"), bytes(str(value())))
    let got = match get(bytes("v")) {
        none => bytes(""),
        some(b) => b,
    }
    match text(got) {
        none => emit("bad"),
        some(s) => emit(s),
    }
}
"#;
    assert!(src.len() <= aurum::MAX_DATA);
    let addr = contract_address(&alice, 0);
    let deploy = Tx::signed(&alice_seed, DEPLOY, 0, 10, 0, src.as_bytes().to_vec());
    let call = Tx::signed(&alice_seed, addr, 777, 100, 1, Vec::new());
    let blk = chain.propose(&val_seed, 1, vec![deploy, call]).unwrap();
    chain.append(blk).unwrap();

    let rs = chain.receipts(1).unwrap();
    assert!(rs[0].ok && rs[1].ok);
    assert_eq!(rs[1].events, vec![String::from("777")]);
    assert_eq!(chain.storage(&addr, b"v"), Some(b"777".as_slice()));

    // Verify exact contract state root formula:
    // storage_root = merkle_root([len(key) as u32 LE || key || value])
    let mut sleaf = Vec::new();
    sleaf.extend_from_slice(&1u32.to_le_bytes());
    sleaf.extend_from_slice(b"v");
    sleaf.extend_from_slice(b"777");
    let storage_root = merkle_root(&[sleaf]);

    let mut cleaf = Vec::with_capacity(96);
    cleaf.extend_from_slice(&addr);
    cleaf.extend_from_slice(&sha256::digest(src.as_bytes()));
    cleaf.extend_from_slice(&storage_root);

    let (ab, an) = chain.account(&alice);
    let (bb, bn) = chain.account(&bob);
    let (cb, cn) = chain.account(&addr);
    let (vb, vn) = chain.account(&val);
    let mut entries: Vec<(aurum::Address, u8, Vec<u8>)> = Vec::new();
    for (a, b, n) in [
        (alice, ab, an),
        (bob, bb, bn),
        (addr, cb, cn),
        (val, vb, vn),
    ] {
        if (b, n) != (0, 0) {
            let mut l = Vec::with_capacity(48);
            l.extend_from_slice(&a);
            l.extend_from_slice(&b.to_le_bytes());
            l.extend_from_slice(&n.to_le_bytes());
            entries.push((a, 0, l));
        }
    }
    entries.push((addr, 1, cleaf));
    entries.sort_by(|x, y| x.0.cmp(&y.0).then_with(|| x.1.cmp(&y.1)));
    let leaves: Vec<Vec<u8>> = entries.into_iter().map(|(_, _, l)| l).collect();
    assert_eq!(chain.state_root(), merkle_root(&leaves));
}

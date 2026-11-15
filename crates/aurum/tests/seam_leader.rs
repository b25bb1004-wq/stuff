//! Leader check for T48: aurum blocks, Merkle tree, transactions, state and validation.
//! Its own model (Merkle, encodings, signing, state) is built from sphinx primitives only.
#![allow(clippy::unwrap_used)]
use assay::Rng;
use aurum::{
    merkle_proof, merkle_root, merkle_verify, Block, BlockError, Chain, DecodeError, Header, Tx,
    TxError, MAX_DATA, MAX_TXS,
};
use sphinx::ed25519;
use sphinx::sha256::digest as sha;
use std::collections::BTreeMap;

type H = [u8; 32];

// ---------- model: Merkle (RFC 6962 / RFC 9162) ----------
fn m_leaf(b: &[u8]) -> H {
    let mut v = vec![0u8];
    v.extend_from_slice(b);
    sha(&v)
}
fn m_node(l: &H, r: &H) -> H {
    let mut v = vec![1u8];
    v.extend_from_slice(l);
    v.extend_from_slice(r);
    sha(&v)
}
fn split(n: usize) -> usize {
    let mut k = 1;
    while k * 2 < n {
        k *= 2;
    }
    k
}
fn m_root(ls: &[Vec<u8>]) -> H {
    match ls.len() {
        0 => sha(b""),
        1 => m_leaf(&ls[0]),
        n => {
            let k = split(n);
            m_node(&m_root(&ls[..k]), &m_root(&ls[k..]))
        }
    }
}
fn m_path(m: usize, ls: &[Vec<u8>]) -> Vec<H> {
    let n = ls.len();
    if n <= 1 {
        return vec![];
    }
    let k = split(n);
    if m < k {
        let mut p = m_path(m, &ls[..k]);
        p.push(m_root(&ls[k..]));
        p
    } else {
        let mut p = m_path(m - k, &ls[k..]);
        p.push(m_root(&ls[..k]));
        p
    }
}
fn m_verify(root: &H, leaf: &[u8], index: usize, count: usize, proof: &[H]) -> bool {
    if index >= count {
        return false;
    }
    let (mut f, mut s) = (index, count - 1);
    let mut r = m_leaf(leaf);
    for p in proof {
        if s == 0 {
            return false;
        }
        if f & 1 == 1 || f == s {
            r = m_node(p, &r);
            while f & 1 == 0 && f != 0 {
                f >>= 1;
                s >>= 1;
            }
        } else {
            r = m_node(&r, p);
        }
        f >>= 1;
        s >>= 1;
    }
    s == 0 && r == *root
}

// ---------- model: encodings and signing ----------
fn seed(i: u8) -> [u8; 32] {
    [i + 1; 32]
}
fn addr(i: u8) -> [u8; 32] {
    ed25519::public_key(&seed(i))
}
fn tx_body(t: &Tx) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(&t.from);
    v.extend_from_slice(&t.to);
    v.extend_from_slice(&t.amount.to_le_bytes());
    v.extend_from_slice(&t.fee.to_le_bytes());
    v.extend_from_slice(&t.nonce.to_le_bytes());
    v.extend_from_slice(&(t.data.len() as u32).to_le_bytes());
    v.extend_from_slice(&t.data);
    v
}
fn tx_enc(t: &Tx) -> Vec<u8> {
    let mut v = tx_body(t);
    v.extend_from_slice(&t.sig);
    v
}
fn tx_msg(t: &Tx) -> Vec<u8> {
    let mut m = b"aurum-tx\0".to_vec();
    m.extend(tx_body(t));
    m
}
fn mk_tx(from: u8, to: [u8; 32], amount: u64, fee: u64, nonce: u64, data: Vec<u8>) -> Tx {
    let mut t = Tx {
        from: addr(from),
        to,
        amount,
        fee,
        nonce,
        data,
        sig: [0; 64],
    };
    t.sig = ed25519::sign(&seed(from), &tx_msg(&t));
    t
}
fn hdr_enc(h: &Header) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(&h.height.to_le_bytes());
    v.extend_from_slice(&h.parent);
    v.extend_from_slice(&h.tx_root);
    v.extend_from_slice(&h.state_root);
    v.extend_from_slice(&h.time.to_le_bytes());
    v.extend_from_slice(&h.proposer);
    v.extend_from_slice(&h.sig);
    v
}
fn hdr_sign(h: &mut Header, s: &[u8; 32]) {
    let mut m = b"aurum-blk".to_vec();
    m.extend_from_slice(&hdr_enc(h)[..144]);
    h.sig = ed25519::sign(s, &m);
}
fn blk_enc(b: &Block) -> Vec<u8> {
    let mut v = hdr_enc(&b.header);
    v.extend_from_slice(&(b.txs.len() as u32).to_le_bytes());
    for t in &b.txs {
        v.extend(tx_enc(t));
    }
    v
}
fn tx_root(txs: &[Tx]) -> H {
    m_root(&txs.iter().map(tx_enc).collect::<Vec<_>>())
}

// ---------- model: state and chain ----------
#[derive(Clone)]
struct Model {
    acct: BTreeMap<[u8; 32], (u64, u64)>,
    validators: Vec<[u8; 32]>,
    tip: Header,
}
impl Model {
    fn new(genesis: &[([u8; 32], u64)], validators: &[[u8; 32]]) -> Model {
        let mut acct = BTreeMap::new();
        for (a, b) in genesis {
            acct.entry(*a).or_insert((0u64, 0u64)).0 += b;
        }
        let mut m = Model {
            acct,
            validators: validators.to_vec(),
            tip: Header {
                height: 0,
                parent: [0; 32],
                tx_root: sha(b""),
                state_root: [0; 32],
                time: 0,
                proposer: [0; 32],
                sig: [0; 64],
            },
        };
        m.tip.state_root = m.root();
        m
    }
    fn get(&self, a: &[u8; 32]) -> (u64, u64) {
        self.acct.get(a).copied().unwrap_or((0, 0))
    }
    fn root(&self) -> H {
        let leaves: Vec<Vec<u8>> = self
            .acct
            .iter()
            .filter(|(_, v)| **v != (0, 0))
            .map(|(a, (b, n))| {
                let mut l = a.to_vec();
                l.extend_from_slice(&b.to_le_bytes());
                l.extend_from_slice(&n.to_le_bytes());
                l
            })
            .collect();
        m_root(&leaves)
    }
    fn apply(&mut self, t: &Tx, p: &[u8; 32]) -> Result<(), TxError> {
        if t.data.len() > MAX_DATA {
            return Err(TxError::TooLarge);
        }
        if ed25519::verify(&t.from, &tx_msg(t), &t.sig).is_err() {
            return Err(TxError::BadSignature);
        }
        let (bal, non) = self.get(&t.from);
        if t.nonce != non {
            return Err(TxError::BadNonce);
        }
        let cost = t
            .amount
            .checked_add(t.fee)
            .ok_or(TxError::InsufficientFunds)?;
        if cost > bal {
            return Err(TxError::InsufficientFunds);
        }
        self.acct.insert(t.from, (bal - cost, non + 1));
        for (who, v) in [(t.to, t.amount), (*p, t.fee)] {
            let e = self.acct.entry(who).or_insert((0, 0));
            e.0 = e.0.checked_add(v).ok_or(TxError::Overflow)?;
        }
        Ok(())
    }
    fn hash(&self) -> H {
        sha(&hdr_enc(&self.tip))
    }
    fn proposer_index(&self) -> usize {
        (self.tip.height as usize + 1) % self.validators.len()
    }
    /// A valid next block from validator seed `v`, and the model after it.
    fn build(&self, v: u8, time: u64, txs: Vec<Tx>) -> (Block, Model) {
        let mut next = self.clone();
        let p = addr(v);
        for t in &txs {
            next.apply(t, &p).unwrap();
        }
        let mut h = Header {
            height: self.tip.height + 1,
            parent: self.hash(),
            tx_root: tx_root(&txs),
            state_root: next.root(),
            time,
            proposer: p,
            sig: [0; 64],
        };
        hdr_sign(&mut h, &seed(v));
        next.tip = h.clone();
        (Block { header: h, txs }, next)
    }
}

// Validators are keys 0, 1, 2; keys 3, 4, 5 are plain accounts.
fn setup() -> (Chain, Model) {
    let genesis: Vec<([u8; 32], u64)> = (0..6)
        .map(|i| (addr(i), 1_000_000 + i as u64))
        .chain([(addr(0), 5)])
        .collect();
    let vals = [addr(0), addr(1), addr(2)];
    (Chain::new(&genesis, &vals), Model::new(&genesis, &vals))
}

fn agree(c: &Chain, m: &Model, ctx: &str) {
    assert_eq!(c.height(), m.tip.height, "{ctx}");
    assert_eq!(*c.tip(), m.tip, "{ctx}: tip header");
    assert_eq!(c.tip().hash(), m.hash(), "{ctx}: tip hash");
    assert_eq!(c.state_root(), m.root(), "{ctx}: state root");
    for i in 0..8 {
        let a = if i < 6 { addr(i) } else { [i; 32] };
        assert_eq!(c.account(&a), m.get(&a), "{ctx}: account {i}");
    }
    assert_eq!(
        c.expected_proposer(),
        m.validators[m.proposer_index()],
        "{ctx}"
    );
}

const VECTORS: [&str; 8] = [
    "6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d",
    "fac54203e7cc696cf0dfcb42c92a1d9dbaf70ad9e621f4bd8d98662f00e3c125",
    "aeb6bcfe274b70a14fb067a5e5578264db0fa9b51af5e0ba159158f329e06e77",
    "d37ee418976dd95753c1c73862b9398fa2a2cf9b4ff0fdfe8b30cd95209614b7",
    "4e3bbb1f7b478dcfe71fb631631519a3bca12c9aefca1612bfce4c13a86264d4",
    "76e67dadbcdf1e10e1b74ddc608abd2f98dfb16fbce75277b5232a127f2087ef",
    "ddb89be403809e325750d3d263cd78929c2942b7942a34b77e122c9594a74c8c",
    "5dc9da79a70659a9ad559cb701ded9a2ab9d823aad2f4960cfe370eff4604328",
];
fn rfc_leaves() -> Vec<Vec<u8>> {
    [
        "",
        "00",
        "10",
        "2021",
        "3031",
        "40414243",
        "5051525354555657",
        "606162636465666768696a6b6c6d6e6f",
    ]
    .iter()
    .map(|h| assay::hex(h))
    .collect()
}
fn random_leaves(r: &mut Rng, n: usize) -> Vec<Vec<u8>> {
    (0..n)
        .map(|_| {
            let mut v = vec![0u8; r.below(40)];
            r.fill(&mut v);
            v
        })
        .collect()
}

#[test]
fn model_self_check() {
    let l = rfc_leaves();
    for n in 1..=8 {
        assert_eq!(assay::to_hex(&m_root(&l[..n])), VECTORS[n - 1]);
    }
    let mut r = Rng::new(0x48_5e1f_0001);
    for n in 1..=33 {
        let ls = random_leaves(&mut r, n);
        let root = m_root(&ls);
        for i in 0..n {
            let p = m_path(i, &ls);
            assert!(m_verify(&root, &ls[i], i, n, &p));
            if n > 1 {
                assert!(!m_verify(&root, &ls[i], (i + 1) % n, n, &p), "n {n} i {i}");
            }
        }
    }
}

#[test]
fn merkle_matches_model() {
    let l = rfc_leaves();
    assert_eq!(merkle_root(&[]), sha(b""));
    for n in 1..=8 {
        assert_eq!(
            assay::to_hex(&merkle_root(&l[..n])),
            VECTORS[n - 1],
            "rfc vector {n}"
        );
    }
    let mut r = Rng::new(0x48_5e1f_0002);
    for n in 0..=70usize {
        let ls = random_leaves(&mut r, n);
        let root = merkle_root(&ls);
        assert_eq!(root, m_root(&ls), "root of {n}");
        assert!(merkle_proof(&ls, n).is_empty(), "proof past the end, n {n}");
        assert!(
            !merkle_verify(&root, b"x", n, n, &[]),
            "index == count, n {n}"
        );
        for i in 0..n {
            let p = merkle_proof(&ls, i);
            assert_eq!(p, m_path(i, &ls), "proof n {n} i {i}");
            assert!(merkle_verify(&root, &ls[i], i, n, &p), "verify n {n} i {i}");
            // equal leaves make a wrong-index proof legitimately verify
            if n > 1 && ls[i] != ls[(i + 1) % n] {
                assert!(
                    !merkle_verify(&root, &ls[i], (i + 1) % n, n, &p),
                    "index n {n} i {i}"
                );
            }
            let mut leaf = ls[i].clone();
            leaf.push(0);
            assert!(!merkle_verify(&root, &leaf, i, n, &p), "leaf n {n} i {i}");
            let mut longer = p.clone();
            longer.push([7; 32]);
            assert!(
                !merkle_verify(&root, &ls[i], i, n, &longer),
                "longer n {n} i {i}"
            );
            if !p.is_empty() {
                assert!(
                    !merkle_verify(&root, &ls[i], i, n, &p[..p.len() - 1]),
                    "shorter"
                );
                let mut bad = p.clone();
                let k = r.below(bad.len());
                bad[k][r.below(32)] ^= 1 << r.below(8);
                assert!(
                    !merkle_verify(&root, &ls[i], i, n, &bad),
                    "flip n {n} i {i}"
                );
            }
            let mut other = root;
            other[0] ^= 1;
            assert!(!merkle_verify(&other, &ls[i], i, n, &p), "root n {n} i {i}");
        }
    }
}

#[test]
fn tx_encoding_and_decoding() {
    let mut r = Rng::new(0x48_5e1f_0003);
    for len in [0usize, 1, 7, 100, MAX_DATA] {
        let mut data = vec![0u8; len];
        r.fill(&mut data);
        let (amount, fee, nonce) = (r.u64(), r.u64(), r.u64());
        let t = Tx::signed(&seed(3), addr(4), amount, fee, nonce, data.clone());
        let m = mk_tx(3, addr(4), amount, fee, nonce, data);
        assert_eq!(t, m, "Tx::signed, data {len}");
        assert_eq!(t.body(), tx_body(&m));
        assert_eq!(t.encode(), tx_enc(&m));
        assert_eq!(t.encode().len(), 156 + len);
        assert_eq!(t.id(), sha(&tx_enc(&m)));
        assert!(t.verify());
        assert_eq!(Tx::decode(&t.encode()), Ok(t.clone()));
        let enc = t.encode();
        for cut in [0, 1, 31, 64, 87, 91, enc.len() - 1] {
            assert_eq!(
                Tx::decode(&enc[..cut]),
                Err(DecodeError::Truncated),
                "cut {cut}"
            );
        }
        let mut longer = enc.clone();
        longer.push(0);
        assert_eq!(Tx::decode(&longer), Err(DecodeError::TrailingBytes));
        for field in [0usize, 40, 70, 90, 92, enc.len() - 3] {
            let mut bad = t.clone();
            let mut e = enc.clone();
            e[field] ^= 4;
            if let Ok(d) = Tx::decode(&e) {
                bad = d;
            }
            assert!(!bad.verify() || bad == t, "flip at {field} still verifies");
        }
    }
    // Length above MAX_DATA is TooLarge as soon as it is read.
    let t = mk_tx(1, addr(2), 5, 1, 0, vec![]);
    let mut e = tx_body(&t);
    e[88..92].copy_from_slice(&((MAX_DATA as u32) + 1).to_le_bytes());
    assert_eq!(Tx::decode(&e), Err(DecodeError::TooLarge));
    let mut s = t.clone();
    s.sig[10] ^= 1;
    assert!(!s.verify());
    let mut s = t.clone();
    s.amount += 1;
    assert!(!s.verify());
}

#[test]
fn header_and_block_encoding() {
    let (c, m) = setup();
    let g = c.genesis();
    assert_eq!(g.header, m.tip, "genesis header");
    assert!(g.txs.is_empty());
    assert_eq!(c.block(0), Some(g));
    assert_eq!(c.block(1), None);
    let txs = vec![
        mk_tx(3, addr(4), 10, 2, 0, b"hi".to_vec()),
        mk_tx(4, addr(3), 7, 0, 0, vec![]),
        mk_tx(3, [9; 32], 1, 1, 1, vec![1; 30]),
    ];
    let v = m.proposer_index() as u8;
    let (b, _) = m.build(v, 10, txs.clone());
    assert_eq!(b.header.encode(), hdr_enc(&b.header));
    assert_eq!(b.header.encode().len(), 208);
    assert_eq!(b.header.hash(), sha(&hdr_enc(&b.header)));
    assert_eq!(b.hash(), b.header.hash());
    assert!(b.header.verify());
    let mut h2 = b.header.clone();
    h2.sig = [0; 64];
    h2.sign(&seed(v));
    assert_eq!(h2, b.header, "Header::sign");
    h2.time += 1;
    assert!(!h2.verify());
    assert_eq!(b.encode(), blk_enc(&b));
    assert_eq!(Block::decode(&b.encode()), Ok(b.clone()));
    let enc = b.encode();
    for cut in [0, 100, 207, 208, 211, 212, 300, enc.len() - 1] {
        assert_eq!(
            Block::decode(&enc[..cut]),
            Err(DecodeError::Truncated),
            "cut {cut}"
        );
    }
    let mut longer = enc.clone();
    longer.push(9);
    assert_eq!(Block::decode(&longer), Err(DecodeError::TrailingBytes));
    let mut many = hdr_enc(&b.header);
    many.extend_from_slice(&((MAX_TXS as u32) + 1).to_le_bytes());
    assert_eq!(Block::decode(&many), Err(DecodeError::TooLarge));
    let p = c.propose(&seed(v), 10, txs).unwrap();
    assert_eq!(p, b, "propose builds the same block as the model");
}

#[test]
fn chain_follows_model() {
    let (mut c, mut m) = setup();
    let mut r = Rng::new(0x48_5e1f_0004);
    let mut time = 0;
    for blk in 0..40 {
        let mut txs = Vec::new();
        let mut scratch = m.clone();
        let v = m.proposer_index() as u8;
        for _ in 0..r.below(13) {
            let from = r.below(6) as u8;
            let to = if r.below(8) == 0 {
                [0xee; 32]
            } else {
                addr(r.below(6) as u8)
            };
            let (bal, nonce) = scratch.get(&addr(from));
            let fee = r.below(5) as u64;
            if bal < fee {
                continue;
            }
            let amount = r.u64() % (bal - fee + 1).min(50_000);
            let mut data = vec![0u8; r.below(40)];
            r.fill(&mut data);
            let t = mk_tx(from, to, amount, fee, nonce, data);
            scratch.apply(&t, &addr(v)).unwrap();
            txs.push(t);
        }
        time += 1 + r.below(30) as u64;
        let (b, next) = m.build(v, time, txs.clone());
        assert_eq!(
            c.propose(&seed(v), time, txs).unwrap(),
            b,
            "block {blk}: propose"
        );
        let wire = Block::decode(&blk_enc(&b)).unwrap();
        c.append(wire)
            .unwrap_or_else(|e| panic!("block {blk}: {e:?}"));
        m = next;
        agree(&c, &m, &format!("after block {blk}"));
        assert_eq!(c.block(m.tip.height).unwrap(), &b);
    }
    for h in 0..=40u64 {
        assert_eq!(c.block(h).unwrap().header.height, h);
    }
}

fn reject(c: &mut Chain, m: &Model, b: Block, want: BlockError, ctx: &str) {
    let got = c.append(b);
    assert_eq!(got, Err(want), "{ctx}");
    agree(c, m, &format!("{ctx}: chain changed after a reject"));
}

/// Re-sign after editing a header, and recompute roots when `roots` is set.
fn reseal(m: &Model, mut b: Block, v: u8, roots: bool) -> Block {
    if roots {
        b.header.tx_root = tx_root(&b.txs);
        let mut s = m.clone();
        for t in &b.txs {
            let _ = s.apply(t, &b.header.proposer);
        }
        b.header.state_root = s.root();
    }
    hdr_sign(&mut b.header, &seed(v));
    b
}

#[test]
fn invalid_block_corpus() {
    let (mut c, mut m) = setup();
    // Two good blocks first so nonces and time are non-trivial.
    for (i, t) in [5u64, 9].iter().enumerate() {
        let v = m.proposer_index() as u8;
        let (b, next) = m.build(v, *t, vec![mk_tx(3, addr(4), 100, 3, i as u64, vec![])]);
        c.append(b).unwrap();
        m = next;
    }
    let v = m.proposer_index() as u8;
    let w = ((v as usize + 1) % 3) as u8; // another validator
    let replay = c.block(2).unwrap().txs[0].clone();
    let good = vec![
        mk_tx(3, addr(5), 50, 1, 2, vec![]),
        mk_tx(4, addr(3), 20, 0, 0, vec![1, 2]),
    ];
    let (valid, after) = m.build(v, 20, good.clone());
    let e = |b: &Block| b.clone();

    let mut b = e(&valid);
    b.header.height += 1;
    reject(
        &mut c,
        &m,
        reseal(&m, b, v, false),
        BlockError::BadHeight,
        "height + 1",
    );
    let mut b = e(&valid);
    b.header.height -= 1;
    reject(
        &mut c,
        &m,
        reseal(&m, b, v, false),
        BlockError::BadHeight,
        "height - 1",
    );
    let mut b = e(&valid);
    b.header.parent[3] ^= 1;
    reject(
        &mut c,
        &m,
        reseal(&m, b, v, false),
        BlockError::BadParent,
        "parent",
    );
    let mut b = e(&valid);
    b.header.time = m.tip.time;
    reject(
        &mut c,
        &m,
        reseal(&m, b, v, false),
        BlockError::BadTime,
        "time equal",
    );
    let mut b = e(&valid);
    b.header.time = 1;
    reject(
        &mut c,
        &m,
        reseal(&m, b, v, false),
        BlockError::BadTime,
        "time back",
    );
    let (b, _) = m.build(w, 20, good.clone());
    reject(&mut c, &m, b, BlockError::WrongProposer, "other validator");
    let mut b = e(&valid);
    b.header.proposer = addr(4);
    reject(
        &mut c,
        &m,
        reseal(&m, b, 4, true),
        BlockError::WrongProposer,
        "not a validator",
    );
    let mut b = e(&valid);
    b.header.sig[5] ^= 1;
    reject(
        &mut c,
        &m,
        b,
        BlockError::BadSignature,
        "flipped header sig",
    );
    let mut b = e(&valid);
    hdr_sign(&mut b.header, &seed(w));
    reject(
        &mut c,
        &m,
        b,
        BlockError::BadSignature,
        "signed by the wrong key",
    );
    let mut b = e(&valid);
    b.header.state_root[0] ^= 1;
    reject(
        &mut c,
        &m,
        b,
        BlockError::BadSignature,
        "edited after signing",
    );
    let mut b = e(&valid);
    b.txs = vec![good[0].clone(); MAX_TXS + 1];
    reject(
        &mut c,
        &m,
        reseal(&m, b, v, false),
        BlockError::TooLarge,
        "too many txs",
    );
    let mut b = e(&valid);
    b.header.tx_root[9] ^= 1;
    reject(
        &mut c,
        &m,
        reseal(&m, b, v, false),
        BlockError::BadTxRoot,
        "tx root",
    );
    let mut b = e(&valid);
    b.txs.swap(0, 1);
    reject(
        &mut c,
        &m,
        reseal(&m, b, v, false),
        BlockError::BadTxRoot,
        "tx order",
    );

    let bad_tx = |t: Tx, idx: usize, err: TxError, ctx: &str, c: &mut Chain| {
        let mut txs = good.clone();
        txs.insert(idx, t);
        let b = Block {
            header: valid.header.clone(),
            txs,
        };
        reject(
            c,
            &m,
            reseal(&m, b, v, true),
            BlockError::BadTx {
                index: idx,
                error: err,
            },
            ctx,
        );
    };
    let mut t = mk_tx(5, addr(3), 1, 0, 0, vec![]);
    t.sig[0] ^= 1;
    bad_tx(t, 1, TxError::BadSignature, "tx sig", &mut c);
    let mut t = mk_tx(5, addr(3), 1, 0, 0, vec![]);
    t.from = addr(4);
    bad_tx(t, 0, TxError::BadSignature, "tx from swapped", &mut c);
    bad_tx(replay, 0, TxError::BadNonce, "replayed tx", &mut c);
    bad_tx(
        mk_tx(5, addr(3), 1, 0, 1, vec![]),
        2,
        TxError::BadNonce,
        "future nonce",
        &mut c,
    );
    // Nonce 2 then 2 again from the same sender inside one block.
    bad_tx(
        mk_tx(3, addr(4), 1, 0, 2, vec![]),
        1,
        TxError::BadNonce,
        "dup nonce",
        &mut c,
    );
    let bal5 = m.get(&addr(5)).0;
    bad_tx(
        mk_tx(5, addr(3), bal5, 1, 0, vec![]),
        0,
        TxError::InsufficientFunds,
        "funds",
        &mut c,
    );
    bad_tx(
        mk_tx(5, addr(3), u64::MAX, 1, 0, vec![]),
        0,
        TxError::InsufficientFunds,
        "sum",
        &mut c,
    );
    let big = mk_tx(5, addr(3), 1, 0, 0, vec![0; MAX_DATA + 1]);
    bad_tx(big, 0, TxError::TooLarge, "data too large", &mut c);
    let mut b = e(&valid);
    b.header.state_root[1] ^= 1;
    reject(
        &mut c,
        &m,
        reseal(&m, b, v, false),
        BlockError::BadStateRoot,
        "state root",
    );

    // Precedence: the earlier check wins.
    let mut b = e(&valid);
    b.header.height += 1;
    b.header.time = 0;
    reject(
        &mut c,
        &m,
        reseal(&m, b, w, false),
        BlockError::BadHeight,
        "height before time",
    );
    let mut b = e(&valid);
    b.header.time = 0;
    reject(
        &mut c,
        &m,
        reseal(&m, b, w, false),
        BlockError::BadTime,
        "time before proposer",
    );
    let mut b = e(&valid);
    b.header.tx_root[0] ^= 1;
    b.txs[0].sig[0] ^= 1;
    reject(
        &mut c,
        &m,
        b,
        BlockError::BadSignature,
        "header sig before tx root",
    );
    let mut b = e(&valid);
    b.txs[0].sig[0] ^= 1;
    let mut b = reseal(&m, b, v, false);
    b.header.tx_root[0] ^= 1;
    reject(
        &mut c,
        &m,
        reseal(&m, b, v, false),
        BlockError::BadTxRoot,
        "tx root before txs",
    );

    // propose reports the first bad tx.
    let mut bad = good.clone();
    bad.push(mk_tx(5, addr(3), 1, 0, 9, vec![]));
    assert_eq!(
        c.propose(&seed(v), 20, bad),
        Err(BlockError::BadTx {
            index: 2,
            error: TxError::BadNonce
        })
    );
    // The valid block still goes in.
    c.append(valid).unwrap();
    agree(&c, &after, "valid block after the corpus");
}

#[test]
fn overflow_and_self_transfer() {
    let near = u64::MAX - 10;
    let genesis = [
        (addr(0), 100),
        (addr(3), 100),
        (addr(4), near),
        (addr(1), near),
    ];
    let vals = [addr(0), addr(1), addr(2)];
    let mut c = Chain::new(&genesis, &vals);
    let m = Model::new(&genesis, &vals);
    agree(&c, &m, "genesis");
    // Proposer at height 1 is validator 1, whose balance is near the top: a fee overflows.
    let v = m.proposer_index() as u8;
    assert_eq!(v, 1);
    let to_full = mk_tx(3, addr(4), 11, 0, 0, vec![]);
    let b = Block {
        header: m.tip.clone(),
        txs: vec![to_full],
    };
    let mut b = b;
    b.header.height = 1;
    b.header.parent = m.hash();
    b.header.time = 1;
    b.header.proposer = addr(v);
    let b1 = reseal(&m, b.clone(), v, true);
    reject(
        &mut c,
        &m,
        b1,
        BlockError::BadTx {
            index: 0,
            error: TxError::Overflow,
        },
        "credit",
    );
    let mut b2 = b.clone();
    b2.txs = vec![mk_tx(3, addr(5), 1, 11, 0, vec![])];
    let b2 = reseal(&m, b2, v, true);
    reject(
        &mut c,
        &m,
        b2,
        BlockError::BadTx {
            index: 0,
            error: TxError::Overflow,
        },
        "fee",
    );
    // Self transfer: only the fee leaves, nonce moves; a zeroed account leaves the root.
    let txs = vec![
        mk_tx(3, addr(3), 40, 1, 0, vec![]),
        mk_tx(3, addr(5), 99, 0, 1, vec![]),
    ];
    let (blk, next) = m.build(v, 1, txs);
    c.append(blk).unwrap();
    agree(&c, &next, "self transfer");
    assert_eq!(c.account(&addr(3)), (0, 2));
}

#[test]
fn decoders_never_panic() {
    let (mut c, m) = setup();
    let v = m.proposer_index() as u8;
    let (b, _) = m.build(v, 3, vec![mk_tx(3, addr(4), 1, 1, 0, b"abc".to_vec())]);
    let enc = blk_enc(&b);
    let mut r = Rng::new(0x48_5e1f_0005);
    for _ in 0..3000 {
        let mut e = enc.clone();
        match r.below(3) {
            0 => {
                let i = r.below(e.len());
                e[i] ^= 1 << r.below(8);
            }
            1 => e.truncate(r.below(e.len())),
            _ => {
                let mut x = vec![0u8; r.below(400)];
                r.fill(&mut x);
                e = x;
            }
        }
        let _ = Tx::decode(&e);
        if let Ok(d) = Block::decode(&e) {
            assert_eq!(Block::decode(&d.encode()), Ok(d.clone()));
            if d != b && c.append(d).is_ok() {
                panic!("a mutated block was accepted");
            }
            agree(&c, &m, "after fuzz append");
        }
    }
    c.append(b).unwrap();
}

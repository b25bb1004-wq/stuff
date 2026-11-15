//! Leader check for T49: contracts on aurum. Own model of balances, refunds, fees, contract
//! storage (counter, token, revert probe) and the state root with contract leaves, over a
//! 30-block random chain on 3 nodes. Merkle and tx helpers copied from seam_leader.rs.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::too_many_lines,
    clippy::type_complexity,
    clippy::cast_possible_truncation,
    dead_code
)]
use assay::Rng;
use sphinx::ed25519;
use sphinx::sha256::digest as sha;
use aurum::{contract_address, Block, Chain, Header, Receipt, Tx, DEPLOY, FUEL_PER_FEE};
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

// ---------- contract sources ----------
const COUNTER: &str = r#"
fn get(k: [byte]) -> [byte]? uses chain = extern "chain.get"
fn set(k: [byte], v: [byte]) uses chain = extern "chain.set"
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
    let cur = match get(bytes("n")) { none => 0, some(b) => dec(b) }
    set(bytes("n"), enc(cur + 1))
    emit(str(cur + 1))
}
"#;

const TOKEN: &str = r#"fn get(k: [byte]) -> [byte]? uses chain = extern "chain.get"
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

/// Writes then: input of 1 byte traps, 2 bytes spins until out of fuel, else succeeds.
const PROBE: &str = r#"
fn set(k: [byte], v: [byte]) uses chain = extern "chain.set"
fn input() -> [byte] uses chain = extern "chain.input"
fn value() -> int uses chain = extern "chain.value"
fn emit(e: str) uses chain = extern "chain.emit"
fn main() uses chain {
    set(bytes("k"), bytes("dirty"))
    emit(str(value()))
    let inp = input()
    if len(inp) == 1 { assert false, "boom" }
    if len(inp) == 2 {
        var i = 0
        while i >= 0 { i = i + 1 }
    }
}
"#;

const BAD: [&[u8]; 5] = [
    b"fn print(s: str) uses io = extern \"io.print\"\nfn main() uses io { print(\"hi\") }\n",
    b"fn main( { broken",
    b"fn helper() -> int { 1 }",
    b"fn nope() uses chain = extern \"chain.nope\"\nfn main() uses chain { nope() }\n",
    &[0xff, 0xfe, 0x00, 0x41],
];

// ---------- model ----------
#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Counter,
    Token,
    Probe,
}

#[derive(Clone)]
struct World {
    acct: BTreeMap<H, (u64, u64)>,
    code: BTreeMap<H, (Kind, &'static str, BTreeMap<Vec<u8>, Vec<u8>>)>,
}

fn enc8(n: u64) -> Vec<u8> {
    n.to_le_bytes().to_vec()
}
fn dec8(b: &[u8]) -> u64 {
    u64::from_le_bytes(b[..8].try_into().unwrap())
}

impl World {
    fn get(&self, a: &H) -> (u64, u64) {
        self.acct.get(a).copied().unwrap_or((0, 0))
    }
    fn credit(&mut self, a: H, v: u64) {
        self.acct.entry(a).or_insert((0, 0)).0 += v;
    }
    fn debit(&mut self, a: H, v: u64) {
        self.acct.get_mut(&a).unwrap().0 -= v;
    }
    fn root(&self) -> H {
        let mut keys: Vec<H> = self.acct.keys().chain(self.code.keys()).copied().collect();
        keys.sort_unstable();
        keys.dedup();
        let mut leaves = Vec::new();
        for a in keys {
            let (b, n) = self.get(&a);
            if (b, n) != (0, 0) {
                let mut l = a.to_vec();
                l.extend_from_slice(&b.to_le_bytes());
                l.extend_from_slice(&n.to_le_bytes());
                leaves.push(l);
            }
            if let Some((_, src, st)) = self.code.get(&a) {
                let sl: Vec<Vec<u8>> = st
                    .iter()
                    .map(|(k, v)| {
                        let mut l = (k.len() as u32).to_le_bytes().to_vec();
                        l.extend_from_slice(k);
                        l.extend_from_slice(v);
                        l
                    })
                    .collect();
                let mut l = a.to_vec();
                l.extend_from_slice(&sha(src.as_bytes()));
                l.extend_from_slice(&m_root(&sl));
                leaves.push(l);
            }
        }
        m_root(&leaves)
    }
    /// Applies a valid tx; returns the expected receipt with `fuel_used` replaced by a class:
    /// 0 = must be 0, 1 = in 1..=budget, 2 = must equal the budget.
    fn apply(
        &mut self,
        t: &Tx,
        proposer: H,
        src_of: &dyn Fn(&[u8]) -> Option<(Kind, &'static str)>,
    ) -> (Receipt, u8) {
        let (_, nonce) = self.get(&t.from);
        self.debit(t.from, t.amount + t.fee);
        self.acct.get_mut(&t.from).unwrap().1 += 1;
        self.credit(proposer, t.fee);
        let plain = Receipt {
            ok: true,
            fuel_used: 0,
            events: vec![],
            contract: None,
        };
        if t.to == DEPLOY && !t.data.is_empty() {
            return match src_of(&t.data) {
                Some((k, src)) => {
                    let a = contract_address(&t.from, nonce);
                    self.credit(a, t.amount);
                    self.code.insert(a, (k, src, BTreeMap::new()));
                    (
                        Receipt {
                            contract: Some(a),
                            ..plain
                        },
                        0,
                    )
                }
                None => {
                    self.credit(t.from, t.amount);
                    (Receipt { ok: false, ..plain }, 0)
                }
            };
        }
        let Some((kind, _, st)) = self.code.get(&t.to).cloned() else {
            self.credit(t.to, t.amount);
            return (plain, 0);
        };
        let mut st = st;
        let fail = |w: &mut World, class| {
            w.credit(t.from, t.amount);
            (
                Receipt {
                    ok: false,
                    ..plain.clone()
                },
                class,
            )
        };
        let events = match kind {
            Kind::Counter => {
                let n = st.get(&b"n"[..]).map_or(0, |b| dec8(b)) + 1;
                st.insert(b"n".to_vec(), enc8(n));
                vec![n.to_string()]
            }
            Kind::Token => {
                let me = t.from.to_vec();
                if t.data.is_empty() {
                    st.insert(me, enc8(1000));
                    vec!["mint".to_string()]
                } else {
                    let to = t.data[..32].to_vec();
                    let amt = dec8(&t.data[32..40]);
                    let sb = st.get(&me).map_or(0, |b| dec8(b));
                    if sb < amt {
                        return fail(self, 1);
                    }
                    let rb = st.get(&to).map_or(0, |b| dec8(b));
                    st.insert(me, enc8(sb - amt));
                    st.insert(to, enc8(rb + amt));
                    vec!["xfer".to_string()]
                }
            }
            Kind::Probe => match t.data.len() {
                1 => return fail(self, 1),
                2 => return fail(self, 2),
                _ => {
                    st.insert(b"k".to_vec(), b"dirty".to_vec());
                    vec![t.amount.to_string()]
                }
            },
        };
        self.credit(t.to, t.amount);
        self.code.get_mut(&t.to).unwrap().2 = st;
        (
            Receipt {
                ok: true,
                fuel_used: 0,
                events,
                contract: None,
            },
            1,
        )
    }
}

fn src_of(d: &[u8]) -> Option<(Kind, &'static str)> {
    [
        (Kind::Counter, COUNTER),
        (Kind::Token, TOKEN),
        (Kind::Probe, PROBE),
    ]
    .into_iter()
    .find(|(_, s)| s.as_bytes() == d)
}

fn check_receipt(got: &Receipt, want: &Receipt, class: u8, fee: u64, ctx: &str) {
    assert_eq!(
        (got.ok, &got.events, got.contract),
        (want.ok, &want.events, want.contract),
        "{ctx}"
    );
    let budget = fee.saturating_mul(FUEL_PER_FEE);
    match class {
        0 => assert_eq!(got.fuel_used, 0, "{ctx}: fuel"),
        1 => assert!(
            got.fuel_used >= 1 && got.fuel_used <= budget,
            "{ctx}: fuel {} of {budget}",
            got.fuel_used
        ),
        _ => assert_eq!(
            got.fuel_used, budget,
            "{ctx}: out of fuel uses the whole budget"
        ),
    }
}

#[test]
fn random_contract_chain_on_three_nodes() {
    let mut r = Rng::new(0x49_0001);
    let genesis: Vec<(H, u64)> = (0..6).map(|i| (addr(i), 10_000_000)).collect();
    let vals = [addr(0), addr(1), addr(2)];
    let mut nodes: Vec<Chain> = (0..3).map(|_| Chain::new(&genesis, &vals)).collect();
    let mut w = World {
        acct: genesis.iter().map(|&(a, b)| (a, (b, 0))).collect(),
        code: BTreeMap::new(),
    };
    assert_eq!(nodes[0].state_root(), w.root(), "genesis root");
    let users: Vec<u8> = (0..6).collect();
    let mut seen = [false; 3];
    for height in 1..=30u64 {
        let mut txs = Vec::new();
        let mut shadow = w.clone();
        let mut wants = Vec::new();
        let proposer = nodes[0].expected_proposer();
        let pseed = seed((0..3).find(|&i| addr(i) == proposer).unwrap());
        for _ in 0..1 + r.below(5) {
            let u = *r.pick(&users);
            let nonce = shadow.get(&addr(u)).1;
            let fee = 30 + r.below(150) as u64;
            let amount = r.below(500) as u64;
            let contracts: Vec<(H, Kind)> = shadow.code.iter().map(|(a, c)| (*a, c.0)).collect();
            let pick = r.below(10);
            let (to, data) = if height <= 3 || pick == 0 {
                let src = [COUNTER, TOKEN, PROBE][(height as usize + r.below(3)) % 3];
                (DEPLOY, src.as_bytes().to_vec())
            } else if pick == 1 {
                seen[0] = true;
                (DEPLOY, r.pick(&BAD).to_vec())
            } else if pick == 2 {
                (
                    if r.chance(1, 2) {
                        DEPLOY
                    } else {
                        addr(r.below(6) as u8)
                    },
                    Vec::new(),
                )
            } else {
                let (a, k) = *r.pick(&contracts);
                let data = match k {
                    Kind::Counter => Vec::new(),
                    Kind::Token if r.chance(1, 3) => Vec::new(),
                    Kind::Token => {
                        let mut d = addr(r.below(6) as u8).to_vec();
                        d.extend(enc8(r.below(1400) as u64));
                        d
                    }
                    Kind::Probe => vec![7; r.below(4)],
                };
                if k == Kind::Probe && data.len() == 2 {
                    seen[1] = true;
                }
                (a, data)
            };
            let t = Tx::signed(&seed(u), to, amount, fee, nonce, data);
            wants.push(shadow.apply(&t, proposer, &src_of));
            txs.push(t);
        }
        let blk = nodes[0].propose(&pseed, height * 10, txs.clone()).unwrap();
        let bytes = blk.encode();
        for (i, n) in nodes.iter_mut().enumerate() {
            n.append(Block::decode(&bytes).unwrap())
                .unwrap_or_else(|e| panic!("node {i} h {height}: {e:?}"));
        }
        w = shadow;
        let got = nodes[0].receipts(height).unwrap().to_vec();
        assert_eq!(got.len(), txs.len());
        for (j, ((want, class), t)) in wants.iter().zip(&txs).enumerate() {
            check_receipt(
                &got[j],
                want,
                *class,
                t.fee,
                &format!("h {height} tx {j} to {:?}", &t.to[..4]),
            );
        }
        for n in &nodes[1..] {
            assert_eq!(
                n.receipts(height).unwrap(),
                &got[..],
                "h {height}: receipts differ across nodes"
            );
            assert_eq!(
                n.state_root(),
                nodes[0].state_root(),
                "h {height}: roots differ across nodes"
            );
        }
        assert_eq!(
            nodes[0].state_root(),
            w.root(),
            "h {height}: state root model"
        );
        for i in 0..6 {
            assert_eq!(
                nodes[0].account(&addr(i)),
                w.get(&addr(i)),
                "h {height}: account {i}"
            );
        }
        for (a, (_, src, st)) in &w.code {
            assert_eq!(nodes[0].code(a), Some(*src));
            assert_eq!(
                nodes[0].account(a),
                w.get(a),
                "h {height}: contract balance"
            );
            for (k, v) in st {
                assert_eq!(nodes[0].storage(a, k), Some(&v[..]), "h {height}: storage");
            }
        }
        seen[2] |= w.code.len() >= 3;
    }
    assert!(
        seen.iter().all(|&s| s),
        "generator never hit every path: {seen:?}"
    );
}

#[test]
fn rejected_deploy_refunds_and_leaves_no_code() {
    let genesis: Vec<(H, u64)> = (0..3).map(|i| (addr(i), 1_000_000)).collect();
    let vals = [addr(0)];
    let mut c = Chain::new(&genesis, &vals);
    let mut txs = Vec::new();
    for (n, bad) in BAD.iter().enumerate() {
        txs.push(Tx::signed(
            &seed(1),
            DEPLOY,
            1000 + n as u64,
            7,
            n as u64,
            bad.to_vec(),
        ));
    }
    let blk = c.propose(&seed(0), 5, txs).unwrap();
    c.append(blk).unwrap();
    let paid: u64 = 7 * BAD.len() as u64;
    assert_eq!(c.account(&addr(1)), (1_000_000 - paid, BAD.len() as u64));
    assert_eq!(c.account(&addr(0)).0, 1_000_000 + paid);
    for (n, rc) in c.receipts(1).unwrap().iter().enumerate() {
        assert!(
            !rc.ok && rc.contract.is_none() && rc.events.is_empty(),
            "bad source {n} accepted"
        );
        assert!(c.code(&contract_address(&addr(1), n as u64)).is_none());
    }
}

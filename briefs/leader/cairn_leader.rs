//! Leader check for T46 (cairn). The leader copies it to crates/cairn/tests/leader.rs.
use assay::{forall, Rng};
use cairn::lock;
use cairn::manifest::Manifest;
use cairn::solve::{solve, Index, Solution};
use cairn::version::{Req, Version};
use std::collections::BTreeMap;
use std::time::Instant;

type V3 = (u64, u64, u64);

fn ver(t: V3) -> Version {
    Version {
        major: t.0,
        minor: t.1,
        patch: t.2,
    }
}
fn tup(v: &Version) -> V3 {
    (v.major, v.minor, v.patch)
}

// ---------- independent requirement model ----------

#[derive(Clone, Debug)]
struct Cmp {
    op: &'static str, // "" bare (caret), "*" any, or an operator
    parts: Vec<u64>,
    wild: bool, // "1.*" / "1.2.*"
}

fn cmp_text(c: &Cmp) -> String {
    if c.op == "*" {
        return "*".into();
    }
    let mut s = c
        .parts
        .iter()
        .map(|p| p.to_string())
        .collect::<Vec<_>>()
        .join(".");
    if c.wild {
        s.push_str(".*");
    }
    s
}

fn next(p: &[u64]) -> V3 {
    match p.len() {
        1 => (p[0] + 1, 0, 0),
        2 => (p[0], p[1] + 1, 0),
        _ => (p[0], p[1], p[2] + 1),
    }
}
fn low(p: &[u64]) -> V3 {
    (p[0], *p.get(1).unwrap_or(&0), *p.get(2).unwrap_or(&0))
}

fn cmp_match(c: &Cmp, v: V3) -> bool {
    if c.op == "*" {
        return true;
    }
    let p = &c.parts;
    let lo = low(p);
    if c.wild {
        return lo <= v && v < next(p);
    }
    match c.op {
        "" | "^" => {
            let hi = if p[0] > 0 || p.len() == 1 {
                (p[0] + 1, 0, 0)
            } else if p.len() == 2 || p[1] > 0 {
                (0, p[1] + 1, 0)
            } else {
                (0, 0, p[2] + 1)
            };
            lo <= v && v < hi
        }
        "~" => {
            let hi = if p.len() == 1 {
                (p[0] + 1, 0, 0)
            } else {
                (p[0], p[1] + 1, 0)
            };
            lo <= v && v < hi
        }
        "=" => lo <= v && v < next(p),
        ">" => v >= next(p),
        ">=" => v >= lo,
        "<" => v < lo,
        "<=" => v < next(p),
        _ => unreachable!(),
    }
}

fn gen_cmp(r: &mut Rng, max: u64) -> Cmp {
    let ops = ["", "^", "~", "=", ">", ">=", "<", "<=", "*", "w"];
    let op = *r.pick(&ops);
    let n = 1 + r.below(3);
    let mut parts: Vec<u64> = (0..n).map(|_| r.range(0, max)).collect();
    if op == "w" {
        parts.truncate(1 + r.below(2));
        return Cmp {
            op: "",
            parts,
            wild: true,
        };
    }
    Cmp {
        op,
        parts,
        wild: false,
    }
}

fn sp(r: &mut Rng) -> &'static str {
    if r.chance(1, 4) {
        " "
    } else {
        ""
    }
}

/// (text with random spacing, canonical Display text, comparators)
fn gen_req(r: &mut Rng, max_cmps: usize, max: u64) -> (String, String, Vec<Cmp>) {
    let k = 1 + r.below(max_cmps);
    let cmps: Vec<Cmp> = (0..k).map(|_| gen_cmp(r, max)).collect();
    let mut text = String::new();
    let mut canon = Vec::new();
    for (i, c) in cmps.iter().enumerate() {
        if i > 0 {
            text.push_str(sp(r));
            text.push(',');
        }
        text.push_str(sp(r));
        if c.op == "*" {
            text.push('*');
            canon.push("*".to_string());
        } else {
            text.push_str(c.op);
            if !c.op.is_empty() {
                text.push_str(sp(r));
            }
            text.push_str(&cmp_text(c));
            canon.push(format!("{}{}", c.op, cmp_text(c)));
        }
    }
    text.push_str(sp(r));
    (text, canon.join(", "), cmps)
}

fn req(s: &str) -> Req {
    Req::parse(s).unwrap_or_else(|e| panic!("Req::parse({s:?}) failed: {e:?}"))
}

#[test]
fn leader_req_model_20000() {
    let mut r = Rng::new(0x46_0000_0001);
    for i in 0..20_000 {
        let (text, canon, cmps) = gen_req(&mut r, 3, 4);
        let q = req(&text);
        assert_eq!(q.to_string(), canon, "case {i}: Display of {text:?}");
        assert_eq!(
            Req::parse(&canon),
            Ok(q.clone()),
            "case {i}: reparse {canon:?}"
        );
        for _ in 0..3 {
            let v = (r.range(0, 5), r.range(0, 5), r.range(0, 5));
            let want = cmps.iter().all(|c| cmp_match(c, v));
            assert_eq!(q.matches(&ver(v)), want, "case {i}: {text:?} matches {v:?}");
        }
    }
}

#[test]
fn leader_req_table() {
    let yes = [
        ("^1.2.3", "1.2.3"),
        ("^1.2.3", "1.9.0"),
        ("^0.2.3", "0.2.9"),
        ("^0.0.3", "0.0.3"),
        ("^0.0", "0.0.7"),
        ("^0", "0.9.9"),
        ("~1", "1.9.0"),
        ("=1.2", "1.2.7"),
        (">1.2", "1.3.0"),
        (">1", "2.0.0"),
        ("<=1.2", "1.2.9"),
        ("<=1", "1.99.0"),
        ("*", "0.0.0"),
        ("1.*", "1.5.0"),
        ("1.2.*", "1.2.0"),
        (">= 1.2, < 1.5", "1.4.9"),
        (" ^1 ", "1.0.0"),
        ("1", "1.3.0"),
    ];
    let no = [
        ("^1.2.3", "2.0.0"),
        ("^1.2.3", "1.2.2"),
        ("^0.2.3", "0.3.0"),
        ("^0.0.3", "0.0.4"),
        ("^0.0", "0.1.0"),
        ("^0", "1.0.0"),
        ("~1.2.3", "1.3.0"),
        ("=1.2.3", "1.2.4"),
        (">1.2.3", "1.2.3"),
        (">1", "1.9.9"),
        ("<1.2", "1.2.0"),
        ("<1", "1.0.0"),
        ("<=1.2.3", "1.2.4"),
        ("1.2.*", "1.3.0"),
        (">=1.2, <1.5", "1.5.0"),
    ];
    for (q, v) in yes {
        assert!(
            req(q).matches(&Version::parse(v).unwrap()),
            "{q} should match {v}"
        );
    }
    for (q, v) in no {
        assert!(
            !req(q).matches(&Version::parse(v).unwrap()),
            "{q} should not match {v}"
        );
    }
    let bad = [
        "",
        " ",
        "1.0.0,",
        ",1.0.0",
        "1.0.0,,2",
        "~>1",
        "=>1",
        "!1",
        "1.*.3",
        "^*",
        ">=1.*",
        "^1.2.*",
        "01.2.3",
        "1.02",
        "1.2.3.4",
        "1.2.3-rc",
        "1.2.3+b",
        "x",
        "1..2",
        "=",
        "^",
        ">= ",
        "18446744073709551616",
        "1.2.3 1.2.4",
        "v1",
        "*.1",
        "1.",
        ".1",
    ];
    for b in bad {
        assert!(Req::parse(b).is_err(), "Req::parse({b:?}) should fail");
    }
    for big in [
        "^18446744073709551615",
        ">18446744073709551615.18446744073709551615.18446744073709551615",
        "<=18446744073709551615",
        "~18446744073709551615.18446744073709551615",
        "=18446744073709551615.18446744073709551615.18446744073709551615",
        "18446744073709551615.*",
        "18446744073709551615.18446744073709551615.*",
    ] {
        // must not panic; when it parses, it must round-trip
        if let Ok(q) = Req::parse(big) {
            assert_eq!(Req::parse(&q.to_string()), Ok(q.clone()));
            q.matches(&ver((u64::MAX, u64::MAX, u64::MAX)));
            q.matches(&ver((0, 0, 0)));
        }
    }
}

#[test]
fn leader_version() {
    let mut r = Rng::new(0x46_0000_0002);
    for _ in 0..3000 {
        let mut n = || match r.below(3) {
            0 => r.range(0, 10),
            1 => r.range(0, 100_000),
            _ => r.u64(),
        };
        let t = (n(), n(), n());
        let s = format!("{}.{}.{}", t.0, t.1, t.2);
        let v = Version::parse(&s).unwrap_or_else(|e| panic!("{s}: {e:?}"));
        assert_eq!(tup(&v), t);
        assert_eq!(v.to_string(), s);
    }
    assert!(Version::parse("1.2.3").unwrap() < Version::parse("1.10.0").unwrap());
    for b in [
        "",
        "1",
        "1.2",
        "1.2.3.4",
        "v1.2.3",
        " 1.2.3",
        "1.2.3 ",
        "01.2.3",
        "1.02.3",
        "1.2.03",
        "1.2.3-rc",
        "1.2.3+b",
        "18446744073709551616.0.0",
        "1..3",
        "a.b.c",
        "-1.2.3",
        "+1.2.3",
        "1.2.*",
        "^1.2.3",
    ] {
        assert!(
            Version::parse(b).is_err(),
            "Version::parse({b:?}) should fail"
        );
    }
}

// ---------- independent solver model + brute force ----------

type Dep = (String, String, Vec<Cmp>); // name, canonical req text, model
struct G {
    pkgs: BTreeMap<String, BTreeMap<V3, Vec<Dep>>>,
}

fn sat(cs: &[Cmp], v: V3) -> bool {
    cs.iter().all(|c| cmp_match(c, v))
}

fn ref_solve(g: &G, root: &[Dep], lock: &BTreeMap<String, V3>) -> Option<BTreeMap<String, V3>> {
    let mut order: Vec<String> = Vec::new();
    let mut c: BTreeMap<String, Vec<Cmp>> = BTreeMap::new();
    for (n, _, cs) in root {
        if !order.contains(n) {
            order.push(n.clone());
        }
        c.entry(n.clone()).or_default().extend(cs.iter().cloned());
    }
    rec(g, lock, BTreeMap::new(), c, order)
}

fn rec(
    g: &G,
    lock: &BTreeMap<String, V3>,
    a: BTreeMap<String, V3>,
    c: BTreeMap<String, Vec<Cmp>>,
    order: Vec<String>,
) -> Option<BTreeMap<String, V3>> {
    let Some(n) = order.iter().find(|n| !a.contains_key(*n)).cloned() else {
        return Some(a);
    };
    let empty = BTreeMap::new();
    let vers = g.pkgs.get(&n).unwrap_or(&empty);
    let reqs = c.get(&n).cloned().unwrap_or_default();
    let mut cands: Vec<V3> = vers
        .keys()
        .rev()
        .filter(|v| sat(&reqs, **v))
        .copied()
        .collect();
    if let Some(l) = lock.get(&n) {
        if let Some(i) = cands.iter().position(|v| v == l) {
            let x = cands.remove(i);
            cands.insert(0, x);
        }
    }
    for v in cands {
        let deps = &vers[&v];
        let ok = deps.iter().all(|(d, _, cs)| {
            let have = if *d == n { Some(v) } else { a.get(d).copied() };
            match have {
                Some(h) => sat(cs, h),
                None => true,
            }
        });
        if !ok {
            continue;
        }
        let mut a2 = a.clone();
        a2.insert(n.clone(), v);
        let mut c2 = c.clone();
        let mut o2 = order.clone();
        for (d, _, cs) in deps {
            c2.entry(d.clone()).or_default().extend(cs.iter().cloned());
            if !o2.contains(d) {
                o2.push(d.clone());
            }
        }
        if let Some(s) = rec(g, lock, a2, c2, o2) {
            return Some(s);
        }
    }
    None
}

fn valid(g: &G, root: &[Dep], s: &BTreeMap<String, V3>) -> bool {
    let ok_dep = |d: &String, cs: &[Cmp]| s.get(d).is_some_and(|v| sat(cs, *v));
    s.iter()
        .all(|(n, v)| g.pkgs.get(n).is_some_and(|m| m.contains_key(v)))
        && root.iter().all(|(d, _, cs)| ok_dep(d, cs))
        && s.iter()
            .all(|(n, v)| g.pkgs[n][v].iter().all(|(d, _, cs)| ok_dep(d, cs)))
}

fn brute_exists(g: &G, root: &[Dep]) -> bool {
    let names: Vec<&String> = g.pkgs.keys().collect();
    fn go(g: &G, root: &[Dep], names: &[&String], i: usize, s: &mut BTreeMap<String, V3>) -> bool {
        if i == names.len() {
            return valid(g, root, s);
        }
        if go(g, root, names, i + 1, s) {
            return true;
        }
        for v in g.pkgs[names[i]].keys() {
            s.insert(names[i].clone(), *v);
            let found = go(g, root, names, i + 1, s);
            s.remove(names[i]);
            if found {
                return true;
            }
        }
        false
    }
    go(g, root, &names, 0, &mut BTreeMap::new())
}

fn gen_dep(r: &mut Rng, names: &[&str]) -> Dep {
    let n = if r.chance(1, 15) {
        "zz"
    } else {
        *r.pick(names)
    };
    let (_, canon, cmps) = gen_req(r, 2, 3);
    (n.to_string(), canon, cmps)
}

fn to_index(g: &G) -> Index {
    let mut idx = Index::new();
    for (n, vs) in &g.pkgs {
        for (v, deps) in vs {
            let d = deps.iter().map(|(d, q, _)| (d.clone(), req(q))).collect();
            idx.add(n, ver(*v), d);
        }
    }
    idx
}

fn sol_tuples(s: &Solution) -> BTreeMap<String, V3> {
    s.iter().map(|(k, v)| (k.clone(), tup(v))).collect()
}

#[test]
fn leader_solver_vs_model_and_brute_force() {
    let names = ["a", "b", "c", "d", "e"];
    let (mut ok, mut fail, mut backtracked) = (0, 0, 0);
    forall(3000, 0x46_0000_0003, |r| {
        let mut g = G {
            pkgs: BTreeMap::new(),
        };
        for n in names.iter().take(2 + r.below(4)) {
            let mut vs = BTreeMap::new();
            for _ in 0..r.below(4) {
                let v = (r.range(0, 2), r.range(0, 3), r.range(0, 2));
                let deps = (0..r.below(3)).map(|_| gen_dep(r, &names)).collect();
                vs.insert(v, deps);
            }
            if !vs.is_empty() {
                g.pkgs.insert(n.to_string(), vs);
            }
        }
        let root: Vec<Dep> = (0..1 + r.below(3)).map(|_| gen_dep(r, &names)).collect();
        let mut lock = BTreeMap::new();
        if r.chance(1, 3) {
            for n in names {
                if r.chance(1, 2) {
                    lock.insert(n.to_string(), (r.range(0, 2), r.range(0, 3), r.range(0, 2)));
                }
            }
        }
        let idx = to_index(&g);
        let root_w: Vec<(String, Req)> = root.iter().map(|(d, q, _)| (d.clone(), req(q))).collect();
        let lock_w: Solution = lock.iter().map(|(k, v)| (k.clone(), ver(*v))).collect();
        let got = solve(
            &idx,
            &root_w,
            if lock.is_empty() { None } else { Some(&lock_w) },
        );
        let want = ref_solve(&g, &root, &lock);
        let exists = brute_exists(&g, &root);
        assert_eq!(
            want.is_some(),
            exists,
            "reference model disagrees with brute force (leader bug)"
        );
        match (&got, &want) {
            (Ok(s), Some(w)) => {
                let s = sol_tuples(s);
                assert!(
                    valid(&g, &root, &s),
                    "invalid solution {s:?} for root {root:?}"
                );
                assert_eq!(
                    &s, w,
                    "solution differs from the normative algorithm; root {root:?}"
                );
                ok += 1;
                let newest_ok = w
                    .iter()
                    .all(|(n, v)| g.pkgs[n].keys().next_back() == Some(v));
                if !newest_ok {
                    backtracked += 1;
                }
            }
            (Err(_), None) => fail += 1,
            _ => panic!("got {got:?}, want {want:?}; root {root:?}, lock {lock:?}"),
        }
    });
    assert!(
        ok > 800 && fail > 300 && backtracked > 200,
        "weak mix: ok {ok} fail {fail} older {backtracked}"
    );
}

fn idx_from(spec: &[(&str, &str, &[(&str, &str)])]) -> Index {
    let mut idx = Index::new();
    for (n, v, deps) in spec {
        let d = deps.iter().map(|(d, q)| (d.to_string(), req(q))).collect();
        idx.add(n, Version::parse(v).unwrap(), d);
    }
    idx
}
fn root(deps: &[(&str, &str)]) -> Vec<(String, Req)> {
    deps.iter().map(|(d, q)| (d.to_string(), req(q))).collect()
}
fn sol(pairs: &[(&str, &str)]) -> Solution {
    pairs
        .iter()
        .map(|(n, v)| (n.to_string(), Version::parse(v).unwrap()))
        .collect()
}

#[test]
fn leader_solver_tables() {
    let a3 = idx_from(&[
        ("a", "1.0.0", &[]),
        ("a", "1.1.0", &[]),
        ("a", "1.2.0", &[]),
    ]);
    let ra = root(&[("a", "^1")]);
    assert_eq!(solve(&a3, &ra, None), Ok(sol(&[("a", "1.2.0")])));
    assert_eq!(
        solve(&a3, &ra, Some(&sol(&[("a", "1.1.0")]))),
        Ok(sol(&[("a", "1.1.0")]))
    );
    assert_eq!(
        solve(&a3, &ra, Some(&sol(&[("a", "0.9.0")]))),
        Ok(sol(&[("a", "1.2.0")]))
    );
    let ra2 = root(&[("a", ">=1.2")]);
    assert_eq!(
        solve(&a3, &ra2, Some(&sol(&[("a", "1.1.0")]))),
        Ok(sol(&[("a", "1.2.0")]))
    );
    // backtracking across a later package
    let bt = idx_from(&[
        ("a", "2.0.0", &[("b", "^2")]),
        ("a", "1.0.0", &[("b", "^1")]),
        ("b", "1.0.0", &[]),
        ("b", "2.0.0", &[]),
        ("c", "1.0.0", &[("b", "<2")]),
    ]);
    let want = sol(&[("a", "1.0.0"), ("b", "1.0.0"), ("c", "1.0.0")]);
    assert_eq!(solve(&bt, &root(&[("a", "*"), ("c", "*")]), None), Ok(want));
    assert!(solve(&bt, &root(&[("a", "^2"), ("c", "*")]), None).is_err());
    assert!(solve(&bt, &root(&[("zz", "*")]), None).is_err());
    // self dependency
    let selfdep = idx_from(&[
        ("a", "1.0.0", &[("a", "^2")]),
        ("a", "0.5.0", &[("a", "^0.5")]),
    ]);
    assert_eq!(
        solve(&selfdep, &root(&[("a", "*")]), None),
        Ok(sol(&[("a", "0.5.0")]))
    );
    // empty root
    assert_eq!(solve(&a3, &[], None), Ok(Solution::new()));
    // Index::add replaces, versions ascending
    let mut idx = Index::new();
    idx.add("a", Version::parse("1.0.0").unwrap(), root(&[("b", "^1")]));
    idx.add("a", Version::parse("0.3.0").unwrap(), vec![]);
    idx.add("a", Version::parse("1.0.0").unwrap(), vec![]);
    let vs: Vec<String> = idx.versions("a").iter().map(|v| v.to_string()).collect();
    assert_eq!(vs, ["0.3.0", "1.0.0"]);
    assert_eq!(
        idx.deps("a", &Version::parse("1.0.0").unwrap())
            .map(|d| d.len()),
        Some(0)
    );
    assert!(idx.deps("a", &Version::parse("9.0.0").unwrap()).is_none());
    assert!(idx.versions("nope").is_empty());
}

#[test]
fn leader_solver_perf_40x8() {
    let mut r = Rng::new(0x46_0000_0004);
    let mut idx = Index::new();
    let forms = ["^1", "~1.7", "*", ">=1.0, <2", "1.*"];
    for i in 0..40 {
        for m in 0..8 {
            let mut deps = Vec::new();
            for _ in 0..r.below(4) {
                if i + 1 >= 40 {
                    break;
                }
                let j = i + 1 + r.below(39 - i);
                let q = if r.chance(1, 2) {
                    format!(">=1.{}", r.range(0, 8))
                } else {
                    r.pick(&forms).to_string()
                };
                deps.push((format!("p{j}"), req(&q)));
            }
            idx.add(&format!("p{i}"), ver((1, m, 0)), deps);
        }
    }
    let rt: Vec<(String, Req)> = (0..40).map(|i| (format!("p{i}"), req("^1"))).collect();
    let t = Instant::now();
    let s = solve(&idx, &rt, None).expect("solvable");
    let el = t.elapsed();
    assert_eq!(s.len(), 40);
    assert!(
        s.values().all(|v| tup(v) == (1, 7, 0)),
        "newest versions expected"
    );
    assert!(el.as_secs_f64() < 2.0, "took {el:?}");
}

#[test]
fn leader_chain_500_small_stack() {
    let h = std::thread::Builder::new()
        .stack_size(128 * 1024)
        .spawn(|| {
            let mut idx = Index::new();
            for i in 0..500 {
                let deps = if i < 499 {
                    vec![(format!("p{}", i + 1), req("^1"))]
                } else {
                    vec![]
                };
                idx.add(&format!("p{i}"), ver((1, 0, 0)), deps);
                idx.add(&format!("p{i}"), ver((2, 0, 0)), vec![]);
            }
            let s = solve(&idx, &[("p0".to_string(), req("^1"))], None).expect("chain solvable");
            assert_eq!(s.len(), 500);
            assert_eq!(tup(&s["p499"]), (1, 0, 0));
        })
        .unwrap();
    h.join().expect("chain solve panicked or overflowed");
}

// ---------- manifest and lock ----------

fn gen_name(r: &mut Rng) -> String {
    let first = b"abcdefghijklmnopqrstuvwxyz";
    let rest = b"abcdefghijklmnopqrstuvwxyz0123456789_-";
    let mut s = String::new();
    s.push(*r.pick(first) as char);
    for _ in 0..r.below(7) {
        s.push(*r.pick(rest) as char);
    }
    s
}

#[test]
fn leader_manifest_round_trip() {
    forall(1500, 0x46_0000_0005, |r| {
        let name = gen_name(r);
        let v = (r.range(0, 20), r.range(0, 20), r.range(0, 20));
        let mut deps: Vec<(String, String, String)> = Vec::new(); // name, raw text, canon
        for _ in 0..r.below(5) {
            let d = gen_name(r);
            if d != name && !deps.iter().any(|x| x.0 == d) {
                let (raw, canon, _) = gen_req(r, 3, 5);
                deps.push((d, raw, canon));
            }
        }
        let mut text = format!(
            "[package]\nname = \"{name}\"\nversion = \"{}.{}.{}\"\n",
            v.0, v.1, v.2
        );
        if !deps.is_empty() {
            text.push_str("\n[dependencies]\n");
            for (d, _, c) in &deps {
                text.push_str(&format!("{d} = \"{c}\"\n"));
            }
        }
        let want = Manifest {
            name: name.clone(),
            version: ver(v),
            dependencies: deps.iter().map(|(d, _, c)| (d.clone(), req(c))).collect(),
        };
        assert_eq!(Manifest::parse(&text), Ok(want.clone()), "{text}");
        assert_eq!(want.to_string(), text);
        // noisy spelling of the same manifest
        let eq = |r: &mut Rng| *r.pick(&["=", " = ", "  =", "=  "]);
        let junk = |r: &mut Rng| *r.pick(&["", "\n", "# note\n", "   # x = \"y\"\n", "\n\n"]);
        let mut noisy = junk(r).to_string();
        noisy.push_str("[package]\n");
        noisy.push_str(junk(r));
        noisy.push_str(&format!("name{}\"{name}\"\n", eq(r)));
        noisy.push_str(junk(r));
        noisy.push_str(&format!("version{}\"{}.{}.{}\"\n", eq(r), v.0, v.1, v.2));
        if !deps.is_empty() || r.chance(1, 2) {
            noisy.push_str(junk(r));
            noisy.push_str("[dependencies]\n");
            for (d, raw, _) in &deps {
                noisy.push_str(junk(r));
                noisy.push_str(&format!("{d}{}\"{raw}\"\n", eq(r)));
            }
        }
        noisy.push_str(junk(r));
        assert_eq!(Manifest::parse(&noisy), Ok(want), "noisy:\n{noisy}");
    });
}

#[test]
fn leader_manifest_errors() {
    let p = "[package]\nname = \"a\"\nversion = \"1.0.0\"\n";
    let cases: Vec<(String, usize)> = vec![
        (format!("{p}[deps]\n"), 4),
        ("name = \"a\"\n".into(), 1),
        (
            "[package]\nname = \"a\"\nname = \"b\"\nversion = \"1.0.0\"\n".into(),
            3,
        ),
        ("[package]\nname = \"a\nversion = \"1.0.0\"\n".into(), 2),
        ("[package]\nname = a\nversion = \"1.0.0\"\n".into(), 2),
        ("[package]\nname = \"A\"\nversion = \"1.0.0\"\n".into(), 2),
        ("[package]\nname = \"1a\"\nversion = \"1.0.0\"\n".into(), 2),
        ("[package]\nname = \"a\"\nversion = \"1.0\"\n".into(), 3),
        (format!("{p}\n[dependencies]\nb = \"^x\"\n"), 6),
        (format!("{p}\n[dependencies]\nb = \"1\"\nb = \"2\"\n"), 7),
        (format!("{p}\n[dependencies]\nB = \"1\"\n"), 6),
        ("[package]\nname = \"a\"\n".into(), 2),
        ("[package]\nversion = \"1.0.0\"\n".into(), 2),
        (format!("{p}edition = \"2021\"\n"), 4),
        (
            "[dependencies]\nb = \"1\"\n[package]\nname = \"a\"\nversion = \"1.0.0\"\n".into(),
            1,
        ),
        (format!("{p}[dependencies]\n[dependencies]\n"), 5),
        (format!("{p}[package]\n"), 4),
        ("[package]\nname \"a\"\nversion = \"1.0.0\"\n".into(), 2),
    ];
    for (text, line) in cases {
        match Manifest::parse(&text) {
            Err(e) => assert_eq!(e.line, line, "wrong line for:\n{text}\n{e:?}"),
            Ok(m) => panic!("should fail:\n{text}\ngot {m:?}"),
        }
    }
    assert!(Manifest::parse("").is_err());
}

#[test]
fn leader_lock() {
    forall(1500, 0x46_0000_0006, |r| {
        let mut s = Solution::new();
        for _ in 0..r.below(7) {
            s.insert(
                gen_name(r),
                ver((r.range(0, 30), r.range(0, 30), r.u64() >> r.below(64))),
            );
        }
        let mut want = String::from("# cairn lock v1\n");
        for (k, v) in &s {
            want.push_str(&format!("{k} = \"{v}\"\n"));
        }
        let text = lock::write(&s);
        assert_eq!(text, want);
        assert_eq!(lock::read(&text), Ok(s.clone()));
        let mut spaced = String::from("# cairn lock v1\n");
        for (k, v) in &s {
            if r.chance(1, 2) {
                spaced.push('\n');
            }
            spaced.push_str(&format!("{k} = \"{v}\"\n"));
        }
        assert_eq!(lock::read(&spaced), Ok(s));
    });
    for bad in [
        "",
        "a = \"1.0.0\"\n",
        "# cairn lock v2\n",
        "# cairn lock v1\na = 1.0.0\n",
        "# cairn lock v1\na = \"1.0\"\n",
        "# cairn lock v1\na = \"1.0.0\"\na = \"1.0.0\"\n",
        "# cairn lock v1\ngarbage\n",
        "\n# cairn lock v1\n",
    ] {
        assert!(lock::read(bad).is_err(), "lock::read({bad:?}) should fail");
    }
}

#[test]
fn leader_parser_fuzz() {
    let alpha = b"0123456789.^~=<>*, \n\"[]#abc-_xyz";
    forall(6000, 0x46_0000_0007, |r| {
        let s = if r.chance(1, 2) {
            String::from_utf8_lossy(&r.bytes(40)).into_owned()
        } else {
            (0..r.below(60)).map(|_| *r.pick(alpha) as char).collect()
        };
        if let Ok(v) = Version::parse(&s) {
            assert_eq!(v.to_string(), s);
        }
        if let Ok(q) = Req::parse(&s) {
            assert_eq!(
                Req::parse(&q.to_string()),
                Ok(q.clone()),
                "req round trip {s:?}"
            );
        }
        if let Ok(m) = Manifest::parse(&s) {
            assert_eq!(Manifest::parse(&m.to_string()), Ok(m.clone()));
        }
        if let Ok(l) = lock::read(&s) {
            assert_eq!(lock::read(&lock::write(&l)), Ok(l.clone()));
        }
        let wrapped =
            format!("[package]\nname = \"a\"\nversion = \"1.0.0\"\n[dependencies]\n{s}\n");
        if let Ok(m) = Manifest::parse(&wrapped) {
            assert_eq!(Manifest::parse(&m.to_string()), Ok(m.clone()));
        }
        let wl = format!("# cairn lock v1\n{s}\n");
        if let Ok(l) = lock::read(&wl) {
            assert_eq!(lock::read(&lock::write(&l)), Ok(l.clone()));
        }
    });
}

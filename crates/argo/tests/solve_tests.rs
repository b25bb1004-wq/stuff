//! Dependency solver tests for argo.
#![allow(clippy::unwrap_used)]

use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use argo::solve::{solve, Index, Solution};
use argo::version::{Req, Version};
use assay::forall;

fn ver(s: &str) -> Version {
    Version::parse(s).unwrap()
}

fn req(s: &str) -> Req {
    Req::parse(s).unwrap()
}

fn dep(name: &str, r: &str) -> (String, Req) {
    (name.to_string(), req(r))
}

fn validate_solution(index: &Index, root: &[(String, Req)], sol: &Solution) {
    let mut needed = BTreeSet::new();
    let mut queue: Vec<String> = Vec::new();
    for (name, r) in root {
        let v = sol
            .get(name)
            .unwrap_or_else(|| panic!("missing root dep {name}"));
        assert!(r.matches(v), "root dep {name} {v} does not match {r}");
        if needed.insert(name.clone()) {
            queue.push(name.clone());
        }
    }
    while let Some(pkg) = queue.pop() {
        let v = sol[&pkg];
        let deps = index
            .deps(&pkg, &v)
            .unwrap_or_else(|| panic!("chosen {pkg}@{v} not in index"));
        for (d, r) in deps {
            let dv = sol
                .get(d)
                .unwrap_or_else(|| panic!("missing transitive dep {d} from {pkg}@{v}"));
            assert!(
                r.matches(dv),
                "dep {d}@{dv} does not match {r} required by {pkg}@{v}"
            );
            if needed.insert(d.clone()) {
                queue.push(d.clone());
            }
        }
    }
    let sol_keys: BTreeSet<String> = sol.keys().cloned().collect();
    assert_eq!(sol_keys, needed, "solution contains unneeded packages");
}

#[test]
fn solve_diamond_and_backtracking() {
    let mut idx = Index::new();
    // a 2.0.0 -> d ^2.0 (incompatible with b -> d ^1.0)
    // a 1.0.0 -> d ^1.2
    idx.add("a", ver("2.0.0"), vec![dep("d", "^2.0")]);
    idx.add("a", ver("1.0.0"), vec![dep("d", "^1.2")]);
    idx.add("b", ver("1.0.0"), vec![dep("d", "^1.0, <1.4")]);
    idx.add("d", ver("1.0.0"), vec![]);
    idx.add("d", ver("1.2.0"), vec![]);
    idx.add("d", ver("1.3.0"), vec![]);
    idx.add("d", ver("1.5.0"), vec![]);
    idx.add("d", ver("2.0.0"), vec![]);

    let root = vec![dep("a", "*"), dep("b", "*")];
    let sol = solve(&idx, &root, None).unwrap();
    validate_solution(&idx, &root, &sol);
    assert_eq!(sol["a"], ver("1.0.0"));
    assert_eq!(sol["b"], ver("1.0.0"));
    assert_eq!(sol["d"], ver("1.3.0"));
}

#[test]
fn solve_lock_kept_and_overridden() {
    let mut idx = Index::new();
    idx.add("a", ver("1.0.0"), vec![]);
    idx.add("a", ver("1.1.0"), vec![]);
    idx.add("a", ver("2.0.0"), vec![]);

    let mut lock = Solution::new();
    lock.insert("a".to_string(), ver("1.0.0"));

    // Lock kept when compatible
    let sol_kept = solve(&idx, &[dep("a", "^1.0")], Some(&lock)).unwrap();
    assert_eq!(sol_kept["a"], ver("1.0.0"));

    // Lock overridden when incompatible
    let sol_overridden = solve(&idx, &[dep("a", "^2.0")], Some(&lock)).unwrap();
    assert_eq!(sol_overridden["a"], ver("2.0.0"));
}

#[test]
fn solve_missing_and_self_dependency() {
    let mut idx = Index::new();
    idx.add("a", ver("1.0.0"), vec![dep("missing", "^1.0")]);
    assert!(solve(&idx, &[dep("missing", "*")], None).is_err());
    assert!(solve(&idx, &[dep("a", "*")], None).is_err());

    // Self dependency: 2.0.0 requires a ^1.0 (fails), 1.0.0 requires a ^1.0 (succeeds)
    let mut self_idx = Index::new();
    self_idx.add("a", ver("2.0.0"), vec![dep("a", "^1.0")]);
    self_idx.add("a", ver("1.0.0"), vec![dep("a", "^1.0")]);
    let sol = solve(&self_idx, &[dep("a", "*")], None).unwrap();
    assert_eq!(sol["a"], ver("1.0.0"));

    // Replace deps via Index::add
    self_idx.add("a", ver("2.0.0"), vec![]);
    let sol2 = solve(&self_idx, &[dep("a", "*")], None).unwrap();
    assert_eq!(sol2["a"], ver("2.0.0"));
}

#[test]
fn solve_500_chain_small_stack() {
    let handle = std::thread::Builder::new()
        .stack_size(128 * 1024)
        .spawn(|| {
            let mut idx = Index::new();
            for i in 0..500 {
                let name = format!("p{i}");
                let deps = if i + 1 < 500 {
                    vec![dep(&format!("p{}", i + 1), "^1.0")]
                } else {
                    vec![]
                };
                idx.add(&name, ver("1.0.0"), deps);
            }
            let root = vec![dep("p0", "^1.0")];
            let sol = solve(&idx, &root, None).unwrap();
            assert_eq!(sol.len(), 500);
            validate_solution(&idx, &root, &sol);
        })
        .unwrap();
    handle.join().unwrap();
}

#[test]
fn solve_40_packages_performance() {
    let mut idx = Index::new();
    for i in 0..40 {
        let name = format!("pkg{i}");
        for minor in 0..8 {
            let mut deps = Vec::new();
            if i + 1 < 40 {
                deps.push(dep(&format!("pkg{}", i + 1), "^1.0"));
            }
            if i + 2 < 40 {
                deps.push(dep(&format!("pkg{}", i + 2), ">=1.0, <2.0"));
            }
            idx.add(
                &name,
                Version {
                    major: 1,
                    minor,
                    patch: 0,
                },
                deps,
            );
        }
    }
    let root = vec![dep("pkg0", "^1.0")];
    let start = Instant::now();
    let sol = solve(&idx, &root, None).unwrap();
    let elapsed = start.elapsed();
    assert!(elapsed.as_secs_f64() < 2.0, "took {elapsed:?}");
    assert_eq!(sol.len(), 40);
    validate_solution(&idx, &root, &sol);
}

#[test]
fn solve_random_small_indexes_vs_bruteforce() {
    let names = ["a", "b", "c", "d"];
    let vers = [ver("1.0.0"), ver("1.1.0"), ver("2.0.0")];
    let reqs = [req("*"), req("^1.0"), req("^1.1"), req("^2.0")];

    forall(300, 0xCA14_0004, |rng| {
        let mut idx = Index::new();
        for &n in &names {
            for &v in &vers {
                if rng.chance(3, 4) {
                    let n_deps = rng.below(3);
                    let mut deps = Vec::new();
                    for _ in 0..n_deps {
                        let d = names[rng.below(names.len())];
                        let r = reqs[rng.below(reqs.len())].clone();
                        deps.push((d.to_string(), r));
                    }
                    idx.add(n, v, deps);
                }
            }
        }
        let root_n = rng.range(1, 3) as usize;
        let mut root = Vec::new();
        for _ in 0..root_n {
            let d = names[rng.below(names.len())];
            let r = reqs[rng.below(reqs.len())].clone();
            root.push((d.to_string(), r));
        }

        let got = solve(&idx, &root, None);
        if let Ok(ref sol) = got {
            validate_solution(&idx, &root, sol);
        }

        // Brute-force check over all subsets/versions of the 4 packages (at most 4^4 = 256 states)
        let opts: Vec<Vec<Option<Version>>> = names
            .iter()
            .map(|n| {
                let mut o = vec![None];
                for v in idx.versions(n) {
                    o.push(Some(v));
                }
                o
            })
            .collect();

        let mut any_valid = false;
        'outer: for v0 in &opts[0] {
            for v1 in &opts[1] {
                for v2 in &opts[2] {
                    for v3 in &opts[3] {
                        let mut cand = BTreeMap::new();
                        for (i, ov) in [v0, v1, v2, v3].into_iter().enumerate() {
                            if let Some(v) = ov {
                                cand.insert(names[i].to_string(), *v);
                            }
                        }
                        if is_valid_assignment(&idx, &root, &cand) {
                            any_valid = true;
                            break 'outer;
                        }
                    }
                }
            }
        }
        assert_eq!(got.is_ok(), any_valid);
    });
}

fn is_valid_assignment(index: &Index, root: &[(String, Req)], sol: &Solution) -> bool {
    let mut needed = BTreeSet::new();
    let mut queue = Vec::new();
    for (n, r) in root {
        let Some(v) = sol.get(n) else { return false };
        if !r.matches(v) {
            return false;
        }
        if needed.insert(n.clone()) {
            queue.push(n.clone());
        }
    }
    while let Some(pkg) = queue.pop() {
        let v = sol[&pkg];
        let Some(deps) = index.deps(&pkg, &v) else {
            return false;
        };
        for (d, r) in deps {
            let Some(dv) = sol.get(d) else { return false };
            if !r.matches(dv) {
                return false;
            }
            if needed.insert(d.clone()) {
                queue.push(d.clone());
            }
        }
    }
    sol.len() == needed.len()
}

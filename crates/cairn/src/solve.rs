//! Iterative backtracking dependency solver.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;

use crate::version::{Req, Version};

/// Resolved mapping from package name to chosen version.
pub type Solution = BTreeMap<String, Version>;

/// Error returned when no valid dependency assignment exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SolveError {
    /// Explanation of why resolution failed.
    pub msg: String,
}

impl fmt::Display for SolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.msg)
    }
}

impl std::error::Error for SolveError {}

/// Package index mapping `(name, version)` to its dependency requirements.
#[derive(Debug, Clone, Default)]
pub struct Index {
    packages: BTreeMap<String, BTreeMap<Version, Vec<(String, Req)>>>,
}

impl Index {
    /// Create an empty package index.
    pub fn new() -> Index {
        Self::default()
    }

    /// Add a package version and its dependencies.
    /// Adding the same name and version again replaces its dependency list.
    pub fn add(&mut self, name: &str, version: Version, deps: Vec<(String, Req)>) {
        self.packages
            .entry(name.to_string())
            .or_default()
            .insert(version, deps);
    }

    /// Return all available versions of `name` in ascending order.
    pub fn versions(&self, name: &str) -> Vec<Version> {
        self.packages
            .get(name)
            .map(|vers| vers.keys().copied().collect())
            .unwrap_or_default()
    }

    /// Return the dependency list for `(name, version)`, if present.
    pub fn deps(&self, name: &str, version: &Version) -> Option<&[(String, Req)]> {
        self.packages
            .get(name)
            .and_then(|vers| vers.get(version))
            .map(Vec::as_slice)
    }
}

struct Frame {
    name: String,
    candidates: Vec<Version>,
    next_cand: usize,
    active: bool,
    prev_order_len: usize,
    pushed_reqs: Vec<String>,
}

fn build_candidates(
    index: &Index,
    name: &str,
    reqs: &HashMap<String, Vec<Req>>,
    lock: Option<&Solution>,
) -> Vec<Version> {
    let req_list = reqs.get(name).map(Vec::as_slice).unwrap_or(&[]);
    let mut candidates: Vec<Version> = index
        .versions(name)
        .into_iter()
        .rev()
        .filter(|v| req_list.iter().all(|r| r.matches(v)))
        .collect();
    if let Some(lock_map) = lock {
        if let Some(locked_ver) = lock_map.get(name) {
            if let Some(pos) = candidates.iter().position(|v| v == locked_ver) {
                if pos > 0 {
                    candidates[..=pos].rotate_right(1);
                }
            }
        }
    }
    candidates
}

fn undo_frame(
    frame: &mut Frame,
    chosen: &mut Solution,
    reqs: &mut HashMap<String, Vec<Req>>,
    order: &mut Vec<String>,
    order_set: &mut HashSet<String>,
) {
    chosen.remove(&frame.name);
    for dep_name in frame.pushed_reqs.drain(..) {
        if let Some(list) = reqs.get_mut(&dep_name) {
            list.pop();
        }
    }
    while order.len() > frame.prev_order_len {
        if let Some(removed) = order.pop() {
            order_set.remove(&removed);
        }
    }
    frame.active = false;
}

/// Resolve `root` dependencies against `index`, preferring `lock` versions when valid.
pub fn solve(
    index: &Index,
    root: &[(String, Req)],
    lock: Option<&Solution>,
) -> Result<Solution, SolveError> {
    let mut chosen: Solution = BTreeMap::new();
    let mut reqs: HashMap<String, Vec<Req>> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut order_set: HashSet<String> = HashSet::new();

    for (name, req) in root {
        reqs.entry(name.clone()).or_default().push(req.clone());
        if order_set.insert(name.clone()) {
            order.push(name.clone());
        }
    }

    if order.is_empty() {
        return Ok(chosen);
    }

    let first_name = order[0].clone();
    let mut stack = vec![Frame {
        candidates: build_candidates(index, &first_name, &reqs, lock),
        name: first_name,
        next_cand: 0,
        active: false,
        prev_order_len: order.len(),
        pushed_reqs: Vec::new(),
    }];
    let mut last_err: Option<String> = None;

    while let Some(frame) = stack.last_mut() {
        if frame.active {
            undo_frame(frame, &mut chosen, &mut reqs, &mut order, &mut order_set);
        }

        if frame.candidates.is_empty() && frame.next_cand == 0 && last_err.is_none() {
            last_err = Some(format!("no version of {} matches requirements", frame.name));
        }

        while frame.next_cand < frame.candidates.len() {
            let v = frame.candidates[frame.next_cand];
            frame.next_cand += 1;

            let deps = index.deps(&frame.name, &v).unwrap_or(&[]);
            let mut conflict = None;
            for (d, r) in deps {
                let target_ver = if d == &frame.name {
                    Some(&v)
                } else {
                    chosen.get(d)
                };
                if let Some(tv) = target_ver {
                    if !r.matches(tv) {
                        conflict = Some((d, r));
                        break;
                    }
                }
            }
            if let Some((d, r)) = conflict {
                last_err = Some(format!(
                    "no version of {d} matches {r} (from {} {v})",
                    frame.name
                ));
                continue;
            }

            frame.active = true;
            frame.prev_order_len = order.len();
            chosen.insert(frame.name.clone(), v);
            for (d, r) in deps {
                reqs.entry(d.clone()).or_default().push(r.clone());
                frame.pushed_reqs.push(d.clone());
                if order_set.insert(d.clone()) {
                    order.push(d.clone());
                }
            }
            break;
        }

        if !frame.active {
            if last_err.is_none() {
                last_err = Some(format!("no compatible version for {}", frame.name));
            }
            stack.pop();
            continue;
        }

        let next_idx = chosen.len();
        if next_idx == order.len() {
            return Ok(chosen);
        }

        let next_name = order[next_idx].clone();
        let next_cands = build_candidates(index, &next_name, &reqs, lock);
        if next_cands.is_empty() {
            last_err = Some(format!("no version of {next_name} matches requirements"));
        }
        stack.push(Frame {
            name: next_name,
            candidates: next_cands,
            next_cand: 0,
            active: false,
            prev_order_len: order.len(),
            pushed_reqs: Vec::new(),
        });
    }

    Err(SolveError {
        msg: last_err.unwrap_or_else(|| "no solution found".to_string()),
    })
}

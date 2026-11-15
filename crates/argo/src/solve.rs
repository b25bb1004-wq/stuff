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

const MAX_SOLVE_STEPS: usize = 50_000;

struct Frame {
    name: String,
    candidates: Vec<Version>,
    next_cand: usize,
    active: bool,
    prev_order_len: usize,
    pushed_reqs: Vec<String>,
    conflict_set: HashSet<usize>,
}

fn build_candidates(
    index: &Index,
    name: &str,
    reqs: &HashMap<String, Vec<(Req, Option<usize>)>>,
    lock: Option<&Solution>,
    intro_level: &HashMap<String, Option<usize>>,
) -> (Vec<Version>, HashSet<usize>) {
    let req_list = reqs.get(name).map(Vec::as_slice).unwrap_or(&[]);
    let mut conflict_set = HashSet::new();
    if let Some(&Some(lvl)) = intro_level.get(name) {
        conflict_set.insert(lvl);
    }
    let mut candidates = Vec::new();
    for v in index.versions(name).into_iter().rev() {
        let mut root_rejected = false;
        let mut min_lvl: Option<usize> = None;
        for (r, src) in req_list {
            if !r.matches(&v) {
                match src {
                    None => {
                        root_rejected = true;
                        break;
                    }
                    Some(l) => {
                        min_lvl = Some(min_lvl.map_or(*l, |m| m.min(*l)));
                    }
                }
            }
        }
        if !root_rejected && min_lvl.is_none() {
            candidates.push(v);
        } else if !root_rejected {
            if let Some(l) = min_lvl {
                conflict_set.insert(l);
            }
        }
    }
    if let Some(lock_map) = lock {
        if let Some(locked_ver) = lock_map.get(name) {
            if let Some(pos) = candidates.iter().position(|v| v == locked_ver) {
                if pos > 0 {
                    candidates[..=pos].rotate_right(1);
                }
            }
        }
    }
    (candidates, conflict_set)
}

fn forward_check_dep(
    index: &Index,
    d: &str,
    cur_level: usize,
    reqs: &HashMap<String, Vec<(Req, Option<usize>)>>,
    intro_level: &HashMap<String, Option<usize>>,
) -> Option<HashSet<usize>> {
    let req_list = reqs.get(d).map(Vec::as_slice).unwrap_or(&[]);
    let mut conflicts = HashSet::new();
    if let Some(&Some(lvl)) = intro_level.get(d) {
        if lvl < cur_level {
            conflicts.insert(lvl);
        }
    }
    for v in index.versions(d) {
        let mut root_rejected = false;
        let mut min_lvl: Option<usize> = None;
        for (r, src) in req_list {
            if !r.matches(&v) {
                match src {
                    None => {
                        root_rejected = true;
                        break;
                    }
                    Some(l) => {
                        min_lvl = Some(min_lvl.map_or(*l, |m| m.min(*l)));
                    }
                }
            }
        }
        if !root_rejected && min_lvl.is_none() {
            return None;
        }
        if !root_rejected {
            if let Some(l) = min_lvl {
                if l < cur_level {
                    conflicts.insert(l);
                }
            }
        }
    }
    Some(conflicts)
}

fn undo_frame(
    frame: &mut Frame,
    chosen: &mut Solution,
    chosen_level: &mut HashMap<String, usize>,
    reqs: &mut HashMap<String, Vec<(Req, Option<usize>)>>,
    order: &mut Vec<String>,
    order_set: &mut HashSet<String>,
    intro_level: &mut HashMap<String, Option<usize>>,
) {
    chosen.remove(&frame.name);
    chosen_level.remove(&frame.name);
    for dep_name in frame.pushed_reqs.drain(..) {
        if let Some(list) = reqs.get_mut(&dep_name) {
            list.pop();
        }
    }
    while order.len() > frame.prev_order_len {
        if let Some(removed) = order.pop() {
            order_set.remove(&removed);
            intro_level.remove(&removed);
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
    let mut chosen_level: HashMap<String, usize> = HashMap::new();
    let mut reqs: HashMap<String, Vec<(Req, Option<usize>)>> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut order_set: HashSet<String> = HashSet::new();
    let mut intro_level: HashMap<String, Option<usize>> = HashMap::new();

    for (name, req) in root {
        reqs.entry(name.clone())
            .or_default()
            .push((req.clone(), None));
        if order_set.insert(name.clone()) {
            order.push(name.clone());
            intro_level.insert(name.clone(), None);
        }
    }

    if order.is_empty() {
        return Ok(chosen);
    }

    for name in &order {
        let req_list = reqs.get(name).map(Vec::as_slice).unwrap_or(&[]);
        let any_ok = index
            .versions(name)
            .into_iter()
            .any(|v| req_list.iter().all(|(r, _)| r.matches(&v)));
        if !any_ok {
            return Err(SolveError {
                msg: format!("no version of {name} matches requirements"),
            });
        }
    }

    let first_name = order[0].clone();
    let (first_cands, first_conflicts) =
        build_candidates(index, &first_name, &reqs, lock, &intro_level);
    let mut stack = vec![Frame {
        candidates: first_cands,
        next_cand: 0,
        active: false,
        prev_order_len: order.len(),
        pushed_reqs: Vec::new(),
        conflict_set: first_conflicts,
        name: first_name,
    }];
    let mut last_err: Option<String> = None;
    let mut steps: usize = 0;

    while !stack.is_empty() {
        steps += 1;
        if steps > MAX_SOLVE_STEPS {
            return Err(SolveError {
                msg: "dependency solver step limit exceeded".to_string(),
            });
        }

        let cur_level = stack.len() - 1;
        {
            let frame = &mut stack[cur_level];
            if frame.active {
                undo_frame(
                    frame,
                    &mut chosen,
                    &mut chosen_level,
                    &mut reqs,
                    &mut order,
                    &mut order_set,
                    &mut intro_level,
                );
            }

            if frame.candidates.is_empty() && frame.next_cand == 0 && last_err.is_none() {
                last_err = Some(format!("no version of {} matches requirements", frame.name));
            }

            while frame.next_cand < frame.candidates.len() {
                steps += 1;
                if steps > MAX_SOLVE_STEPS {
                    return Err(SolveError {
                        msg: "dependency solver step limit exceeded".to_string(),
                    });
                }

                let v = frame.candidates[frame.next_cand];
                frame.next_cand += 1;

                let deps = index.deps(&frame.name, &v).unwrap_or(&[]);
                let mut conflict = None;
                for (d, r) in deps {
                    if d == &frame.name {
                        if !r.matches(&v) {
                            conflict = Some((d, r, None));
                            break;
                        }
                    } else if let Some(tv) = chosen.get(d) {
                        if !r.matches(tv) {
                            conflict = Some((d, r, chosen_level.get(d).copied()));
                            break;
                        }
                    }
                }
                if let Some((d, r, c_lvl)) = conflict {
                    if let Some(l) = c_lvl {
                        frame.conflict_set.insert(l);
                    }
                    last_err = Some(format!(
                        "no version of {d} matches {r} (from {} {v})",
                        frame.name
                    ));
                    continue;
                }

                frame.active = true;
                frame.prev_order_len = order.len();
                chosen.insert(frame.name.clone(), v);
                chosen_level.insert(frame.name.clone(), cur_level);
                let mut check_deps: Vec<&str> = Vec::new();
                for (d, r) in deps {
                    reqs.entry(d.clone())
                        .or_default()
                        .push((r.clone(), Some(cur_level)));
                    frame.pushed_reqs.push(d.clone());
                    if order_set.insert(d.clone()) {
                        order.push(d.clone());
                        intro_level.insert(d.clone(), Some(cur_level));
                    }
                    if d != &frame.name
                        && !chosen.contains_key(d)
                        && !check_deps.contains(&d.as_str())
                    {
                        check_deps.push(d.as_str());
                    }
                }

                let mut fc_fail: Option<(&str, HashSet<usize>)> = None;
                for d in check_deps {
                    if let Some(fc_conflicts) =
                        forward_check_dep(index, d, cur_level, &reqs, &intro_level)
                    {
                        fc_fail = Some((d, fc_conflicts));
                        break;
                    }
                }
                if let Some((bad_d, fc_conflicts)) = fc_fail {
                    undo_frame(
                        frame,
                        &mut chosen,
                        &mut chosen_level,
                        &mut reqs,
                        &mut order,
                        &mut order_set,
                        &mut intro_level,
                    );
                    frame.conflict_set.extend(fc_conflicts);
                    last_err = Some(format!("no version of {bad_d} matches requirements"));
                    continue;
                }
                break;
            }
        }

        if !stack[cur_level].active {
            if last_err.is_none() {
                last_err = Some(format!(
                    "no compatible version for {}",
                    stack[cur_level].name
                ));
            }
            let mut conflicts = std::mem::take(&mut stack[cur_level].conflict_set);
            stack.pop();
            conflicts.remove(&cur_level);
            if let Some(&target_level) = conflicts.iter().max() {
                while stack.len() > target_level + 1 {
                    if let Some(mut skipped) = stack.pop() {
                        if skipped.active {
                            undo_frame(
                                &mut skipped,
                                &mut chosen,
                                &mut chosen_level,
                                &mut reqs,
                                &mut order,
                                &mut order_set,
                                &mut intro_level,
                            );
                        }
                    }
                }
                if let Some(target_frame) = stack.last_mut() {
                    for c in conflicts {
                        if c < target_level {
                            target_frame.conflict_set.insert(c);
                        }
                    }
                }
            } else {
                break;
            }
            continue;
        }

        let next_idx = chosen.len();
        if next_idx == order.len() {
            return Ok(chosen);
        }

        let next_name = order[next_idx].clone();
        let (next_cands, next_conflicts) =
            build_candidates(index, &next_name, &reqs, lock, &intro_level);
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
            conflict_set: next_conflicts,
        });
    }

    Err(SolveError {
        msg: last_err.unwrap_or_else(|| "no solution found".to_string()),
    })
}

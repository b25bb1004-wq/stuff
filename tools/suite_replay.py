#!/usr/bin/env python3
"""Replay an exam suite's reference solutions through `monolith exec` (a dev aid for writing
suites; `monolith eval --selftest` is the real check). For each task: make a computer, run the
setup, require that at least one check FAILS, run the solution, require that every check PASSES.

usage: suite_replay.py --monolith EXE --bin TOOL_DIR --home EMPTY_DIR SUITE [--only ID] [-v]
"""
import argparse, os, shlex, subprocess, sys

def parse(text):
    tasks, cur, block = [], None, None
    for n, raw in enumerate(text.split("\n"), 1):
        line = raw.rstrip("\r")
        if block is not None and (line.startswith("| ") or line == "|"):
            block.append(line[2:] if line.startswith("| ") else "")
            continue
        block = None
        if not line.strip() or line.startswith("#"):
            continue
        if line.startswith("== "):
            cur = {"id": line[3:].strip(), "prompt": "", "computer": None, "setup": [], "checks": [], "solution": []}
            tasks.append(cur)
            continue
        if cur is None or ": " not in line and not line.endswith(":"):
            sys.exit(f"line {n}: cannot parse: {line}")
        key, _, val = line.partition(":")
        val = val.strip()
        if key == "prompt":
            block = [val]; cur["prompt_lines"] = block
        elif key == "computer":
            cur["computer"] = val
        elif key in ("setup", "solution"):
            verb, _, arg = val.partition(" ")
            act = {"verb": verb, "arg": arg.strip(), "lines": []}
            cur[key].append(act)
            if verb == "write":
                block = act["lines"]
        elif key == "check":
            left, sep, exp = val.rpartition(" => ")
            if not sep:
                sys.exit(f"line {n}: check needs ' => '")
            kind, _, want = exp.partition(" ")
            cur["checks"].append((left.strip(), kind, want.strip(), val))
        else:
            sys.exit(f"line {n}: unknown key {key}")
    return tasks

class Box:
    def __init__(self, a, name):
        self.a, self.name = a, name
        self.root = os.path.join(a.home, "computers", name, "root")
        self.env = dict(os.environ, MONOLITH_HOME=a.home, MONOLITH_BIN=a.bin)
        if not os.path.isdir(self.root):
            subprocess.run([a.monolith, "new", name], env=self.env, check=True, capture_output=True)
    def run(self, line):
        argv = shlex.split(line)
        if len(argv) < 2 or argv[1] not in ("--json", "--describe"):
            argv.insert(1, "--json")
        p = subprocess.run([self.a.monolith, "exec", self.name, "--"] + argv, env=self.env, capture_output=True)
        return p.returncode, p.stdout.decode("utf-8", "replace") + p.stderr.decode("utf-8", "replace")
    def path(self, rel):
        if rel.startswith(("/", "\\", "~")) or ".." in rel.replace("\\", "/").split("/") or ":" in rel:
            sys.exit(f"path leaves the computer: {rel}")
        return os.path.join(self.root, rel)
    def act(self, act, verbose):
        v, arg = act["verb"], act["arg"]
        if v == "run":
            code, out = self.run(arg)
            if verbose: print(f"    run {arg} -> exit {code}: {out.strip()[:300]}")
            return None
        if v == "write":
            p = self.path(arg); os.makedirs(os.path.dirname(p), exist_ok=True)
            with open(p, "w", encoding="utf-8", newline="\n") as f:
                f.write("".join(l + "\n" for l in act["lines"]))
            return None
        if v == "done":
            return arg
        if v in ("read", "help", "remember", "recall"):
            return None
        sys.exit(f"unknown verb {v}")
    def check(self, chk, answer):
        left, kind, want, _ = chk
        if left == "answer":
            got = (answer or "").lower()
            return (want.lower() in got) if kind == "contains" else (want.lower() not in got), answer or ""
        if left.startswith("read "):
            try:
                code, out = 0, open(self.path(left[5:].strip()), encoding="utf-8", errors="replace").read()
            except OSError as e:
                code, out = 1, ""
        else:
            code, out = self.run(left)
        if kind == "contains": return want in out, out
        if kind == "lacks": return want not in out, out
        if kind == "exit": return code == int(want), f"exit {code}"
        sys.exit(f"bad check kind {kind}")

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--monolith", required=True); ap.add_argument("--bin", required=True)
    ap.add_argument("--home", required=True); ap.add_argument("suite"); ap.add_argument("--only")
    ap.add_argument("-v", action="store_true")
    a = ap.parse_args()
    a.home, a.bin, a.monolith = map(os.path.abspath, (a.home, a.bin, a.monolith))
    os.makedirs(a.home, exist_ok=True)
    tasks = parse(open(a.suite, encoding="utf-8").read())
    bad = 0
    for i, t in enumerate(tasks):
        if a.only and t["id"] != a.only: continue
        if not t["checks"] or not t["prompt_lines"][0]:
            print(f"BAD  {t['id']}: needs a prompt and a check"); bad += 1; continue
        box = Box(a, t["computer"] or f"t{i}-{os.getpid()}")
        for s in t["setup"]: box.act(s, a.v)
        before = [box.check(c, None) for c in t["checks"]]
        answer = None
        for s in t["solution"]:
            r = box.act(s, a.v)
            if r is not None: answer = r
        after = [box.check(c, answer) for c in t["checks"]]
        ok = (not all(p for p, _ in before)) and all(p for p, _ in after)
        print(("PASS " if ok else "FAIL ") + t["id"])
        if not ok:
            bad += 1
            if all(p for p, _ in before): print("    every check already passes before the solution")
            for c, (p, got) in zip(t["checks"], after):
                if not p: print(f"    check failed after the solution: {c[3]}\n      got: {got.strip()[:300]}")
    print(f"{len(tasks)} tasks, {bad} bad")
    sys.exit(1 if bad else 0)

main()

"""One-shot rename Strata -> Monolith (tools/rename-map.txt). Case-preserving, whole words.
Keeps the LOOP.md round log as written and leaves filesystem paths to the strata folder alone.
Run from the repo root on a clean tree: python tools/rename.py"""
import re, subprocess, pathlib

pairs = [l.split() for l in open("tools/rename-map.txt") if l.strip()]
low = {a: b for a, b in pairs}
# a path to the checkout (ADMIN/strata, strata-gate-target, ...) is not the project name
SKIP_BEFORE = re.compile(r"[/\\]$")
word = re.compile(r"\b(" + "|".join(sorted(low, key=len, reverse=True)) + r")\b", re.I)


def sub(text):
    def f(m):
        w = m.group(0)
        k = w.lower()
        if k == "strata" and (SKIP_BEFORE.search(text[: m.start()][-1:] or "") or text[m.end() : m.end() + 1] == "-"):
            return w
        new = low[k]
        if w.isupper():
            return new.upper()
        if w[0].isupper():
            return new[0].upper() + new[1:]
        return new
    return word.sub(f, text)


files = subprocess.run(["git", "ls-files"], capture_output=True, text=True, check=True).stdout.split("\n")
changed = 0
for f in filter(None, files):
    p = pathlib.Path(f)
    if p.name == "rename-map.txt" or p.name == "rename.py" or not p.is_file():
        continue
    try:
        t = p.read_text(encoding="utf-8")
    except UnicodeDecodeError:
        continue
    if p.name == "LOOP.md":
        head, sep, log = t.partition("## Round log")
        n = sub(head) + sep + log
    else:
        n = sub(t)
    if n != t:
        p.write_text(n, encoding="utf-8", newline="")
        changed += 1

# move paths whose components are old names (deepest first)
moved = 0
for f in sorted(filter(None, files), key=lambda s: -s.count("/")):
    parts = f.split("/")
    newparts = [sub(x) if x.split(".")[0].lower() in low else x for x in parts]
    if newparts != parts and pathlib.Path(f).exists():
        dst = pathlib.Path("/".join(newparts))
        dst.parent.mkdir(parents=True, exist_ok=True)
        pathlib.Path(f).rename(dst)  # plain rename: git mv per file races other git readers
        moved += 1
print(f"rewrote {changed} files, moved {moved}")

"""Leader check data for T41b: random join and index queries through sqlite3 (STRICT tables).
Writes join_cases.txt next to this file, same format as sql_cases.txt.
Usage: python gen_join_cases.py [seed]"""
import os
import random
import sqlite3
import sys

R = random.Random(int(sys.argv[1], 0) if len(sys.argv) > 1 else 0x41B)
NAMES = ["ann", "bob", "Cy", "dee", "", "eve"]


def n_or(v, p=0.15):
    return "NULL" if R.random() < p else v


def ins_t():
    rows = [f"({n_or(str(R.randint(0, 8)))}, {n_or(repr(R.choice(NAMES)))})" for _ in range(R.randint(1, 5))]
    return "INSERT INTO t (a, b) VALUES " + ", ".join(rows)


def ins_u():
    rows = [f"({n_or(str(R.randint(1, 12)))}, {n_or(str(R.randint(-3, 9)))}, {n_or(repr(R.choice(NAMES)))})"
            for _ in range(R.randint(1, 5))]
    return "INSERT INTO u (tid, v, w) VALUES " + ", ".join(rows)


def ins_x():
    rows = [f"({n_or(str(R.randint(-3, 9)))}, {repr(R.choice(NAMES))})" for _ in range(R.randint(1, 4))]
    return "INSERT INTO x (k, name) VALUES " + ", ".join(rows)


def cmp(col, lo, hi):
    k = R.randint(0, 5)
    if k == 0:
        return f"{col} = {R.randint(lo, hi)}"
    if k == 1:
        return f"{col} BETWEEN {R.randint(lo, hi)} AND {R.randint(lo, hi)}"
    if k == 2:
        return f"{col} {R.choice(['<', '<=', '>', '>='])} {R.randint(lo, hi)}"
    if k == 3:
        return f"{col} IS {R.choice(['', 'NOT '])}NULL"
    if k == 4:
        return f"{col} IN ({R.randint(lo, hi)}, {R.randint(lo, hi)})"
    return f"{col} <> {R.randint(lo, hi)}"


def where(cols):
    if R.random() < 0.35:
        return ""
    terms = [cmp(*R.choice(cols)) for _ in range(R.randint(1, 3))]
    return " WHERE " + R.choice([" AND ", " AND ", " OR "]).join(terms)


TC = [("t.a", 0, 8), ("t.id", 0, 12), ("t.rowid", 0, 12)]
UC = [("u.tid", 0, 12), ("u.v", -3, 9), ("u.id", 0, 20)]
XC = [("x.k", -3, 9)]


def select():
    k = R.randint(0, 11)
    kind = R.choice(["JOIN", "INNER JOIN", "LEFT JOIN"])
    if k <= 2:
        cols = R.choice(["t.id, u.id", "t.id, t.a, u.v, u.w", "t.b, u.id", "t.a + u.v, u.w || t.b", "u.id, t.id AS tt"])
        order = " ORDER BY t.id, u.id" if R.random() < 0.7 else ""
        lim = f" LIMIT {R.randint(0, 5)}" if order and R.random() < 0.3 else ""
        return f"SELECT {cols} FROM t {kind} u ON u.tid = t.id{where(TC + UC)}{order}{lim}"
    if k == 3:
        return (f"SELECT p.id, q.id, q.v FROM t AS p {kind} u AS q ON q.tid = p.id"
                f"{where([('p.a', 0, 8), ('q.v', -3, 9)])} ORDER BY p.id, q.id")
    if k == 4:
        k2 = R.choice(["JOIN", "LEFT JOIN"])
        return (f"SELECT t.id, u.id, x.k, x.name FROM t {kind} u ON u.tid = t.id {k2} x ON x.k = u.v"
                f"{where(TC + UC + XC)} ORDER BY t.id, u.id, x.k, x.name")
    if k == 5:
        agg = R.choice(["count(u.id)", "count(*), sum(u.v)", "max(u.w), min(u.v)", "count(u.v), sum(u.v)"])
        hav = f" HAVING count(u.id) >= {R.randint(0, 2)}" if R.random() < 0.4 else ""
        return f"SELECT t.id, {agg} FROM t {kind} u ON u.tid = t.id GROUP BY t.id{hav} ORDER BY t.id"
    if k == 6:
        return f"SELECT * FROM t {kind} x ON x.k = t.a{where(TC + XC)} ORDER BY t.id, x.k, x.name"
    if k == 7:
        return f"SELECT DISTINCT u.v FROM t {kind} u ON u.tid = t.id{where(TC + UC)} ORDER BY u.v"
    if k == 8:
        return f"SELECT t.id, u.id FROM t {kind} u ON u.tid = t.id AND u.v > {R.randint(-3, 9)}{where(TC)}"
    if k == 9:
        return R.choice([f"SELECT id FROM t JOIN u ON u.tid = t.id", "SELECT t.nope FROM t JOIN u ON u.tid = t.id",
                         "SELECT * FROM t JOIN nosuch ON 1", f"SELECT u.v FROM u WHERE u.tid = {R.randint(0, 12)} ORDER BY u.id",
                         "SELECT t.id FROM t JOIN u ON u.tid = t.id JOIN"])
    if k == 10:
        return f"SELECT t.id, t.a, t.b FROM t{where(TC)} ORDER BY t.id"
    return f"SELECT u.id, u.tid, u.v FROM u{where(UC)} ORDER BY u.id"


IDX = ["CREATE INDEX ut ON u (tid)", "CREATE INDEX utv ON u (tid, v)", "CREATE INDEX ta ON t (a)",
       "CREATE INDEX xk ON x (k)", "CREATE INDEX uv ON u (v)", "CREATE INDEX xkn ON x (k, name)"]


def classify(m):
    for key, kind in [("ambiguous", "Ambiguous"), ("no such table", "NoSuchTable"), ("no such column", "NoSuchColumn"),
                      ("no such index", "NoSuchIndex"), ("already exists", "Exists"), ("syntax error", "Parse"),
                      ("incomplete input", "Parse"), ("NOT NULL constraint", "ConstraintNN"),
                      ("UNIQUE constraint", "ConstraintU"), ("cannot store", "Type")]:
        if key in m:
            return kind
    return "Any"


def enc(v):
    if v is None:
        return "N"
    if isinstance(v, int):
        return f"I{v}"
    if isinstance(v, str):
        return "T" + v.encode().hex()
    return "B" + bytes(v).hex()


def session(out):
    db = sqlite3.connect(":memory:", isolation_level=None)
    stmts = ["CREATE TABLE t (id INTEGER PRIMARY KEY, a INTEGER, b TEXT)",
             "CREATE TABLE u (id INTEGER PRIMARY KEY, tid INTEGER, v INTEGER, w TEXT)",
             "CREATE TABLE x (k INTEGER, name TEXT)"]
    stmts += [ins_t(), ins_t(), ins_u(), ins_u(), ins_u(), ins_x(), ins_x()]
    stmts += R.sample(IDX, R.randint(0, 3))
    for i in range(80):
        r = R.random()
        if i == 40:
            stmts.append("REOPEN")
        elif r < 0.72:
            stmts.append(select())
        elif r < 0.84:
            stmts.append(R.choice([ins_t, ins_u, ins_x])())
        elif r < 0.9:
            stmts.append(R.choice([f"UPDATE u SET v = v + 1 WHERE tid = {R.randint(0, 12)}",
                                   f"DELETE FROM u WHERE v = {R.randint(-3, 9)}", f"UPDATE t SET a = {R.randint(0, 8)} WHERE id = {R.randint(1, 12)}",
                                   f"DELETE FROM x WHERE k = {R.randint(-3, 9)}"]))
        else:
            stmts.append(R.choice(IDX + ["DROP INDEX ut", "DROP INDEX ta", "DROP INDEX utv", "DROP INDEX xk"]))
    n = 0
    for s in stmts:
        if s == "REOPEN":
            out.append("REOPEN")
            continue
        n += 1
        out.append("S " + s.encode().hex())
        before = db.total_changes
        try:
            cur = db.execute(s + " STRICT" if s.startswith("CREATE TABLE") else s)
            rows = cur.fetchall()
        except sqlite3.Error as ex:
            out.append("E " + classify(str(ex)))
            continue
        is_sel = s.startswith("SELECT")
        ordered = 0 if is_sel and " ORDER BY " not in s else 1
        cols = [d[0] for d in cur.description] if is_sel and cur.description else []
        ch = db.total_changes - before if s.split()[0] in ("INSERT", "UPDATE", "DELETE") else 0
        out.append(f"OK {ch} {ordered} {len(rows)} " + ",".join(c.encode().hex() for c in cols))
        out.extend("R " + ",".join(enc(v) for v in row) for row in rows)
    return n


out, total = [], 0
for _ in range(4):
    out.append("NEW")
    total += session(out)
path = os.path.join(os.path.dirname(os.path.abspath(__file__)), "join_cases.txt")
with open(path, "w", newline="\n") as f:
    f.write("\n".join(out) + "\n")
print("statements", total)

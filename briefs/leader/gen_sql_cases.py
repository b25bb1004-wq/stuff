"""Leader check data for T41a: random single-table SQL run through sqlite3 (STRICT tables).
Writes sql_cases.txt next to this file. Usage: python gen_sql_cases.py [seed]"""
import os
import random
import sqlite3
import sys

R = random.Random(int(sys.argv[1], 0) if len(sys.argv) > 1 else 0x41A)
WORDS = ["apple", "Apple", "banana", "cherry", "date", "fig", "grape", "kiwi", "lemon", "mango", "a_b", "x%y", "", "it's"]


def lit_int():
    return str(R.randint(-60, 60))


def lit_text():
    return "'" + R.choice(WORDS).replace("'", "''") + "'"


def lit_blob():
    return "x'" + "".join(R.choice("0123456789ab") for _ in range(2 * R.randint(0, 3))) + "'"


def val(col):
    if col in "abe" and R.random() < 0.15:
        return "NULL"
    if col == "d":
        return "NULL" if R.random() < 0.1 else "'" + R.choice(WORDS[:10]) + str(R.randint(0, 25)) + "'"
    if col == "e":
        return lit_blob()
    if col == "b":
        return lit_text()
    return lit_int()


def int_expr(d=0):
    if d > 1 or R.random() < 0.4:
        return R.choice(["a", "c", "id", lit_int()])
    return f"{int_expr(d + 1)} {R.choice(['+', '-', '*', '/', '%'])} {int_expr(d + 1)}"


def cond(d=0):
    r = R.random()
    if d < 2 and r < 0.2:
        return f"({cond(d + 1)}) {R.choice(['AND', 'OR'])} ({cond(d + 1)})"
    if d < 2 and r < 0.25:
        return f"NOT ({cond(d + 1)})"
    k = R.randint(0, 9)
    neg = R.choice(["", "NOT "])
    if k == 0:
        return f"{R.choice('abde')} IS {neg}NULL"
    if k == 1:
        return f"{int_expr()} {neg}BETWEEN {lit_int()} AND {lit_int()}"
    if k == 2:
        items = ", ".join(R.choice([lit_int(), lit_int(), "NULL"]) for _ in range(R.randint(1, 4)))
        return f"a {neg}IN ({items})"
    if k == 3:
        pat = R.choice(["a%", "%an%", "_i%", "APPLE", "%", "x%y", "a_b", "%'%"]).replace("'", "''")
        return f"b {neg}LIKE '{pat}'"
    if k == 4:
        return f"b {R.choice(['<', '>=', '=', '<>'])} {lit_text()}"
    if k == 5:
        return f"e {R.choice(['<', '>', '='])} {lit_blob()}"
    if k == 6:
        return f"d {R.choice(['<', '>', '!=', '=='])} {lit_text()}"
    return f"{int_expr()} {R.choice(['=', '<>', '<', '<=', '>', '>='])} {int_expr()}"


PROJ = ["a + c", "a * 2", "-a", "b || d", "upper(b)", "lower(d)", "length(b)", "abs(a)", "coalesce(a, c)",
        "substr(b, 2, 3)", "substr(d, 3)", "typeof(e)", "typeof(a)",
        "CASE WHEN a > 0 THEN 'p' WHEN a < 0 THEN 'n' ELSE 'z' END",
        "CASE c % 3 WHEN 0 THEN 'zero' WHEN 1 THEN 'one' END", "a / 3", "c % 4", "a IS NULL", "b LIKE 'a%'",
        "a BETWEEN 0 AND 20", "length(e)", "id", "a", "b", "c", "d", "e"]


def where():
    return f" WHERE {cond()}" if R.random() < 0.7 else ""


def select():
    k = R.randint(0, 9)
    if k <= 2:
        if R.random() < 0.2:
            cols = "*"
        else:
            cols = ", ".join(p + (f" AS n{i}" if R.random() < 0.2 else "")
                             for i, p in enumerate(R.sample(PROJ, R.randint(1, 4))))
        tail = ""
        if R.random() < 0.6:
            keys = R.sample(["a", "b", "c DESC", "d", "a DESC", "b DESC"], R.randint(0, 2)) + [R.choice(["id", "id DESC"])]
            tail = " ORDER BY " + ", ".join(keys)
            if R.random() < 0.4:
                tail += f" LIMIT {R.randint(0, 6)}" + (f" OFFSET {R.randint(0, 4)}" if R.random() < 0.5 else "")
        return f"SELECT {cols} FROM t{where()}{tail}"
    if k == 3:
        return f"SELECT count(*), count(a), sum(a), min(b), max(c), min(e), max(d) FROM t{where()}"
    if k <= 5:
        g = R.choice(["c % 3", "a", "b", "a IS NULL", "length(b)"])
        agg = ", ".join(R.sample(["count(*)", "sum(c)", "min(a)", "max(b)", "count(d)", "sum(a)"], R.randint(1, 3)))
        hav = f" HAVING count(*) {R.choice(['>', '>=', '<'])} {R.randint(0, 3)}" if R.random() < 0.5 else ""
        return f"SELECT {g}, {agg} FROM t{where()} GROUP BY {g}{hav} ORDER BY {g}{R.choice(['', ' DESC'])}"
    if k == 6:
        c = R.choice(["b", "a", "c % 5", "upper(b)", "typeof(e)"])
        return f"SELECT DISTINCT {c} FROM t{where()} ORDER BY {c}{R.choice(['', ' DESC'])}"
    if k == 7:
        return "SELECT " + ", ".join(R.sample([
            "1 + 2", "'x' || 'y'", "7 / 2", "-7 % 3", "-7 / 2", "NULL IS NULL", "typeof(x'00')",
            "5 BETWEEN 1 AND 9", "1 / 0", "NULL = NULL", "NULL OR 1", "NULL AND 0", "1 < 'a'", "'a' < x'00'",
            "coalesce(NULL, NULL, 3)", "abs(-4)", "length('hello')", "upper('mIx')", "substr('abcdef', 2, 2)",
            "3 IN (1, NULL)", "3 NOT IN (1, 2)", "'ab' LIKE 'A_'", "9223372036854775807 + 1",
            "-9223372036854775807 - 2", "4611686018427387904 * 2", "CASE WHEN NULL THEN 1 ELSE 2 END"],
            R.randint(1, 4)))
    if k == 8:
        if R.random() < 0.3:
            return "SELECT id, a FROM t WHERE nosuch = 1"
        return f"SELECT a, sum(c) FROM t GROUP BY a ORDER BY a LIMIT {R.randint(1, 3)}"
    return R.choice(["SELECT * FROM nosuch", "SELEC a FROM t", "SELECT a FROM t WHERE", "SELECT (1 + FROM t",
                     "SELECT count(*) FROM t", "SELECT max(id) FROM t"])


def insert():
    r = R.random()
    if r < 0.05:
        return f"INSERT INTO t VALUES (NULL, {', '.join(val(c) for c in 'abcde')})"
    if r < 0.09:
        return "INSERT INTO t (a, b, c) VALUES ('notint', 'x', 1)"
    if r < 0.12:
        return "INSERT INTO t (a, b, c) VALUES (1, x'01', 1)"
    if r < 0.14:
        return "INSERT INTO t (zz) VALUES (1)"
    cols = "abcde" if r > 0.2 else "abde"  # missing c breaks NOT NULL
    rows = ["(" + ", ".join(val(c) for c in cols) + ")" for _ in range(R.randint(1, 6))]
    return f"INSERT INTO t ({', '.join(cols)}) VALUES {', '.join(rows)}"


def update():
    sets = R.sample([f"a = {int_expr()}", f"b = {R.choice([lit_text(), 'upper(b)', 'b || ' + lit_text(), 'NULL'])}",
                     f"c = c + {lit_int()}", f"d = {R.choice([lit_text(), 'NULL', 'lower(d)'])}",
                     f"e = {lit_blob()}", "c = NULL"], R.randint(1, 2))
    return f"UPDATE t SET {', '.join(sets)}{where()}"


def classify(m):
    if "NOT NULL constraint" in m:
        return "ConstraintNN"
    if "UNIQUE constraint" in m:
        return "ConstraintU"
    if "cannot store" in m:
        return "Type"
    if "no such index" in m:
        return "NoSuchIndex"
    if "no such table" in m:
        return "NoSuchTable"
    if "no such column" in m or "has no column" in m:
        return "NoSuchColumn"
    if "already exists" in m:
        return "Exists"
    if "transaction" in m:
        return "Misuse"
    if "syntax error" in m or "incomplete input" in m:
        return "Parse"
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
    stmts = ["CREATE TABLE t (id INTEGER PRIMARY KEY, a INTEGER, b TEXT, c INTEGER NOT NULL, d TEXT UNIQUE, e BLOB)",
             "CREATE TABLE t (x INTEGER)", "CREATE TABLE u (k TEXT PRIMARY KEY, v INTEGER)",
             "INSERT INTO u VALUES ('one', 1), ('two', 2)", "INSERT INTO u VALUES ('one', 3)",
             "SELECT k, v FROM u ORDER BY k"]
    stmts += [insert() for _ in range(6)]
    stmts.append(R.choice(["CREATE INDEX ia ON t (a)", "CREATE INDEX ibc ON t (b, c)"]))
    for i in range(46):
        r = R.random()
        if i == 23:
            stmts.append("REOPEN")
        elif r < 0.45:
            stmts.append(select())
        elif r < 0.65:
            stmts.append(insert())
        elif r < 0.78:
            stmts.append(update())
        elif r < 0.84:
            stmts.append(f"DELETE FROM t{where()}")
        elif r < 0.9:
            stmts += ["BEGIN", insert(), update(), select(), R.choice(["ROLLBACK", "COMMIT"]),
                      "SELECT count(*), sum(c) FROM t"]
        else:
            stmts.append(R.choice(["COMMIT", "ROLLBACK", "CREATE UNIQUE INDEX ud ON t (c)", "DROP INDEX ia",
                                   "DROP INDEX nosuch", "DROP TABLE nosuch", "CREATE INDEX ix ON t (nosuch)",
                                   "SELECT * FROM t ORDER BY id"]))
    stmts += ["SELECT * FROM t ORDER BY id", "DROP TABLE t", "SELECT * FROM t", "CREATE TABLE t (z TEXT)",
              "SELECT * FROM t"]
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
        if any(isinstance(v, float) for row in rows for v in row):
            out.append("E Overflow")
            continue
        is_sel = s.startswith("SELECT")
        ordered = 0 if is_sel and " FROM " in s and " ORDER BY " not in s else 1
        cols = [d[0] for d in cur.description] if is_sel and cur.description else []
        dml = s.split()[0] in ("INSERT", "UPDATE", "DELETE")
        ch = db.total_changes - before if dml else 0
        out.append(f"OK {ch} {ordered} {len(rows)} " + ",".join(c.encode().hex() for c in cols))
        out.extend("R " + ",".join(enc(v) for v in row) for row in rows)
    return n


out, total = [], 0
for _ in range(6):
    out.append("NEW")
    total += session(out)
path = os.path.join(os.path.dirname(os.path.abspath(__file__)), "sql_cases.txt")
with open(path, "w", newline="\n") as f:
    f.write("\n".join(out) + "\n")
print("statements", total)

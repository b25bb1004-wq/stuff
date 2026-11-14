//! Leader check for T10 (public API only): well-typed programs written from docs/MICA.md must
//! check, and their IR must pass the leader's own validator (labels, registers, call arity,
//! types of Eq/Ne and Branch, no And/Or, spans inside the file); then spot checks of the type
//! table, layer bits and constant folding; then one reject program per rule, each an `Err` whose
//! span lies inside the file, and the layer error worded exactly as the spec says.
#![allow(clippy::unwrap_used)]
use mica::ast::BinOp;
use mica::check::check;
use mica::ir::{Body, Inst, Lit, Program, Reg, Ty, TypeInfo, TypeKind};
use mica::parser::parse;

const BUILTIN_LAYERS: [&str; 7] = ["io", "fs", "net", "time", "rand", "chain", "sys"];

const GOOD: &[(&str, &str)] = &[
    (
        "spec example",
        "type Shape = Circle(int) | Rect(int, int)
fn say(s: str) uses io = extern \"io.print\"
fn area(s: Shape) -> int {
    match s {
        Circle(r) => 3 * r * r,
        Rect(w, h) => w * h,
    }
}
fn total(shapes: [Shape]) -> int {
    var sum = 0
    for s in shapes { sum = sum + area(s) }
    sum
}
fn main() uses io {
    [Circle(2), Rect(3, 4)] |> total() |> str() |> say()
}
test \"a rectangle\" { assert area(Rect(3, 4)) == 12 }
prop \"area is never negative\" (w: int, h: int) where w >= 0 && w < 1000 && h >= 0 && h < 1000 {
    assert area(Rect(w, h)) >= 0
}
",
    ),
    (
        "records and places",
        "type Point = { x: int, y: int }
fn bump(p: Point) {
    var q = p
    q.x = q.x +% 1
    q.y = q.y -% 1
}
fn main() {
    let p = Point { x: 1, y: 2 }
    bump(p)
    var xs = [p, Point { x: 0, y: 0 }]
    xs[1].x = 5
    xs[0] = Point { x: 9, y: 9 }
    assert xs[1].x == 5
}
",
    ),
    (
        "options consts layers",
        "layer audit
const limit: int = 0x10 * 4 + 0b11
fn say(s: str) uses io = extern \"io.print\"
fn line() -> str? uses io = extern \"io.read_line\"
fn log(s: str) uses io, audit { say(s) }
fn show(v: int?) uses io, audit {
    if v == none { return }
    match v {
        none => log(\"none\"),
        some(n) => if n > limit { log(\"big\") } else { log(\"small\") },
    }
}
fn main() uses io, audit {
    match line() {
        none => log(\"bye\"),
        some(t) => t |> parse_int() |> show(),
    }
}
",
    ),
    (
        "function values and loops",
        "fn twice(f: fn(int) -> int, x: int) -> int { f(f(x)) }
fn inc(x: int) -> int { x + 1 }
fn main() {
    var xs = [0; 8]
    for i in 0..len(xs) { xs[i] = twice(inc, i) }
    let ys: [int] = xs[2..5]
    for y in ys { assert y >= 2, \"too small\" }
    let e: [int] = []
    let u = ()
    var n = 0
    while true {
        n = n + 1
        if n == 3 { continue }
        if n > 5 { break }
    }
}
",
    ),
    (
        "tuples, option equality, short circuit",
        "fn pair(a: int) -> (int, bool) { (a, a > 0) }
fn same(a: (int, bool), b: (int, bool)) -> bool { a == b }
fn differ(a: int?, b: int?) -> bool { a != b }
fn both(a: bool, b: bool) -> bool { a && b || !a }
fn main() {
    assert same(pair(1), (1, true))
    assert differ(some(1), none)
    assert both(true, true)
    let t = pair(3)
    assert t.0 == 3 && t.1
}
",
    ),
    (
        "recursive variants and literal patterns",
        "type Tree = Leaf | Node(Tree, int, Tree)
type Res = Ok(int) | Err(str)
fn sum(t: Tree) -> int {
    match t {
        Leaf => 0,
        Node(l, v, r) => sum(l) + v + sum(r),
    }
}
fn grade(r: Res) -> str {
    match r {
        Ok(0) => \"zero\",
        Ok(_) => \"some\",
        Err(\"\") => \"empty\",
        Err(m) => m,
    }
}
fn sign(a: int) -> int { if a < 0 { -1 } else if a == 0 { 0 } else { 1 } }
test \"tree\" { assert sum(Node(Leaf, 3, Node(Leaf, 4, Leaf))) == 7 }
",
    ),
    (
        "built-in functions",
        "fn main() {
    var xs: [int] = []
    push(xs, 3)
    push(xs, 4)
    let last = pop(xs)
    assert last == some(4)
    let s = \"ab\" + \"cd\"
    assert len(s) == 4
    let b = bytes(s)
    assert text(b) == some(\"abcd\")
    assert int(b[0]) == 97
    assert byte(98) == b[1]
    assert min(1, 2) + max(3, 4) + abs(-5) == 10
    let joined = [1, 2] + [3]
    assert len(joined) == 3
    assert str(42) == \"42\"
    assert parse_int(\"x\") == none
    assert b'a' < b'b' && \"a\" < \"b\"
}
",
    ),
    (
        "matches on int, bool, tuple; bytes",
        "fn classify(n: int) -> str {
    match n {
        0 => \"zero\",
        1 => \"one\",
        _ => \"many\",
    }
}
fn flip(b: bool) -> int {
    match b { true => 1, false => 0 }
}
fn both(p: (bool, bool)) -> int {
    match p {
        (true, true) => 2,
        (true, false) => 1,
        _ => 0,
    }
}
fn main() {
    let x: byte = b'z'
    let y = x - b'a'
    assert flip(true) + both((true, false)) == 2
    assert classify(int(y)) == \"many\"
}
",
    ),
    (
        "tests use layers, props are pure",
        "fn clock() -> int uses time = extern \"time.now\"
fn say(s: str) uses io = extern \"io.print\"
fn stamp() uses io, time { clock() |> str() |> say() }
test \"can use layers\" { stamp() }
prop \"abs is non-negative\" (a: int) where a > -1000000 { assert abs(a) >= 0 }
fn main() uses io, time, rand { stamp() }
",
    ),
    (
        "declared layers",
        "layer audit
layer metrics
fn post(e: str) uses chain = extern \"chain.emit\"
fn rec(e: str) uses chain, metrics { post(e) }
fn main() uses chain, audit, metrics { rec(\"x\") }
",
    ),
    (
        "function value with a smaller layer set",
        "fn say(s: str) uses io = extern \"io.print\"
fn quiet(s: str) { }
fn run(f: fn(str) -> unit uses io, s: str) uses io { f(s) }
fn main() uses io {
    run(say, \"a\")
    run(quiet, \"b\")
}
",
    ),
    (
        "block values",
        "fn f(a: int) -> int {
    let b = {
        let c = a * 2
        c + 1
    }
    let d = if b > 3 { b } else { 3 }
    var e = 0
    for i in 0..d { e = e + i }
    e
}
",
    ),
    (
        "constant folding",
        "const k: int = 2 * 3 + 1
fn seven() -> int { k }
",
    ),
];

/// (rule, program). Every one must be rejected.
const BAD: &[(&str, &str)] = &[
    ("unknown value", "fn f() -> int { y }\n"),
    ("unknown type", "fn f(p: Pt) { }\n"),
    ("unknown variant", "type A = B | C\nfn f() -> A { D }\n"),
    ("unknown layer", "fn f() uses nope { }\n"),
    ("unknown function", "fn f() { g() }\n"),
    ("duplicate function", "fn f() { }\nfn f() { }\n"),
    ("duplicate type", "type A = B\ntype A = C\n"),
    ("duplicate variant", "type A = B | B\n"),
    ("duplicate field", "type P = { x: int, x: int }\n"),
    ("duplicate layer", "layer a\nlayer a\n"),
    ("redeclared built-in layer", "layer io\n"),
    ("duplicate parameter", "fn f(a: int, a: int) { }\n"),
    ("byte where int", "fn f(b: byte) -> int { b }\n"),
    ("int plus byte", "fn f(a: int, b: byte) -> int { a + b }\n"),
    ("str plus int", "fn f() -> str { \"a\" + 1 }\n"),
    ("let annotation", "fn f() { let x: int = \"s\" }\n"),
    ("untyped empty array", "fn f() { let x = [] }\n"),
    ("untyped none", "fn f() { let x = none }\n"),
    ("== on arrays", "fn f(a: [int], b: [int]) -> bool { a == b }\n"),
    ("== on records", "type P = { x: int }\nfn f(a: P, b: P) -> bool { a == b }\n"),
    ("== across types", "fn f(a: int, b: str) -> bool { a == b }\n"),
    ("< on bool", "fn f(a: bool, b: bool) -> bool { a < b }\n"),
    ("match missing variant", "type A = B | C\nfn f(a: A) -> int { match a { B => 1 } }\n"),
    ("match missing bool", "fn f(a: bool) -> int { match a { true => 1 } }\n"),
    ("match missing none", "fn f(a: int?) -> int { match a { some(x) => x } }\n"),
    ("match missing int", "fn f(a: int) -> int { match a { 0 => 1 } }\n"),
    ("match pattern type", "fn f(a: int) -> int { match a { true => 1, _ => 0 } }\n"),
    ("pattern arity", "type A = B(int)\nfn f(a: A) -> int { match a { B(x, y) => x } }\n"),
    ("assign to let", "fn f() {\n  let x = 1\n  x = 2\n}\n"),
    ("assign to literal", "fn f() { 1 = 2 }\n"),
    ("assign to call", "fn g() -> int { 1 }\nfn f() { g() = 2 }\n"),
    ("assign wrong type", "fn f() {\n  var x = 1\n  x = true\n}\n"),
    ("break outside loop", "fn f() { break }\n"),
    ("continue outside loop", "fn f() { continue }\n"),
    ("return wrong type", "fn f() -> int { return true }\n"),
    ("return value from unit fn", "fn f() { return 1 }\n"),
    ("bare return in int fn", "fn f() -> int { return }\n"),
    ("body type", "fn f() -> int { true }\n"),
    ("if branch types", "fn f(a: bool) -> int { if a { 1 } else { true } }\n"),
    ("if without else as value", "fn f(a: bool) -> int { if a { 1 } }\n"),
    ("match arm types", "fn f(a: bool) -> int { match a { true => 1, false => \"x\" } }\n"),
    ("if condition", "fn f() { if 1 { } }\n"),
    ("while condition", "fn f() { while 1 { } }\n"),
    ("for over int", "fn f() { for i in 5 { } }\n"),
    ("range as a value", "fn f() { let r = 0..3 }\n"),
    ("call a non-function", "fn f(a: int) -> int { a(1) }\n"),
    ("argument count", "fn g(a: int) -> int { a }\nfn f() -> int { g() }\n"),
    ("argument type", "fn g(a: int) -> int { a }\nfn f() -> int { g(true) }\n"),
    ("record missing field", "type P = { x: int, y: int }\nfn f() -> P { P { x: 1 } }\n"),
    ("record unknown field", "type P = { x: int }\nfn f() -> P { P { x: 1, z: 2 } }\n"),
    ("field access", "type P = { x: int }\nfn f(p: P) -> int { p.z }\n"),
    ("tuple index", "fn f(t: (int, int)) -> int { t.2 }\n"),
    ("variant arity", "type A = B(int)\nfn f() -> A { B(1, 2) }\n"),
    ("index type", "fn f(xs: [int]) -> int { xs[true] }\n"),
    ("push type", "fn f(xs: [int]) { push(xs, \"a\") }\n"),
    ("assert condition", "fn f() { assert 1 }\n"),
    ("assert message", "fn f() { assert true, 5 }\n"),
    ("negate bool", "fn f() -> bool { -true }\n"),
    ("not int", "fn f() -> int { !1 }\n"),
    ("const from a call", "fn g() -> int { 1 }\nconst k: int = g()\n"),
    ("const overflow", "const k: int = 9223372036854775807 + 1\n"),
    ("const type", "const k: int = true\n"),
    ("main with parameters", "fn main(a: int) { }\n"),
    ("built-in name taken", "fn len(x: int) -> int { x }\n"),
    (
        "prop body uses a layer",
        "fn say(s: str) uses io = extern \"io.print\"\nprop \"p\" (a: int) { say(\"x\") }\n",
    ),
    (
        "prop guard uses a layer",
        "fn clock() -> int uses time = extern \"time.now\"\nprop \"p\" (a: int) where clock() > 0 { assert true }\n",
    ),
    ("prop guard type", "prop \"p\" (a: int) where a { assert true }\n"),
    (
        "layer through a function value",
        "fn say(s: str) uses io = extern \"io.print\"\nfn run(f: fn(str) -> unit, s: str) { f(s) }\nfn main() uses io { run(say, \"a\") }\n",
    ),
    (
        "call through a value with a bigger set",
        "fn run(f: fn(str) -> unit uses io, s: str) { f(s) }\n",
    ),
];

const LAYER_PATH: &str = "fn fetch() uses net { }
fn sync() { fetch() }
fn main() { sync() }
";

fn checked(name: &str, src: &str) -> Program {
    let m = parse(src).unwrap_or_else(|d| panic!("{name}: parse failed: {d:?}"));
    let p = check(&m).unwrap_or_else(|d| panic!("{name}: check failed: {d:?}"));
    assert_eq!(check(&m).unwrap(), p, "{name}: check is not deterministic");
    validate(name, src, &p);
    p
}

fn func<'a>(p: &'a Program, name: &str) -> &'a mica::ir::Func {
    p.funcs
        .iter()
        .find(|f| f.name == name)
        .unwrap_or_else(|| panic!("no function {name}"))
}

fn regs_of(i: &Inst) -> Vec<Reg> {
    let mut v = Vec::new();
    match i {
        Inst::Lit(d, _) | Inst::NewNone(d) => v.push(*d),
        Inst::Move(a, b) | Inst::Un(a, _, b) | Inst::NewSome(a, b) | Inst::Tag(a, b) => {
            v.extend([*a, *b])
        }
        Inst::GetField(a, b, _) | Inst::SetField(a, _, b) => v.extend([*a, *b]),
        Inst::Bin(d, _, a, b)
        | Inst::NewFill(d, a, b)
        | Inst::GetIndex(d, a, b)
        | Inst::SetIndex(d, a, b) => v.extend([*d, *a, *b]),
        Inst::Call(d, _, xs)
        | Inst::Builtin(d, _, xs)
        | Inst::NewArray(d, xs)
        | Inst::NewTuple(d, xs)
        | Inst::NewRecord(d, _, xs)
        | Inst::NewVariant(d, _, _, xs) => {
            v.push(*d);
            v.extend(xs.iter().copied());
        }
        Inst::CallValue(d, f, xs) => {
            v.extend([*d, *f]);
            v.extend(xs.iter().copied());
        }
        Inst::Branch(c, _, _) | Inst::Ret(c) | Inst::Assert(c, _) => v.push(*c),
        Inst::Label(_) | Inst::Jump(_) => {}
    }
    v
}

fn named_ok(p: &Program, t: &Ty) -> bool {
    match t {
        Ty::Named(id) => (*id as usize) < p.types.len(),
        Ty::Array(e) | Ty::Opt(e) => named_ok(p, e),
        Ty::Tuple(xs) => xs.iter().all(|x| named_ok(p, x)),
        Ty::Fn { params, ret, .. } => params.iter().all(|x| named_ok(p, x)) && named_ok(p, ret),
        _ => true,
    }
}

/// The leader's own validator over the public IR.
fn validate(name: &str, src: &str, p: &Program) {
    assert!(
        p.layers.len() >= 7 && p.layers.len() <= 64,
        "{name}: layer table"
    );
    for (i, l) in BUILTIN_LAYERS.iter().enumerate() {
        assert_eq!(p.layers[i], *l, "{name}: built-in layer {i}");
    }
    let all_layers = if p.layers.len() == 64 {
        u64::MAX
    } else {
        (1u64 << p.layers.len()) - 1
    };
    for f in &p.funcs {
        let ctx = format!("{name}: fn {}", f.name);
        assert!(
            f.params as usize <= f.regs.len(),
            "{ctx}: params beyond regs"
        );
        assert_eq!(
            f.layers & !all_layers,
            0,
            "{ctx}: layer bit beyond the table"
        );
        assert!(
            named_ok(p, &f.ret) && f.regs.iter().all(|t| named_ok(p, t)),
            "{ctx}: bad TypeId"
        );
        let (insts, spans) = match &f.body {
            Body::Extern(imp) => {
                assert!(!imp.is_empty(), "{ctx}: empty import name");
                continue;
            }
            Body::Code { insts, spans } => (insts, spans),
        };
        assert!(!insts.is_empty(), "{ctx}: empty body");
        assert_eq!(insts.len(), spans.len(), "{ctx}: one span per instruction");
        for s in spans {
            assert!(
                s.start <= s.end && s.end as usize <= src.len(),
                "{ctx}: span {s:?} outside file"
            );
        }
        assert!(
            matches!(insts.last(), Some(Inst::Ret(_)) | Some(Inst::Jump(_))),
            "{ctx}: body must end in Ret or Jump, ends in {:?}",
            insts.last()
        );
        let mut defined = std::collections::HashSet::new();
        for i in insts {
            if let Inst::Label(l) = i {
                assert!(defined.insert(*l), "{ctx}: label {l} defined twice");
            }
        }
        let ty = |r: Reg| -> &Ty {
            f.regs
                .get(r as usize)
                .unwrap_or_else(|| panic!("{ctx}: register {r} has no type"))
        };
        for i in insts {
            for r in regs_of(i) {
                ty(r);
            }
            match i {
                Inst::Jump(l) => assert!(defined.contains(l), "{ctx}: jump to undefined {l}"),
                Inst::Branch(c, a, b) => {
                    assert!(
                        defined.contains(a) && defined.contains(b),
                        "{ctx}: branch target"
                    );
                    assert_eq!(ty(*c), &Ty::Bool, "{ctx}: branch on a non-bool");
                }
                Inst::Bin(_, op, a, b) => {
                    assert!(
                        !matches!(op, BinOp::And | BinOp::Or),
                        "{ctx}: And/Or not lowered"
                    );
                    if matches!(op, BinOp::Eq | BinOp::Ne) {
                        assert!(
                            matches!(ty(*a), Ty::Int | Ty::Byte | Ty::Bool | Ty::Unit | Ty::Str),
                            "{ctx}: Eq/Ne on {:?}",
                            ty(*a)
                        );
                        assert_eq!(ty(*a), ty(*b), "{ctx}: Eq/Ne across types");
                    }
                }
                Inst::Ret(r) => assert_eq!(ty(*r), &f.ret, "{ctx}: Ret type"),
                Inst::Call(d, id, xs) => {
                    let g = p
                        .funcs
                        .get(*id as usize)
                        .unwrap_or_else(|| panic!("{ctx}: bad FuncId"));
                    assert_eq!(
                        xs.len(),
                        g.params as usize,
                        "{ctx}: arity of call to {}",
                        g.name
                    );
                    assert_eq!(ty(*d), &g.ret, "{ctx}: result type of call to {}", g.name);
                    for (k, x) in xs.iter().enumerate() {
                        let (want, got) = (&g.regs[k], ty(*x));
                        if !matches!(want, Ty::Fn { .. }) {
                            assert_eq!(got, want, "{ctx}: argument {k} of {}", g.name);
                        }
                    }
                }
                Inst::Lit(_, Lit::Fn(id)) => {
                    assert!((*id as usize) < p.funcs.len(), "{ctx}: Lit::Fn")
                }
                Inst::NewRecord(_, t, xs) => match p.types.get(*t as usize).map(|t| &t.kind) {
                    Some(TypeKind::Record(fs)) => {
                        assert_eq!(fs.len(), xs.len(), "{ctx}: record fields")
                    }
                    other => panic!("{ctx}: NewRecord of {other:?}"),
                },
                Inst::NewVariant(_, t, tag, xs) => {
                    match p.types.get(*t as usize).map(|t| &t.kind) {
                        Some(TypeKind::Variants(vs)) => {
                            let v = vs
                                .get(*tag as usize)
                                .unwrap_or_else(|| panic!("{ctx}: tag"));
                            assert_eq!(v.1.len(), xs.len(), "{ctx}: payload of {}", v.0);
                        }
                        other => panic!("{ctx}: NewVariant of {other:?}"),
                    }
                }
                _ => {}
            }
        }
    }
    if let Some(m) = p.main {
        let f = &p.funcs[m as usize];
        assert_eq!((f.name.as_str(), f.params), ("main", 0), "{name}: main");
    }
    for (t, id) in &p.tests {
        let f = p
            .funcs
            .get(*id as usize)
            .unwrap_or_else(|| panic!("{name}: test {t}"));
        assert_eq!(f.params, 0, "{name}: test {t} takes parameters");
    }
    for pr in &p.props {
        let f = &p.funcs[pr.func as usize];
        assert_eq!(f.layers, 0, "{name}: prop {} has layers", pr.name);
        if let Some(g) = pr.guard {
            let g = &p.funcs[g as usize];
            assert_eq!(g.params, f.params, "{name}: guard of {} params", pr.name);
            assert_eq!(g.ret, Ty::Bool, "{name}: guard of {} type", pr.name);
            assert_eq!(g.layers, 0, "{name}: guard of {} layers", pr.name);
        }
    }
}

#[test]
fn leader_good_programs_check_and_validate() {
    let mut n = 0;
    for (name, src) in GOOD {
        let p = checked(name, src);
        n += p
            .funcs
            .iter()
            .map(|f| match &f.body {
                Body::Code { insts, .. } => insts.len(),
                Body::Extern(_) => 0,
            })
            .sum::<usize>();
    }
    eprintln!("{} programs, {n} IR instructions validated", GOOD.len());
    assert!(n > 300, "suspiciously little IR: {n} instructions");
}

#[test]
fn leader_spot_checks() {
    let p = checked("spec example", GOOD[0].1);
    assert_eq!(
        p.types,
        vec![TypeInfo {
            name: "Shape".into(),
            kind: TypeKind::Variants(vec![
                ("Circle".into(), vec![Ty::Int]),
                ("Rect".into(), vec![Ty::Int, Ty::Int]),
            ]),
        }]
    );
    assert_eq!(p.tests.len(), 1);
    assert_eq!(p.tests[0].0, "a rectangle");
    assert_eq!(p.props.len(), 1);
    assert_eq!(p.props[0].name, "area is never negative");
    assert!(p.props[0].guard.is_some());
    assert!(p.main.is_some());
    assert_eq!(func(&p, "say").body, Body::Extern("io.print".into()));
    assert_eq!(func(&p, "area").ret, Ty::Int);
    assert_eq!(func(&p, "total").regs[0], Ty::Array(Box::new(Ty::Named(0))));

    let p = checked("options consts layers", GOOD[2].1);
    assert_eq!(p.layers.len(), 8);
    assert_eq!(p.layers[7], "audit");
    assert_eq!(func(&p, "log").layers, 1 | 1 << 7);
    assert_eq!(func(&p, "line").ret, Ty::Opt(Box::new(Ty::Str)));

    let p = checked("tests use layers", GOOD[8].1);
    assert_eq!(func(&p, "stamp").layers, 1 | 1 << 3);
    assert_eq!(func(&p, "main").layers, 1 | 1 << 3 | 1 << 4);

    let p = checked("declared layers", GOOD[9].1);
    let names: Vec<&str> = p.layers.iter().map(|s| s.as_str()).collect();
    assert_eq!(
        names,
        ["io", "fs", "net", "time", "rand", "chain", "sys", "audit", "metrics"]
    );
    assert_eq!(func(&p, "rec").layers, 1 << 5 | 1 << 8);
    assert_eq!(func(&p, "main").layers, 1 << 5 | 1 << 7 | 1 << 8);

    let p = checked("constant folding", GOOD[12].1);
    let Body::Code { insts, .. } = &func(&p, "seven").body else {
        panic!("seven is extern")
    };
    assert!(
        insts.iter().any(|i| matches!(i, Inst::Lit(_, Lit::Int(7)))),
        "k not folded to 7: {insts:?}"
    );
    assert!(
        !insts.iter().any(|i| matches!(i, Inst::Bin(..))),
        "arithmetic left in seven: {insts:?}"
    );
}

#[test]
fn leader_rejects_one_program_per_rule() {
    let mut accepted = Vec::new();
    for (rule, src) in BAD {
        // The parser already rejects some programs (e.g. `1 = 2`); that counts as a rejection.
        let res = parse(src).and_then(|m| check(&m).map(|_| ()));
        match res {
            Ok(_) => accepted.push(*rule),
            Err(d) => assert!(
                d.span.start <= d.span.end && d.span.end as usize <= src.len(),
                "{rule}: span {:?} outside the file",
                d.span
            ),
        }
    }
    assert!(
        accepted.is_empty(),
        "accepted programs that break a rule: {accepted:?}"
    );
    eprintln!("{} reject programs", BAD.len());
}

#[test]
fn leader_layer_error_names_the_full_path() {
    let d = check(&parse(LAYER_PATH).unwrap()).unwrap_err();
    assert_eq!(
        d.message,
        "layer 'net' is not allowed here: main -> sync -> fetch (uses net)"
    );
}

#[test]
fn leader_more_than_64_layers_is_an_error() {
    let mut src = String::new();
    for i in 0..57 {
        src.push_str(&format!("layer l{i}\n"));
    }
    assert!(
        check(&parse(&src).unwrap()).is_ok(),
        "exactly 64 layers must be fine"
    );
    src.push_str("layer l57\n");
    assert!(
        check(&parse(&src).unwrap()).is_err(),
        "65 layers must be a Diag"
    );
}

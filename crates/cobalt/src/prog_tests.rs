//! 30 hand-written Logos programs testing every language construct against the oracle.
#![allow(clippy::unwrap_used)]

use crate::tests::assert_diff_all;

const HANDWRITTEN_30: &[&str] = &[
    // 1. Mutual recursion (even / odd)
    r#"
fn print(s: str) uses io = extern "io.print"
fn is_even(n: int) -> bool { if n == 0 { true } else { is_odd(n - 1) } }
fn is_odd(n: int) -> bool { if n == 0 { false } else { is_even(n - 1) } }
fn main() uses io {
    if is_even(20) && is_odd(21) && !is_odd(18) { print("ok1") }
}
"#,
    // 2. Fibonacci recursion + memo array
    r#"
fn print(s: str) uses io = extern "io.print"
fn fib(n: int) -> int {
    var memo = [0; 25]
    memo[1] = 1
    for i in 2..n + 1 { memo[i] = memo[i - 1] + memo[i - 2] }
    memo[n]
}
fn main() uses io { print(str(fib(20))) }
"#,
    // 3. Ackermann small values
    r#"
fn print(s: str) uses io = extern "io.print"
fn ack(m: int, n: int) -> int {
    if m == 0 { n + 1 }
    else if n == 0 { ack(m - 1, 1) }
    else { ack(m - 1, ack(m, n - 1)) }
}
fn main() uses io { print(str(ack(3, 3))) }
"#,
    // 4. Shared record mutation through multiple aliases
    r#"
fn print(s: str) uses io = extern "io.print"
type Boxed = { a: int, b: str }
fn mutate(x: Boxed, y: Boxed) {
    x.a = x.a + 10
    y.b = y.b + "!"
}
fn main() uses io {
    let r = Boxed { a: 5, b: "hi" }
    mutate(r, r)
    print(str(r.a) + ":" + r.b)
}
"#,
    // 5. Nested records shared and mutated
    r#"
fn print(s: str) uses io = extern "io.print"
type Inner = { v: int }
type Outer = { left: Inner, right: Inner }
fn main() uses io {
    let shared = Inner { v: 3 }
    let o = Outer { left: shared, right: shared }
    o.left.v = 42
    print(str(o.right.v))
}
"#,
    // 6. Array of records mutated in loop
    r#"
fn print(s: str) uses io = extern "io.print"
type Cell = { n: int }
fn main() uses io {
    let c = Cell { n: 1 }
    var arr = [c, c, c]
    for item in arr { item.n = item.n * 2 }
    print(str(c.n))
}
"#,
    // 7. Nested variants (expression evaluator)
    r#"
fn print(s: str) uses io = extern "io.print"
type Expr = Lit(int) | Add(Expr, Expr) | Mul(Expr, Expr) | Neg(Expr)
fn eval(e: Expr) -> int {
    match e {
        Lit(v) => v,
        Add(a, b) => eval(a) + eval(b),
        Mul(a, b) => eval(a) * eval(b),
        Neg(x) => -eval(x),
    }
}
fn main() uses io {
    let e = Mul(Add(Lit(2), Lit(3)), Neg(Lit(-4)))
    print(str(eval(e)))
}
"#,
    // 8. Variant with multiple fields and nested tuple pattern
    r#"
fn print(s: str) uses io = extern "io.print"
type Msg = Move(int, int) | Text(str) | Pair((int, bool)) | Quit
fn describe(m: Msg) -> str {
    match m {
        Move(0, 0) => "origin",
        Move(x, y) => str(x) + "," + str(y),
        Text(s) => s,
        Pair((n, true)) => "yes:" + str(n),
        Pair((n, false)) => "no:" + str(n),
        Quit => "quit",
    }
}
fn main() uses io {
    let ms = [Move(0, 0), Move(3, 4), Pair((7, true)), Pair((8, false)), Quit]
    for m in ms { print(describe(m) + ";") }
}
"#,
    // 9. Options and nested options (`int??`)
    r#"
fn print(s: str) uses io = extern "io.print"
fn unwrap_or(x: int??, def: int) -> int {
    match x {
        some(some(v)) => v,
        some(none) => -1,
        none => def,
    }
}
fn main() uses io {
    let a: int?? = some(some(10))
    let b: int?? = some(none)
    let c: int?? = none
    print(str(unwrap_or(a, 0)) + "," + str(unwrap_or(b, 0)) + "," + str(unwrap_or(c, 99)))
}
"#,
    // 10. Option equality and tuple equality
    r#"
fn print(s: str) uses io = extern "io.print"
fn main() uses io {
    let a: (int?, str) = (some(5), "x")
    let b: (int?, str) = (some(5), "x")
    let c: (int?, str) = (none, "x")
    assert a == b && a != c
    print("eq_ok")
}
"#,
    // 11. Array slicing and independence of copied slice
    r#"
fn print(s: str) uses io = extern "io.print"
fn main() uses io {
    var xs = [10, 20, 30, 40, 50]
    var s1 = xs[1..4]
    var s2 = s1[1..3]
    s1[0] = 99
    s2[1] = 88
    print(str(xs[1]) + "," + str(s1[0]) + "," + str(s1[2]) + "," + str(s2[1]))
}
"#,
    // 12. UTF-8 string slicing and concatenation
    r#"
fn print(s: str) uses io = extern "io.print"
fn main() uses io {
    let s = "αβγδε"
    let mid = s[2..8]
    print(mid + ":" + str(len(mid)))
}
"#,
    // 13. String building in loop with int conversion
    r#"
fn print(s: str) uses io = extern "io.print"
fn main() uses io {
    var out = ""
    for i in 1..6 {
        if len(out) > 0 { out = out + "-" }
        out = out + str(i * i)
    }
    print(out)
}
"#,
    // 14. Pipe chains with multi-argument functions
    r#"
fn print(s: str) uses io = extern "io.print"
fn clamp(x: int, lo: int, hi: int) -> int { max(lo, min(x, hi)) }
fn scale(x: int, factor: int, offset: int) -> int { x * factor + offset }
fn main() uses io {
    50 |> clamp(0, 10) |> scale(3, -5) |> str() |> print()
}
"#,
    // 15. First-class function values in arrays and tuples
    r#"
fn print(s: str) uses io = extern "io.print"
fn double(x: int) -> int { x * 2 }
fn square(x: int) -> int { x * x }
fn negate(x: int) -> int { -x }
fn main() uses io {
    let ops = [double, square, negate]
    var v = 3
    for f in ops { v = f(v) }
    print(str(v))
}
"#,
    // 16. Higher-order fold / reduce
    r#"
fn print(s: str) uses io = extern "io.print"
fn fold(xs: [int], init: int, f: fn(int, int) -> int) -> int {
    var acc = init
    for x in xs { acc = f(acc, x) }
    acc
}
fn add(a: int, b: int) -> int { a + b }
fn mul(a: int, b: int) -> int { a * b }
fn main() uses io {
    let xs = [1, 2, 3, 4, 5]
    print(str(fold(xs, 0, add)) + "," + str(fold(xs, 1, mul)))
}
"#,
    // 17. Nested loops with break and continue
    r#"
fn print(s: str) uses io = extern "io.print"
fn main() uses io {
    var total = 0
    for i in 0..6 {
        if i == 2 { continue }
        var j = 0
        while j < 6 {
            j = j + 1
            if j % 2 == 0 { continue }
            if i + j > 6 { break }
            total = total + i * 10 + j
        }
    }
    print(str(total))
}
"#,
    // 18. Byte array operations: push, pop, slice, concat, text
    r#"
fn print(s: str) uses io = extern "io.print"
fn main() uses io {
    var b = bytes("hello")
    push(b, b'!')
    let _ = pop(b)
    let b2 = b[1..4] + [b'X', b'Y']
    match text(b2) {
        some(s) => print(s),
        none => print("bad"),
    }
}
"#,
    // 19. Byte arithmetic and bitwise / comparison ops
    r#"
fn print(s: str) uses io = extern "io.print"
fn main() uses io {
    let a = byte(15)
    let b = byte(4)
    let sum = a + b
    let diff = a - b
    let prod = a * b
    let quot = a / b
    let rem = a % b
    assert sum == byte(19) && diff == byte(11) && prod == byte(60)
    assert quot == byte(3) && rem == byte(3)
    print("byte_ok")
}
"#,
    // 20. Bitwise and wrapping integer arithmetic
    r#"
fn print(s: str) uses io = extern "io.print"
fn main() uses io {
    let a = 0xff00ff00
    let b = 0x0f0f0f0f
    let x = (a & b) | (a ^ b)
    let w = (-9223372036854775807 - 1) -% 1
    let m = 3037000500 *% 3037000500
    print(str(x) + "," + str(w) + "," + str(m))
}
"#,
    // 21. Parseint edge cases
    r#"
fn print(s: str) uses io = extern "io.print"
fn check_p(s: str) -> int {
    match parse_int(s) {
        some(n) => n,
        none => -999,
    }
}
fn main() uses io {
    print(str(check_p("0")) + "," + str(check_p("-9223372036854775808")) + "," + str(check_p("9223372036854775808")) + "," + str(check_p("")))
}
"#,
    // 22. Min, max, abs when dst aliases input register
    r#"
fn print(s: str) uses io = extern "io.print"
fn main() uses io {
    var x = -42
    x = abs(x)
    var y = 10
    y = min(y, 20)
    y = min(30, y)
    y = max(y, 5)
    y = max(25, y)
    print(str(x) + "," + str(y))
}
"#,
    // 23. Short-circuit side-effect ordering
    r#"
fn print(s: str) uses io = extern "io.print"
fn side(tag: str, v: bool) -> bool uses io {
    print(tag)
    v
}
fn main() uses io {
    let _ = side("1", false) && side("2", true)
    let _ = side("3", true) && side("4", false)
    let _ = side("5", true) || side("6", false)
    let _ = side("7", false) || side("8", true)
}
"#,
    // 24. Match on strings and bytes
    r#"
fn print(s: str) uses io = extern "io.print"
fn tok(s: str, b: byte) -> int {
    let a = match s {
        "add" => 10,
        "sub" => 20,
        _ => 0,
    }
    let c = match b {
        b'+' => 1,
        b'-' => 2,
        _ => 3,
    }
    a + c
}
fn main() uses io {
    print(str(tok("add", b'+')) + "," + str(tok("sub", b'x')) + "," + str(tok("nop", b'-')))
}
"#,
    // 25. Dynamic array growth during for-loop iteration
    r#"
fn print(s: str) uses io = extern "io.print"
fn main() uses io {
    var xs = [1, 2]
    var out = 0
    for x in xs {
        out = out * 10 + x
        if len(xs) < 5 { push(xs, x + 2) }
    }
    print(str(out) + ":" + str(len(xs)))
}
"#,
    // 26. Multi-parameter calls (> 6 args) staging registers
    r#"
fn print(s: str) uses io = extern "io.print"
fn sum8(a: int, b: int, c: int, d: int, e: int, f: int, g: int, h: int) -> int {
    a + b * 2 + c * 3 + d * 4 + e * 5 + f * 6 + g * 7 + h * 8
}
fn main() uses io {
    print(str(sum8(1, 2, 3, 4, 5, 6, 7, 8)))
}
"#,
    // 27. Extern function passed as first-class value
    r#"
fn print(s: str) uses io = extern "io.print"
fn invoke_twice(f: fn(str) -> unit uses io, msg: str) uses io {
    f(msg)
    f("!")
}
fn main() uses io {
    invoke_twice(print, "hello")
}
"#,
    // 28. Binary search in array
    r#"
fn print(s: str) uses io = extern "io.print"
fn bsearch(xs: [int], target: int) -> int? {
    var lo = 0
    var hi = len(xs)
    while lo < hi {
        let mid = (lo + hi) / 2
        if xs[mid] == target { return some(mid) }
        if xs[mid] < target { lo = mid + 1 } else { hi = mid }
    }
    none
}
fn main() uses io {
    let xs = [2, 5, 8, 12, 16, 23, 38, 56, 72, 91]
    match (bsearch(xs, 23), bsearch(xs, 15)) {
        (some(i), none) => print("found:" + str(i)),
        _ => print("bad"),
    }
}
"#,
    // 29. Property test over user-defined variant and record types
    r#"
type PairRec = { a: int, b: int }
type Choice = One(int) | Two(int, int) | Zero
fn eval_choice(c: Choice) -> int {
    match c {
        One(x) => x,
        Two(x, y) => x +% y,
        Zero => 0,
    }
}
test "choice basic" {
    assert eval_choice(Two(3, 4)) == 7
}
prop "record mutation" (p: PairRec, c: Choice) {
    let old = p.a
    p.a = old +% 1
    assert p.a == old +% 1
    let _ = eval_choice(c)
}
"#,
    // 30. Quicksort in-place on shared array
    r#"
fn print(s: str) uses io = extern "io.print"
fn swap(xs: [int], i: int, j: int) {
    let tmp = xs[i]
    xs[i] = xs[j]
    xs[j] = tmp
}
fn qsort(xs: [int], lo: int, hi: int) {
    if lo >= hi { return }
    let pivot = xs[hi]
    var i = lo
    for j in lo..hi {
        if xs[j] <= pivot {
            swap(xs, i, j)
            i = i + 1
        }
    }
    swap(xs, i, hi)
    qsort(xs, lo, i - 1)
    qsort(xs, i + 1, hi)
}
fn main() uses io {
    var xs = [9, -3, 5, 0, 12, 5, 1, -8, 4]
    qsort(xs, 0, len(xs) - 1)
    for x in xs { print(str(x) + ",") }
}
"#,
];

macro_rules! prog_test {
    ($($name:ident: $idx:expr),* $(,)?) => {
        $(
            #[test]
            fn $name() {
                assert_diff_all(HANDWRITTEN_30[$idx], &[""]);
            }
        )*
    };
}

prog_test! {
    prog_01: 0, prog_02: 1, prog_03: 2, prog_04: 3, prog_05: 4,
    prog_06: 5, prog_07: 6, prog_08: 7, prog_09: 8, prog_10: 9,
    prog_11: 10, prog_12: 11, prog_13: 12, prog_14: 13, prog_15: 14,
    prog_16: 15, prog_17: 16, prog_18: 17, prog_19: 18, prog_20: 19,
    prog_21: 20, prog_22: 21, prog_23: 22, prog_24: 23, prog_25: 24,
    prog_26: 25, prog_27: 26, prog_28: 27, prog_29: 28, prog_30: 29,
}

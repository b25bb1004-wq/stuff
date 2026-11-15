//! Compiles a small Logos program and runs its `main`, unit test and property test.
use cobalt::{compile, opt, run_main, run_prop, run_test};
use logos::check::check;
use logos::parser::parse;

const SOURCE: &str = r#"
type Shape = Circle(int) | Rect(int, int)

fn print(s: str) uses io = extern "io.print"

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
    [Circle(2), Rect(3, 4)] |> total() |> str() |> print()
}

test "a rectangle" { assert area(Rect(3, 4)) == 12 }

prop "area is never negative" (w: int, h: int) where w >= 0 && w < 1000 && h >= 0 && h < 1000 {
    assert area(Rect(w, h)) >= 0
}
"#;

const FUEL: u64 = 1_000_000;
const SEED: u64 = 1;
const CASES: u32 = 100;

fn main() {
    println!("1. parse and check {} bytes of Logos source", SOURCE.len());
    let ast = match parse(SOURCE) {
        Ok(m) => m,
        Err(d) => {
            println!("parse error: {}", d.message);
            std::process::exit(1);
        }
    };
    let mut prog = match check(&ast) {
        Ok(p) => p,
        Err(d) => {
            println!("check error: {}", d.message);
            std::process::exit(1);
        }
    };
    println!(
        "   {} unit test(s), {} property test(s)",
        prog.tests.len(),
        prog.props.len()
    );

    let stats = opt::optimize(&mut prog);
    println!(
        "2. optimise: {} folded, {} copies, {} removed",
        stats.folded, stats.copies, stats.removed
    );

    let c = compile(&prog);
    println!(
        "3. compile: {} Elysium functions, main = {}",
        c.module.funcs.len(),
        if c.main.is_some() {
            "present"
        } else {
            "absent"
        }
    );

    let run = run_main(&c, "", FUEL);
    print!("4. run main prints: {}", run.output);
    println!();
    println!("   main finished without a trap: {}", run.result.is_ok());

    for (i, (name, _)) in c.tests.iter().enumerate() {
        let out = run_test(&c, i, FUEL);
        println!(
            "5. unit test {:?}: {}",
            name,
            if out.result.is_ok() { "pass" } else { "fail" }
        );
    }

    for (i, p) in c.props.iter().enumerate() {
        let out = run_prop(&c, i, SEED, CASES, FUEL);
        println!(
            "6. property {:?} over {} seeded cases (seed {}): {}",
            p.name,
            CASES,
            SEED,
            if out.result.is_ok() { "pass" } else { "fail" }
        );
    }
}

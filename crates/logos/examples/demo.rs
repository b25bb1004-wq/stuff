//! Parse, print, check and run a small Logos program, then show a diagnostic for a broken one.
use logos::{check::check, eval, line_col, parser::parse, print::print};

const SOURCE: &str = r#"type Shape = Circle(int) | Rect(int, int)

fn print(s: str) uses io = extern "io.print"

fn area(s: Shape) -> int {
    match s {
        Circle(r) => 3 * r * r,
        Rect(w, h) => w * h,
    }
}

fn main() uses io {
    print(str(area(Rect(3, 4))))
}

test "rectangle" { assert area(Rect(3, 4)) == 12 }
test "circle" { assert area(Circle(2)) == 13, "a circle of radius 2" }
prop "area is never negative" (w: int, h: int) where w >= 0 && w < 1000 && h >= 0 && h < 1000 {
    assert area(Rect(w, h)) >= 0
}
"#;

const BROKEN: &str = "fn main() uses io {\n    print(missing)\n}\n";

fn main() {
    let module = match parse(SOURCE) {
        Ok(m) => m,
        Err(d) => {
            println!("parse error: {}", d.message);
            return;
        }
    };
    let canonical = print(&module);
    println!(
        "1. parsed {} items, canonical text has {} lines",
        module.items.len(),
        canonical.lines().count()
    );

    let program = match check(&module) {
        Ok(p) => p,
        Err(d) => {
            println!("check error: {}", d.message);
            return;
        }
    };
    println!(
        "2. checked: {} functions, {} tests, {} properties",
        program.funcs.len(),
        program.tests.len(),
        program.props.len()
    );

    let main_run = eval::run_main(&program, "", 1_000_000);
    println!(
        "3. main output {:?}, ok = {}",
        main_run.output,
        main_run.result.is_ok()
    );

    for (i, (name, _)) in program.tests.iter().enumerate() {
        let out = eval::run_test(&program, i, 1_000_000);
        match out.result {
            Ok(()) => println!("4. test \"{name}\": pass"),
            Err((trap, span)) => println!(
                "4. test \"{name}\": {trap:?} at bytes {}..{}",
                span.start, span.end
            ),
        }
    }

    let prop_out = eval::run_prop(&program, 0, 42, 100, 1_000_000);
    println!(
        "5. property with seed 42, 100 cases: ok = {}",
        prop_out.result.is_ok()
    );

    match parse(BROKEN).and_then(|m| check(&m)) {
        Ok(_) => println!("6. broken program was accepted"),
        Err(d) => {
            let (line, col) = line_col(BROKEN, d.span.start);
            println!(
                "6. broken program rejected at line {line}, column {col}: {}",
                d.message
            );
        }
    }
}

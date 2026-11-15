//! Host tests for the Athena debugger session: start, breakpoints, each step mode, variables, frames,
//! budgets, traps, compile errors, the line protocol and the global session used by the wasm exports.
#![allow(clippy::unwrap_used)]

use athena::debug::{
    encode, frame_text, parse_breakpoints, start_text, step_text, stop, Debugger, State, Status,
};
use cobalt::Step;

/// `add` is on line 1; `main` calls it on line 4.
const CALL_SRC: &str =
    "fn add(a: int, b: int) -> int { a + b }\nfn main() -> int {\n var x = 1\n add(2, 3)\n}\n";
const ADD_LINE: u32 = 1;
const CALL_LINE: u32 = 4;
/// Line 1 is the body of `total`, which takes an array and a string.
const ARGS_SRC: &str =
    "fn total(xs: [int], s: str) -> int { 0 }\nfn main() -> int {\n total([1, 2, 3], \"hi\")\n}\n";
const ENDLESS_SRC: &str = "fn main() -> int {\n var i = 0\n while true {\n i = i + 1\n }\n i\n}\n";

fn start(src: &str) -> Debugger {
    Debugger::start(src).unwrap_or_else(|e| panic!("start failed: {e}\n{src}"))
}

fn names(st: &State) -> Vec<&str> {
    st.frames.iter().map(|f| f.name.as_str()).collect()
}

#[test]
fn start_pauses_before_the_first_line_of_main() {
    let d = start(CALL_SRC);
    let st = d.state();
    assert_eq!(st.status, Status::Paused);
    assert_eq!(names(&st), ["main"]);
    assert_eq!(st.line, 3, "first op of main is `var x = 1` on line 3");
    assert!(st.col >= 1);
    assert!(st.vars.is_empty());
    assert!(st.output.is_empty());
}

#[test]
fn breakpoint_in_callee_stops_there_with_parameters() {
    let mut d = start(CALL_SRC);
    let st = d.step(Step::Continue, &[ADD_LINE]);
    assert_eq!(st.status, Status::Paused);
    assert_eq!(names(&st), ["add", "main"]);
    assert_eq!(st.line, ADD_LINE);
    assert_eq!(
        st.frames[1].line, CALL_LINE,
        "the caller shows the line of its call"
    );
    let vars: Vec<(&str, &str)> = st
        .vars
        .iter()
        .map(|v| (v.name.as_str(), v.value.as_str()))
        .collect();
    assert_eq!(vars, [("a", "2"), ("b", "3")]);
}

#[test]
fn over_does_not_enter_the_call() {
    let mut d = start(CALL_SRC);
    let st = d.step(Step::Over, &[]);
    assert_eq!(st.line, CALL_LINE);
    assert_eq!(names(&st), ["main"]);
    // Line 4 is main's last statement, so Over runs the call and finishes main.
    let st = d.step(Step::Over, &[]);
    assert!(
        !names(&st).contains(&"add"),
        "over must not stop inside add: {st:?}"
    );
    assert_eq!(st.status, Status::Done);
}

#[test]
fn over_across_budget_pauses_keeps_its_anchor() {
    // `work` runs about 3000 ops, far more than the 50-op budget, so Over must survive many pauses.
    let src = "fn work(n: int) -> int {\n var i = 0\n while i < n { i = i + 1 }\n i\n}\nfn main() -> int {\n var x = work(300)\n x\n}\n";
    let whole = start(src).step_budget(Step::Over, &[], 1 << 40);
    let mut d = start(src);
    let mut budgets = 0;
    let mut st = d.step_budget(Step::Over, &[], 50);
    while st.status == Status::Budget && budgets < 10_000 {
        budgets += 1;
        st = d.step_budget(Step::Over, &[], 50);
    }
    assert!(
        budgets > 10,
        "the step should have paused on budget many times"
    );
    assert_eq!(st.status, whole.status);
    assert_eq!(
        names(&st),
        ["main"],
        "over must not stop inside work: {st:?}"
    );
    assert_eq!(st.line, whole.line, "same stop as an unbudgeted step");
}

#[test]
fn into_enters_the_call() {
    let mut d = start(CALL_SRC);
    let st = d.step(Step::Into, &[]);
    assert_eq!(st.line, CALL_LINE);
    let st = d.step(Step::Into, &[]);
    assert_eq!(names(&st), ["add", "main"]);
    assert_eq!(st.line, ADD_LINE);
}

#[test]
fn out_returns_to_the_caller() {
    let mut d = start(CALL_SRC);
    let st = d.step(Step::Continue, &[ADD_LINE]);
    assert_eq!(names(&st), ["add", "main"]);
    let st = d.step(Step::Out, &[]);
    assert_eq!(names(&st), ["main"], "out must leave add: {st:?}");
}

#[test]
fn selecting_a_frame_shows_its_variables() {
    let mut d = start(CALL_SRC);
    d.step(Step::Continue, &[ADD_LINE]);
    let caller = d.select(1).unwrap();
    assert_eq!(caller.selected, 1);
    assert_eq!(names(&caller), ["add", "main"]);
    let vars: Vec<(&str, &str)> = caller
        .vars
        .iter()
        .map(|v| (v.name.as_str(), v.value.as_str()))
        .collect();
    assert_eq!(vars, [("x", "1")], "main's `var x` is in scope at the call");
    assert!(d.select(9).is_err());
}

#[test]
fn values_print_as_logos_source() {
    let mut d = start(ARGS_SRC);
    let st = d.step(Step::Continue, &[1]);
    assert_eq!(names(&st), ["total", "main"]);
    let vars: Vec<(&str, &str)> = st
        .vars
        .iter()
        .map(|v| (v.name.as_str(), v.value.as_str()))
        .collect();
    assert_eq!(vars, [("xs", "[1, 2, 3]"), ("s", "\"hi\"")]);
}

#[test]
fn endless_loop_returns_budget_states_without_hanging() {
    let mut d = start(ENDLESS_SRC);
    for _ in 0..3 {
        let st = d.step(Step::Continue, &[]);
        assert_eq!(st.status, Status::Budget);
        assert_eq!(names(&st), ["main"]);
    }
}

#[test]
fn compile_error_is_reported_with_position() {
    let err = Debugger::start("fn main() -> int { 1 + }\n").err().unwrap();
    assert!(err.starts_with("1:"), "got {err}");
}

#[test]
fn program_output_is_captured_as_it_runs() {
    let src = "fn say(s: str) uses io = extern \"io.print\"\nfn main() uses io {\n say(\"hi\")\n say(\"there\")\n}\n";
    let mut d = start(src);
    let st = d.step(Step::Continue, &[]);
    assert_eq!(st.status, Status::Done);
    assert!(
        st.output.contains("hi") && st.output.contains("there"),
        "output {:?}",
        st.output
    );
    assert!(st.frames.is_empty());
}

#[test]
fn trap_is_reported_as_trapped() {
    let src = "fn zero() -> int { 0 }\nfn main() -> int {\n 1 / zero()\n}\n";
    let mut d = start(src);
    let st = d.step(Step::Continue, &[]);
    assert_eq!(st.status, Status::Trapped);
    assert!(
        st.message.contains("DivideByZero"),
        "message {:?}",
        st.message
    );
    let again = d.step(Step::Continue, &[]);
    assert_eq!(
        again.status,
        Status::Trapped,
        "a finished run stays finished"
    );
}

#[test]
fn breakpoints_parse_digit_runs() {
    assert_eq!(parse_breakpoints("3, 5\n7 x 0 12"), [3, 5, 7, 12]);
    assert!(parse_breakpoints("").is_empty());
}

#[test]
fn protocol_lines_are_escaped_and_ordered() {
    let st = State {
        status: Status::Paused,
        line: 2,
        col: 5,
        selected: 0,
        message: String::new(),
        output: String::from("a\nb\\c"),
        frames: Vec::new(),
        vars: Vec::new(),
    };
    let text = encode(&st);
    assert!(
        text.starts_with("status paused\nline 2\ncol 5\nselected 0\nmessage \noutput a\\nb\\\\c\n"),
        "{text}"
    );
}

#[test]
fn global_session_round_trip() {
    let text = start_text(CALL_SRC);
    assert!(text.starts_with("status paused\n"), "{text}");
    let text = step_text(0, &[ADD_LINE]);
    assert!(text.contains("frame 0 add 1\n"), "{text}");
    assert!(text.contains("var a 2\nvar b 3\n"), "{text}");
    let text = frame_text(1);
    assert!(
        text.contains("frame 1 main 4\n") && text.contains("selected 1\n"),
        "{text}"
    );
    assert!(step_text(9, &[]).starts_with("status error\n"));
    stop();
    assert!(step_text(0, &[]).contains("no debug session"));
    assert!(start_text("fn main( {").starts_with("status error\n"));
}

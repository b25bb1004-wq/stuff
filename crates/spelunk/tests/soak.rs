//! Seeded soak of every HTML, CSS, selector, cascade and layout entry point.
//! Env: STRATA_SOAK_MS per target (default 300), STRATA_SOAK_SEED (hex 0x.. or decimal).
#![allow(clippy::unwrap_used)]

use assay::Rng;
use spelunk::{css, dom::Document, html, layout, select, style};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::time::{Duration, Instant};

/// Largest input in bytes. Keeps every case far below the 64 MiB allocation cap.
const MAX_INPUT: usize = 64 * 1024;
/// Deepest nesting generated; 5000 `<div>` (plus closers) stays under `MAX_INPUT`.
const MAX_DEPTH: usize = 5_000;
/// Wall-clock limit for one case.
const CASE_LIMIT: Duration = Duration::from_secs(1);

const HTML_SEEDS: &[&str] = &[
    r#"<!DOCTYPE html><html><head><title>t</title><style>p{color:red}</style></head><body><div id="a" class="x y"><p>Hi &amp; bye &#x41; &notin;</p><span>s</span><!-- c --><img src=a.png alt=""><br><table><tr><td>1</td></tr></table></div></body></html>"#,
    r#"<div><b><i>x</b></i><p>a<p>b<ul><li>1<li>2</ul><select><option>o</select><template><b>t</b></template></div>"#,
    r#"<svg><circle r="3"/></svg><math><mi>x</mi></math><script>if (a<b) {}</script><textarea>&lt;</textarea>"#,
];
const CSS_SEEDS: &[&str] = &[
    "div.a > p:first-child, #b { color: rgb(1, 2, 3); margin: 4px 1em !important }",
    "@media (min-width: 10px) { a[href^=\"x\"] { display: flex; flex-grow: 2 } } /* c */",
    "@import url(x.css); h1 + p ~ span:not(.q) { width: calc(100% - 3px); background: #fff }",
];
const SEL_SEEDS: &[&str] = &[
    "div.a > p:first-child, #b",
    "a[href^=\"x\"] + span:not(.b)",
    "li:nth-child(2n+1) ~ li",
];
const SOUP: &[&str] = &[
    "<",
    ">",
    "</",
    "/>",
    "<!--",
    "-->",
    "<!DOCTYPE html>",
    "<html>",
    "<body>",
    "<div class=\"a b\">",
    "<p>",
    "<table>",
    "<td>",
    "<select>",
    "<template>",
    "<script>",
    "<style>",
    "<svg>",
    "<math>",
    "</p>",
    "&amp;",
    "&#x41;",
    "&#0;",
    "&notin",
    "&",
    ";",
    "=",
    "\"",
    "'",
    " ",
    "\n",
    "\0",
    "\u{FFFD}",
    "\u{e9}",
    "\u{1F600}",
    "{",
    "}",
    "(",
    ")",
    "[",
    "]",
    ":",
    "@media ",
    "@import ",
    "url(",
    "\\",
    "/*",
    "*/",
    "#",
    ".",
    "*",
    "+",
    "~",
    "|",
    "!important",
    "1e3",
    "-",
    "%",
    "rgb(",
    "::before",
    ":not(",
    "nth-child(2n+1)",
    " > ",
    "p",
    "div",
    "span",
    "a",
    "color:red;",
];

fn master_seed() -> u64 {
    match std::env::var("STRATA_SOAK_SEED") {
        Ok(v) => {
            let t = v.trim().replace('_', "");
            match t.strip_prefix("0x") {
                Some(h) => u64::from_str_radix(h, 16),
                None => t.parse(),
            }
            .unwrap_or_else(|_| panic!("STRATA_SOAK_SEED is not a u64: {v}"))
        }
        Err(_) => 0x05EE_D79F,
    }
}

fn budget_ms() -> u64 {
    match std::env::var("STRATA_SOAK_MS") {
        Ok(v) => v
            .trim()
            .parse()
            .unwrap_or_else(|_| panic!("STRATA_SOAK_MS is not a number: {v}")),
        Err(_) => 300,
    }
}

/// Run `case` on derived seeds until the budget is spent; each case must not panic and must
/// return within `CASE_LIMIT`. A failing case reports its own seed for replay.
fn run(name: &str, case: fn(&mut Rng)) {
    let seed = master_seed();
    let budget = Duration::from_millis(budget_ms());
    let mut master = Rng::new(seed);
    let start = Instant::now();
    let mut cases = 0u64;
    loop {
        let case_seed = master.u64();
        let t = Instant::now();
        let outcome = catch_unwind(AssertUnwindSafe(|| case(&mut Rng::new(case_seed))));
        let took = t.elapsed();
        if outcome.is_err() {
            panic!("soak {name}: case seed {case_seed:#018x} panicked");
        }
        assert!(
            took <= CASE_LIMIT,
            "soak {name}: case seed {case_seed:#018x} took {took:?}"
        );
        cases += 1;
        if start.elapsed() >= budget {
            break;
        }
    }
    println!(
        "soak {name}: seed={seed:#x} cases={cases} budget={}ms",
        budget.as_millis()
    );
}

fn lossy(b: &[u8]) -> String {
    String::from_utf8_lossy(&b[..b.len().min(MAX_INPUT)]).into_owned()
}

fn soup(rng: &mut Rng) -> String {
    let mut s = String::new();
    for _ in 0..rng.range(0, 400) {
        s.push_str(rng.pick(SOUP));
    }
    s
}

/// Mutation of a valid input: bit flip, truncation, splice from another seed, repeated chunk.
fn mutate(rng: &mut Rng, base: &str, others: &[&str]) -> String {
    let mut b = base.as_bytes().to_vec();
    for _ in 0..rng.range(1, 6) {
        match rng.below(5) {
            0 if !b.is_empty() => {
                let i = rng.below(b.len());
                b[i] ^= 1 << rng.below(8);
            }
            1 => {
                let n = rng.below(b.len() + 1);
                b.truncate(n);
            }
            2 => {
                let other = rng.pick(others).as_bytes();
                let from = rng.below(other.len() + 1);
                let to = from + rng.below(other.len() - from + 1);
                let at = rng.below(b.len() + 1);
                b.splice(at..at, other[from..to].iter().copied());
            }
            3 if !b.is_empty() => {
                let s = rng.below(b.len());
                let e = s + rng.below((b.len() - s).min(64) + 1);
                let chunk = b[s..e].to_vec();
                for _ in 0..rng.range(1, 50) {
                    if b.len() + chunk.len() > MAX_INPUT {
                        break;
                    }
                    b.splice(e..e, chunk.iter().copied());
                }
            }
            _ => {
                let i = rng.below(b.len() + 1);
                b.insert(i, rng.u64() as u8);
            }
        }
    }
    lossy(&b)
}

/// Half random bytes or token soup, half mutations of the valid seeds.
fn input(rng: &mut Rng, seeds: &[&str]) -> String {
    match rng.below(4) {
        0 => lossy(&rng.bytes(16 * 1024)),
        1 => soup(rng),
        _ => {
            let base = *rng.pick(seeds);
            mutate(rng, base, seeds)
        }
    }
}

fn deep(rng: &mut Rng, open: &str, close: &str) -> String {
    let d = rng.range(100, MAX_DEPTH as u64) as usize;
    let mut s = open.repeat(d);
    if rng.chance(1, 2) {
        s.push_str(&close.repeat(d));
    }
    s
}

fn html_input(rng: &mut Rng) -> String {
    if rng.chance(1, 8) {
        return deep(rng, "<div>", "</div>");
    }
    if rng.chance(1, 4) {
        // A dump the crate produces is also a valid-looking input to mutate.
        let dump = html::parse(rng.pick(HTML_SEEDS)).dump();
        return mutate(rng, &dump, HTML_SEEDS);
    }
    input(rng, HTML_SEEDS)
}

/// Well-formed nesting of div/span/b/i. Returns (div count, element count) it generated.
fn nested(rng: &mut Rng, depth: usize, out: &mut String) -> (usize, usize) {
    let (mut divs, mut all) = (0, 0);
    for _ in 0..rng.range(1, 4) {
        let tag = *rng.pick(&["div", "span", "b", "i"]);
        out.push_str(&format!("<{tag}>"));
        all += 1;
        divs += usize::from(tag == "div");
        if depth < 5 && rng.chance(1, 2) {
            let (d, a) = nested(rng, depth + 1, out);
            divs += d;
            all += a;
        } else {
            out.push('t');
        }
        out.push_str(&format!("</{tag}>"));
    }
    (divs, all)
}

fn check_arena(doc: &Document) {
    for (id, node) in doc.nodes.iter().enumerate() {
        for &c in &node.children {
            assert_eq!(doc.nodes.get(c).and_then(|n| n.parent), Some(id));
        }
    }
    for id in doc.elements() {
        assert!(doc.name(id).is_some());
    }
}

fn html_tokenizer_case(rng: &mut Rng) {
    let _ = html::tokenize(&html_input(rng));
}

fn html_tree_case(rng: &mut Rng) {
    if rng.chance(1, 4) {
        // Round trip: generated well-formed markup must come back with the same elements.
        let mut s = String::new();
        let (divs, all) = nested(rng, 0, &mut s);
        let doc = html::parse(&s);
        assert_eq!(doc.elements().len(), all + 3); // html, head, body
        assert_eq!(select::select(&doc, "div").map_or(0, |v| v.len()), divs);
        return;
    }
    let doc = html::parse(&html_input(rng));
    check_arena(&doc);
    let _ = doc.dump();
}

fn css_parser_case(rng: &mut Rng) {
    if rng.chance(1, 4) {
        // Round trip: generated rules and declarations come back with the same count.
        let n = rng.range(1, 40) as usize;
        let mut sheet = String::new();
        for i in 0..n {
            sheet.push_str(&format!(".c{i} {{ color: red; margin: {i}px }}\n"));
        }
        assert_eq!(css::parse_stylesheet(&sheet).len(), n);
        let d = rng.range(1, 20) as usize;
        let decls: Vec<String> = (0..d).map(|i| format!("margin: {i}px")).collect();
        assert_eq!(css::parse_declarations(&decls.join(";")).len(), d);
        return;
    }
    let s = if rng.chance(1, 8) {
        deep(rng, "a{", "}")
    } else {
        input(rng, CSS_SEEDS)
    };
    let _ = css::tokenize(&s);
    let _ = css::parse_values(&s);
    let _ = css::parse_declarations(&s);
    let _ = css::parse_stylesheet(&s);
}

fn css_cascade_case(rng: &mut Rng) {
    let sheet = input(rng, CSS_SEEDS);
    let inline = input(rng, CSS_SEEDS).replace('"', "&quot;");
    let page = format!(
        "<html><head><style>{sheet}</style></head><body><div style=\"{inline}\" class=\"a\"><p id=\"b\">{}</p></div></body></html>",
        html_input(rng)
    );
    let doc = html::parse(&page);
    check_arena(&doc);
    let styles = style::compute(&doc);
    let _ = style::compute_flex(&doc);
    assert_eq!(styles.len(), doc.nodes.len());
    for (id, style) in styles.iter().enumerate() {
        assert_eq!(style.is_some(), doc.name(id).is_some());
    }
}

fn layout_case(rng: &mut Rng) {
    let doc = html::parse(&html_input(rng));
    let styles = style::compute(&doc);
    let viewport = match rng.below(8) {
        0 => i32::MAX,
        1 => 0,
        2 => -1,
        _ => rng.range(1, 4000) as i32,
    };
    let _ = layout::layout(&doc, &styles, viewport);
}

fn selectors_case(rng: &mut Rng) {
    if rng.chance(1, 4) {
        // Valid seeds must parse; they are the crate's own selector syntax.
        assert!(select::parse_selectors(rng.pick(SEL_SEEDS)).is_ok());
    }
    let s = input(rng, SEL_SEEDS);
    if let Ok(list) = select::parse_selectors(&s) {
        for sel in &list {
            let _ = sel.specificity();
        }
    }
    let doc = html::parse(HTML_SEEDS[0]);
    let _ = select::select(&doc, &s);
}

#[test]
fn soak_html_tokenizer() {
    run("html_tokenizer", html_tokenizer_case);
}

#[test]
fn soak_html_tree() {
    run("html_tree", html_tree_case);
}

#[test]
fn soak_css_parser() {
    run("css_parser", css_parser_case);
}

#[test]
fn soak_css_cascade() {
    run("css_cascade", css_cascade_case);
}

#[test]
fn soak_selectors() {
    run("selectors", selectors_case);
}

#[test]
fn soak_layout() {
    run("layout", layout_case);
}

// Regression for the CSS nesting overflow: 5000 open blocks used to exhaust the stack in clone/drop.
#[test]
fn regression_deep_css_blocks_keep_stack() {
    let deep = format!("a{}{}", "{".repeat(5000), "}".repeat(5000));
    let rules = css::parse_stylesheet(&deep);
    assert_eq!(rules.len(), 1);
    let nested = format!("{}x{}", "(".repeat(300), ")".repeat(300));
    assert_eq!(css::parse_values(&nested).len(), 1);
}

// Regression for the quadratic dump: indentation used to grow without limit with depth.
#[test]
fn regression_deep_dump_is_linear() {
    let depth = 5_000;
    let doc = html::parse(&"<div>".repeat(depth));
    let started = Instant::now();
    let dump = doc.dump();
    assert!(
        started.elapsed() <= CASE_LIMIT,
        "dump took {:?}",
        started.elapsed()
    );
    assert!(dump.len() < depth * 600, "dump is {} bytes", dump.len());
}

// Regression for the open-p counter: closing a p with raw stack pops left the counter stuck,
// so every later scope check walked the whole stack (quadratic in depth).
#[test]
fn regression_p_scope_after_close_is_linear() {
    let depth = 5_000;
    let page = format!("<p>{}", "<div>".repeat(depth));
    let started = Instant::now();
    let doc = html::parse(&page);
    assert!(
        started.elapsed() <= CASE_LIMIT,
        "parse took {:?}",
        started.elapsed()
    );
    assert_eq!(doc.elements().len(), depth + 4); // html, head, body, p, then the divs
}

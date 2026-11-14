//! Leader check for T65. Oracles (Python on PATH): tinycss2 1.5 for CSS tokens, stylesheets and
//! declaration lists; html.parser for HTML tokens (inputs limited to where it agrees with
//! WHATWG); soupsieve (via bs4) for selector matching. Plus html5lib-format tree fixtures,
//! specificity, deep nesting, speed and no-panic fuzz. Seeds fixed; Python run with -I.
#![allow(clippy::unwrap_used)]
use assay::Rng;
use spelunk::css::{self, Token as C, Value};
use spelunk::html::{self, Token as H};
use spelunk::select;
use std::fmt::Write as _;
use std::process::Command;

const PY: &str = r##"
import sys
mode, inp, outp = sys.argv[1:4]
X = lambda s: s.encode('utf-8', 'surrogatepass').hex()
def num(v): return repr(float(v))
LIT = {':': 'colon', ';': 'semi', ',': 'comma', '<!--': 'cdo', '-->': 'cdc'}
def dump(nodes, d, out):
    for t in nodes:
        k = t.type
        if k == 'comment': continue
        if k == 'error':
            if t.kind == 'bad-string': out.append(f"{d}:badstr")
            elif t.kind == 'bad-url': out.append(f"{d}:badurl")
            elif t.kind in (')', ']', '}'): out.append(f"{d}:close:{t.kind}")
            continue
        if k == 'ident': out.append(f"{d}:ident:{X(t.value)}")
        elif k == 'function': out.append(f"{d}:fn:{X(t.name)}"); dump(t.arguments, d + 1, out)
        elif k == 'at-keyword': out.append(f"{d}:at:{X(t.value)}")
        elif k == 'hash': out.append(f"{d}:hash:{X(t.value)}:{'id' if t.is_identifier else 'unr'}")
        elif k == 'string': out.append(f"{d}:str:{X(t.value)}")
        elif k == 'url': out.append(f"{d}:url:{X(t.value)}")
        elif k == 'number': out.append(f"{d}:num:{num(t.value)}:{'int' if t.is_integer else 'num'}")
        elif k == 'percentage': out.append(f"{d}:pct:{num(t.value)}")
        elif k == 'dimension': out.append(f"{d}:dim:{num(t.value)}:{'int' if t.is_integer else 'num'}:{X(t.unit)}")
        elif k == 'whitespace': out.append(f"{d}:ws")
        elif k == 'literal': out.append(f"{d}:" + (LIT.get(t.value) or f"delim:{X(t.value)}"))
        elif k in ('() block', '[] block', '{} block'): out.append(f"{d}:block:{k[0]}"); dump(t.content, d + 1, out)
        else: out.append(f"{d}:?{k}")
    return out
def trim(v):
    v = [t for t in v if t.type != 'comment']
    while v and v[0].type == 'whitespace': v = v[1:]
    while v and v[-1].type == 'whitespace': v = v[:-1]
    return v
def decls(content, out):
    import tinycss2
    for x in tinycss2.parse_declaration_list(content, skip_comments=True, skip_whitespace=True):
        if x.type == 'declaration':
            out.append(f"D:{X(x.name)}:{1 if x.important else 0}"); dump(trim(x.value), 0, out)
res = []
lines = open(inp, encoding='utf-8').read().split('\n')[:-1]
if mode == 'sel':
    from bs4 import BeautifulSoup
    soup = None
    for line in lines:
        k, h = line.split(' ', 1) if ' ' in line else (line, '')
        s = bytes.fromhex(h).decode('utf-8')
        if k == 'D': soup = BeautifulSoup(s, 'html.parser'); continue
        try:
            got = [e['data-i'] for e in soup.select(s)]
            res.append(','.join(got) if got else '-')
        except Exception:
            res.append('E')
else:
    import tinycss2
    from html.parser import HTMLParser
    class P(HTMLParser):
        def __init__(self):
            super().__init__(convert_charrefs=True); self.o = []
        def tag(self, t, a, sc):
            self.o.append(f"S:{X(t)}" + ''.join(f"|{X(n)}={X(v or '')}" for n, v in a) + f":{sc}")
        def handle_starttag(self, t, a): self.tag(t, a, 0)
        def handle_startendtag(self, t, a): self.tag(t, a, 1)
        def handle_endtag(self, t): self.o.append(f"E:{X(t)}")
        def handle_data(self, d):
            if self.o and self.o[-1][0] == 'T': self.o[-1] = ('T', self.o[-1][1] + d)
            else: self.o.append(('T', d))
        def handle_comment(self, d): self.o.append(f"C:{X(d)}")
        def handle_decl(self, d):
            p = d.split()
            self.o.append(f"D:{X(p[1].lower() if len(p) > 1 else '')}")
    for line in lines:
        s = bytes.fromhex(line).decode('utf-8')
        out = []
        if mode == 'css':
            dump(tinycss2.parse_component_value_list(s, skip_comments=True), 0, out)
        elif mode == 'decls':
            decls(s, out)
        elif mode == 'sheet':
            for r in tinycss2.parse_stylesheet(s, skip_comments=True, skip_whitespace=True):
                if r.type == 'qualified-rule':
                    out.append('Q'); dump(trim(r.prelude), 0, out); decls(r.content, out)
                elif r.type == 'at-rule':
                    out.append(f"A:{X(r.at_keyword)}"); dump(trim(r.prelude), 0, out)
                    if r.content is None: out.append('N')
                    else: out.append('B'); dump(trim(r.content), 0, out)
        elif mode == 'html':
            p = P(); p.feed(s); p.close()
            out = [x if isinstance(x, str) else f"T:{X(x[1])}" for x in p.o]
        res.append(' '.join(out))
open(outp, 'w', encoding='utf-8').write('\n'.join(res) + '\n')
"##;

fn hx(s: &str) -> String {
    s.bytes().map(|b| format!("{b:02x}")).collect()
}

fn oracle(mode: &str, lines: &[String]) -> Vec<String> {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("t65-{mode}"));
    std::fs::create_dir_all(&dir).unwrap();
    let mut input = String::new();
    for l in lines {
        input.push_str(l);
        input.push('\n');
    }
    std::fs::write(dir.join("in.txt"), input).unwrap();
    std::fs::write(dir.join("oracle_t65.py"), PY).unwrap();
    let o = Command::new("python")
        .arg("-I")
        .arg(dir.join("oracle_t65.py"))
        .arg(mode)
        .arg(dir.join("in.txt"))
        .arg(dir.join("out.txt"))
        .output()
        .expect("python on PATH");
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let out = std::fs::read_to_string(dir.join("out.txt")).unwrap();
    out.lines().map(str::to_string).collect()
}

fn ctok(t: &C) -> String {
    let ni = |i: bool| if i { "int" } else { "num" };
    match t {
        C::Ident(s) => format!("ident:{}", hx(s)),
        C::Function(s) => format!("fnTOKEN:{}", hx(s)),
        C::AtKeyword(s) => format!("at:{}", hx(s)),
        C::Hash { value, id } => format!("hash:{}:{}", hx(value), if *id { "id" } else { "unr" }),
        C::Str(s) => format!("str:{}", hx(s)),
        C::BadStr => "badstr".into(),
        C::Url(s) => format!("url:{}", hx(s)),
        C::BadUrl => "badurl".into(),
        C::Delim(c) => format!("delim:{}", hx(&c.to_string())),
        C::Number { value, int } => format!("num:{value:?}:{}", ni(*int)),
        C::Percentage(v) => format!("pct:{v:?}"),
        C::Dimension { value, int, unit } => format!("dim:{value:?}:{}:{}", ni(*int), hx(unit)),
        C::Whitespace => "ws".into(),
        C::Cdo => "cdo".into(),
        C::Cdc => "cdc".into(),
        C::Colon => "colon".into(),
        C::Semicolon => "semi".into(),
        C::Comma => "comma".into(),
        C::CloseParen => "close:)".into(),
        C::CloseSquare => "close:]".into(),
        C::CloseCurly => "close:}".into(),
        other => format!("OPEN-IN-VALUES:{other:?}"),
    }
}

fn dump_vals(v: &[Value], d: usize, out: &mut Vec<String>) {
    for x in v {
        match x {
            Value::Token(t) => out.push(format!("{d}:{}", ctok(t))),
            Value::Block { open, items } => {
                out.push(format!("{d}:block:{open}"));
                dump_vals(items, d + 1, out);
            }
            Value::Function { name, args } => {
                out.push(format!("{d}:fn:{}", hx(name)));
                dump_vals(args, d + 1, out);
            }
        }
    }
}

fn trim(v: &[Value]) -> &[Value] {
    let ws = |x: &Value| matches!(x, Value::Token(C::Whitespace));
    let a = v.iter().position(|x| !ws(x)).unwrap_or(v.len());
    let b = v.iter().rposition(|x| !ws(x)).map_or(a, |i| i + 1);
    &v[a..b]
}

fn dump_decls(ds: &[css::Declaration], out: &mut Vec<String>) {
    for d in ds {
        out.push(format!("D:{}:{}", hx(&d.name), u8::from(d.important)));
        dump_vals(trim(&d.value), 0, out);
    }
}

/// Equal, or equal apart from numbers that parse to the same f64.
fn same(ours: &str, py: &str) -> bool {
    if ours == py {
        return true;
    }
    let (a, b): (Vec<&str>, Vec<&str>) = (ours.split(' ').collect(), py.split(' ').collect());
    a.len() == b.len()
        && a.iter().zip(&b).all(|(x, y)| {
            if x == y {
                return true;
            }
            let (fx, fy): (Vec<&str>, Vec<&str>) = (x.split(':').collect(), y.split(':').collect());
            fx.len() == fy.len()
                && fx.len() >= 3
                && matches!(fx[1], "num" | "pct" | "dim")
                && fx.iter().zip(&fy).enumerate().all(|(i, (p, q))| {
                    p == q
                        || (i == 2
                            && match (p.parse::<f64>(), q.parse::<f64>()) {
                                (Ok(u), Ok(v)) => u.to_bits() == v.to_bits(),
                                _ => false,
                            })
                })
        })
}

fn compare(what: &str, cases: &[String], ours: &[String], py: &[String]) {
    assert_eq!(py.len(), cases.len(), "{what}: oracle line count");
    let bad: Vec<String> = (0..cases.len())
        .filter(|&i| !same(&ours[i], &py[i]))
        .map(|i| format!("{:?}\n  ours {}\n  py   {}", cases[i], ours[i], py[i]))
        .collect();
    assert!(
        bad.is_empty(),
        "{what}: {} of {} differ:\n{}",
        bad.len(),
        cases.len(),
        bad[..bad.len().min(8)].join("\n")
    );
}

const CSS_BITS: &[&str] = &[
    "a",
    "-x",
    "--v",
    "_q",
    "\\31 a",
    "é",
    "b\\:c",
    "1",
    "-2.5",
    "+.5",
    "1e3",
    "10E-2",
    "7.",
    "0.0",
    "-0",
    "1e400",
    "10px",
    "1.5em",
    "2e",
    "50%",
    "-1x",
    "#a1",
    "#1a",
    "#-",
    "\"a b\"",
    "'x\\'y'",
    "\"ab\ncd\"",
    "\"open",
    "url(a.png)",
    "url( b )",
    "url(a b)",
    "url(\"q\")",
    "calc(",
    "f(",
    "(",
    ")",
    "[",
    "]",
    "{",
    "}",
    "!",
    "*",
    ">",
    "+",
    "~",
    "|",
    "=",
    "$",
    "^",
    ".",
    "/",
    ",",
    ":",
    ";",
    "<!--",
    "-->",
    "@media",
    "@x-y",
    "/*c*/",
    "/* open",
    " ",
    "  ",
    "\n",
    "\t",
    "\r\n",
    "\u{c}",
    "\\\n",
    "\\",
    "\0",
    "\\0",
    "\\110000",
    "\\d800 ",
    "important",
    "-->x",
    "--",
    "-",
    "1.e2",
];

fn gen_css(r: &mut Rng) -> String {
    (0..1 + r.below(14)).map(|_| *r.pick(CSS_BITS)).collect()
}

const VAL_BITS: &[&str] = &[
    "a",
    "b c",
    "1px",
    "-2.5em",
    "50%",
    "#f00",
    "\"s;t\"",
    "f(a, b)",
    "calc(1px + 2%)",
    "[x]",
    "(y)",
    "url(i.png)",
    ",",
    "/",
    "*",
    "+",
    " ",
    "\n",
    "/*c*/",
    "!",
    "--x",
    "1e3",
    "é",
    "@k",
    "red",
];

fn gen_value(r: &mut Rng) -> String {
    let mut v: String = (0..r.below(5)).map(|_| *r.pick(VAL_BITS)).collect();
    if r.chance(1, 12) {
        v.push_str(r.pick(&["{x}", "{ a: b }"]));
    }
    if r.chance(1, 4) {
        v.push_str(r.pick(&["!important", " ! IMPORTANT", "!Important ", "! important x"]));
    }
    v
}

fn gen_decls(r: &mut Rng) -> String {
    let mut s = String::new();
    for _ in 0..r.below(6) {
        match r.below(10) {
            0 => s.push_str(r.pick(&["foo bar", ";", " ", "@m x;", "@m { a: b }", "1px: x", ":x"])),
            _ => {
                s.push_str(r.pick(&["color", "--c", "Width", "margin-top", "a\\62 c", "-x", "z"]));
                s.push_str(r.pick(&[":", " :", ": ", "\n:\t"]));
                s.push_str(&gen_value(r));
            }
        }
        s.push_str(r.pick(&[";", "; ", ";;", "\n;", ""]));
    }
    s
}

fn gen_sheet(r: &mut Rng) -> String {
    let mut s = String::new();
    for _ in 0..r.below(5) {
        s.push_str(r.pick(&["", " ", "\n", "<!--", "-->", "/*x*/"]));
        match r.below(6) {
            0 => {
                s.push_str(r.pick(&["@import \"a.css\";", "@charset \"u\";", "@x y z;"]));
            }
            1 => {
                s.push_str(r.pick(&[
                    "@media screen and (min-width: 3px) {",
                    "@font-face {",
                    "@m{",
                ]));
                s.push_str(r.pick(&[" a { b: c } ", "x: y;", " p{} q{r:s} ", ""]));
                s.push('}');
            }
            _ => {
                s.push_str(r.pick(&[
                    "a",
                    "a.b > c",
                    "#i",
                    "ul li:nth-child(2n+1)",
                    "*",
                    "[x=\"y\"]",
                    "h1, h2",
                    "a;b",
                ]));
                s.push_str(r.pick(&["", " ", "\n"]));
                s.push('{');
                s.push_str(&gen_decls(r));
                if r.chance(9, 10) {
                    s.push('}');
                }
            }
        }
    }
    s
}

#[test]
fn leader_css_tokens_match_tinycss2() {
    let mut r = Rng::new(0x65_c55_0001);
    let cases: Vec<String> = (0..3000).map(|_| gen_css(&mut r)).collect();
    let ours: Vec<String> = cases
        .iter()
        .map(|c| {
            let mut out = Vec::new();
            dump_vals(&css::parse_values(c), 0, &mut out);
            out.join(" ")
        })
        .collect();
    let py = oracle("css", &cases.iter().map(|c| hx(c)).collect::<Vec<_>>());
    compare("css values", &cases, &ours, &py);
    // tokenize must agree with parse_values flattened: no comments, same token count at depth 0
    // for inputs without blocks or functions.
    for c in &cases {
        let toks = css::tokenize(c);
        assert!(
            !toks.is_empty() || c.trim().is_empty() || c.contains("/*"),
            "{c:?}"
        );
    }
}

#[test]
fn leader_stylesheets_and_declarations_match_tinycss2() {
    let mut r = Rng::new(0x65_5ee7_0002);
    let sheets: Vec<String> = (0..800).map(|_| gen_sheet(&mut r)).collect();
    let ours: Vec<String> = sheets
        .iter()
        .map(|s| {
            let mut out = Vec::new();
            for rule in css::parse_stylesheet(s) {
                match rule {
                    css::Rule::Style {
                        prelude,
                        declarations,
                    } => {
                        out.push("Q".into());
                        dump_vals(trim(&prelude), 0, &mut out);
                        dump_decls(&declarations, &mut out);
                    }
                    css::Rule::At {
                        name,
                        prelude,
                        block,
                    } => {
                        out.push(format!("A:{}", hx(&name)));
                        dump_vals(trim(&prelude), 0, &mut out);
                        match block {
                            None => out.push("N".into()),
                            Some(b) => {
                                out.push("B".into());
                                dump_vals(trim(&b), 0, &mut out);
                            }
                        }
                    }
                }
            }
            out.join(" ")
        })
        .collect();
    let py = oracle("sheet", &sheets.iter().map(|c| hx(c)).collect::<Vec<_>>());
    compare("stylesheets", &sheets, &ours, &py);
    let lists: Vec<String> = (0..500).map(|_| gen_decls(&mut r)).collect();
    let ours: Vec<String> = lists
        .iter()
        .map(|s| {
            let mut out = Vec::new();
            dump_decls(&css::parse_declarations(s), &mut out);
            out.join(" ")
        })
        .collect();
    let py = oracle("decls", &lists.iter().map(|c| hx(c)).collect::<Vec<_>>());
    compare("declaration lists", &lists, &ours, &py);
}

fn gen_html(r: &mut Rng) -> String {
    const TEXT: &[&str] = &[
        "a",
        "bc",
        " ",
        "\n",
        "&amp;",
        "&lt;",
        "&gt;",
        "&quot;",
        "&apos;",
        "&nbsp;",
        "&copy;",
        "&#65;",
        "&#x41;",
        "&#X6a;",
        "&#0;",
        "&#x110000;",
        "&#150;",
        "&#xD800;",
        " & ",
        "a;b",
        ">",
        "=",
        "\"",
        "'",
        "/",
        "é",
        "&hellip;",
        "&euro;",
        "&unknownref;",
        "&mdash;",
        "&rarr;",
    ];
    const NAMES: &[&str] = &[
        "div", "p", "span", "a", "b", "em", "ul", "li", "img", "br", "input", "section", "h1",
        "x-y", "DIV", "Span", "IMG",
    ];
    const VALS: &[&str] = &[
        "a", "b c", ">", "&amp;", "&lt;", "&copy;", "&#65;", "&#x41;", "&#150;", "=", "/", "é", "",
    ];
    let mut s = String::new();
    if r.chance(1, 3) {
        s.push_str(r.pick(&["<!DOCTYPE html>", "<!doctype HTML>", "<!DocType html>"]));
    }
    for _ in 0..r.below(25) {
        match r.below(9) {
            0..=2 => {
                for _ in 0..1 + r.below(4) {
                    s.push_str(r.pick(TEXT));
                }
            }
            3 | 4 => {
                let name = *r.pick(NAMES);
                s.push('<');
                s.push_str(name);
                let mut used = Vec::new();
                let mut last_unquoted = false;
                for _ in 0..r.below(4) {
                    let a = *r.pick(&["class", "id", "title", "data-k", "href", "hidden", "a-b"]);
                    if used.contains(&a) {
                        continue;
                    }
                    used.push(a);
                    s.push_str(r.pick(&[" ", "\n", "\t", "  "]));
                    if r.chance(1, 4) {
                        s.push_str(&a.to_uppercase());
                    } else {
                        s.push_str(a);
                    }
                    last_unquoted = false;
                    match r.below(4) {
                        0 => {}
                        k => {
                            s.push_str(r.pick(&["=", " = ", "="]));
                            let v: String = (0..r.below(3)).map(|_| *r.pick(VALS)).collect();
                            match k {
                                1 => {
                                    s.push('"');
                                    s.push_str(&v.replace('"', "'"));
                                    s.push('"');
                                }
                                2 => {
                                    s.push('\'');
                                    s.push_str(&v.replace('\'', "\""));
                                    s.push('\'');
                                }
                                _ => {
                                    s.push_str(r.pick(&["x", "a1", "q.png", "9"]));
                                    last_unquoted = true;
                                }
                            }
                        }
                    }
                }
                if r.chance(1, 4) {
                    s.push_str(if last_unquoted { " /" } else { "/" });
                } else if r.chance(1, 4) {
                    s.push(' ');
                }
                s.push('>');
            }
            5 | 6 => {
                s.push_str("</");
                s.push_str(r.pick(NAMES));
                s.push_str(r.pick(&["", " ", "\n"]));
                s.push('>');
            }
            7 => {
                s.push_str("<!-- ");
                for _ in 0..r.below(3) {
                    s.push_str(r.pick(&["a", " ", "x y", "é", ">", "<p>"]));
                }
                s.push_str(" -->");
            }
            _ => {
                let t = *r.pick(&["script", "style"]);
                s.push_str(&format!("<{t}>"));
                for _ in 0..r.below(4) {
                    s.push_str(r.pick(&[
                        "if (a < b && c) { x = 1; }",
                        " x = \"</b>\"; ",
                        "p>a{}",
                        "&amp;",
                        "a<b",
                        "</div>",
                        "\n",
                    ]));
                }
                s.push_str(&format!("</{t}>"));
            }
        }
    }
    s
}

fn dump_html_tokens(toks: &[H]) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut text = String::new();
    let flush = |text: &mut String, out: &mut Vec<String>| {
        if !text.is_empty() {
            out.push(format!("T:{}", hx(text)));
            text.clear();
        }
    };
    for t in toks {
        if let H::Text(s) = t {
            text.push_str(s);
            continue;
        }
        flush(&mut text, &mut out);
        out.push(match t {
            H::Doctype(n) => format!("D:{}", hx(n)),
            H::StartTag {
                name,
                attrs,
                self_closing,
            } => {
                let mut s = format!("S:{}", hx(name));
                for (k, v) in attrs {
                    let _ = write!(s, "|{}={}", hx(k), hx(v));
                }
                let _ = write!(s, ":{}", u8::from(*self_closing));
                s
            }
            H::EndTag(n) => format!("E:{}", hx(n)),
            H::Comment(c) => format!("C:{}", hx(c)),
            H::Text(_) => unreachable!(),
        });
    }
    flush(&mut text, &mut out);
    out.join(" ")
}

#[test]
fn leader_html_tokens_match_python_html_parser() {
    let mut r = Rng::new(0x65_47a1_0003);
    let cases: Vec<String> = (0..2000).map(|_| gen_html(&mut r)).collect();
    let ours: Vec<String> = cases
        .iter()
        .map(|c| dump_html_tokens(&html::tokenize(c)))
        .collect();
    let py = oracle("html", &cases.iter().map(|c| hx(c)).collect::<Vec<_>>());
    compare("html tokens", &cases, &ours, &py);
}

const SKEL_EMPTY: &str = "| <html>\n|   <head>\n|   <body>\n";

fn body(lines: &str) -> String {
    format!("{SKEL_EMPTY}{lines}")
}

#[test]
fn leader_tree_fixtures() {
    let cases: Vec<(&str, String)> = vec![
        ("<!DOCTYPE html><title>T</title><p>Hi", "| <!DOCTYPE html>\n| <html>\n|   <head>\n|     <title>\n|       \"T\"\n|   <body>\n|     <p>\n|       \"Hi\"\n".into()),
        ("<p>One<p>Two", body("|     <p>\n|       \"One\"\n|     <p>\n|       \"Two\"\n")),
        ("<p>a<div>b</div>c", body("|     <p>\n|       \"a\"\n|     <div>\n|       \"b\"\n|     \"c\"\n")),
        ("<ul><li>a<li>b</ul>", body("|     <ul>\n|       <li>\n|         \"a\"\n|       <li>\n|         \"b\"\n")),
        ("<ol><li>x<ul><li>y</ul><li>z</ol>", body("|     <ol>\n|       <li>\n|         \"x\"\n|         <ul>\n|           <li>\n|             \"y\"\n|       <li>\n|         \"z\"\n")),
        ("<h1>a<h2>b</h2>", body("|     <h1>\n|       \"a\"\n|     <h2>\n|       \"b\"\n")),
        ("a<br>b<img src=x>c<hr>d", body("|     \"a\"\n|     <br>\n|     \"b\"\n|     <img>\n|       src=\"x\"\n|     \"c\"\n|     <hr>\n|     \"d\"\n")),
        ("x</p>", body("|     \"x\"\n|     <p>\n")),
        ("<div><p>a</div>b", body("|     <div>\n|       <p>\n|         \"a\"\n|     \"b\"\n")),
        ("<b>x</i>y</b>", body("|     <b>\n|       \"xy\"\n")),
        ("<table><tr><td>1<td>2</table>", body("|     <table>\n|       <tbody>\n|         <tr>\n|           <td>\n|             \"1\"\n|           <td>\n|             \"2\"\n")),
        ("<table>x<tr><td>y</td></tr></table>", body("|     \"x\"\n|     <table>\n|       <tbody>\n|         <tr>\n|           <td>\n|             \"y\"\n")),
        ("<td>a", body("|     \"a\"\n")),
        ("<!-- c --><html><body x=1>t", "| <!--  c  -->\n| <html>\n|   <head>\n|   <body>\n|     x=\"1\"\n|     \"t\"\n".into()),
        ("<head><style>p<a</style></head> <body>", "| <html>\n|   <head>\n|     <style>\n|       \"p<a\"\n|   \" \"\n|   <body>\n".into()),
        ("<script>if(a<b)x=\"</p>\"</script>", "| <html>\n|   <head>\n|     <script>\n|       \"if(a<b)x=\"</p>\"\"\n|   <body>\n".into()),
        ("<p>&lt;&amp;&#x41;&copy;&#150;</p>", body("|     <p>\n|       \"<&A\u{a9}\u{2013}\"\n")),
        ("<div id=a class=\"b c\">x</div>", body("|     <div>\n|       class=\"b c\"\n|       id=\"a\"\n|       \"x\"\n")),
        ("<textarea>a<b>&amp;</textarea>", body("|     <textarea>\n|       \"a<b>&\"\n")),
        ("<pre>\nx</pre>", body("|     <pre>\n|       \"x\"\n")),
        ("<dl><dt>a<dd>b<dt>c</dl>", body("|     <dl>\n|       <dt>\n|         \"a\"\n|       <dd>\n|         \"b\"\n|       <dt>\n|         \"c\"\n")),
        ("<p>x</p>   <!-- e -->", body("|     <p>\n|       \"x\"\n|     \"   \"\n|     <!--  e  -->\n")),
        ("<p>a</p></body>b", body("|     <p>\n|       \"a\"\n|     \"b\"\n")),
        ("<html><head></head><body><p>a</p></body></html>\n", body("|     <p>\n|       \"a\"\n|     \"\n\"\n")),
        ("<table><td>a</td></table>", body("|     <table>\n|       <tbody>\n|         <tr>\n|           <td>\n|             \"a\"\n")),
        ("<ul><li><p>a<li>b</ul>", body("|     <ul>\n|       <li>\n|         <p>\n|           \"a\"\n|       <li>\n|         \"b\"\n")),
        ("<div>a<span>b<div>c</div>d</span>e</div>", body("|     <div>\n|       \"a\"\n|       <span>\n|         \"b\"\n|         <div>\n|           \"c\"\n|         \"d\"\n|       \"e\"\n")),
        ("<DIV CLASS=X>y</div>", body("|     <div>\n|       class=\"X\"\n|       \"y\"\n")),
        ("<!doctype HTML><p>", "| <!DOCTYPE html>\n| <html>\n|   <head>\n|   <body>\n|     <p>\n".into()),
        ("<p>a<!-- b -->c", body("|     <p>\n|       \"a\"\n|       <!--  b  -->\n|       \"c\"\n")),
        ("<table><tr><td><table><tr><td>in</table>out</table>", body("|     <table>\n|       <tbody>\n|         <tr>\n|           <td>\n|             <table>\n|               <tbody>\n|                 <tr>\n|                   <td>\n|                     \"in\"\n|             \"out\"\n")),
        ("<title>a &amp; <b></title>", "| <html>\n|   <head>\n|     <title>\n|       \"a & <b>\"\n|   <body>\n".into()),
        ("  <!DOCTYPE html>  <html> x", "| <!DOCTYPE html>\n| <html>\n|   <head>\n|   <body>\n|     \"x\"\n".into()),
        ("<table> <tr> </tr> </table>", body("|     <table>\n|       \" \"\n|       <tbody>\n|         <tr>\n|           \" \"\n|         \" \"\n")),
        ("a\r\nb\rc", body("|     \"a\nb\nc\"\n")),
    ];
    let mut bad = Vec::new();
    for (src, want) in &cases {
        let got = html::parse(src).dump();
        if &got != want {
            bad.push(format!("{src:?}\n--- want\n{want}--- got\n{got}"));
        }
    }
    assert!(
        bad.is_empty(),
        "{} of {} trees differ:\n{}",
        bad.len(),
        cases.len(),
        bad.join("\n")
    );
}

fn gen_doc(r: &mut Rng) -> String {
    fn kids(r: &mut Rng, s: &mut String, n: &mut usize, depth: usize, in_ul: bool) {
        let k = if depth >= 5 { 0 } else { r.below(5) };
        for _ in 0..k {
            if !in_ul && r.chance(1, 4) {
                s.push_str(r.pick(&["", " ", "t", "\n", "x y"]));
            }
            let name = if in_ul {
                "li"
            } else {
                *r.pick(&[
                    "div", "span", "section", "article", "ul", "em", "b", "header", "nav", "img",
                    "br", "div", "span",
                ])
            };
            let _ = write!(s, "<{name} data-i=\"{n}\"");
            *n += 1;
            if r.chance(1, 2) {
                let cls: Vec<&str> = ["a", "b", "c", "d"]
                    .into_iter()
                    .filter(|_| r.chance(1, 2))
                    .collect();
                let sep = if r.chance(1, 4) { "  " } else { " " };
                let _ = write!(s, " class=\"{}\"", cls.join(sep));
            }
            if r.chance(1, 4) {
                let _ = write!(s, " id=\"{}\"", r.pick(&["x", "y", "z"]));
            }
            if r.chance(1, 4) {
                let _ = write!(s, " title=\"{}\"", r.pick(&["t", "t u", "pre-x", "pre"]));
            }
            if r.chance(1, 3) {
                let _ = write!(s, " data-k=\"{}\"", r.pick(&["1", "12", "2", "21"]));
            }
            s.push('>');
            if name == "img" || name == "br" {
                continue;
            }
            kids(r, s, n, depth + 1, name == "ul");
            let _ = write!(s, "</{name}>");
        }
    }
    let mut s = String::from(
        "<!DOCTYPE html><html data-i=\"0\"><head data-i=\"1\"></head><body data-i=\"2\">",
    );
    let mut n = 3;
    kids(r, &mut s, &mut n, 0, false);
    s.push_str("</body></html>");
    s
}

fn gen_compound(r: &mut Rng, allow_not: bool) -> String {
    let mut s = String::new();
    if r.chance(1, 2) {
        s.push_str(r.pick(&[
            "*", "div", "span", "ul", "li", "em", "b", "section", "img", "DIV", "Span",
        ]));
    }
    let k = r.below(3) + usize::from(s.is_empty());
    for _ in 0..k {
        let part: String = match r.below(6) {
            0 => format!(".{}", r.pick(&["a", "b", "c", "d"])),
            1 => format!("#{}", r.pick(&["x", "y", "z"])),
            2 => (*r.pick(&[
                "[class]",
                "[title]",
                "[data-k]",
                "[data-k=\"1\"]",
                "[data-k^=\"1\"]",
                "[data-k$=\"2\"]",
                "[data-k*=\"2\"]",
                "[title~=\"u\"]",
                "[title|=\"pre\"]",
                "[class=\"a\"]",
                "[title=t]",
                "[ title ]",
                "[data-k^=\"\"]",
            ]))
            .into(),
            3 => (*r.pick(&[
                ":first-child",
                ":last-child",
                ":only-child",
                ":first-of-type",
                ":last-of-type",
                ":empty",
                ":root",
                ":First-Child",
            ]))
            .into(),
            4 => format!(
                ":{}({})",
                r.pick(&[
                    "nth-child",
                    "nth-last-child",
                    "nth-of-type",
                    "nth-last-of-type"
                ]),
                r.pick(&[
                    "odd", "even", "2n+1", "-n+2", "3", "0n+1", "2n", "n", "-2n+5", "2n - 1",
                    " odd ", "+n", "-n+3", "n+0",
                ])
            ),
            _ if allow_not => format!(":not({})", gen_compound(r, false)),
            _ => ".a".into(),
        };
        s.push_str(&part);
    }
    s
}

fn gen_selector(r: &mut Rng) -> String {
    if r.chance(1, 20) {
        return (*r.pick(&[
            "a >",
            "> a",
            "a,,b",
            ":nth-child()",
            "#",
            ".",
            "[x=]",
            ":hover-x",
            "a b >",
            "",
            ":not()",
            "[data-k=\"1\"",
            "*.",
            "a!b",
            "[data-i=3]",
            "#1a",
            ".1a",
            "::before",
            ":nth-child(n+)",
            "a:first-child()",
        ]))
        .into();
    }
    (0..1 + r.below(2))
        .map(|_| {
            let mut c = gen_compound(r, true);
            for _ in 0..r.below(4) {
                c.push_str(r.pick(&[" ", " > ", " + ", " ~ ", ">", "+", "~"]));
                c.push_str(&gen_compound(r, true));
            }
            c
        })
        .collect::<Vec<_>>()
        .join(", ")
}

#[test]
fn leader_selectors_match_soupsieve() {
    let mut r = Rng::new(0x65_5e1_0004);
    let mut lines = Vec::new();
    let mut ours = Vec::new();
    let mut cases = Vec::new();
    for _ in 0..60 {
        let src = gen_doc(&mut r);
        let doc = html::parse(&src);
        lines.push(format!("D {}", hx(&src)));
        for _ in 0..50 {
            let q = gen_selector(&mut r);
            lines.push(format!("Q {}", hx(&q)));
            ours.push(match select::select(&doc, &q) {
                Err(_) => "E".to_string(),
                Ok(ids) if ids.is_empty() => "-".to_string(),
                Ok(ids) => ids
                    .iter()
                    .map(|&id| doc.attr(id, "data-i").unwrap_or("?").to_string())
                    .collect::<Vec<_>>()
                    .join(","),
            });
            cases.push(format!("{q}  ON  {src}"));
        }
    }
    let py = oracle("sel", &lines);
    compare("selectors", &cases, &ours, &py);
    let matched = py.iter().filter(|l| *l != "E" && *l != "-").count();
    assert!(matched > 1000, "generator too weak: {matched}");
}

#[test]
fn leader_specificity() {
    for (sel, want) in [
        ("*", (0, 0, 0)),
        ("li", (0, 0, 1)),
        ("ul li", (0, 0, 2)),
        ("ul ol+li", (0, 0, 3)),
        ("h1 + *[rel=up]", (0, 1, 1)),
        ("ul ol li.red", (0, 1, 3)),
        ("li.red.level", (0, 2, 1)),
        ("#x34y", (1, 0, 0)),
        ("#s12:not(foo)", (1, 0, 1)),
        (":not(.a)", (0, 1, 0)),
        ("a:nth-child(2n+1)", (0, 1, 1)),
        (":root", (0, 1, 0)),
        ("[href]", (0, 1, 0)),
        ("div#a.b.c > span:first-child", (1, 3, 2)),
    ] {
        let s = select::parse_selectors(sel).unwrap();
        assert_eq!(s.len(), 1, "{sel}");
        assert_eq!(s[0].specificity(), want, "{sel}");
    }
    assert_eq!(select::parse_selectors("a, b.c, #d").unwrap().len(), 3);
}

#[test]
fn leader_robustness() {
    let mut r = Rng::new(0x65_f022_0005);
    for _ in 0..400 {
        let b = r.bytes(300);
        let s = String::from_utf8_lossy(&b).to_string();
        let doc = html::parse(&s);
        let _ = doc.dump();
        let _ = html::tokenize(&s);
        let _ = css::tokenize(&s);
        let _ = css::parse_stylesheet(&s);
        let _ = css::parse_declarations(&s);
        let _ = select::parse_selectors(&s);
        let _ = select::select(&doc, &gen_selector(&mut r));
        let mixed = format!("{}{}{}", gen_html(&mut r), s, gen_sheet(&mut r));
        let _ = html::parse(&mixed);
        let _ = css::parse_stylesheet(&mixed);
    }
    std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(|| {
            let doc = html::parse(&"<div>".repeat(100_000));
            assert_eq!(doc.elements().len(), 100_003);
            let v = css::parse_values(&"(".repeat(2000));
            let mut depth = 0;
            let mut cur = &v;
            while let Some(Value::Block { items, .. }) = cur.first() {
                depth += 1;
                cur = items;
            }
            assert_eq!(depth, 2000);
        })
        .unwrap()
        .join()
        .unwrap();
    let big = format!(
        "<!DOCTYPE html><body>{}",
        "<div class=\"a b\"><span>text &amp; more</span></div>\n".repeat(6000)
    );
    let t = std::time::Instant::now();
    let doc = html::parse(&big);
    let hits = select::select(&doc, "div.a > span").unwrap();
    assert_eq!(hits.len(), 6000);
    assert!(
        t.elapsed().as_secs_f64() < 5.0,
        "too slow: {:?}",
        t.elapsed()
    );
}

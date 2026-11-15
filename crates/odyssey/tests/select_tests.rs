//! Integration tests for CSS Selectors Level 4 parsing and matching.
#![allow(clippy::unwrap_used)]

use odyssey::html::parse;
use odyssey::select::{parse_selectors, select, SelectorError};

#[test]
fn selector_errors_table() {
    let invalid = [
        "",
        "   ",
        ",",
        "div,",
        ",div",
        "div,,p",
        "> div",
        "div >",
        "div + ~ p",
        "#1a",
        "[a=3]",
        ":unknown",
        "::before",
        ":not()",
        ":not(a b)",
        ":not(:not(a))",
        ":nth-child()",
        ":nth-child(2 n)",
        ":nth-child(1.5)",
        ".",
        "div..foo",
    ];
    for s in invalid {
        assert_eq!(parse_selectors(s), Err(SelectorError), "selector={s:?}");
    }
}

#[test]
fn selector_specificity_table() {
    let cases = [
        ("*", (0, 0, 0)),
        ("div", (0, 0, 1)),
        ("div p", (0, 0, 2)),
        (".a.b", (0, 2, 0)),
        ("#id", (1, 0, 0)),
        ("ul#nav li.active > a[href]:first-child", (1, 3, 3)),
        ("div:not(#x.y)", (1, 1, 1)),
    ];
    for (sel, expected) in cases {
        let parsed = parse_selectors(sel).unwrap();
        assert_eq!(parsed[0].specificity(), expected, "sel={sel:?}");
    }
}

#[test]
fn selector_matching() {
    let doc = parse(
        "<div id=main class='wrap box'>\
         <p class='a b' data-lang='en-US' data-empty=''>one</p>\
         <p class='b' data-lang='en'><span></span></p>\
         <p class='c'><!-- comment -->   </p>\
         <span id='s1'>s</span>\
         </div>",
    );

    let names = |sel: &str| -> Vec<String> {
        select(&doc, sel)
            .unwrap()
            .into_iter()
            .map(|id| doc.name(id).unwrap().to_owned())
            .collect()
    };

    assert_eq!(names(":root"), vec!["html"]);
    assert_eq!(
        names(":root:first-child:last-child:only-child:nth-child(1)"),
        vec!["html"]
    );
    assert_eq!(names("#main > p.b"), vec!["p", "p"]);
    assert_eq!(names("p[data-lang|='en']"), vec!["p", "p"]);
    assert_eq!(names("p[data-empty^='']"), Vec::<String>::new());
    assert_eq!(names("p[data-empty$='']"), Vec::<String>::new());
    assert_eq!(names("p[data-empty*='']"), Vec::<String>::new());
    assert_eq!(names("p:empty"), vec!["p"]); // third <p> has only comment + whitespace
    assert_eq!(names("p:nth-child(2n+1)"), vec!["p", "p"]);
    assert_eq!(names("p:nth-child( even )"), vec!["p"]);
    assert_eq!(names("p:nth-last-of-type(-n + 2)"), vec!["p", "p"]);
    assert_eq!(names("#main > p + span#s1"), vec!["span"]);
    assert_eq!(names("p.a ~ span"), vec!["span"]);
    assert_eq!(names("div > :not(p)"), vec!["span"]);
    assert_eq!(names("p.a, #s1"), vec!["p", "span"]);
}

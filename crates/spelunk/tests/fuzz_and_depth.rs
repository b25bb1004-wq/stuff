//! Property tests, no-panic fuzzing, and deep-nesting stack-safety tests.
#![allow(clippy::unwrap_used)]

use assay::forall;
use spelunk::{css, html, select};

#[test]
fn fuzz_all_parsers_no_panic() {
    forall(200, 0x5EED_CAFE, |rng| {
        let raw = rng.bytes(256);
        let s = String::from_utf8_lossy(&raw);

        let _ = html::tokenize(&s);
        let doc = html::parse(&s);
        let _ = doc.dump();
        let _ = doc.elements();

        let _ = css::tokenize(&s);
        let _ = css::parse_values(&s);
        let _ = css::parse_declarations(&s);
        let _ = css::parse_stylesheet(&s);

        let _ = select::parse_selectors(&s);
        let _ = select::select(&doc, &s);
    });
}

#[test]
fn html_round_trip_props() {
    forall(100, 0x1234_5678, |rng| {
        let mut s = String::new();
        let n = rng.range(1, 20);
        for _ in 0..n {
            let tag = *rng.pick(&["div", "p", "span", "b", "i"]);
            let txt = format!("t{}", rng.range(0, 100));
            s.push('<');
            s.push_str(tag);
            s.push('>');
            s.push_str(&txt);
            s.push_str("</");
            s.push_str(tag);
            s.push('>');
        }
        let doc = html::parse(&s);
        let dump = doc.dump();
        assert!(dump.contains("| <html>\n"));
        assert!(dump.contains("|   <body>\n"));
    });
}

#[test]
fn deep_nesting_on_2mib_stack() {
    let handle = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            let depth_html = 100_000;
            let mut html_in = String::with_capacity(depth_html * 11);
            for _ in 0..depth_html {
                html_in.push_str("<div>");
            }
            for _ in 0..depth_html {
                html_in.push_str("</div>");
            }
            let doc = html::parse(&html_in);
            let els = doc.elements();
            assert_eq!(els.len(), depth_html + 3); // html, head, body + 100k divs

            let depth_css = 2_000;
            let mut css_in = String::with_capacity(depth_css * 2);
            for _ in 0..depth_css {
                css_in.push('(');
            }
            for _ in 0..depth_css {
                css_in.push(')');
            }
            let vals = css::parse_values(&css_in);
            assert_eq!(vals.len(), 1);
        })
        .unwrap();
    handle.join().unwrap();
}

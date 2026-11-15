//! Integration tests for `odyssey::script`.
#![allow(clippy::unwrap_used)]

use odyssey::dom::Data;
use odyssey::script::{run_page_scripts, DEFAULT_FUEL};
use odyssey::{html, layout, style};

fn element_text(doc: &odyssey::dom::Document, id_attr: &str) -> Option<String> {
    let el = doc
        .elements()
        .into_iter()
        .find(|&id| doc.attr(id, "id") == Some(id_attr))?;
    let mut s = String::new();
    for &cid in &doc.nodes[el].children {
        if let Data::Text(t) = &doc.nodes[cid].data {
            s.push_str(t);
        }
    }
    Some(s)
}

#[test]
fn script_sets_counter_and_reads_back_in_document_order() {
    let src = r#"<!doctype html><html><body>
      <div id="counter">0</div>
      <script type="text/logos">
        fn p_text(id: str) -> str? = extern "page.text"
        fn p_set(id: str, s: str) -> bool = extern "page.set_text"
        fn p_log(s: str) = extern "page.log"

        fn main() {
            let before = match p_text("counter") {
                some(v) => v,
                none => "none",
            }
            p_log(before)
            if p_set("counter", "1") {
                p_log("updated1")
            }
        }
      </script>
      <script type="text/logos">
        fn p_text(id: str) -> str? = extern "page.text"
        fn p_set(id: str, s: str) -> bool = extern "page.set_text"
        fn p_log(s: str) = extern "page.log"

        fn main() {
            match p_text("counter") {
                some(v) => {
                    if v == "1" {
                        let _ = p_set("counter", "2")
                        p_log("updated2")
                    }
                },
                none => p_log("missing"),
            }
        }
      </script>
    </body></html>"#;

    let mut doc = html::parse(src);
    let log = run_page_scripts(&mut doc, DEFAULT_FUEL);
    assert_eq!(element_text(&doc, "counter").as_deref(), Some("2"));
    assert_eq!(log.entries(), &["0", "updated1", "updated2"]);

    let styles = style::compute(&doc);
    let lay = layout::layout(&doc, &styles, 320);
    assert_eq!(lay.runs.len(), 1);
    assert_eq!(lay.runs[0].text, "2");
}

#[test]
fn script_compile_error_is_logged_and_page_renders() {
    let src = r#"<!doctype html><html><body>
      <p id="msg">visible</p>
      <script type="text/logos">
        fn main() {
            let x = unknown_var + 1
        }
      </script>
    </body></html>"#;

    let mut doc = html::parse(src);
    let log = run_page_scripts(&mut doc, DEFAULT_FUEL);
    assert!(!log.is_empty());
    assert!(log.contains("compile error"), "unexpected log: {log}");
    assert_eq!(element_text(&doc, "msg").as_deref(), Some("visible"));

    let styles = style::compute(&doc);
    let lay = layout::layout(&doc, &styles, 320);
    assert_eq!(lay.runs.len(), 1);
    assert_eq!(lay.runs[0].text, "visible");
}

#[test]
fn script_infinite_loop_hits_fuel_and_page_renders() {
    let src = r#"<!doctype html><html><body>
      <p id="status">ok</p>
      <script type="text/logos">
        fn main() {
            while true {
            }
        }
      </script>
      <script type="text/logos">
        fn p_set(id: str, s: str) -> bool = extern "page.set_text"
        fn main() {
            let _ = p_set("status", "after-loop")
        }
      </script>
    </body></html>"#;

    let mut doc = html::parse(src);
    let log = run_page_scripts(&mut doc, 500);
    assert!(log.contains("OutOfFuel"), "expected OutOfFuel in {log}");
    assert_eq!(element_text(&doc, "status").as_deref(), Some("after-loop"));

    let styles = style::compute(&doc);
    let lay = layout::layout(&doc, &styles, 320);
    assert_eq!(lay.runs[0].text, "after-loop");
}

#[test]
fn script_missing_id_returns_false_and_none() {
    let src = r#"<!doctype html><html><body>
      <div id="present">hello</div>
      <script type="text/logos">
        fn p_text(id: str) -> str? = extern "page.text"
        fn p_set(id: str, s: str) -> bool = extern "page.set_text"
        fn p_log(s: str) = extern "page.log"

        fn main() {
            let set_ok = p_set("ghost", "nope")
            if !set_ok {
                p_log("set=false")
            } else {
                p_log("set=true")
            }
            match p_text("ghost") {
                some(v) => p_log(v),
                none => p_log("text=none"),
            }
        }
      </script>
      <script type="text/javascript">
        this should not be parsed as logos
      </script>
    </body></html>"#;

    let mut doc = html::parse(src);
    let log = run_page_scripts(&mut doc, DEFAULT_FUEL);
    assert_eq!(log.entries(), &["set=false", "text=none"]);
    assert_eq!(element_text(&doc, "present").as_deref(), Some("hello"));
}

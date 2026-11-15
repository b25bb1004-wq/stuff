//! Demo: parse a small page, read its styles, select nodes, and lay it out.
//! Everything here is fixed input; the program prints one plain line per step.

use odyssey::css::parse_stylesheet;
use odyssey::dom::Data;
use odyssey::html;
use odyssey::layout::layout;
use odyssey::select::{parse_selectors, select};
use odyssey::style::compute;

const PAGE: &str = "<!DOCTYPE html>
<html>
<head>
<style>
  .note { color: red; padding: 4px; }
  #title { font-size: 24px; }
  ul { margin: 8px; }
</style>
</head>
<body>
<h1 id=\"title\">Odyssey demo</h1>
<p class=\"note\">Parsed, styled and laid out.</p>
<ul><li>first</li><li>second</li><li>third</li></ul>
</body>
</html>";

fn main() {
    // 1. HTML: tokenize and build the tree.
    let doc = html::parse(PAGE);
    let elements = doc.elements();
    println!(
        "html: parsed {} nodes, {} elements",
        doc.nodes.len(),
        elements.len()
    );

    // 2. CSS: parse the text of the <style> block on its own.
    let css = "  .note { color: red; padding: 4px; }\n  #title { font-size: 24px; }\n  ul { margin: 8px; }";
    let rules = parse_stylesheet(css);
    println!("css: parsed {} style rules", rules.len());

    // 3. Selectors: parse a list and match it against the document.
    let selectors = parse_selectors("p.note, li:nth-child(2)")
        .map(|v| v.len())
        .unwrap_or(0);
    println!("selectors: list has {selectors} selectors");
    match select(&doc, "p.note, li:nth-child(2)") {
        Ok(ids) => {
            for id in ids {
                let name = doc.name(id).unwrap_or("?");
                println!("select: matched <{name}> at node {id}");
            }
        }
        Err(_) => println!("select: failed"),
    }
    match select(&doc, "p[") {
        Ok(_) => println!("select: unexpected success on a bad selector"),
        Err(_) => println!("select: bad selector \"p[\" rejected with an error"),
    }

    // 4. Style: the cascade gives every element a computed style.
    let styles = compute(&doc);
    for id in &elements {
        if let (Some(name), Some(Some(st))) = (doc.name(*id), styles.get(*id)) {
            println!(
                "style: <{name}> display={:?} font={}px margin-top={:?}",
                st.display, st.font_px, st.margin[0]
            );
        }
    }

    // 5. Layout: place boxes and words in a 320 pixel wide viewport.
    let lay = layout(&doc, &styles, 320);
    println!(
        "layout: root height {} px, {} boxes, {} words",
        lay.height,
        lay.boxes.len(),
        lay.runs.len()
    );
    for run in lay.runs.iter().take(4) {
        println!(
            "layout: word \"{}\" at x={} y={} scale={}",
            run.text, run.rect.x, run.rect.y, run.scale
        );
    }

    // 6. Text nodes are where the words come from.
    let text_nodes = doc
        .nodes
        .iter()
        .filter(|n| matches!(n.data, Data::Text(_)))
        .count();
    println!("dom: {text_nodes} text nodes");
    println!("demo: done");
}

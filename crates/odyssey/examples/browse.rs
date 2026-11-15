//! Fetch a URL over HTTP, run `<script type="text/logos">` scripts, and print layout boxes.

use std::env;
use std::process;

use mercury::Limits;
use odyssey::{fetch, html, layout, script, style};

fn main() {
    let Some(url) = env::args().nth(1) else {
        eprintln!("usage: browse <http://host[:port]/path>");
        process::exit(1);
    };

    let resp = match fetch::get(&url, &Limits::default()) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("fetch error: {e}");
            process::exit(1);
        }
    };

    let body = String::from_utf8_lossy(&resp.body);
    let mut doc = html::parse(&body);
    let log = script::run_page_scripts(&mut doc, script::DEFAULT_FUEL);
    if !log.is_empty() {
        println!("script log:");
        for entry in log.entries() {
            println!("  {entry}");
        }
    }

    let styles = style::compute(&doc);
    let lay = layout::layout(&doc, &styles, 800);
    println!("status: {} {}", resp.status, resp.reason);
    println!(
        "layout height: {} px, {} boxes, {} runs",
        lay.height,
        lay.boxes.len(),
        lay.runs.len()
    );
    for (id, rect) in &lay.boxes {
        let tag = doc.name(*id).unwrap_or("?");
        println!(
            "  box #{id} <{tag}> x={} y={} w={} h={}",
            rect.x, rect.y, rect.w, rect.h
        );
    }
}

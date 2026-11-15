# odyssey

odyssey is the HTML and CSS side of Monolith's page stack. It turns HTML into a tree, reads the CSS in `<style>` blocks and `style` attributes, works out the computed style of each element, and places the boxes and words in a viewport. It uses only the standard library and the pigment crate, so the whole pipeline can be read and tested in one place.

## Features

- HTML tokenizer with data, RCDATA, RAWTEXT and script data states, comments, DOCTYPE and character references.
- HTML tree builder, plus `Document::dump()` in html5lib test format.
- CSS tokenizer, component values, declarations and stylesheets.
- Selectors: type, id, class, the six attribute match operators, `:root`, `:empty`, `:first-child`, `:last-child`, `:only-child`, `:first-of-type`, `:last-of-type`, the four `:nth-*` pseudo-classes, `:not()`, and the descendant, child, `+` and `~` combinators.
- Cascade: a user-agent sheet, `<style>` sheets and `style` attributes, with `!important` and specificity. `color` and `font-size` are inherited.
- Properties: `display`, `width`, `height`, `margin`, `padding`, `border-width`, `color`, `background-color`, `font-size`, `flex-direction`, `justify-content`, `align-items`, `flex-grow` and `gap`.
- Layout: block boxes, inline words placed on lines, and flex containers.
- Paint: software rendering of backgrounds, solid per-side borders, and scaled bitmap text runs onto a `pigment::Canvas`.
- Fetch: HTTP/1.1 GET client over `std::net::TcpStream` using `mercury` with 2s I/O timeouts, up to 5 redirects, and an 8 MiB body cap.
- Script: `<script type="text/logos">` execution via `logos`/`cobalt`/`elysium` with `page.text`, `page.set_text`, and `page.log` externs.
- Shell: `indium`-based `Browser` widget tree with address bar, Back, Reload, status line, clickable `<a href>` links, and error pages.
- Not handled: grid, and at-rules such as `@media`. The parser reads at-rules, but the cascade ignores them.

## Public API

| Item | What it does |
| --- | --- |
| `html::parse(&str) -> Document` | Build a DOM tree from HTML |
| `html::tokenize(&str) -> Vec<Token>` | HTML tokens only |
| `css::tokenize(&str) -> Vec<Token>` | CSS tokens only |
| `css::parse_stylesheet(&str) -> Vec<Rule>` | Parse a stylesheet into rules |
| `css::parse_declarations(&str) -> Vec<Declaration>` | Parse a declaration list such as a `style` attribute |
| `css::parse_values(&str) -> Vec<Value>` | Parse a list of component values |
| `select::parse_selectors(&str)` | Parse a selector list, or return `SelectorError` |
| `select::select(&Document, &str)` | Matching element ids, in document order |
| `style::compute(&Document) -> Vec<Option<Computed>>` | Computed style for each node |
| `style::compute_flex(&Document) -> Vec<Option<Flex>>` | Flex properties for each node |
| `layout::layout(&Document, &[Option<Computed>], i32) -> Layout` | Boxes, words and height for a viewport width |
| `paint::paint(&Document, &[Option<Computed>], &Layout, &mut Canvas)` | Paint backgrounds, borders and text runs |
| `fetch::get(&str, &Limits) -> Result<Response, FetchError>` | Fetch an `http://` URL over HTTP/1.1 |
| `script::run_page_scripts(&mut Document, u64) -> ScriptLog` | Run `<script type="text/logos">` blocks |
| `shell::Browser` | Interactive browser shell (`navigate`, `back`, `reload`, `frame`, `event`) |
| `dom::Document` | Arena tree with `name`, `attr`, `elements` and `dump` |

## Demo

The demo parses a small page, parses a stylesheet, runs selectors (including one bad selector), prints computed styles, and lays the page out at 320 pixels. It uses fixed input and needs no network.

```
cargo run -p odyssey --example demo
```

Real output:

```text
html: parsed 25 nodes, 10 elements
css: parsed 3 style rules
selectors: list has 2 selectors
select: matched <p> at node 14
select: matched <li> at node 20
select: bad selector "p[" rejected with an error
style: <html> display=Block font=16px margin-top=Px(0)
style: <head> display=None font=16px margin-top=Px(0)
style: <style> display=None font=16px margin-top=Px(0)
style: <body> display=Block font=16px margin-top=Px(8)
style: <h1> display=Block font=24px margin-top=Px(21)
style: <p> display=Block font=16px margin-top=Px(16)
style: <ul> display=Block font=16px margin-top=Px(8)
style: <li> display=Block font=16px margin-top=Px(0)
style: <li> display=Block font=16px margin-top=Px(0)
style: <li> display=Block font=16px margin-top=Px(0)
layout: root height 186 px, 8 boxes, 10 words
layout: word "Odyssey" at x=8 y=29 scale=2
layout: word "demo" at x=136 y=29 scale=2
layout: word "Parsed," at x=12 y=86 scale=1
layout: word "styled" at x=76 y=86 scale=1
dom: 13 text nodes
demo: done
```

## Tests

```
cargo test -p odyssey
```

The crate's gate is `bash tools/check.sh odyssey`, which runs format, clippy with warnings denied, the tests, and the zero-dependency check.

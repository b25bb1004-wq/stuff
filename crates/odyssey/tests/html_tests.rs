//! Integration tests for HTML tokenization and tree construction.
#![allow(clippy::unwrap_used)]

use odyssey::dom::Data;
use odyssey::html::{parse, tokenize, Token};

#[test]
fn html_tokenizer_table() {
    let cases: &[(&str, Vec<Token>)] = &[
        ("", vec![]),
        ("hello world", vec![Token::Text("hello world".into())]),
        ("a\r\nb\rc", vec![Token::Text("a\nb\nc".into())]),
        ("<!-- x -->", vec![Token::Comment(" x ".into())]),
        ("<!DOCTYPE HTML>", vec![Token::Doctype("html".into())]),
        (
            "<DIV ID='a' class=b disabled data-x=\"1\" id=\"dup\">",
            vec![Token::StartTag {
                name: "div".into(),
                attrs: vec![
                    ("id".into(), "a".into()),
                    ("class".into(), "b".into()),
                    ("disabled".into(), "".into()),
                    ("data-x".into(), "1".into()),
                ],
                self_closing: false,
            }],
        ),
        (
            "<br/>",
            vec![Token::StartTag {
                name: "br".into(),
                attrs: vec![],
                self_closing: true,
            }],
        ),
        ("</DIV>", vec![Token::EndTag("div".into())]),
        (
            "a&amp;b&#65;&#x41;&#X41;&#150;&#0;&#x110000;&#xD800;&unknown;&",
            vec![Token::Text(
                "a&bAAA\u{2013}\u{FFFD}\u{FFFD}\u{FFFD}&unknown;&".into(),
            )],
        ),
        (
            "<a title=\"&lt;&hearts;&rarr;&euro;\">",
            vec![Token::StartTag {
                name: "a".into(),
                attrs: vec![("title".into(), "<\u{2665}\u{2192}\u{20AC}".into())],
                self_closing: false,
            }],
        ),
        (
            "<script>if (a < b && &amp;) {}</script>",
            vec![
                Token::StartTag {
                    name: "script".into(),
                    attrs: vec![],
                    self_closing: false,
                },
                Token::Text("if (a < b && &amp;) {}".into()),
                Token::EndTag("script".into()),
            ],
        ),
        (
            "<style>p > a { content: \"&lt;\"; }</style>",
            vec![
                Token::StartTag {
                    name: "style".into(),
                    attrs: vec![],
                    self_closing: false,
                },
                Token::Text("p > a { content: \"&lt;\"; }".into()),
                Token::EndTag("style".into()),
            ],
        ),
        (
            "<title><b>a &amp; b</b></title>",
            vec![
                Token::StartTag {
                    name: "title".into(),
                    attrs: vec![],
                    self_closing: false,
                },
                Token::Text("<b>a & b</b>".into()),
                Token::EndTag("title".into()),
            ],
        ),
        (
            "<textarea><i>hi &lt;</i></textarea>",
            vec![
                Token::StartTag {
                    name: "textarea".into(),
                    attrs: vec![],
                    self_closing: false,
                },
                Token::Text("<i>hi <</i>".into()),
                Token::EndTag("textarea".into()),
            ],
        ),
    ];

    for (input, expected) in cases {
        assert_eq!(tokenize(input), *expected, "input={input:?}");
    }
}

#[test]
fn html_tree_fixtures() {
    let cases: &[(&str, &str)] = &[
        (
            "",
            "| <html>\n|   <head>\n|   <body>\n",
        ),
        (
            "<!DOCTYPE html><!-- top --><html><head><title>T</title></head><body><p>Hi</body></html>",
            "| <!DOCTYPE html>\n| <!--  top  -->\n| <html>\n|   <head>\n|     <title>\n|       \"T\"\n|   <body>\n|     <p>\n|       \"Hi\"\n",
        ),
        (
            "<meta charset=utf-8><link rel=stylesheet><p>hello",
            "| <html>\n|   <head>\n|     <meta>\n|       charset=\"utf-8\"\n|     <link>\n|       rel=\"stylesheet\"\n|   <body>\n|     <p>\n|       \"hello\"\n",
        ),
        (
            "</head>   <p>x</p>",
            "| <html>\n|   <head>\n|   \"   \"\n|   <body>\n|     <p>\n|       \"x\"\n",
        ),
        (
            "<html lang=en><body class=a><html dir=ltr><body id=b>",
            "| <html>\n|   dir=\"ltr\"\n|   lang=\"en\"\n|   <head>\n|   <body>\n|     class=\"a\"\n|     id=\"b\"\n",
        ),
        (
            "</body></html><p>after",
            "| <html>\n|   <head>\n|   <body>\n|     <p>\n|       \"after\"\n",
        ),
        (
            "<p>a<div>b",
            "| <html>\n|   <head>\n|   <body>\n|     <p>\n|       \"a\"\n|     <div>\n|       \"b\"\n",
        ),
        (
            "<h1>a<h2>b",
            "| <html>\n|   <head>\n|   <body>\n|     <h1>\n|       \"a\"\n|     <h2>\n|       \"b\"\n",
        ),
        (
            "<ul><li>1<li>2</ul>",
            "| <html>\n|   <head>\n|   <body>\n|     <ul>\n|       <li>\n|         \"1\"\n|       <li>\n|         \"2\"\n",
        ),
        (
            "<dl><dt>a<dd>b<dt>c</dl>",
            "| <html>\n|   <head>\n|   <body>\n|     <dl>\n|       <dt>\n|         \"a\"\n|       <dd>\n|         \"b\"\n|       <dt>\n|         \"c\"\n",
        ),
        (
            "<pre>\n\nhello</pre>",
            "| <html>\n|   <head>\n|   <body>\n|     <pre>\n|       \"\nhello\"\n",
        ),
        (
            "<div></p></div>",
            "| <html>\n|   <head>\n|   <body>\n|     <div>\n|       <p>\n",
        ),
        (
            "<table><tr><td>1<td>2<tr><th>3</table>",
            "| <html>\n|   <head>\n|   <body>\n|     <table>\n|       <tbody>\n|         <tr>\n|           <td>\n|             \"1\"\n|           <td>\n|             \"2\"\n|         <tr>\n|           <th>\n|             \"3\"\n",
        ),
        (
            "<table>foster<tr><td>cell</td></tr></table>",
            "| <html>\n|   <head>\n|   <body>\n|     \"foster\"\n|     <table>\n|       <tbody>\n|         <tr>\n|           <td>\n|             \"cell\"\n",
        ),
        (
            "<td>ignored</td><p>ok</p>",
            "| <html>\n|   <head>\n|   <body>\n|     \"ignored\"\n|     <p>\n|       \"ok\"\n",
        ),
        (
            "<img src=a alt=b><span>hi</span>",
            "| <html>\n|   <head>\n|   <body>\n|     <img>\n|       alt=\"b\"\n|       src=\"a\"\n|     <span>\n|       \"hi\"\n",
        ),
    ];

    for (input, expected) in cases {
        let doc = parse(input);
        assert_eq!(doc.dump(), *expected, "input={input:?}");
    }

    let doc = parse("<div id=x data-a=1><span>a</span></div>");
    assert_eq!(doc.nodes[0].data, Data::Document);
    let els = doc.elements();
    assert_eq!(els.len(), 5); // html, head, body, div, span
    let div = els[3];
    assert_eq!(doc.name(div), Some("div"));
    assert_eq!(doc.attr(div, "id"), Some("x"));
    assert_eq!(doc.attr(div, "missing"), None);
}

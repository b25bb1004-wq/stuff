//! HTML5 tree construction building a [`crate::dom::Document`].

use crate::dom::{Data, Document, Node, NodeId};

use super::tokenizer::{tokenize, Token};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Initial,
    BeforeHead,
    InHead,
    AfterHead,
    InBody,
}

fn is_ws(s: &str) -> bool {
    s.bytes()
        .all(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\x0C'))
}

fn is_void(name: &str) -> bool {
    matches!(
        name,
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "source"
            | "track"
            | "wbr"
    )
}

fn is_head_content(name: &str) -> bool {
    matches!(
        name,
        "title" | "meta" | "link" | "style" | "script" | "base"
    )
}

fn closes_p_on_start(name: &str) -> bool {
    matches!(
        name,
        "address"
            | "article"
            | "aside"
            | "blockquote"
            | "center"
            | "details"
            | "dialog"
            | "dir"
            | "div"
            | "dl"
            | "fieldset"
            | "figcaption"
            | "figure"
            | "footer"
            | "header"
            | "hgroup"
            | "main"
            | "menu"
            | "nav"
            | "ol"
            | "p"
            | "section"
            | "summary"
            | "ul"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "pre"
            | "listing"
            | "form"
            | "hr"
            | "table"
            | "li"
            | "dd"
            | "dt"
    )
}

fn is_heading(name: &str) -> bool {
    matches!(name, "h1" | "h2" | "h3" | "h4" | "h5" | "h6")
}

fn is_implied_end(name: &str) -> bool {
    matches!(
        name,
        "dd" | "dt" | "li" | "optgroup" | "option" | "p" | "rb" | "rp" | "rt" | "rtc"
    )
}

fn is_scope_boundary(name: &str, button_scope: bool) -> bool {
    if button_scope && name == "button" {
        return true;
    }
    matches!(
        name,
        "applet" | "caption" | "html" | "table" | "td" | "th" | "marquee" | "object" | "template"
    )
}

fn is_special(name: &str) -> bool {
    matches!(
        name,
        "address"
            | "applet"
            | "area"
            | "article"
            | "aside"
            | "base"
            | "basefont"
            | "bgsound"
            | "blockquote"
            | "body"
            | "br"
            | "button"
            | "caption"
            | "center"
            | "col"
            | "colgroup"
            | "dd"
            | "details"
            | "dir"
            | "div"
            | "dl"
            | "dt"
            | "embed"
            | "fieldset"
            | "figcaption"
            | "figure"
            | "footer"
            | "form"
            | "frame"
            | "frameset"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "head"
            | "header"
            | "hgroup"
            | "hr"
            | "html"
            | "iframe"
            | "img"
            | "input"
            | "keygen"
            | "li"
            | "link"
            | "listing"
            | "main"
            | "marquee"
            | "menu"
            | "meta"
            | "nav"
            | "noembed"
            | "noframes"
            | "noscript"
            | "object"
            | "ol"
            | "p"
            | "param"
            | "plaintext"
            | "pre"
            | "script"
            | "section"
            | "select"
            | "source"
            | "style"
            | "summary"
            | "table"
            | "tbody"
            | "td"
            | "template"
            | "textarea"
            | "tfoot"
            | "th"
            | "thead"
            | "title"
            | "tr"
            | "track"
            | "ul"
            | "wbr"
            | "xmp"
    )
}

fn is_table_only_tag(name: &str) -> bool {
    matches!(
        name,
        "caption" | "col" | "colgroup" | "tbody" | "td" | "tfoot" | "th" | "thead" | "tr"
    )
}

struct Builder {
    doc: Document,
    stack: Vec<NodeId>,
    p_count: usize,
    phase: Phase,
    html_id: Option<NodeId>,
    head_id: Option<NodeId>,
    body_id: Option<NodeId>,
    strip_newline: bool,
}

impl Builder {
    fn new() -> Self {
        Self {
            doc: Document::new(),
            stack: Vec::new(),
            p_count: 0,
            phase: Phase::Initial,
            html_id: None,
            head_id: None,
            body_id: None,
            strip_newline: false,
        }
    }

    fn push_stack(&mut self, id: NodeId) {
        if self.name_of(id) == "p" {
            self.p_count += 1;
        }
        self.stack.push(id);
    }

    fn pop_stack(&mut self) -> Option<NodeId> {
        let id = self.stack.pop()?;
        if self.name_of(id) == "p" {
            self.p_count = self.p_count.saturating_sub(1);
        }
        Some(id)
    }

    fn alloc(&mut self, data: Data, parent: NodeId) -> NodeId {
        let id = self.doc.nodes.len();
        self.doc.nodes.push(Node {
            data,
            parent: Some(parent),
            children: Vec::new(),
        });
        if let Some(p) = self.doc.nodes.get_mut(parent) {
            p.children.push(id);
        }
        id
    }

    fn append_text(&mut self, parent: NodeId, text: &str) {
        if text.is_empty() {
            return;
        }
        if let Some(last_id) = self
            .doc
            .nodes
            .get(parent)
            .and_then(|p| p.children.last().copied())
        {
            if let Some(Node {
                data: Data::Text(prev),
                ..
            }) = self.doc.nodes.get_mut(last_id)
            {
                prev.push_str(text);
                return;
            }
        }
        self.alloc(Data::Text(text.to_owned()), parent);
    }

    fn foster_text(&mut self, table_id: NodeId, text: &str) {
        if text.is_empty() {
            return;
        }
        let Some(parent) = self.doc.nodes.get(table_id).and_then(|n| n.parent) else {
            return;
        };
        let pos = self
            .doc
            .nodes
            .get(parent)
            .and_then(|p| p.children.iter().position(|&c| c == table_id));
        let Some(idx) = pos else {
            return;
        };
        if idx > 0 {
            let prev_id = self.doc.nodes[parent].children[idx - 1];
            if let Some(Node {
                data: Data::Text(prev),
                ..
            }) = self.doc.nodes.get_mut(prev_id)
            {
                prev.push_str(text);
                return;
            }
        }
        let id = self.doc.nodes.len();
        self.doc.nodes.push(Node {
            data: Data::Text(text.to_owned()),
            parent: Some(parent),
            children: Vec::new(),
        });
        self.doc.nodes[parent].children.insert(idx, id);
    }

    fn merge_attrs(&mut self, id: NodeId, attrs: Vec<(String, String)>) {
        if let Some(Node {
            data: Data::Element {
                attrs: existing, ..
            },
            ..
        }) = self.doc.nodes.get_mut(id)
        {
            for (k, v) in attrs {
                if !existing.iter().any(|(ek, _)| ek == &k) {
                    existing.push((k, v));
                }
            }
        }
    }

    fn current_id(&self) -> NodeId {
        self.stack.last().copied().unwrap_or(0)
    }

    fn name_of(&self, id: NodeId) -> &str {
        self.doc.name(id).unwrap_or("")
    }

    fn in_scope(&self, target: &str, button_scope: bool) -> bool {
        if target == "p" && self.p_count == 0 {
            return false;
        }
        for &id in self.stack.iter().rev() {
            let n = self.name_of(id);
            if n == target {
                return true;
            }
            if is_scope_boundary(n, button_scope) {
                return false;
            }
        }
        false
    }

    fn in_table_scope(&self, target: &str) -> bool {
        for &id in self.stack.iter().rev() {
            let n = self.name_of(id);
            if n == target {
                return true;
            }
            if matches!(n, "html" | "table" | "template") {
                return false;
            }
        }
        false
    }

    fn gen_implied_end_tags(&mut self, except: &str) {
        while let Some(&id) = self.stack.last() {
            let n = self.name_of(id);
            if n != except && is_implied_end(n) {
                self.pop_stack();
            } else {
                break;
            }
        }
    }

    fn close_p_in_button_scope(&mut self) {
        if !self.in_scope("p", true) {
            return;
        }
        self.gen_implied_end_tags("p");
        while let Some(id) = self.stack.pop() {
            if self.name_of(id) == "p" {
                break;
            }
        }
    }

    fn close_li(&mut self) {
        for i in (0..self.stack.len()).rev() {
            let n = self.name_of(self.stack[i]);
            if n == "li" {
                self.gen_implied_end_tags("li");
                while let Some(id) = self.stack.pop() {
                    if self.name_of(id) == "li" {
                        break;
                    }
                }
                return;
            }
            if is_special(n) && !matches!(n, "address" | "div" | "p") {
                return;
            }
        }
    }

    fn close_dd_dt(&mut self) {
        for i in (0..self.stack.len()).rev() {
            let n = self.name_of(self.stack[i]);
            if n == "dd" || n == "dt" {
                let target = if n == "dd" { "dd" } else { "dt" };
                self.gen_implied_end_tags(target);
                while let Some(id) = self.stack.pop() {
                    let popped = self.name_of(id);
                    if popped == "dd" || popped == "dt" {
                        break;
                    }
                }
                return;
            }
            if is_special(n) && !matches!(n, "address" | "div" | "p") {
                return;
            }
        }
    }

    fn ensure_html(&mut self, attrs: Vec<(String, String)>) -> NodeId {
        if let Some(id) = self.html_id {
            return id;
        }
        let id = self.alloc(
            Data::Element {
                name: "html".to_owned(),
                attrs,
            },
            0,
        );
        self.html_id = Some(id);
        self.push_stack(id);
        id
    }

    fn ensure_head(&mut self, attrs: Vec<(String, String)>) -> NodeId {
        if let Some(id) = self.head_id {
            return id;
        }
        let html = self.ensure_html(Vec::new());
        let id = self.alloc(
            Data::Element {
                name: "head".to_owned(),
                attrs,
            },
            html,
        );
        self.head_id = Some(id);
        id
    }

    fn ensure_body(&mut self, attrs: Vec<(String, String)>) -> NodeId {
        if let Some(id) = self.body_id {
            return id;
        }
        let html = self.ensure_html(Vec::new());
        let id = self.alloc(
            Data::Element {
                name: "body".to_owned(),
                attrs,
            },
            html,
        );
        self.body_id = Some(id);
        self.push_stack(id);
        id
    }

    fn clear_back_to_table_context(&mut self) {
        while let Some(&id) = self.stack.last() {
            if matches!(self.name_of(id), "table" | "template" | "html") {
                break;
            }
            self.pop_stack();
        }
    }

    fn clear_back_to_table_body_context(&mut self) {
        while let Some(&id) = self.stack.last() {
            if matches!(
                self.name_of(id),
                "tbody" | "tfoot" | "thead" | "table" | "template" | "html"
            ) {
                break;
            }
            self.pop_stack();
        }
    }

    fn clear_back_to_table_row_context(&mut self) {
        while let Some(&id) = self.stack.last() {
            if matches!(
                self.name_of(id),
                "tr" | "tbody" | "tfoot" | "thead" | "table" | "template" | "html"
            ) {
                break;
            }
            self.pop_stack();
        }
    }

    fn nearest_table(&self) -> Option<NodeId> {
        self.stack
            .iter()
            .rev()
            .copied()
            .find(|&id| self.name_of(id) == "table")
    }

    fn process(&mut self, mut tok: Token) {
        let mut strip = false;
        if self.strip_newline {
            self.strip_newline = false;
            if let Token::Text(ref mut t) = tok {
                if t.starts_with('\n') {
                    t.remove(0);
                    if t.is_empty() {
                        return;
                    }
                }
            }
        }

        let mut pending = Some(tok);
        while let Some(mut t) = pending.take() {
            // Before body content WHATWG handles text one character at a time: leading
            // whitespace of a mixed run is processed (dropped or inserted) on its own first.
            let pre_body = match self.phase {
                Phase::Initial | Phase::BeforeHead | Phase::AfterHead => true,
                Phase::InHead => self.current_id() == self.head_id.unwrap_or(0),
                _ => false,
            };
            if let Token::Text(ref mut s) = t {
                let n = s.len() - s.trim_start_matches([' ', '\t', '\n', '\x0C']).len();
                if pre_body && n > 0 && n < s.len() {
                    let rest = s.split_off(n);
                    self.process(Token::Text(core::mem::take(s)));
                    pending = Some(Token::Text(rest));
                    continue;
                }
            }
            match self.phase {
                Phase::Initial => match t {
                    Token::Doctype(name) => {
                        self.alloc(Data::Doctype(name), 0);
                    }
                    Token::Comment(c) => {
                        self.alloc(Data::Comment(c), 0);
                    }
                    Token::Text(ref s) if is_ws(s) => {}
                    Token::StartTag { name, attrs, .. } if name == "html" => {
                        self.ensure_html(attrs);
                        self.phase = Phase::BeforeHead;
                    }
                    other => {
                        self.ensure_html(Vec::new());
                        self.phase = Phase::BeforeHead;
                        pending = Some(other);
                    }
                },
                Phase::BeforeHead => match t {
                    Token::Doctype(_) => {}
                    Token::Comment(c) => {
                        let html = self.ensure_html(Vec::new());
                        self.alloc(Data::Comment(c), html);
                    }
                    Token::Text(ref s) if is_ws(s) => {}
                    Token::StartTag { name, attrs, .. } if name == "html" => {
                        if let Some(id) = self.html_id {
                            self.merge_attrs(id, attrs);
                        }
                    }
                    Token::StartTag { name, attrs, .. } if name == "head" => {
                        let head = self.ensure_head(attrs);
                        self.stack.push(head);
                        self.phase = Phase::InHead;
                    }
                    Token::EndTag(ref name)
                        if !matches!(name.as_str(), "head" | "body" | "html" | "br") => {}
                    other => {
                        let head = self.ensure_head(Vec::new());
                        self.stack.push(head);
                        self.phase = Phase::InHead;
                        pending = Some(other);
                    }
                },
                Phase::InHead => match t {
                    Token::Doctype(_) => {}
                    Token::Comment(c) => {
                        let cur = self.current_id();
                        self.alloc(Data::Comment(c), cur);
                    }
                    Token::Text(ref s) => {
                        let cur = self.current_id();
                        if cur != self.head_id.unwrap_or(0) || is_ws(s) {
                            self.append_text(cur, s);
                        } else {
                            self.pop_stack();
                            self.phase = Phase::AfterHead;
                            pending = Some(t);
                        }
                    }
                    Token::StartTag { name, attrs, .. } if name == "html" => {
                        if let Some(id) = self.html_id {
                            self.merge_attrs(id, attrs);
                        }
                    }
                    Token::StartTag { ref name, .. } if name == "head" => {}
                    Token::StartTag { name, attrs, .. }
                        if matches!(name.as_str(), "base" | "link" | "meta") =>
                    {
                        let head = self.head_id.unwrap_or_else(|| self.current_id());
                        self.alloc(Data::Element { name, attrs }, head);
                    }
                    Token::StartTag { name, attrs, .. }
                        if matches!(name.as_str(), "title" | "style" | "script") =>
                    {
                        let head = self.head_id.unwrap_or_else(|| self.current_id());
                        let id = self.alloc(Data::Element { name, attrs }, head);
                        self.push_stack(id);
                    }
                    Token::EndTag(ref name) if name == "head" => {
                        if Some(self.current_id()) == self.head_id {
                            self.pop_stack();
                        }
                        self.phase = Phase::AfterHead;
                    }
                    Token::EndTag(ref name)
                        if matches!(name.as_str(), "title" | "style" | "script") =>
                    {
                        if self.name_of(self.current_id()) == name {
                            self.pop_stack();
                        }
                    }
                    Token::EndTag(ref name) if matches!(name.as_str(), "body" | "html" | "br") => {
                        if Some(self.current_id()) == self.head_id {
                            self.pop_stack();
                        }
                        self.phase = Phase::AfterHead;
                        pending = Some(t);
                    }
                    Token::EndTag(_) => {}
                    other => {
                        if Some(self.current_id()) == self.head_id {
                            self.pop_stack();
                        }
                        self.phase = Phase::AfterHead;
                        pending = Some(other);
                    }
                },
                Phase::AfterHead => match t {
                    Token::Doctype(_) => {}
                    Token::Comment(c) => {
                        let html = self.html_id.unwrap_or(0);
                        self.alloc(Data::Comment(c), html);
                    }
                    Token::Text(ref s) if is_ws(s) => {
                        let html = self.html_id.unwrap_or(0);
                        self.append_text(html, s);
                    }
                    Token::StartTag { name, attrs, .. } if name == "html" => {
                        if let Some(id) = self.html_id {
                            self.merge_attrs(id, attrs);
                        }
                    }
                    Token::StartTag { name, attrs, .. } if name == "body" => {
                        self.ensure_body(attrs);
                        self.phase = Phase::InBody;
                    }
                    Token::StartTag { ref name, .. } if name == "head" => {}
                    Token::StartTag { name, attrs, .. } if is_head_content(&name) => {
                        if let Some(head) = self.head_id {
                            if is_void(&name) {
                                self.alloc(Data::Element { name, attrs }, head);
                            } else {
                                self.stack.push(head);
                                self.phase = Phase::InHead;
                                pending = Some(Token::StartTag {
                                    name,
                                    attrs,
                                    self_closing: false,
                                });
                            }
                        }
                    }
                    Token::EndTag(ref name) if !matches!(name.as_str(), "body" | "html" | "br") => {
                    }
                    other => {
                        self.ensure_body(Vec::new());
                        self.phase = Phase::InBody;
                        pending = Some(other);
                    }
                },
                Phase::InBody => {
                    strip = self.process_in_body(t);
                }
            }
        }
        self.strip_newline = strip;
    }

    fn process_in_body(&mut self, tok: Token) -> bool {
        match tok {
            Token::Doctype(_) => false,
            Token::Comment(c) => {
                let cur = self.current_id();
                self.alloc(Data::Comment(c), cur);
                false
            }
            Token::Text(s) => {
                let cur = self.current_id();
                let cur_name = self.name_of(cur);
                if matches!(cur_name, "table" | "tbody" | "thead" | "tfoot" | "tr") && !is_ws(&s) {
                    if let Some(table_id) = self.nearest_table() {
                        self.foster_text(table_id, &s);
                        return false;
                    }
                }
                self.append_text(cur, &s);
                false
            }
            Token::StartTag { name, attrs, .. } => self.handle_body_start_tag(name, attrs),
            Token::EndTag(name) => {
                self.handle_body_end_tag(&name);
                false
            }
        }
    }

    fn handle_body_start_tag(&mut self, name: String, attrs: Vec<(String, String)>) -> bool {
        if name == "html" {
            if let Some(id) = self.html_id {
                self.merge_attrs(id, attrs);
            }
            return false;
        }
        if name == "body" {
            if let Some(id) = self.body_id {
                self.merge_attrs(id, attrs);
            }
            return false;
        }
        if name == "head" {
            return false;
        }

        if is_table_only_tag(&name) {
            if !self.in_table_scope("table") {
                return false;
            }
            match name.as_str() {
                "tbody" | "thead" | "tfoot" => {
                    self.clear_back_to_table_context();
                    let cur = self.current_id();
                    let id = self.alloc(Data::Element { name, attrs }, cur);
                    self.push_stack(id);
                }
                "tr" => {
                    self.clear_back_to_table_body_context();
                    if self.name_of(self.current_id()) == "table" {
                        let cur = self.current_id();
                        let tbody = self.alloc(
                            Data::Element {
                                name: "tbody".to_owned(),
                                attrs: Vec::new(),
                            },
                            cur,
                        );
                        self.stack.push(tbody);
                    }
                    let cur = self.current_id();
                    let id = self.alloc(Data::Element { name, attrs }, cur);
                    self.push_stack(id);
                }
                "td" | "th" => {
                    self.clear_back_to_table_row_context();
                    if self.name_of(self.current_id()) != "tr" {
                        self.clear_back_to_table_body_context();
                        if self.name_of(self.current_id()) == "table" {
                            let cur = self.current_id();
                            let tbody = self.alloc(
                                Data::Element {
                                    name: "tbody".to_owned(),
                                    attrs: Vec::new(),
                                },
                                cur,
                            );
                            self.stack.push(tbody);
                        }
                        let cur = self.current_id();
                        let tr = self.alloc(
                            Data::Element {
                                name: "tr".to_owned(),
                                attrs: Vec::new(),
                            },
                            cur,
                        );
                        self.stack.push(tr);
                    }
                    let cur = self.current_id();
                    let id = self.alloc(Data::Element { name, attrs }, cur);
                    self.push_stack(id);
                }
                _ => {
                    let cur = self.current_id();
                    let id = self.alloc(
                        Data::Element {
                            name: name.clone(),
                            attrs,
                        },
                        cur,
                    );
                    if !is_void(&name) {
                        self.push_stack(id);
                    }
                }
            }
            return false;
        }

        if closes_p_on_start(&name) {
            self.close_p_in_button_scope();
        }
        if is_heading(&name) && is_heading(self.name_of(self.current_id())) {
            self.pop_stack();
        } else if name == "li" {
            self.close_li();
        } else if name == "dd" || name == "dt" {
            self.close_dd_dt();
        }

        let strip = matches!(name.as_str(), "pre" | "listing" | "textarea");
        let void = is_void(&name);
        let cur = self.current_id();
        let id = self.alloc(Data::Element { name, attrs }, cur);
        if !void {
            self.push_stack(id);
        }
        strip
    }

    fn handle_body_end_tag(&mut self, name: &str) {
        if matches!(name, "body" | "html") {
            return;
        }
        if name == "p" {
            if !self.in_scope("p", true) {
                let cur = self.current_id();
                self.alloc(
                    Data::Element {
                        name: "p".to_owned(),
                        attrs: Vec::new(),
                    },
                    cur,
                );
            } else {
                self.close_p_in_button_scope();
            }
            return;
        }
        if name == "table" {
            if self.in_table_scope("table") {
                while let Some(id) = self.stack.pop() {
                    if self.name_of(id) == "table" {
                        break;
                    }
                }
            }
            return;
        }
        if matches!(name, "tbody" | "thead" | "tfoot" | "tr" | "td" | "th") {
            if self.in_table_scope(name) {
                self.gen_implied_end_tags(name);
                while let Some(id) = self.stack.pop() {
                    if self.name_of(id) == name {
                        break;
                    }
                }
            }
            return;
        }
        if self.in_scope(name, false) {
            self.gen_implied_end_tags(name);
            while let Some(id) = self.stack.pop() {
                if self.name_of(id) == name {
                    break;
                }
            }
        }
    }

    fn finish(mut self) -> Document {
        if self.html_id.is_none() {
            self.ensure_html(Vec::new());
        }
        if self.head_id.is_none() {
            self.ensure_head(Vec::new());
        }
        if self.body_id.is_none() {
            self.ensure_body(Vec::new());
        }
        self.doc
    }
}

/// Parse an HTML string into a [`Document`].
pub fn parse(input: &str) -> Document {
    let tokens = tokenize(input);
    let mut builder = Builder::new();
    for tok in tokens {
        builder.process(tok);
    }
    builder.finish()
}

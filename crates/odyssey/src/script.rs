//! Execution of `<script type="text/logos">` blocks against the DOM.

use std::fmt;

use elysium::{Heap, Host, Limits, Obj, Trap, Value, Vm};
use logos::ir::Body;

use crate::dom::{Data, Document, Node, NodeId};

/// Default per-script instruction fuel limit.
pub const DEFAULT_FUEL: u64 = 1_000_000;

const MAX_LOG_ENTRIES: usize = 1024;
const MAX_LOG_BYTES: usize = 64 * 1024;

const HOST_NAMES: &[&str] = &["page.text", "page.set_text", "page.log"];

/// Log of messages and errors produced by [`run_page_scripts`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScriptLog {
    /// Ordered messages from `page.log` and script compile or runtime errors.
    pub entries: Vec<String>,
    total_bytes: usize,
}

impl ScriptLog {
    /// Create an empty script log.
    pub fn new() -> Self {
        Self::default()
    }

    /// Return all recorded log entries in order.
    pub fn entries(&self) -> &[String] {
        &self.entries
    }

    /// Return `true` if no entries have been logged.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Return the number of recorded entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Return `true` if any log entry contains `needle`.
    pub fn contains(&self, needle: &str) -> bool {
        self.entries.iter().any(|e| e.contains(needle))
    }

    /// Return all log entries joined by newlines.
    pub fn text(&self) -> String {
        self.entries.join("\n")
    }

    fn push(&mut self, msg: String) {
        if self.entries.len() >= MAX_LOG_ENTRIES || self.total_bytes >= MAX_LOG_BYTES {
            return;
        }
        let avail = MAX_LOG_BYTES.saturating_sub(self.total_bytes);
        let mut end = avail.min(msg.len());
        while !msg.is_char_boundary(end) {
            end = end.saturating_sub(1);
        }
        let clipped = msg.get(..end).unwrap_or("").to_string();
        self.total_bytes = self.total_bytes.saturating_add(clipped.len());
        self.entries.push(clipped);
    }
}

impl fmt::Display for ScriptLog {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, entry) in self.entries.iter().enumerate() {
            if i > 0 {
                f.write_str("\n")?;
            }
            f.write_str(entry)?;
        }
        Ok(())
    }
}

struct PageHost<'a> {
    doc: &'a mut Document,
    log: &'a mut ScriptLog,
}

impl Host for PageHost<'_> {
    fn call(&mut self, name: &str, args: &[Value], heap: &mut Heap) -> Result<Value, Trap> {
        match name {
            "page.text" => {
                if args.len() != 1 {
                    return Err(Trap::TypeError);
                }
                let id = arg_str(args, 0, heap)?;
                let found = find_by_id(self.doc, &id).map(|el| collect_text(self.doc, el));
                match found {
                    Some(text) => {
                        let sh = heap.alloc(Obj::Str(text))?;
                        let vh = heap.alloc(Obj::Variant {
                            tag: 1,
                            fields: vec![Value::Ref(sh)],
                        })?;
                        Ok(Value::Ref(vh))
                    }
                    None => {
                        let vh = heap.alloc(Obj::Variant {
                            tag: 0,
                            fields: Vec::new(),
                        })?;
                        Ok(Value::Ref(vh))
                    }
                }
            }
            "page.set_text" => {
                if args.len() != 2 {
                    return Err(Trap::TypeError);
                }
                let id = arg_str(args, 0, heap)?;
                let text = arg_str(args, 1, heap)?;
                Ok(Value::Bool(set_element_text(self.doc, &id, text)))
            }
            "page.log" => {
                if args.len() != 1 {
                    return Err(Trap::TypeError);
                }
                let msg = arg_str(args, 0, heap)?;
                self.log.push(msg);
                Ok(Value::Unit)
            }
            _ => Err(Trap::Host(format!("unknown host function: {name}"))),
        }
    }
}

fn arg_str(args: &[Value], idx: usize, heap: &Heap) -> Result<String, Trap> {
    let Some(Value::Ref(h)) = args.get(idx).copied() else {
        return Err(Trap::TypeError);
    };
    match heap.get(h)? {
        Obj::Str(s) => Ok(s.clone()),
        _ => Err(Trap::TypeError),
    }
}

fn find_by_id(doc: &Document, id: &str) -> Option<NodeId> {
    doc.elements()
        .into_iter()
        .find(|&el| doc.attr(el, "id") == Some(id))
}

fn collect_text(doc: &Document, root: NodeId) -> String {
    let mut out = String::new();
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        let Some(node) = doc.nodes.get(id) else {
            continue;
        };
        if let Data::Text(t) = &node.data {
            out.push_str(t);
        }
        for &c in node.children.iter().rev() {
            stack.push(c);
        }
    }
    out
}

fn set_element_text(doc: &mut Document, id: &str, text: String) -> bool {
    let Some(el) = find_by_id(doc, id) else {
        return false;
    };
    let old_children = match doc.nodes.get_mut(el) {
        Some(node) => std::mem::take(&mut node.children),
        None => return false,
    };
    for child_id in old_children {
        if let Some(child) = doc.nodes.get_mut(child_id) {
            child.parent = None;
        }
    }
    let text_id = doc.nodes.len();
    doc.nodes.push(Node {
        data: Data::Text(text),
        parent: Some(el),
        children: Vec::new(),
    });
    if let Some(node) = doc.nodes.get_mut(el) {
        node.children.push(text_id);
        true
    } else {
        false
    }
}

fn format_diag(src: &str, d: logos::Diag) -> String {
    let (line, col) = logos::line_col(src, d.span.start);
    format!("{line}:{col}: {}", d.message)
}

fn run_one(src: &str, doc: &mut Document, fuel: u64, log: &mut ScriptLog) {
    let ast = match logos::parser::parse(src) {
        Ok(ast) => ast,
        Err(d) => {
            log.push(format!("compile error: {}", format_diag(src, d)));
            return;
        }
    };
    let ir = match logos::check::check(&ast) {
        Ok(ir) => ir,
        Err(d) => {
            log.push(format!("compile error: {}", format_diag(src, d)));
            return;
        }
    };
    let Some(main_fn) = ir.funcs.iter().find(|f| f.name == "main") else {
        log.push(String::from(
            "compile error: missing required function `fn main()`",
        ));
        return;
    };
    if main_fn.params != 0 || matches!(main_fn.body, Body::Extern(_)) {
        log.push(String::from(
            "compile error: `main` must take no parameters and cannot be extern",
        ));
        return;
    }

    let compiled = cobalt::compile(&ir);
    let mut vm = match Vm::new(compiled.module, HOST_NAMES) {
        Ok(vm) => vm,
        Err(e) => {
            log.push(format!("vm load error: {e:?}"));
            return;
        }
    };
    let limits = Limits {
        fuel,
        ..Limits::default()
    };
    let mut host = PageHost { doc, log };
    if let Err(trap) = vm.call("main", &[], &mut host, limits) {
        host.log.push(format!("trap: {trap:?}"));
    }
}

/// Compile and execute every `<script type="text/logos">` in `doc` in document order.
pub fn run_page_scripts(doc: &mut Document, fuel: u64) -> ScriptLog {
    let mut log = ScriptLog::new();
    let scripts: Vec<String> = doc
        .elements()
        .into_iter()
        .filter(|&id| {
            doc.name(id) == Some("script")
                && doc
                    .attr(id, "type")
                    .is_some_and(|t| t.trim().eq_ignore_ascii_case("text/logos"))
        })
        .map(|id| collect_text(doc, id))
        .collect();

    for src in scripts {
        run_one(&src, doc, fuel, &mut log);
    }
    log
}

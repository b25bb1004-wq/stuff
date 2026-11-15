//! Browser shell built on `indium::Ui` with address bar, Back, Reload, status line and page area.

use indium::{
    Action, Align, Canvas, Dir, Event, Insets, Kind, NodeId, Rect, Rgba, Style, Theme, Ui,
};
use mercury::Limits;

use crate::dom::{Document, NodeId as DomNodeId};
use crate::fetch::{self, resolve_url};
use crate::html;
use crate::layout::{self, Layout};
use crate::paint;
use crate::script::{self, ScriptLog, DEFAULT_FUEL};
use crate::style::{self, Computed};

/// Interactive software-rendered browser shell combining `indium` chrome and `odyssey` pages.
pub struct Browser {
    width: u32,
    height: u32,
    ui: Ui,
    back_btn: NodeId,
    reload_btn: NodeId,
    addr_field: NodeId,
    status_label: NodeId,
    page_box: NodeId,
    doc: Document,
    styles: Vec<Option<Computed>>,
    layout: Layout,
    script_log: ScriptLog,
    history: Vec<String>,
    current_url: Option<String>,
    status: String,
    canvas: Canvas,
    page_pressed: Option<String>,
    limits: Limits,
    fuel: u64,
}

impl Default for Browser {
    fn default() -> Self {
        Self::new(800, 600)
    }
}

impl Browser {
    /// Create a new browser shell sized `width` by `height` pixels.
    pub fn new(width: u32, height: u32) -> Self {
        let mut ui = Ui::new(Theme::light());
        let root = ui.root();
        let toolbar = ui
            .add(
                root,
                Kind::Box,
                Style {
                    dir: Dir::Row,
                    align: Align::Center,
                    gap: 4,
                    padding: Insets {
                        top: 4,
                        right: 4,
                        bottom: 4,
                        left: 4,
                    },
                    ..Style::default()
                },
            )
            .unwrap_or(root);
        let btn_pad = Insets {
            top: 4,
            right: 8,
            bottom: 4,
            left: 8,
        };
        let back_btn = ui
            .add(
                toolbar,
                Kind::Button(String::from("Back")),
                Style {
                    padding: btn_pad,
                    ..Style::default()
                },
            )
            .unwrap_or(root);
        let reload_btn = ui
            .add(
                toolbar,
                Kind::Button(String::from("Reload")),
                Style {
                    padding: btn_pad,
                    ..Style::default()
                },
            )
            .unwrap_or(root);
        let addr_field = ui
            .add(
                toolbar,
                Kind::Field(String::new()),
                Style {
                    grow: 1,
                    padding: Insets {
                        top: 4,
                        right: 6,
                        bottom: 4,
                        left: 6,
                    },
                    ..Style::default()
                },
            )
            .unwrap_or(root);
        let status_label = ui
            .add(
                root,
                Kind::Label(String::from("Ready")),
                Style {
                    padding: Insets {
                        top: 2,
                        right: 6,
                        bottom: 2,
                        left: 6,
                    },
                    ..Style::default()
                },
            )
            .unwrap_or(root);
        let page_box = ui
            .add(
                root,
                Kind::Box,
                Style {
                    grow: 1,
                    ..Style::default()
                },
            )
            .unwrap_or(root);

        ui.layout(width, height);

        let doc = html::parse("");
        let styles = style::compute(&doc);
        let page_w = page_width(&ui, page_box);
        let layout = layout::layout(&doc, &styles, page_w);

        let mut browser = Self {
            width,
            height,
            ui,
            back_btn,
            reload_btn,
            addr_field,
            status_label,
            page_box,
            doc,
            styles,
            layout,
            script_log: ScriptLog::new(),
            history: Vec::new(),
            current_url: None,
            status: String::from("Ready"),
            canvas: Canvas::new(width, height, Rgba(255, 255, 255, 255)),
            page_pressed: None,
            limits: Limits::default(),
            fuel: DEFAULT_FUEL,
        };
        browser.render_frame();
        browser
    }

    /// Navigate to `url`, fetching over HTTP, running page scripts, laying out, and painting.
    pub fn navigate(&mut self, url: &str) {
        self.load_url(url, true);
    }

    /// Navigate back to the previous URL in history, returning `true` if history moved back.
    pub fn back(&mut self) -> bool {
        if self.history.len() <= 1 {
            return false;
        }
        self.history.pop();
        let Some(prev) = self.history.last().cloned() else {
            return false;
        };
        self.load_url(&prev, false);
        true
    }

    /// Reload the current URL, returning `true` if a URL is loaded.
    pub fn reload(&mut self) -> bool {
        let Some(cur) = self.history.last().cloned() else {
            return false;
        };
        self.load_url(&cur, false);
        true
    }

    /// Return the latest rendered frame containing the chrome and painted page.
    pub fn frame(&self) -> &Canvas {
        &self.canvas
    }

    /// Deliver an `indium::Event` (mouse, key, or text) to the browser UI and page links.
    pub fn event(&mut self, ev: Event) {
        let actions = self.ui.event(ev);
        let mut navigated = false;
        for action in actions {
            match action {
                Action::Clicked(id) if id == self.back_btn => {
                    self.back();
                    navigated = true;
                }
                Action::Clicked(id) if id == self.reload_btn => {
                    self.reload();
                    navigated = true;
                }
                Action::Submitted(id) if id == self.addr_field => {
                    if let Some(url) = self.ui.text(self.addr_field).map(str::to_string) {
                        self.navigate(&url);
                        navigated = true;
                    }
                }
                _ => {}
            }
        }

        if !navigated {
            match ev {
                Event::MouseDown { x, y } => {
                    if self.ui.hit(x, y) == Some(self.page_box) {
                        self.page_pressed = self.link_at(x, y).map(str::to_string);
                    } else {
                        self.page_pressed = None;
                    }
                }
                Event::MouseUp { x, y } => {
                    if let Some(href) = self.page_pressed.take() {
                        if self.ui.hit(x, y) == Some(self.page_box)
                            && self.link_at(x, y) == Some(href.as_str())
                        {
                            let target = match self.current_url.as_deref() {
                                Some(base) => resolve_url(base, &href).unwrap_or(href),
                                None => href,
                            };
                            self.navigate(&target);
                            return;
                        }
                    }
                }
                _ => {}
            }
        }

        self.ui.layout(self.width, self.height);
        self.render_frame();
    }

    /// Return the active URL history stack (current URL last).
    pub fn history(&self) -> &[String] {
        &self.history
    }

    /// Return the current URL, if any.
    pub fn url(&self) -> Option<&str> {
        self.current_url.as_deref()
    }

    /// Return the current status line text.
    pub fn status(&self) -> &str {
        &self.status
    }

    /// Return the current parsed DOM document.
    pub fn doc(&self) -> &Document {
        &self.doc
    }

    /// Return the current page layout.
    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    /// Return the script log from the most recent page load.
    pub fn script_log(&self) -> &ScriptLog {
        &self.script_log
    }

    /// Return the `href` of a link at canvas coordinates `(x, y)`, if any.
    pub fn link_at(&self, x: i32, y: i32) -> Option<&str> {
        let pr = self.ui.rect(self.page_box)?;
        if !rect_contains(pr, x, y) {
            return None;
        }
        let px = x.saturating_sub(pr.x);
        let py = y.saturating_sub(pr.y);
        for run in self.layout.runs.iter().rev() {
            let rx = i64::from(run.rect.x);
            let ry = i64::from(run.rect.y);
            let rw = i64::from(run.rect.w.max(0));
            let rh = i64::from(run.rect.h.max(0));
            let px64 = i64::from(px);
            let py64 = i64::from(py);
            if px64 >= rx && px64 < rx + rw && py64 >= ry && py64 < ry + rh {
                if let Some(href) = find_link_href(&self.doc, run.node) {
                    return Some(href);
                }
            }
        }
        None
    }

    /// Return clickable link rectangles (in canvas coordinates) paired with their `href`.
    pub fn links(&self) -> Vec<(Rect, String)> {
        let Some(pr) = self.ui.rect(self.page_box) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for run in &self.layout.runs {
            if run.rect.w <= 0 || run.rect.h <= 0 {
                continue;
            }
            if let Some(href) = find_link_href(&self.doc, run.node) {
                out.push((
                    Rect {
                        x: pr.x.saturating_add(run.rect.x),
                        y: pr.y.saturating_add(run.rect.y),
                        w: run.rect.w as u32,
                        h: run.rect.h as u32,
                    },
                    href.to_string(),
                ));
            }
        }
        out
    }

    /// Return the Back button rectangle in canvas coordinates.
    pub fn back_rect(&self) -> Option<Rect> {
        self.ui.rect(self.back_btn)
    }

    /// Return the Reload button rectangle in canvas coordinates.
    pub fn reload_rect(&self) -> Option<Rect> {
        self.ui.rect(self.reload_btn)
    }

    /// Return the address field rectangle in canvas coordinates.
    pub fn address_rect(&self) -> Option<Rect> {
        self.ui.rect(self.addr_field)
    }

    /// Return the page viewport rectangle in canvas coordinates.
    pub fn page_rect(&self) -> Option<Rect> {
        self.ui.rect(self.page_box)
    }

    fn load_url(&mut self, url: &str, push_history: bool) {
        self.page_pressed = None;
        let (html_src, display_url, status) = match fetch::get_with_final_url(url, &self.limits) {
            Ok((resp, final_url)) => {
                let reason = if resp.reason.is_empty() {
                    "OK"
                } else {
                    resp.reason.as_str()
                };
                let status = format!("{} {reason}", resp.status);
                let body = String::from_utf8_lossy(&resp.body).into_owned();
                (body, final_url, status)
            }
            Err(err) => {
                let escaped = escape_html(&err.to_string());
                let body = format!(
                    "<!doctype html><html><body style=\"background-color: #fff; color: #222; padding: 16px\">\
                     <h1 style=\"color: #b00020\">Load failed</h1>\
                     <p>{escaped}</p></body></html>"
                );
                let status = format!("Error: {err}");
                (body, url.to_string(), status)
            }
        };

        if push_history {
            self.history.push(display_url.clone());
        } else if let Some(last) = self.history.last_mut() {
            *last = display_url.clone();
        }
        self.current_url = Some(display_url.clone());
        self.status = status;
        self.ui.set_text(self.addr_field, &display_url);
        self.ui.set_text(self.status_label, &self.status);
        self.ui.layout(self.width, self.height);

        let mut doc = html::parse(&html_src);
        let log = script::run_page_scripts(&mut doc, self.fuel);
        let styles = style::compute(&doc);
        let page_w = page_width(&self.ui, self.page_box);
        let layout = layout::layout(&doc, &styles, page_w);

        self.doc = doc;
        self.styles = styles;
        self.layout = layout;
        self.script_log = log;
        self.render_frame();
    }

    fn render_frame(&mut self) {
        let mut frame = Canvas::new(self.width, self.height, Rgba(255, 255, 255, 255));
        self.ui.render(&mut frame);
        if let Some(pr) = self.ui.rect(self.page_box) {
            if pr.w > 0 && pr.h > 0 {
                let mut page_canvas = Canvas::new(pr.w, pr.h, Rgba(255, 255, 255, 255));
                paint::paint(&self.doc, &self.styles, &self.layout, &mut page_canvas);
                frame.draw(&page_canvas, pr.x, pr.y);
            }
        }
        self.canvas = frame;
    }
}

fn page_width(ui: &Ui, page_box: NodeId) -> i32 {
    ui.rect(page_box)
        .map_or(0, |r| i32::try_from(r.w).unwrap_or(i32::MAX))
}

fn rect_contains(rect: Rect, x: i32, y: i32) -> bool {
    let px = i64::from(x);
    let py = i64::from(y);
    let rx = i64::from(rect.x);
    let ry = i64::from(rect.y);
    px >= rx
        && px < rx.saturating_add(i64::from(rect.w))
        && py >= ry
        && py < ry.saturating_add(i64::from(rect.h))
}

fn find_link_href(doc: &Document, start: DomNodeId) -> Option<&str> {
    let mut cur = Some(start);
    let mut steps = 0usize;
    let limit = doc.nodes.len();
    while let Some(id) = cur {
        if steps > limit {
            break;
        }
        steps += 1;
        if doc.name(id) == Some("a") {
            if let Some(href) = doc.attr(id, "href") {
                return Some(href);
            }
        }
        cur = doc.nodes.get(id).and_then(|n| n.parent);
    }
    None
}

fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
    out
}

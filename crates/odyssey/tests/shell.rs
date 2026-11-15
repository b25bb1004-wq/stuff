//! Integration tests for `odyssey::shell`.
#![allow(clippy::unwrap_used)]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use assay::{forall, Rng};
use indium::{Event, Key};
use odyssey::shell::Browser;
use pigment::Canvas;

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0100_0000_01b3;

fn fnv_canvas(c: &Canvas) -> u64 {
    let mut h = FNV_OFFSET;
    for b in c.width.to_le_bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(FNV_PRIME);
    }
    for b in c.height.to_le_bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(FNV_PRIME);
    }
    for p in &c.pixels {
        for b in [p.0, p.1, p.2, p.3] {
            h ^= u64::from(b);
            h = h.wrapping_mul(FNV_PRIME);
        }
    }
    h
}

fn click(b: &mut Browser, x: i32, y: i32) {
    b.event(Event::MouseMove { x, y });
    b.event(Event::MouseDown { x, y });
    b.event(Event::MouseUp { x, y });
}

fn send_http(stream: &mut std::net::TcpStream, body: &str) {
    let resp = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(resp.as_bytes());
}

#[test]
fn shell_navigate_click_link_back_and_reload() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let addr = listener.local_addr().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let index_hits = Arc::new(AtomicUsize::new(0));
    let stop_srv = Arc::clone(&stop);
    let hits_srv = Arc::clone(&index_hits);

    let handle = thread::spawn(move || {
        let mut buf = [0u8; 2048];
        while !stop_srv.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
                    let n = stream.read(&mut buf).unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);
                    let path = req
                        .lines()
                        .next()
                        .and_then(|l| l.split_whitespace().nth(1))
                        .unwrap_or("/");
                    if path == "/sub/page2.html" {
                        send_http(
                            &mut stream,
                            "<!doctype html><html><body style=\"margin:0;background-color:#dceeff\"><p id=\"p2\" style=\"color:#002244\">Initial</p><script type=\"text/logos\">\nfn p_set(id: str, s: str) -> bool = extern \"page.set_text\"\nfn main() {\n    let _ = p_set(\"p2\", \"PageTwo\")\n}\n</script></body></html>",
                        );
                    } else {
                        let count = hits_srv.fetch_add(1, Ordering::Relaxed) + 1;
                        let body = format!(
                            r#"<!doctype html><html><body style="margin:0;background-color:#ffffff"><p style="color:#111111">Hits:{count} <a href="sub/page2.html" style="color:#0000ff">Next</a></p></body></html>"#
                        );
                        send_http(&mut stream, &body);
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(_) => break,
            }
        }
    });

    let url1 = format!("http://{addr}/index.html");
    let url2 = format!("http://{addr}/sub/page2.html");

    let mut browser = Browser::new(480, 320);

    // 1. Navigate to page 1
    browser.navigate(&url1);
    assert_eq!(browser.history(), std::slice::from_ref(&url1));
    assert_eq!(browser.url(), Some(url1.as_str()));
    assert_eq!(browser.status(), "200 OK");
    let h1 = fnv_canvas(browser.frame());
    assert_ne!(h1, 0);

    // 2. Click link to relative sub/page2.html
    let links = browser.links();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].1, "sub/page2.html");
    let link_rect = links[0].0;
    click(
        &mut browser,
        link_rect.x + (link_rect.w as i32) / 2,
        link_rect.y + (link_rect.h as i32) / 2,
    );
    assert_eq!(browser.history(), &[url1.clone(), url2.clone()]);
    assert_eq!(browser.url(), Some(url2.as_str()));
    assert!(
        browser.script_log().is_empty(),
        "unexpected script log: {}",
        browser.script_log()
    );
    assert!(browser.layout().runs.iter().any(|r| r.text == "PageTwo"));
    let h2 = fnv_canvas(browser.frame());
    assert_ne!(h2, h1);

    // 3. Click Back button
    let back_rect = browser.back_rect().unwrap();
    click(
        &mut browser,
        back_rect.x + (back_rect.w as i32) / 2,
        back_rect.y + (back_rect.h as i32) / 2,
    );
    assert_eq!(browser.history(), std::slice::from_ref(&url1));
    assert_eq!(browser.url(), Some(url1.as_str()));
    assert!(browser.layout().runs.iter().any(|r| r.text == "Hits:2"));
    let h3 = fnv_canvas(browser.frame());
    assert_ne!(h3, h2);

    // 4. Click Reload button
    let reload_rect = browser.reload_rect().unwrap();
    click(
        &mut browser,
        reload_rect.x + (reload_rect.w as i32) / 2,
        reload_rect.y + (reload_rect.h as i32) / 2,
    );
    assert_eq!(browser.history(), &[url1]);
    assert!(browser.layout().runs.iter().any(|r| r.text == "Hits:3"));
    let h4 = fnv_canvas(browser.frame());
    assert_ne!(h4, h3);
    assert_eq!(index_hits.load(Ordering::Relaxed), 3);

    stop.store(true, Ordering::Relaxed);
    handle.join().unwrap();
}

#[test]
fn shell_failed_fetch_renders_error_page() {
    let mut browser = Browser::new(400, 260);
    browser.navigate("https://127.0.0.1/blocked");
    assert!(browser.status().starts_with("Error:"));
    assert!(browser.layout().runs.iter().any(|r| r.text == "Load"));
    assert_eq!(browser.history(), &["https://127.0.0.1/blocked"]);

    browser.navigate("not-a-valid-url");
    assert!(browser.status().starts_with("Error:"));
    assert!(browser.back());
    assert_eq!(browser.history(), &["https://127.0.0.1/blocked"]);
}

#[test]
fn shell_random_clicks_and_keys_never_panic() {
    let keys = [
        Key::Tab,
        Key::ShiftTab,
        Key::Enter,
        Key::Space,
        Key::Backspace,
        Key::Escape,
    ];
    forall(25, 0xdead_beef_c0de_u64, |rng: &mut Rng| {
        let w = (100 + rng.below(500)) as u32;
        let h = (80 + rng.below(350)) as u32;
        let mut b = Browser::new(w, h);
        for _ in 0..20 {
            let x = (rng.below((w + 80) as usize) as i32) - 40;
            let y = (rng.below((h + 80) as usize) as i32) - 40;
            match rng.below(6) {
                0 => b.event(Event::MouseMove { x, y }),
                1 => b.event(Event::MouseDown { x, y }),
                2 => b.event(Event::MouseUp { x, y }),
                3 => {
                    let k = keys[rng.below(keys.len())];
                    b.event(Event::Key(k));
                }
                4 => {
                    let ch = char::from(b'a' + (rng.below(26) as u8));
                    b.event(Event::Text(ch));
                }
                _ => {
                    let _ = b.back();
                }
            }
        }
        assert_eq!(b.frame().width, w);
        assert_eq!(b.frame().height, h);
    });
}

//! the editor's own tests: each one pins a rule the screen must keep, one file per topic.

use super::*;
use std::io::{Read, Write};
use std::net::TcpListener;

/// the frame, row by row: the plain text helper loses the line breaks, and a
/// layout bug is exactly a question of which row something lands on.
fn render_rows(ed: &mut Editor, w: u16, h: u16) -> Vec<String> {
    let backend = ratatui::backend::TestBackend::new(w, h);
    let mut term = ratatui::Terminal::new(backend).unwrap();
    term.draw(|f| draw(f, ed)).unwrap();
    let buf = term.backend().buffer();
    (0..h)
        .map(|y| (0..w).map(|x| buf[(x, y)].symbol()).collect())
        .collect()
}

fn render_text(ed: &mut Editor, w: u16, h: u16) -> String {
    let backend = ratatui::backend::TestBackend::new(w, h);
    let mut term = ratatui::Terminal::new(backend).unwrap();
    term.draw(|f| draw(f, ed)).unwrap();
    term.backend()
        .buffer()
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect()
}

/// a unique scratch directory per call. tests run in parallel inside one
/// process: a name built from the pid alone gets trampled, and one test's
/// cleanup then deletes another test's fixture mid-run.
fn scratch(tag: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "nana-{tag}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

mod agent_colour;
mod agent_input;
mod agent_pane;
mod agent_view;
mod bootcamp;
mod busy_look;
mod completion;
mod diag;
mod float;
mod glitch_safety;
mod hub_hover;
mod hub_is_the_start;
mod hub_takes_the_screen;
mod hub_truncation;
mod hub_ui;
mod human_inference;
mod layout;
mod live_pace;
mod live_sync;
mod live_write;
mod pair;
mod paste;
mod persona;
mod robustness;
mod rounded_bars;
mod rounded_everywhere;
mod sober_badge;
mod status_badge_follows_the_agent;
mod statusbar_background;
mod tab;
mod terminal_look;

/// waits for one client, but gives up after `limit`: a test whose request never
/// comes must fail in seconds, not hang the whole suite. Returns none on timeout.
fn accept_within(
    listener: &TcpListener,
    limit: std::time::Duration,
) -> Option<std::net::TcpStream> {
    listener.set_nonblocking(true).ok()?;
    let deadline = std::time::Instant::now() + limit;
    loop {
        match listener.accept() {
            Ok((sock, _)) => {
                sock.set_nonblocking(false).ok()?;
                return Some(sock);
            }
            Err(_) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(_) => return None,
        }
    }
}

/// a fake openai-compatible server: answers once with `answer` and hands back the request it got.
fn mock(answer: &'static str) -> (String, std::thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = std::thread::spawn(move || {
        let Some(mut sock) = accept_within(&listener, std::time::Duration::from_secs(5)) else {
            return String::new();
        };
        sock.set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .ok();
        let mut buf: Vec<u8> = Vec::new();
        let mut chunk = [0u8; 8192];
        let mut head: Option<usize> = None;
        let mut want = 0usize;
        loop {
            if let Some(h) = head {
                if buf.len() >= h + want {
                    break;
                }
            }
            match sock.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
                Err(_) => break,
            }
            if head.is_none() {
                if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    head = Some(i + 4);
                    let h = String::from_utf8_lossy(&buf[..i]).to_ascii_lowercase();
                    want = h
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .and_then(|v| v.trim().parse().ok())
                        .unwrap_or(0);
                }
            }
        }
        let req = String::from_utf8_lossy(&buf).to_string();
        let payload = format!(
            "{{\"choices\":[{{\"message\":{{\"content\":{}}}}}]}}",
            serde_json::to_string(answer).unwrap()
        );
        let resp = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            payload.len(),
            payload
        );
        let _ = sock.write_all(resp.as_bytes());
        req
    });
    (format!("http://{addr}"), handle)
}

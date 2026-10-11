//! persona tests of the editor screen.

use super::*;

use std::io::{Read, Write};
use std::net::TcpListener;

/// a project with a persona chosen, and a provider that records what it
/// was asked.
fn project_with_persona(tag: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let d = std::env::temp_dir().join(format!(
        "nana-persona-{tag}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join(".nana/personas")).unwrap();
    std::fs::write(
        d.join(".nana/personas/lead.md"),
        "---\ntitle: The Lead\ndescription: d\n---\nANSWER ONLY IN LATIN.\n",
    )
    .unwrap();
    let mut s = crate::settings::Settings::load(&d);
    s.persona = Some("lead".into());
    s.save_project(&d).unwrap();
    d
}

fn mock(answer: &'static str) -> (String, std::thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = std::thread::spawn(move || {
        let (mut sock, _) = listener.accept().unwrap();
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

/// the persona chosen in the project frames the agent panel too — not only
/// the employees of the company.
#[test]
fn the_agent_panel_runs_under_the_project_persona() {
    let d = project_with_persona("panel");
    let (url, server) = mock("salve");
    // point the project at the mock, so the run reaches our server
    let mut s = crate::settings::Settings::load(&d);
    s.model = Some("mock".into());
    s.provider = Some("openai-compatible".into());
    s.base_url = Some(url.clone());
    s.save_project(&d).unwrap();

    let mut ed = Editor::open(None).unwrap();
    ed.file = Some(d.join("x.py"));
    std::fs::write(d.join("x.py"), "print(1)\n").unwrap();
    ed.on_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));
    let pane = ed.agent_pane.as_ref().unwrap();
    assert_eq!(pane.persona.as_deref(), Some("lead"), "the card shows it");
    // ask something, and let the worker talk to the mock
    ed.agent_pane.as_mut().unwrap().input = "say hi".into();
    ed.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
    let req = server.join().unwrap();
    assert!(
        req.contains("ANSWER ONLY IN LATIN"),
        "the persona reached the model: {}",
        &req[..req.len().min(600)]
    );
    let _ = std::fs::remove_dir_all(&d);
}

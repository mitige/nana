//! persona tests of the editor screen.

use super::*;

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

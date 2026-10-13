//! float tests of the editor screen.

use super::*;

/// a card has a top edge: its corners sit on one row, with no title in it.
fn has_card_top(text: &str) -> bool {
    text.lines().any(|r| r.contains('╭') && r.contains('╮'))
}

/// the ide stays on screen while the agent works: the explorer and the
/// editor are drawn, and the agent card is a band over them, not a clean
/// screen with a card floating in it.
#[test]
fn the_ide_stays_on_screen_while_the_agent_works() {
    let mut ed = Editor::open(None).unwrap();
    ed.lines = (0..20)
        .map(|i| format!("VISIBLE_LINE_{i}_STAYS_ON_SCREEN"))
        .collect();
    ed.file = Some(std::path::PathBuf::from("x.py"));
    ed.ext = "py".into();
    ed.modified = true;
    ed.agent_pane = Some(AgentPane::new());
    let text = render_text(&mut ed, 160, 40);
    assert!(has_card_top(&text), "the card is drawn:\n{text}");
    assert!(
        text.contains("VISIBLE_LINE_0_STAYS_ON_SCREEN"),
        "the ide is drawn while the agent works:\n{text}"
    );
}

/// the agent's write plays in a second view behind the card: the lines it
/// writes are on screen, outside the card, and the card stays clean.
#[test]
fn the_turn_end_saves_the_opening_and_the_last_reply() {
    let root = std::env::temp_dir().join(format!("nana-turn-end-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".nana")).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let mut pane = AgentPane::new();
    pane.rx = Some(rx);
    pane.root = root.clone();
    pane.session = "auto".into();
    pane.opening = Some("first ask".into());
    tx.send(crate::agent::Event::Text("first answer".into()))
        .unwrap();
    tx.send(crate::agent::Event::Text("last answer".into()))
        .unwrap();
    tx.send(crate::agent::Event::Finished { steps: 2 }).unwrap();
    pane.poll();
    let saved = crate::memory::Memory::open(&root)
        .read(crate::memory::Class::Project, "conversation-auto")
        .unwrap_or_default();
    assert!(saved.contains("first ask"), "{saved}");
    assert!(
        saved.contains("last answer"),
        "the last reply is kept: {saved}"
    );
    assert!(
        !saved.contains("first answer"),
        "only the last reply is kept: {saved}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn ctrl_y_keeps_the_conversation_so_far_in_memory() {
    let root = std::env::temp_dir().join(format!("nana-ctrl-y-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".nana")).unwrap();
    let mut pane = AgentPane::new();
    pane.root = root.clone();
    pane.session = "manual".into();
    pane.opening = Some("build the parser".into());
    pane.closing = Some("parser done".into());
    pane.save_turn_now();
    let saved = crate::memory::Memory::open(&root)
        .read(crate::memory::Class::Project, "conversation-manual")
        .unwrap_or_default();
    assert!(saved.contains("build the parser"), "{saved}");
    assert!(saved.contains("parser done"), "{saved}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_agent_write_plays_in_a_view_behind_the_card() {
    let mut ed = Editor::open(None).unwrap();
    let mut pane = AgentPane::new();
    pane.writes.push(LiveWrite {
        path: PathBuf::from("src/lib.rs"),
        text: vec!["fn behind_the_card() {}".into()],
    });
    ed.agent_pane = Some(pane);
    ed.poll_agent();
    std::thread::sleep(VIEW_SWEEP);
    let text = render_text(&mut ed, 160, 40);
    assert!(has_card_top(&text), "the card is drawn:\n{text}");
    assert!(
        text.contains("fn behind_the_card"),
        "the write plays behind the card:\n{text}"
    );
}

/// the search is a window on its own too: the editor behind it must not
/// show anywhere on the screen, the same as the agent menu and the hub.
#[test]
fn the_search_shows_nothing_of_the_ide_around_it() {
    let mut ed = Editor::open(None).unwrap();
    ed.lines = (0..20)
        .map(|i| format!("SECRET_LINE_{i}_SHOULD_NOT_SHOW_THROUGH"))
        .collect();
    ed.file = Some(std::path::PathBuf::from("x.py"));
    ed.ext = "py".into();
    ed.focus = Focus::Search;
    ed.search = Some(FileSearch::new(std::path::PathBuf::from(".")));
    let text = render_text(&mut ed, 100, 30);
    assert!(text.contains("❯"), "the search prompt is drawn");
    assert!(
        !text.contains("SECRET_LINE"),
        "the ide shows around the search:\n{text}"
    );
}

/// the agent card floats over the ide, so the user sees the code the agent
/// touches as a human would: the ide stays visible, no trail lane is drawn.
#[test]
fn the_agent_menu_has_no_trail_under_its_card() {
    let mut ed = Editor::open(None).unwrap();
    ed.lines = (0..20)
        .map(|i| format!("SECRET_LINE_{i}_STAYS_VISIBLE"))
        .collect();
    ed.file = Some(std::path::PathBuf::from("x.py"));
    ed.ext = "py".into();
    let mut pane = AgentPane::new();
    pane.trail.push(crate::trail::Step {
        lane: crate::trail::Lane::Tool,
        label: "read_file src/main.rs".into(),
        at: 0.0,
        ok: true,
    });
    pane.trail.push(crate::trail::Step {
        lane: crate::trail::Lane::Result,
        label: "write_file: done".into(),
        at: 2.0,
        ok: true,
    });
    ed.agent_pane = Some(pane);
    let text = render_text(&mut ed, 160, 40);
    assert!(has_card_top(&text), "the card is drawn");
    assert!(
        text.contains("SECRET_LINE_0_STAYS_VISIBLE"),
        "the ide is drawn behind the card:\n{text}"
    );
    assert!(
        !text.contains("tool "),
        "no trail lane is drawn under the card:\n{text}"
    );
    assert!(
        !text.contains("read_file src/main.rs"),
        "the trail actions are not listed under the card:\n{text}"
    );
}

//! live write tests of the editor screen: the agent's view, beside the user's
//! buffer, never written by the agent and never behind it.

use super::*;

/// the agent's write lands whole in the agent's own view, at once, beside the
/// user's buffer. the user's buffer is never written by it.
#[test]
fn a_write_lands_whole_beside_the_user_buffer() {
    let d = scratch("live");
    let target = d.join("new.rs");
    let mut ed = Editor::open(None).unwrap();
    ed.file = Some(d.join("mine.rs"));
    ed.lines = vec!["my own line".into()];
    let mut pane = AgentPane::new();
    pane.writes.push(LiveWrite {
        path: target.clone(),
        text: vec![
            "fn one() {}".into(),
            "fn two() {}".into(),
            "fn three() {}".into(),
        ],
    });
    ed.agent_pane = Some(pane);
    ed.poll_agent();
    std::thread::sleep(VIEW_SWEEP);
    let text = render_text(&mut ed, 160, 40);
    assert!(
        text.contains("fn three() {}"),
        "the file is whole on screen at the first poll"
    );
    assert_eq!(
        ed.lines,
        vec!["my own line".to_string()],
        "the user's buffer is never written by the agent"
    );
    assert!(!ed.modified, "the user's buffer is not marked as edited");
    let _ = std::fs::remove_dir_all(&d);
}

/// the user's text and the agent's text are both on screen at once: the
/// agent column sits beside the editor, neither one overrides the other.
#[test]
fn the_agent_text_is_on_screen_and_the_user_text_is_not() {
    let d = scratch("live-side");
    let mut ed = Editor::open(None).unwrap();
    ed.file = Some(d.join("mine.rs"));
    ed.lines = vec!["USER_TEXT_STAYS".into()];
    let mut pane = AgentPane::new();
    pane.writes.push(LiveWrite {
        path: d.join("new.rs"),
        text: vec!["AGENT_TEXT_SHOWS".into()],
    });
    ed.agent_pane = Some(pane);
    ed.poll_agent();
    std::thread::sleep(VIEW_SWEEP);
    let rows = render_rows(&mut ed, 160, 40);
    assert!(
        rows.iter()
            .any(|r| r.contains("USER_TEXT_STAYS") || r.contains("AGENT_TEXT_SHOWS")),
        "{rows:?}"
    );
    let all = rows.join("\n");
    assert!(
        all.contains("AGENT_TEXT_SHOWS"),
        "the agent's write plays in the background view:\n{all}"
    );
    assert!(
        !all.contains("USER_TEXT_STAYS"),
        "the user's buffer is not drawn behind the card:\n{all}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// a read is shown the same way, whole at once, and it changes nothing: the
/// file is not saved again and the buffer is not marked as edited.
#[test]
fn a_read_is_shown_whole_and_changes_nothing() {
    let d = scratch("live-read-show");
    let target = d.join("notes.txt");
    let mut ed = Editor::open(None).unwrap();
    ed.file = Some(d.join("other.txt"));
    ed.lines = vec![String::new()];
    let mut pane = AgentPane::new();
    pane.writes.push(LiveWrite {
        path: target.clone(),
        text: vec!["alpha".into(), "beta".into()],
    });
    ed.agent_pane = Some(pane);
    ed.poll_agent();
    assert_eq!(
        ed.agent_view.lines,
        vec!["alpha".to_string(), "beta".to_string()],
        "the read appears whole in the agent view"
    );
    assert_eq!(
        ed.lines,
        vec![String::new()],
        "the user's buffer is untouched by the read"
    );
    assert!(!ed.modified, "a read is not an edit");
    assert_ne!(
        ed.saved, ed.agent_view.lines,
        "a read does not write its text as the saved state"
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// the user's unsaved work is never replaced by something the agent read.
#[test]
fn a_read_does_not_replace_unsaved_work() {
    let d = scratch("live-read-guard");
    let mut ed = Editor::open(None).unwrap();
    ed.file = Some(d.join("mine.txt"));
    ed.lines = vec!["my unsaved line".into()];
    ed.modified = true;
    let mut pane = AgentPane::new();
    pane.writes.push(LiveWrite {
        path: d.join("theirs.txt"),
        text: vec!["something else".into()],
    });
    ed.agent_pane = Some(pane);
    ed.poll_agent();
    assert_eq!(
        ed.lines,
        vec!["my unsaved line".to_string()],
        "the buffer is untouched"
    );
    assert!(ed.modified, "the edit is still marked as unsaved");
    assert_eq!(
        ed.agent_view.lines,
        vec!["something else".to_string()],
        "the agent sees its own read, the user keeps their work"
    );
    let _ = std::fs::remove_dir_all(&d);
}

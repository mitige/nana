//! live sync tests of the editor screen: the agent's view can never fall
//! behind the agent.
//!
//! the rule these tests hold is the one the user asked for: a write is shown
//! whole the moment it arrives, so there is no queue, no duration and no way
//! for the view to accumulate a lag. every test here would fail on the old
//! playback, which replayed each write over the time the agent took to make it.

use super::*;

fn write_of(path: &str, text: Vec<String>) -> LiveWrite {
    LiveWrite {
        path: PathBuf::from(path),
        text,
    }
}

/// a write is on screen in full as soon as the agent's event is polled: one
/// poll, no frames in between.
#[test]
fn a_write_is_on_screen_in_full_at_the_first_poll() {
    let mut ed = Editor::open(None).unwrap();
    let mut pane = AgentPane::new();
    pane.writes
        .push(write_of("a.rs", vec!["alpha".into(), "beta".into()]));
    ed.agent_pane = Some(pane);
    ed.poll_agent();
    assert_eq!(ed.agent_view.lines, vec!["alpha", "beta"]);
}

/// a flood does not queue: the screen shows the last write at once, and the
/// number of frames it took is zero, because nothing is left to play.
#[test]
fn a_flood_never_queues_and_the_screen_ends_on_the_last_write() {
    let mut ed = Editor::open(None).unwrap();
    let mut pane = AgentPane::new();
    for i in 0..40 {
        pane.writes.push(write_of(
            &format!("f{i}.rs"),
            vec![format!("fn f{i}() {{}}")],
        ));
    }
    ed.agent_pane = Some(pane);
    ed.poll_agent();
    assert_eq!(
        ed.agent_view.lines,
        vec!["fn f39() {}".to_string()],
        "the view shows the last write"
    );
}

/// the writes the pane handed over are consumed by the poll: nothing is held
/// back for a later frame.
#[test]
fn no_write_is_held_back_for_a_later_frame() {
    let mut ed = Editor::open(None).unwrap();
    let mut pane = AgentPane::new();
    for i in 0..5 {
        pane.writes.push(write_of(
            &format!("f{i}.rs"),
            vec![format!("fn f{i}() {{}}")],
        ));
    }
    ed.agent_pane = Some(pane);
    ed.poll_agent();
    assert!(
        ed.agent_pane.as_ref().unwrap().writes.is_empty(),
        "a write was left waiting in the pane"
    );
}

/// however long the agent took to reach a write, nothing about the view waits:
/// a second poll with no new event changes nothing and shows nothing new.
#[test]
fn the_view_cannot_trail_behind_between_polls() {
    let mut ed = Editor::open(None).unwrap();
    let mut pane = AgentPane::new();
    pane.writes.push(write_of("a.rs", vec!["x".repeat(400)]));
    ed.agent_pane = Some(pane);
    ed.poll_agent();
    let shown = ed.agent_view.lines.clone();
    for _ in 0..100 {
        ed.poll_agent();
    }
    assert_eq!(ed.agent_view.lines, shown, "the view moved on its own");
}

/// the file of the write is named in the view, so the card says which file the
/// agent is working on even when the write is not shown line by line.
#[test]
fn the_view_names_the_file_of_the_write() {
    let mut ed = Editor::open(None).unwrap();
    let mut pane = AgentPane::new();
    pane.writes
        .push(write_of("src/pool.rs", vec!["fn one() {}".into()]));
    ed.agent_pane = Some(pane);
    ed.poll_agent();
    assert_eq!(
        ed.agent_view.path.as_deref(),
        Some(std::path::Path::new("src/pool.rs"))
    );
}

/// the text the agent wrote is never cut, and the cursor sits at its end, so
/// what the user reads is what the agent wrote.
#[test]
fn the_write_lands_in_full_with_the_cursor_at_its_end() {
    let mut ed = Editor::open(None).unwrap();
    let mut pane = AgentPane::new();
    let long = "gamma".repeat(60);
    pane.writes
        .push(write_of("b.rs", vec!["alpha".into(), long.clone()]));
    ed.agent_pane = Some(pane);
    ed.poll_agent();
    assert_eq!(ed.agent_view.lines, vec!["alpha", &long]);
    assert_eq!(ed.agent_view.cy, 1, "the cursor is on the last line");
    assert_eq!(ed.agent_view.cx, long.chars().count());
}

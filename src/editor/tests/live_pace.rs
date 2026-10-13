//! the pace of the agent's view: there is none to fall behind.
//!
//! the old pacer replayed a write at the speed of the clock, one step per
//! tick, which is exactly how the view accumulated a lag: the agent produced
//! writes faster than the animation drained them. these tests hold the rule
//! that replaced it — a write is shown whole the moment it arrives, so no
//! amount of time, and no wake-up of the draw loop, can put the view behind.

use super::*;

/// the draw loop wakes on every key and mouse move, not only every tick. no
/// number of wake-ups may change what the view shows.
#[test]
fn no_wake_up_of_the_loop_advances_the_view() {
    let mut ed = Editor::open(None).unwrap();
    let mut pane = AgentPane::new();
    pane.writes.push(LiveWrite {
        path: PathBuf::from("a.rs"),
        text: vec!["fn one() {}".into()],
    });
    ed.agent_pane = Some(pane);
    ed.poll_agent();
    let shown = ed.agent_view.lines.clone();
    for _ in 0..500 {
        ed.poll_agent();
    }
    assert_eq!(ed.agent_view.lines, shown, "a wake-up moved the view");
}

/// time passing does not change the view either: there is no pending step to
/// become due, because nothing is pending at all.
#[test]
fn time_passing_does_not_change_what_the_view_shows() {
    let mut ed = Editor::open(None).unwrap();
    let mut pane = AgentPane::new();
    pane.writes.push(LiveWrite {
        path: PathBuf::from("a.rs"),
        text: vec!["x".repeat(200)],
    });
    ed.agent_pane = Some(pane);
    ed.poll_agent();
    let shown = ed.agent_view.lines.clone();
    std::thread::sleep(Duration::from_millis(120));
    assert_eq!(ed.agent_view.lines, shown, "the view moved with the clock");
}

/// a long write is never accelerated and never truncated: the user reads the
/// whole text as soon as it arrives, not a first line then the rest later.
#[test]
fn a_long_write_is_whole_at_once_and_is_never_cut() {
    let mut ed = Editor::open(None).unwrap();
    let lines: Vec<String> = (0..300).map(|i| format!("line {i}")).collect();
    let mut pane = AgentPane::new();
    pane.writes.push(LiveWrite {
        path: PathBuf::from("big.rs"),
        text: lines.clone(),
    });
    ed.agent_pane = Some(pane);
    ed.poll_agent();
    assert_eq!(ed.agent_view.lines, lines, "the write is whole at once");
}

/// the number of writes the agent made never matters: the view is up to date
/// after one poll, whatever the flood.
#[test]
fn a_flood_of_writes_is_up_to_date_after_one_poll() {
    let mut ed = Editor::open(None).unwrap();
    let mut pane = AgentPane::new();
    for i in 0..200 {
        pane.writes.push(LiveWrite {
            path: PathBuf::from(format!("f{i}.rs")),
            text: vec![format!("fn f{i}() {{}}")],
        });
    }
    ed.agent_pane = Some(pane);
    ed.poll_agent();
    assert_eq!(ed.agent_view.lines, vec!["fn f199() {}".to_string()]);
}

//! live sync tests of the editor screen.

use super::*;

fn write_of(path: &str, text: Vec<String>, ticks: usize) -> LiveWrite {
    LiveWrite {
        path: PathBuf::from(path),
        text,
        shown: 0,
        col: 0,
        ticks,
        elapsed: 0,
    }
}

/// the queue is bounded, not the duration: four long writes queued at once keep
/// their own pace, none is dropped and none is shortened.
#[test]
fn queued_long_writes_keep_their_duration() {
    let mut ed = Editor::open(None).unwrap();
    let mut pane = AgentPane::new();
    for _ in 0..4 {
        pane.writes
            .push(write_of("src/lib.rs", vec!["x".repeat(400)], 2000));
    }
    ed.agent_pane = Some(pane);
    ed.poll_agent();
    assert_eq!(ed.live.len(), 4, "no write is dropped");
    assert!(
        ed.live.iter().all(|w| w.ticks == 2000),
        "a queued write was shortened"
    );
}

/// a flood of writes, each at the agent's own pace, is caught up: the queue is
/// bounded, and the screen ends on the last thing the agent did.
#[test]
fn a_flood_of_writes_is_caught_up_and_the_screen_ends_on_the_last_one() {
    let mut ed = Editor::open(None).unwrap();
    let mut pane = AgentPane::new();
    for i in 0..40 {
        pane.writes.push(write_of(
            &format!("f{i}.rs"),
            vec![format!("fn f{i}() {{}}")],
            16,
        ));
    }
    ed.agent_pane = Some(pane);
    ed.poll_agent();
    let mut frames = 0;
    while !ed.live.is_empty() && frames < 100_000 {
        ed.play_live();
        frames += 1;
    }
    assert!(frames <= 400, "the flood took {frames} frames to show");
    assert_eq!(
        ed.agent_view.lines,
        vec!["fn f39() {}".to_string()],
        "the view shows the last write"
    );
}

/// the write on screen keeps its pace when a flood arrives behind it: its
/// duration is never rewritten mid-line, or the text would jump ahead.
#[test]
fn the_write_on_screen_keeps_its_pace_when_a_flood_arrives() {
    let mut ed = Editor::open(None).unwrap();
    let mut pane = AgentPane::new();
    pane.writes.push(write_of("a.rs", vec!["x".repeat(50)], 50));
    ed.agent_pane = Some(pane);
    ed.poll_agent();
    for _ in 0..10 {
        ed.play_live();
    }
    let mut flood = AgentPane::new();
    for _ in 0..4 {
        flood
            .writes
            .push(write_of("b.rs", vec!["y".repeat(400)], 2000));
    }
    ed.agent_pane = Some(flood);
    ed.poll_agent();
    assert_eq!(ed.live[0].ticks, 50, "the write on screen was rewritten");
}

/// a write that is alone in the queue keeps its duration, however long it is.
#[test]
fn a_lone_long_write_is_never_accelerated() {
    let mut ed = Editor::open(None).unwrap();
    let mut pane = AgentPane::new();
    pane.writes
        .push(write_of("a.rs", vec!["x".repeat(400)], 2000));
    ed.agent_pane = Some(pane);
    ed.poll_agent();
    assert_eq!(ed.live[0].ticks, 2000, "a lone write was accelerated");
}

/// queued writes still land in full: the last one written is the one left on
/// screen, and nothing is cut from the text the agent wrote.
#[test]
fn queued_writes_still_land_in_full() {
    let mut ed = Editor::open(None).unwrap();
    let mut pane = AgentPane::new();
    pane.writes
        .push(write_of("a.rs", vec!["alpha".into(), "beta".into()], 900));
    pane.writes
        .push(write_of("b.rs", vec!["gamma".repeat(60)], 900));
    ed.agent_pane = Some(pane);
    ed.poll_agent();
    let mut guard = 0;
    while !ed.live.is_empty() && guard < 100_000 {
        ed.play_live();
        guard += 1;
    }
    assert_eq!(ed.agent_view.lines, vec!["gamma".repeat(60)]);
}

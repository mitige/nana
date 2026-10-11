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

/// the playback must not trail the agent: four long writes queued at once
/// are compressed so their whole backlog fits in the lag budget (94 ticks).
#[test]
fn the_playback_never_trails_the_agent_by_more_than_its_lag_budget() {
    let mut ed = Editor::open(None).unwrap();
    let mut pane = AgentPane::new();
    for _ in 0..4 {
        pane.writes
            .push(write_of("src/lib.rs", vec!["x".repeat(400)], 2000));
    }
    ed.agent_pane = Some(pane);
    ed.poll_agent();
    let backlog: usize = ed
        .live
        .iter()
        .map(|w| w.ticks.saturating_sub(w.elapsed))
        .sum();
    assert!(backlog <= 94, "the playback trails by {backlog} ticks");
    assert_eq!(ed.live.len(), 4, "no write is dropped, only compressed");
}

/// a flood of writes, each at the agent's own pace, is caught up: the whole
/// backlog plays within the lag budget, and the screen ends on the last
/// thing the agent did.
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
    assert!(frames <= 94, "the flood took {frames} frames to show");
    assert_eq!(
        ed.agent_view.lines,
        vec!["fn f39() {}".to_string()],
        "the view shows the last write"
    );
}

/// a compressed write still lands in full: the lag rule shortens the
/// playback, it never cuts the text the agent wrote.
#[test]
fn a_compressed_write_still_lands_in_full() {
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

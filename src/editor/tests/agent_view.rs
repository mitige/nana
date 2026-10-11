//! agent view tests of the editor screen.

use super::*;

fn rows(ed: &mut Editor, w: u16, h: u16) -> Vec<String> {
    render_rows(ed, w, h)
}

/// the agent's view keeps its own bottom border on screen: nothing is
/// drawn over it now that the trail is gone.
#[test]
fn the_agent_view_keeps_its_bottom_border_above_the_trail() {
    let mut ed = Editor::open(None).unwrap();
    let mut pane = AgentPane::new();
    pane.writes.push(LiveWrite {
        path: PathBuf::from("src/lib.rs"),
        text: vec!["fn a() {}".into()],
        shown: 0,
        col: 0,
        ticks: 0,
        elapsed: 0,
    });
    ed.agent_pane = Some(pane);
    ed.poll_agent();
    ed.play_live();
    ed.play_live();
    let screen = rows(&mut ed, 120, 40);
    assert!(
        screen.iter().any(|r| r.starts_with('╰')),
        "the view has no bottom border on screen:\n{}",
        screen.join("\n")
    );
}

/// two shell commands in a row are both played: the second call must not
/// erase the first one before its answer comes back.
#[test]
fn a_shell_command_is_not_skipped_when_another_follows() {
    let mut ed = Editor::open(None).unwrap();
    let mut pane = AgentPane::new();
    let (tx, rx) = channel();
    pane.rx = Some(rx);
    tx.send(crate::agent::Event::ToolCall {
        name: "run_shell".into(),
        args: serde_json::json!({"command": "ls"}),
    })
    .unwrap();
    tx.send(crate::agent::Event::ToolCall {
        name: "run_shell".into(),
        args: serde_json::json!({"command": "cargo test"}),
    })
    .unwrap();
    tx.send(crate::agent::Event::ToolResult {
        name: "run_shell".into(),
        ok: true,
        text: "a\nb".into(),
    })
    .unwrap();
    tx.send(crate::agent::Event::ToolResult {
        name: "run_shell".into(),
        ok: true,
        text: "test result: ok".into(),
    })
    .unwrap();
    ed.agent_pane = Some(pane);
    ed.poll_agent();
    assert_eq!(
        ed.live.len(),
        2,
        "both shell commands must be played, the first one is not skipped"
    );
}

/// every action of the agent is played in the live view, not only the file
/// writes: a memory listing or a memory page it opened is on screen too.
#[test]
fn a_memory_listing_and_a_memory_page_are_played_in_the_live_view() {
    let mut ed = Editor::open(None).unwrap();
    let mut pane = AgentPane::new();
    let (tx, rx) = channel();
    pane.rx = Some(rx);
    tx.send(crate::agent::Event::ToolCall {
        name: "memory_list".into(),
        args: serde_json::json!({}),
    })
    .unwrap();
    tx.send(crate::agent::Event::ToolResult {
        name: "memory_list".into(),
        ok: true,
        text: "project: stack\nuser: tone".into(),
    })
    .unwrap();
    tx.send(crate::agent::Event::ToolCall {
        name: "memory_read".into(),
        args: serde_json::json!({"class": "project", "name": "stack"}),
    })
    .unwrap();
    tx.send(crate::agent::Event::ToolResult {
        name: "memory_read".into(),
        ok: true,
        text: "# stack\n\nrust, no async".into(),
    })
    .unwrap();
    ed.agent_pane = Some(pane);
    ed.poll_agent();
    assert_eq!(ed.live.len(), 2, "each memory action must be played");
    // each action has its own window that is replaced when the next one
    // starts, so every action is checked while its window is on screen
    let mut seen = String::new();
    let mut guard = 0;
    while !ed.live.is_empty() && guard < 10_000 {
        ed.play_live();
        seen.push_str(&ed.agent_view.lines.join("\n"));
        seen.push('\n');
        guard += 1;
    }
    assert!(
        seen.contains("memory_list"),
        "the listing call is not shown:\n{seen}"
    );
    assert!(
        seen.contains("project: stack"),
        "the listing is not shown:\n{seen}"
    );
    assert!(
        seen.contains("rust, no async"),
        "the opened page is not shown:\n{seen}"
    );
}

/// a file change made by a tool that is not a plain write (edit_file) is
/// played too, so no change of the agent happens off screen.
#[test]
fn an_edit_is_played_in_the_live_view() {
    let mut ed = Editor::open(None).unwrap();
    let mut pane = AgentPane::new();
    let (tx, rx) = channel();
    pane.rx = Some(rx);
    tx.send(crate::agent::Event::ToolCall {
        name: "edit_file".into(),
        args: serde_json::json!({"path": "src/lib.rs", "find": "a", "replace": "b"}),
    })
    .unwrap();
    tx.send(crate::agent::Event::ToolResult {
        name: "edit_file".into(),
        ok: true,
        text: "edited src/lib.rs".into(),
    })
    .unwrap();
    ed.agent_pane = Some(pane);
    ed.poll_agent();
    assert_eq!(ed.live.len(), 1, "the edit is not played in the live view");
}

/// the transcript is wrapped once, when a line arrives, and not again on
/// every frame: a long conversation used to cost more each keystroke.
/// the cached rows must match a full wrap, so the screen does not change.
#[test]
fn the_transcript_is_wrapped_once_per_line_not_on_every_frame() {
    let mut pane = AgentPane::new();
    for i in 0..50 {
        pane.lines.push((
            "agent".into(),
            format!("answer {i} with several words to wrap"),
        ));
    }
    let text = |rows: &[Line]| -> Vec<String> {
        rows.iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    };
    let first = text(pane.rendered_rows(24));
    assert_eq!(pane.transcript.wrapped, 51, "every line is wrapped once");
    let again = text(pane.rendered_rows(24));
    assert_eq!(
        pane.transcript.wrapped, 51,
        "a frame with no new line wraps nothing"
    );
    assert_eq!(first, again);
    pane.lines.push(("you".into(), "one more".into()));
    let grown = text(pane.rendered_rows(24));
    assert_eq!(pane.transcript.wrapped, 52, "only the new line is wrapped");
    assert!(grown.len() > first.len());
    pane.transcript = TranscriptRows::default();
    assert_eq!(
        text(pane.rendered_rows(24)),
        grown,
        "a full wrap gives the same rows as the cache"
    );
}

/// the agent types one letter per tick, like a person coding: the cursor
/// sits after the last letter on screen, never a whole line at once.
#[test]
fn the_agent_writes_one_letter_per_tick() {
    let mut ed = Editor::open(None).unwrap();
    let mut pane = AgentPane::new();
    pane.writes.push(LiveWrite {
        path: PathBuf::from("src/lib.rs"),
        text: vec!["abc".into()],
        shown: 0,
        col: 0,
        ticks: 0,
        elapsed: 0,
    });
    ed.agent_pane = Some(pane);
    ed.poll_agent();
    ed.play_live();
    assert_eq!(ed.agent_view.lines, vec!["a".to_string()], "one letter");
    assert_eq!(ed.agent_view.cx, 1, "the cursor follows the letter");
    ed.play_live();
    assert_eq!(ed.agent_view.lines, vec!["ab".to_string()], "two letters");
    ed.play_live();
    ed.play_live();
    assert!(ed.live.is_empty(), "the line is whole, the write is over");
    assert_eq!(ed.agent_view.lines, vec!["abc".to_string()]);
}

/// a write with a set duration lands in exactly that many ticks, however
/// long its text is: the pace stretches to the time it was given.
#[test]
fn a_write_lasts_exactly_the_ticks_it_is_given() {
    let mut ed = Editor::open(None).unwrap();
    let mut pane = AgentPane::new();
    pane.writes.push(LiveWrite {
        path: PathBuf::from("src/lib.rs"),
        text: vec!["abcdefghij".into()],
        shown: 0,
        col: 0,
        ticks: 5,
        elapsed: 0,
    });
    ed.agent_pane = Some(pane);
    ed.poll_agent();
    ed.play_live();
    assert_eq!(
        ed.agent_view.lines,
        vec!["ab".to_string()],
        "2 of 10 in tick 1"
    );
    for _ in 0..3 {
        ed.play_live();
    }
    assert_eq!(ed.live.len(), 1, "still writing after 4 ticks of 5");
    ed.play_live();
    assert!(ed.live.is_empty(), "the write ends on tick 5");
    assert_eq!(ed.agent_view.lines, vec!["abcdefghij".to_string()]);
}

/// the playback is a view of work that goes on: when the user leaves the
/// agent's view to do something else, the playback stops at once rather
/// than running on behind their back.
#[test]
fn the_agent_playback_stops_when_the_card_closes() {
    let mut ed = Editor::open(None).unwrap();
    let mut pane = AgentPane::new();
    pane.writes.push(LiveWrite {
        path: PathBuf::from("src/lib.rs"),
        text: vec!["abcdefghij".into()],
        shown: 0,
        col: 0,
        ticks: 0,
        elapsed: 0,
    });
    ed.agent_pane = Some(pane);
    ed.poll_agent();
    ed.play_live();
    ed.agent_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(ed.agent_pane.is_none(), "the card closed");
    assert!(ed.live.is_empty(), "the playback stopped with the card");
    assert_eq!(
        ed.agent_view.lines,
        vec!["a".to_string()],
        "what was shown stays"
    );
}

/// opening a file from the explorer is leaving the agent's view: the
/// playback stops, and the file the user opens is the one they see.
#[test]
fn the_agent_playback_stops_when_a_file_is_opened_from_the_explorer() {
    let d = scratch("live-interrupt-explorer");
    let file = d.join("notes.txt");
    std::fs::write(&file, "hello\n").unwrap();
    let mut ed = Editor::open(None).unwrap();
    let mut pane = AgentPane::new();
    pane.writes.push(LiveWrite {
        path: PathBuf::from("src/lib.rs"),
        text: vec!["abcdefghij".into()],
        shown: 0,
        col: 0,
        ticks: 0,
        elapsed: 0,
    });
    ed.agent_pane = Some(pane);
    ed.poll_agent();
    ed.play_live();
    ed.open_from_explorer(file);
    assert!(ed.live.is_empty(), "the explorer open stops the playback");
    assert_eq!(
        ed.lines,
        vec!["hello".to_string()],
        "the user's file is shown"
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// once the agent stops writing, its view shows no current line: the
/// line number of the last line is drawn like the others.
#[test]
fn the_agent_view_has_no_cursor_when_the_agent_is_idle() {
    let mut ed = Editor::open(None).unwrap();
    let mut pane = AgentPane::new();
    pane.writes.push(LiveWrite {
        path: PathBuf::from("src/lib.rs"),
        text: vec!["fn a() {}".into(), "fn b() {}".into()],
        shown: 0,
        col: 0,
        ticks: 0,
        elapsed: 0,
    });
    ed.agent_pane = Some(pane);
    ed.poll_agent();
    while !ed.live.is_empty() {
        ed.play_live();
    }
    // the view sweeps in before it is read: look at it once it has landed
    std::thread::sleep(VIEW_SWEEP);
    let backend = ratatui::backend::TestBackend::new(120, 40);
    let mut term = ratatui::Terminal::new(backend).unwrap();
    term.draw(|f| draw(f, &mut ed)).unwrap();
    let buf = term.backend().buffer().clone();
    let row = (0..40u16)
        .find(|&y| {
            (0..120u16)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
                .contains("fn b() {}")
        })
        .expect("the last line is drawn");
    let number = buf[(3, row)].fg;
    assert_eq!(
        number,
        Ed::gutter(),
        "the idle agent shows a cursor on its last line"
    );
}

/// wheeling the transcript up is clamped to what it holds, so one more
/// Down always scrolls back: the counter does not run away.
#[test]
fn the_agent_transcript_scroll_stops_at_the_top() {
    let mut ed = Editor::open(None).unwrap();
    let mut pane = AgentPane::new();
    pane.lines.push(("agent".into(), "une seule ligne".into()));
    pane.scroll = 500;
    ed.agent_pane = Some(pane);
    let _ = rows(&mut ed, 120, 40);
    let scroll = ed.agent_pane.as_ref().unwrap().scroll;
    assert!(scroll < 20, "the scroll ran away to {scroll}");
}

/// the agent took a while to reach a write: the write lasts as long, so the
/// reader watches it land in the time the agent actually spent.
#[test]
fn a_write_lasts_as_long_as_the_agent_took_to_reach_it() {
    let mut pane = AgentPane::new();
    let (tx, rx) = channel();
    pane.rx = Some(rx);
    pane.last_event = Instant::now() - Duration::from_millis(3000);
    tx.send(crate::agent::Event::ToolCall {
        name: "write_file".into(),
        args: serde_json::json!({"path": "src/lib.rs", "content": "fn a() {}"}),
    })
    .unwrap();
    tx.send(crate::agent::Event::File {
        path: "src/lib.rs".into(),
        content: "fn a() {}".into(),
        write: true,
    })
    .unwrap();
    pane.poll();
    let ticks = pane.writes[0].ticks;
    assert!(
        (187..=190).contains(&ticks),
        "3 s at one tick per 16 ms: {ticks} ticks"
    );
}

/// a new view sweeps in from the left as the card hands over to a file:
/// half-way, its rounded edge sits mid-card and nothing is drawn past it.
#[test]
fn the_view_sweeps_in_from_the_left() {
    let mut ed = Editor::open(None).unwrap();
    ed.agent_pane = Some(AgentPane::new());
    ed.agent_view = AgentView {
        path: Some(PathBuf::from("b.rs")),
        lines: vec!["two".into()],
        cy: 0,
        cx: 3,
        opened: Some(Instant::now() - VIEW_SWEEP / 2),
        glide: None,
    };
    let rows = render_rows(&mut ed, 120, 40);
    // the card's top border is the row that holds the rounded corner; the
    // header and the ide's own first row sit above it now
    let top = rows
        .iter()
        .find(|r| r.contains('╮'))
        .expect("the sweep has a rounded edge");
    let edge = top.chars().position(|c| c == '╮').unwrap();
    // half-way is 60 columns of the 120: the edge lands past mid-card but
    // well short of the far side, whatever the frame's exact timing
    assert!(
        (50..=80).contains(&edge),
        "the edge is not mid-card, at {edge}: {top}"
    );
    assert!(
        top.chars().skip(edge + 1).all(|c| c == ' '
            || c == '─'
            || c == '╮'
            || c == '│'
            || c == '╭'),
        "the view is drawn past the sweep: {top}"
    );
}

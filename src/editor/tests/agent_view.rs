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
    });
    ed.agent_pane = Some(pane);
    ed.poll_agent();
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
    let transcript: String = ed
        .agent_pane
        .as_ref()
        .unwrap()
        .lines
        .iter()
        .map(|(_, t)| t.clone())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        transcript.contains("ls") && transcript.contains("cargo test"),
        "both shell commands are kept, the first one is not skipped:\n{transcript}"
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
    // no queue means the window shows the last action at once: it is never
    // behind the agent. every action is kept in the transcript.
    let shown = ed.agent_view.lines.join("\n");
    assert!(
        shown.contains("rust, no async"),
        "the last action, the opened page, is on screen:\n{shown}"
    );
    let transcript: String = ed
        .agent_pane
        .as_ref()
        .unwrap()
        .lines
        .iter()
        .map(|(_, t)| t.clone())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        transcript.contains("memory_list") && transcript.contains("project: stack"),
        "the earlier memory actions are kept in the transcript:\n{transcript}"
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
    assert!(
        ed.agent_view.path.is_some(),
        "the edit opens a window in the agent's own view"
    );
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

/// the whole text of the write is on screen at once, and the cursor sits at
/// its end: what the user reads is what the agent wrote, with nothing left to
/// arrive later.
#[test]
fn the_whole_write_is_on_screen_with_the_cursor_at_its_end() {
    let mut ed = Editor::open(None).unwrap();
    let mut pane = AgentPane::new();
    pane.writes.push(LiveWrite {
        path: PathBuf::from("src/lib.rs"),
        text: vec!["abc".into()],
    });
    ed.agent_pane = Some(pane);
    ed.poll_agent();
    assert_eq!(ed.agent_view.lines, vec!["abc".to_string()]);
    assert_eq!(ed.agent_view.cx, 3, "the cursor is after the last letter");
}

/// however long the text of a write is, it is never cut and never arrives in
/// pieces: the view is up to date at the first poll.
#[test]
fn a_write_is_never_cut_and_never_arrives_in_pieces() {
    let mut ed = Editor::open(None).unwrap();
    let mut pane = AgentPane::new();
    pane.writes.push(LiveWrite {
        path: PathBuf::from("src/lib.rs"),
        text: vec!["abcdefghij".into()],
    });
    ed.agent_pane = Some(pane);
    ed.poll_agent();
    assert_eq!(ed.agent_view.lines, vec!["abcdefghij".to_string()]);
}

/// leaving the agent's view ends its window, and nothing runs on behind the
/// user's back: there is no pending work left to play.
#[test]
fn nothing_runs_on_after_the_card_closes() {
    let mut ed = Editor::open(None).unwrap();
    let mut pane = AgentPane::new();
    pane.writes.push(LiveWrite {
        path: PathBuf::from("src/lib.rs"),
        text: vec!["abcdefghij".into()],
    });
    ed.agent_pane = Some(pane);
    ed.poll_agent();
    ed.agent_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(ed.agent_pane.is_none(), "the card closed");
    assert_eq!(
        ed.agent_view.lines,
        vec!["abcdefghij".to_string()],
        "what was written stays, whole"
    );
}

/// opening a file from the explorer leaves the agent's view: the file the user
/// opens is the one they see, and no agent write waits to be played over it.
#[test]
fn the_file_opened_from_the_explorer_is_the_one_shown() {
    let d = scratch("live-interrupt-explorer");
    let file = d.join("notes.txt");
    std::fs::write(&file, "hello\n").unwrap();
    let mut ed = Editor::open(None).unwrap();
    let mut pane = AgentPane::new();
    pane.writes.push(LiveWrite {
        path: PathBuf::from("src/lib.rs"),
        text: vec!["abcdefghij".into()],
    });
    ed.agent_pane = Some(pane);
    ed.poll_agent();
    ed.open_from_explorer(file);
    assert_eq!(
        ed.lines,
        vec!["hello".to_string()],
        "the user's file is shown"
    );
    assert_eq!(
        ed.agent_view.lines,
        vec!["abcdefghij".to_string()],
        "the agent's write is not played over the user's file"
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
    });
    ed.agent_pane = Some(pane);
    ed.poll_agent();
    // the agent has long stopped: its last write is older than the hold and no
    // glide is in flight, so the view is a file like any other and shows no
    // current line.
    ed.agent_view.opened = Some(Instant::now() - AGENT_VIEW_HOLD * 2);
    ed.agent_view.glide = None;
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

/// however long the agent took to reach a write, the write carries no delay
/// of its own: it is ready to be shown whole the moment it arrives, so the
/// view can never trail behind the agent.
#[test]
fn a_write_carries_no_delay_of_its_own() {
    let mut pane = AgentPane::new();
    let (tx, rx) = channel();
    pane.rx = Some(rx);
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
    assert_eq!(pane.writes[0].text, vec!["fn a() {}".to_string()]);
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

//! glitch safety tests of the editor screen.

use super::*;

fn reversed_cells(ed: &mut Editor, w: u16, h: u16) -> Vec<(u16, u16)> {
    let backend = ratatui::backend::TestBackend::new(w, h);
    let mut term = ratatui::Terminal::new(backend).unwrap();
    term.draw(|f| draw(f, ed)).unwrap();
    let buf = term.backend().buffer().clone();
    (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .filter(|&(x, y)| buf[(x, y)].modifier.contains(Modifier::REVERSED))
        .collect()
}

/// a tab, a carriage return, an escape sequence or a zero-width mark in
/// text from the agent is not printed: the terminal would act on it.
#[test]
fn safe_text_keeps_the_letters_and_drops_every_byte_a_terminal_acts_on() {
    assert_eq!(
        safe_text("a\tb"),
        "a   b",
        "a tab becomes spaces to the next stop"
    );
    assert_eq!(
        safe_text("x\r\u{7}y"),
        "xy",
        "carriage return and bell are dropped"
    );
    assert_eq!(
        safe_text("\x1b[31mred\x1b[0m"),
        "red",
        "colour codes go whole"
    );
    assert_eq!(
        safe_text("\x1b]0;title\x07ok"),
        "ok",
        "window titles go whole"
    );
    assert_eq!(
        safe_text("e\u{200d}\u{fe0f}!"),
        "e!",
        "zero-width marks are dropped"
    );
    assert_eq!(
        safe_text("ok\nnext"),
        "ok\nnext",
        "a line break separates lines"
    );
}

/// the agent's answers, file contents and tool outputs, all dirty, put
/// nothing that is a control byte on the screen, and the text still shows.
#[test]
fn the_agent_screen_never_holds_a_control_byte() {
    let mut ed = Editor::open(None).unwrap();
    let mut pane = AgentPane::new();
    let (tx, rx) = channel();
    pane.rx = Some(rx);
    tx.send(crate::agent::Event::File {
        path: "src/b\tc.rs".into(),
        content: "fn b() {\r\n\tlet y = 1;\r\n}\r\n".into(),
        write: true,
    })
    .unwrap();
    tx.send(crate::agent::Event::ToolCall {
        name: "run_shell".into(),
        args: serde_json::json!({"command": "cargo build"}),
    })
    .unwrap();
    tx.send(crate::agent::Event::ToolResult {
        name: "run_shell".into(),
        ok: true,
        text: "src/a.rs:3:\tlet x\r\n\x1b[31mred\x1b[0m done\u{7}\u{200d}".into(),
    })
    .unwrap();
    tx.send(crate::agent::Event::Text("plain\tsaid\x1b[2J".into()))
        .unwrap();
    ed.agent_pane = Some(pane);
    ed.poll_agent();
    let mut guard = 0;
    while !ed.live.is_empty() && guard < 10_000 {
        ed.play_live();
        guard += 1;
    }
    std::thread::sleep(VIEW_SWEEP);
    let text = render_text(&mut ed, 160, 40);
    let bad: Vec<char> = text.chars().filter(|c| c.is_control()).collect();
    assert!(bad.is_empty(), "control bytes reached a cell: {bad:?}");
    assert!(text.contains("red done"), "the text still shows:\n{text}");
}

/// the user's own buffer is held to the same rule: a lone carriage return
/// or an escape in a file never reaches the terminal.
#[test]
fn the_user_buffer_never_holds_a_control_byte_either() {
    let mut ed = Editor::open(None).unwrap();
    ed.lines = vec!["a\rb\x1b[2Jc\u{200d}d".into()];
    ed.modified = true;
    let text = render_text(&mut ed, 100, 30);
    let bad: Vec<char> = text.chars().filter(|c| c.is_control()).collect();
    assert!(bad.is_empty(), "control bytes reached a cell: {bad:?}");
    assert!(text.contains("abcd"), "the visible text is kept:\n{text}");
}

/// the agent's cursor is a block drawn right after the last letter it
/// typed: that is what makes the write read as a person typing.
#[test]
fn the_agent_cursor_sits_after_the_last_letter_it_wrote() {
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
    ed.play_live();
    std::thread::sleep(VIEW_SWEEP);
    // the number gutter is 5 columns, the box border one: the text starts at x 6.
    // rows: the header and the card's top border sit above the first line, row 3
    assert_eq!(reversed_cells(&mut ed, 160, 40), vec![(8, 3)]);
}

/// the cursor does not jump: when the agent moves to the next line, it
/// glides down from the line it left, and lands on the new line.
#[test]
fn the_cursor_glides_from_the_line_it_left_to_the_next() {
    let mut ed = Editor::open(None).unwrap();
    ed.agent_pane = Some(AgentPane::new());
    ed.agent_view = AgentView {
        path: Some(PathBuf::from("src/lib.rs")),
        lines: vec!["one".into(), "two".into(), "three".into()],
        cy: 2,
        cx: 0,
        opened: Some(Instant::now() - VIEW_SWEEP * 2),
        glide: Some((0, Instant::now() - CURSOR_GLIDE / 2)),
    };
    ed.live.push(LiveWrite {
        path: PathBuf::from("src/lib.rs"),
        text: vec!["x".into()],
        shown: 0,
        col: 0,
        ticks: 0,
        elapsed: 0,
    });
    // half-way: between line 1 (row 3) and line 3 (row 5), so on row 4
    assert_eq!(reversed_cells(&mut ed, 160, 40), vec![(6, 4)]);
    ed.agent_view.glide = None;
    assert_eq!(reversed_cells(&mut ed, 160, 40), vec![(6, 5)]);
}

/// playing a write that goes to a new line starts a glide from the line
/// the cursor left.
#[test]
fn a_new_line_starts_a_glide_from_the_old_one() {
    let mut ed = Editor::open(None).unwrap();
    let mut pane = AgentPane::new();
    pane.writes.push(LiveWrite {
        path: PathBuf::from("src/lib.rs"),
        text: vec!["abc".into(), "def".into()],
        shown: 0,
        col: 0,
        ticks: 0,
        elapsed: 0,
    });
    ed.agent_pane = Some(pane);
    ed.poll_agent();
    let mut guard = 0;
    while ed.agent_view.cy == 0 && !ed.live.is_empty() && guard < 100 {
        ed.play_live();
        guard += 1;
    }
    assert_eq!(ed.agent_view.cy, 1, "the cursor reached the second line");
    assert_eq!(ed.agent_view.glide.map(|(from, _)| from), Some(0));
}

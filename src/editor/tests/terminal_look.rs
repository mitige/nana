//! terminal look tests of the editor screen.

use super::*;

/// when the agent runs a shell command, the live view turns into a
/// terminal: the command appears with a `$` and its output under it, not
/// as a json call in a generic action list.
#[test]
fn every_tool_action_is_played_as_a_terminal_line() {
    let mut ed = Editor::open(None).unwrap();
    let mut pane = AgentPane::new();
    let (tx, rx) = channel();
    pane.rx = Some(rx);
    tx.send(crate::agent::Event::ToolCall {
        name: "list_dir".into(),
        args: serde_json::json!({"path": "src"}),
    })
    .unwrap();
    tx.send(crate::agent::Event::ToolResult {
        name: "list_dir".into(),
        ok: true,
        text: "editor.rs".into(),
    })
    .unwrap();
    ed.agent_pane = Some(pane);
    ed.poll_agent();
    let shown = ed.agent_view.lines.join("\n");
    assert!(
        shown.contains("$ ls src"),
        "a list_dir is not played as a terminal line:\n{shown}"
    );
    assert!(
        !shown.contains("> list_dir"),
        "the json call is still shown:\n{shown}"
    );
}

#[test]
fn a_shell_command_is_played_as_a_terminal_session() {
    let mut ed = Editor::open(None).unwrap();
    let mut pane = AgentPane::new();
    let (tx, rx) = channel();
    pane.rx = Some(rx);
    tx.send(crate::agent::Event::ToolCall {
        name: "run_shell".into(),
        args: serde_json::json!({"command": "ls"}),
    })
    .unwrap();
    tx.send(crate::agent::Event::ToolResult {
        name: "run_shell".into(),
        ok: true,
        text: "Cargo.toml\nsrc".into(),
    })
    .unwrap();
    ed.agent_pane = Some(pane);
    ed.poll_agent();
    let shown = ed.agent_view.lines.join("\n");
    assert!(
        shown.contains("$ ls"),
        "the command has no prompt:\n{shown}"
    );
    assert!(
        shown.contains("Cargo.toml"),
        "the output is not shown:\n{shown}"
    );
    assert!(
        !shown.contains("> run_shell"),
        "the json call is still shown:\n{shown}"
    );
}

/// the trail, the band under the card, shows a shell command as a prompt
/// too: the json of the call is not what a person would read.
#[test]
fn the_trail_shows_a_shell_command_as_a_prompt_not_as_json() {
    let mut ed = Editor::open(None).unwrap();
    let mut pane = AgentPane::new();
    let (tx, rx) = channel();
    pane.rx = Some(rx);
    tx.send(crate::agent::Event::ToolCall {
        name: "run_shell".into(),
        args: serde_json::json!({"command": "ls"}),
    })
    .unwrap();
    ed.agent_pane = Some(pane);
    ed.poll_agent();
    let trail: Vec<String> = ed
        .agent_pane
        .as_ref()
        .unwrap()
        .trail
        .iter()
        .map(|s| s.label.clone())
        .collect();
    assert!(
        trail.iter().any(|t| t == "$ ls"),
        "the trail shows json, not a prompt: {trail:?}"
    );
}

/// on screen, a shell action reads as a terminal typed by a person: no
/// line numbers in a gutter, no window named after the internal view.
#[test]
fn a_shell_action_on_screen_has_no_file_gutter_and_no_machine_title() {
    let mut ed = Editor::open(None).unwrap();
    let mut pane = AgentPane::new();
    let (tx, rx) = channel();
    pane.rx = Some(rx);
    tx.send(crate::agent::Event::ToolCall {
        name: "run_shell".into(),
        args: serde_json::json!({"command": "ls"}),
    })
    .unwrap();
    tx.send(crate::agent::Event::ToolResult {
        name: "run_shell".into(),
        ok: true,
        text: "Cargo.toml".into(),
    })
    .unwrap();
    ed.agent_pane = Some(pane);
    ed.poll_agent();
    ed.agent_view.opened = Some(Instant::now() - VIEW_SWEEP * 2);
    let screen = render_rows(&mut ed, 120, 40).join("\n");
    assert!(
        !screen.contains("agent actions"),
        "the window is named after the internal view:\n{screen}"
    );
    assert!(
        !screen.contains("1 $ ls"),
        "the terminal is drawn with a file's line numbers:\n{screen}"
    );
    assert!(
        screen.contains("$ ls"),
        "the command is not on screen:\n{screen}"
    );
}

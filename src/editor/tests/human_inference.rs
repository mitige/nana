//! human inference tests of the editor screen.

use super::*;

/// every action opens its own window: two tool actions never share one
/// "agent actions" view that the second one overwrites.
#[test]
fn each_action_opens_its_own_window() {
    let mut ed = Editor::open(None).unwrap();
    let mut pane = AgentPane::new();
    let (tx, rx) = channel();
    pane.rx = Some(rx);
    for (name, args, text) in [
        (
            "run_shell",
            serde_json::json!({"command": "ls"}),
            "Cargo.toml",
        ),
        ("list_dir", serde_json::json!({"path": "src"}), "editor.rs"),
    ] {
        tx.send(crate::agent::Event::ToolCall {
            name: name.into(),
            args,
        })
        .unwrap();
        tx.send(crate::agent::Event::ToolResult {
            name: name.into(),
            ok: true,
            text: text.into(),
        })
        .unwrap();
    }
    ed.agent_pane = Some(pane);
    ed.poll_agent();
    let paths: Vec<PathBuf> = ed.live.iter().map(|w| w.path.clone()).collect();
    assert_eq!(paths.len(), 2, "two actions, two windows: {paths:?}");
    assert_ne!(
        paths[0], paths[1],
        "two actions share one window: {paths:?}"
    );
    assert!(
        paths
            .iter()
            .all(|p| is_action_view(p) && p.to_string_lossy().contains(" ")),
        "an action lost its own window name: {paths:?}"
    );
}

/// while the agent writes a file, the tree selects that file, as a person
/// working in an ide would see it.
#[test]
fn the_tree_selects_the_file_the_agent_writes() {
    let mut ed = Editor::open(None).unwrap();
    ed.explorer = Some(crate::explorer::Explorer::new(PathBuf::from(".")));
    let mut pane = AgentPane::new();
    let (tx, rx) = channel();
    pane.rx = Some(rx);
    tx.send(crate::agent::Event::File {
        path: "Cargo.toml".into(),
        content: "[package]".into(),
        write: true,
    })
    .unwrap();
    ed.agent_pane = Some(pane);
    ed.poll_agent();
    ed.play_live();
    let ex = ed.explorer.as_ref().unwrap();
    let picked = ex.rows()[ex.sel()].path.clone();
    assert!(
        picked.ends_with("Cargo.toml"),
        "the tree does not follow the write: {picked:?}"
    );
}

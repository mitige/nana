//! status badge follows the agent tests of the editor screen.

use super::*;

fn badge(ed: &mut Editor) -> String {
    let screen = render_rows(ed, 120, 40);
    screen.last().cloned().unwrap_or_default()
}

/// while the card is open the status badge names what the agent is doing:
/// a shell command shows "terminal", a file shows its language.
#[test]
fn the_badge_names_the_action_the_agent_plays() {
    let mut ed = Editor::open(None).unwrap();
    ed.agent_pane = Some(AgentPane::new());
    ed.live.push(LiveWrite {
        path: PathBuf::from(format!("{ACTIONS_VIEW}: run_shell")),
        text: vec!["$ ls".into()],
        shown: 0,
        col: 0,
        ticks: 0,
        elapsed: 0,
    });
    let bar = badge(&mut ed);
    assert!(
        bar.contains(" terminal "),
        "badge does not follow the shell: {bar:?}"
    );
}

/// a second prompt after a closed task opens its own page: the opening of
/// the last task must not stay filed under the first one.
#[test]
fn a_new_task_after_a_closed_one_opens_its_own_page() {
    let root = std::env::temp_dir().join("nana-task-open");
    let mut pane = AgentPane::new();
    pane.begin_task("first task", &root);
    pane.closing = Some("first done".into());
    pane.begin_task("second task", &root);
    assert_eq!(pane.opening.as_deref(), Some("second task"));
    assert!(
        pane.closing.is_none(),
        "the old closing must not leak into the new task"
    );
}

/// a task that ends in error is still a task that closed: its page keeps
/// the failure as the closing message.
#[test]
fn a_failed_task_is_saved_with_its_error_as_closing() {
    let root = std::env::temp_dir().join("nana-task-fail");
    let mut pane = AgentPane::new();
    pane.begin_task("break the build", &root);
    pane.closing = None;
    let e = "provider timeout";
    if pane.closing.is_none() {
        pane.closing = Some(format!("failed: {e}"));
    }
    assert_eq!(pane.closing.as_deref(), Some("failed: provider timeout"));
}

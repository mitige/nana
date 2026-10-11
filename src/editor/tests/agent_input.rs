//! agent input tests of the editor screen.

use super::*;

/// a paste while the agent card is open goes into its prompt, newlines
/// included, and never into the user's buffer behind the card.
#[test]
fn a_paste_lands_in_the_agent_prompt() {
    let mut ed = Editor::open(None).unwrap();
    ed.agent_pane = Some(AgentPane::new());
    let before = ed.lines.clone();
    ed.paste("first line\nsecond line");
    assert_eq!(
        ed.agent_pane.as_ref().unwrap().input,
        "first line\nsecond line",
        "the pasted text is in the prompt"
    );
    assert_eq!(ed.lines, before, "the user's buffer is untouched");
}

/// a long prompt wraps on the card instead of running off its right edge.
#[test]
fn a_long_prompt_wraps_on_several_lines_inside_the_card() {
    let mut ed = Editor::open(None).unwrap();
    let mut pane = AgentPane::new();
    pane.input = "alpha beta gamma delta epsilon zeta eta theta iota kappa".into();
    ed.agent_pane = Some(pane);
    let rows = render_rows(&mut ed, 60, 24);
    let joined = rows.join("\n");
    assert!(
        joined.contains("alpha beta"),
        "the start is shown:\n{joined}"
    );
    assert!(joined.contains("kappa"), "the end is shown too:\n{joined}");
    assert!(
        !joined.contains("..."),
        "nothing is cut with dots:\n{joined}"
    );
}

/// a pasted block of several lines shows each line on its own row.
#[test]
fn a_multi_line_prompt_shows_every_line() {
    let mut ed = Editor::open(None).unwrap();
    let mut pane = AgentPane::new();
    pane.input = "one\ntwo\nthree".into();
    ed.agent_pane = Some(pane);
    let rows = render_rows(&mut ed, 80, 24);
    let joined = rows.join("\n");
    for word in ["one", "two", "three"] {
        assert!(joined.contains(word), "{word} is shown:\n{joined}");
    }
    let row_of = |w: &str| rows.iter().position(|r| r.contains(w)).unwrap();
    assert!(row_of("one") < row_of("two"), "lines keep their order");
    assert!(row_of("two") < row_of("three"), "lines keep their order");
}

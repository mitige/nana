//! paste tests of the editor screen.

use super::*;

/// Pasting must not reformat. Typing enter auto-indents — right for
/// writing code, wrong for pasting it: the text arrives as it was copied.
#[test]
fn pasting_keeps_the_text_as_it_was() {
    let mut ed = Editor::open(None).unwrap();
    ed.paste("one\ntwo\nthree");
    assert_eq!(ed.lines, vec!["one", "two", "three"]);
    assert_eq!(ed.cy, 2, "the cursor ends on the last pasted line");

    // a paste under an indented line must not inherit that indentation
    ed.lines = vec!["    indented".into(), String::new()];
    ed.cy = 1;
    ed.cx = 0;
    ed.paste("a\nb");
    assert_eq!(ed.lines[1], "a");
    assert_eq!(ed.lines[2], "b", "no tab of ours was added");

    // and typing enter still auto-indents: that part is wanted
    ed.cy = 0;
    ed.cx = ed.lines[0].len();
    ed.insert_newline();
    assert!(
        ed.lines[1].starts_with("    "),
        "the auto-indent still works: {:?}",
        ed.lines
    );
}

#[test]
fn a_paste_lands_at_the_cursor_and_marks_the_buffer_dirty() {
    let mut ed = Editor::open(None).unwrap();
    ed.lines = vec!["xx".into()];
    ed.cx = 1;
    ed.paste("AB");
    assert_eq!(ed.lines[0], "xABx");
    assert!(ed.modified, "the buffer knows it changed");
    // windows line endings are normalised
    ed.paste("\r\nZ");
    assert!(ed.lines.iter().all(|l| !l.contains('\r')), "{:?}", ed.lines);
}

#[test]
fn a_paste_cannot_reach_a_modal_card() {
    let mut ed = Editor::open(None).unwrap();
    ed.lines = vec!["untouched".into()];
    ed.agent_pane = Some(AgentPane::new());
    ed.paste("sneaky");
    assert_eq!(ed.lines, vec!["untouched"], "the card protects the buffer");
    ed.agent_pane = None;
    ed.hub = Some(crate::hub::Hub::open(std::path::Path::new(".")));
    ed.paste("sneaky");
    assert_eq!(ed.lines, vec!["untouched"], "the hub too");
}

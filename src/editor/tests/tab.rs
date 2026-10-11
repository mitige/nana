//! tab tests of the editor screen.

use super::*;

#[test]
fn a_tab_is_expanded_to_the_next_stop() {
    assert_eq!(expand_tabs("a\tb"), "a   b");
    assert_eq!(expand_tabs("abcd\te"), "abcd    e");
    assert_eq!(expand_tabs("\tx"), "    x");
    assert_eq!(expand_tabs("no tabs"), "no tabs");
}

#[test]
fn the_cursor_column_follows_the_expansion() {
    assert_eq!(drawn_width("a"), 1);
    assert_eq!(drawn_width("a\t"), 4);
    assert_eq!(drawn_width("ab\t"), 4);
    assert_eq!(drawn_width("pla"), 3);
}

/// A tab must never reach a cell: the terminal would move its own cursor
/// and the whole frame would drift — this is the makefile glitch.
#[test]
fn no_cell_ever_holds_a_tab() {
    let mut ed = Editor::open(None).unwrap();
    ed.lines = vec![
        "NAME\t\t:= tesseract".to_string(),
        "\t$(CC) -o $@ $<".to_string(),
        "plain".to_string(),
    ];
    ed.file = Some(std::path::PathBuf::from("Makefile"));
    ed.ext = "mk".into();
    ed.modified = true;
    let text = render_text(&mut ed, 100, 20);
    assert!(!text.contains('\t'), "a tab survived into the frame");
    assert!(text.contains("NAME        := tesseract"), "aligned: {text}");
    assert!(text.contains("$(CC) -o $@ $<"), "the recipe is readable");
}

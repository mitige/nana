//! rounded everywhere tests of the editor screen.

use super::*;

/// every card, in every state the user can open, is drawn with rounded
/// corners: a square corner anywhere is a regression of the visual promise.
#[test]
fn no_panel_has_a_square_corner() {
    let root = scratch("rounded");
    std::fs::write(root.join("a.rs"), "fn main() {}\n").unwrap();
    for setup in 0..3 {
        let mut ed = Editor::open(None).unwrap();
        ed.lines = vec!["fn main() {}".into()];
        ed.explorer = Some(Explorer::new(root.clone()));
        if setup == 1 {
            ed.term = TermPane::spawn(&root, 8, 60).ok();
        }
        if setup == 2 {
            ed.search = Some(FileSearch::new(root.clone()));
        }
        let text: String = render_text(&mut ed, 110, 32);
        for square in ["┌", "┐", "└", "┘", "├", "┤"] {
            assert!(
                !text.contains(square),
                "square corner {square} in state {setup}"
            );
        }
    }
    let _ = std::fs::remove_dir_all(&root);
}

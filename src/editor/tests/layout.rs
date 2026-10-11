//! layout tests of the editor screen.

use super::*;

/// The loop decides how far it can scroll from a computed inner height.
/// If that number does not match the box actually drawn, the cursor walks
/// below the visible area — two lines too far, in the case that bit.
#[test]
fn the_scroll_geometry_matches_the_box_on_screen() {
    let root = std::env::temp_dir().join(format!("nana-geo-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&root);
    let (w, h) = (100u16, 30u16);
    for with_terminal in [false, true] {
        let mut ed = Editor::open(None).unwrap();
        ed.lines = (0..200).map(|i| format!("line {i}")).collect();
        ed.explorer = Some(Explorer::new(root.clone()));
        if with_terminal {
            ed.term = TermPane::spawn(&root, 8, 60).ok();
        }
        // exactly what loop_run computes
        let term_h = if ed.term.is_some() { TERM_H + 1 } else { 0 };
        let expl_w = if ed.explorer.is_some() { EXPL_W + 1 } else { 0 };
        let inner_h = h.saturating_sub(4 + 2 + term_h) as usize;
        let inner_w = w.saturating_sub(expl_w + 2);

        // and what the frame really shows: the editor box's inner height
        let rows = render_rows(&mut ed, w, h);
        let top = rows
            .iter()
            .position(|r| r.chars().nth(expl_w as usize) == Some('╭'))
            .expect("the editor box opens");
        let bottom = rows
            .iter()
            .position(|r| r.chars().nth(expl_w as usize) == Some('╰'))
            .expect("the editor box closes");
        let drawn_h = bottom - top - 1;
        assert_eq!(
            drawn_h, inner_h,
            "terminal={with_terminal}: the loop believes {inner_h} rows, the box has {drawn_h}"
        );
        assert!(
            inner_w > 10 && inner_h > 3,
            "the editor has room: {inner_w}x{inner_h}"
        );
    }
    let _ = std::fs::remove_dir_all(&root);
}

/// The cursor never ends up below the box, however far you scroll.
#[test]
fn the_cursor_stays_inside_the_box() {
    let root = std::env::temp_dir().join(format!("nana-geo2-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&root);
    let (w, h) = (100u16, 30u16);
    let mut ed = Editor::open(None).unwrap();
    ed.lines = (0..200).map(|i| format!("line {i}")).collect();
    ed.explorer = Some(Explorer::new(root.clone()));
    let inner_h = h.saturating_sub(6) as usize;
    let inner_w = w.saturating_sub(EXPL_W + 3);
    // walk down the file the way a held key does
    for _ in 0..200 {
        ed.cy = (ed.cy + 1).min(ed.lines.len() - 1);
        ed.keep_cursor_visible(inner_h, inner_w as usize);
    }
    assert_eq!(ed.cy, 199);
    let rows = render_rows(&mut ed, w, h);
    let top = rows
        .iter()
        .position(|r| r.chars().nth(EXPL_W as usize + 1) == Some('╭'))
        .expect("the editor box opens");
    let bottom = rows
        .iter()
        .position(|r| r.chars().nth(EXPL_W as usize + 1) == Some('╰'))
        .expect("the editor box closes");
    let cursor_row = top + 1 + (ed.cy - ed.scroll_y);
    assert!(
        cursor_row < bottom,
        "the cursor sits at row {cursor_row}, the box ends at {bottom}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

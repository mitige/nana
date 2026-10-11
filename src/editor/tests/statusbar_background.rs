//! statusbar background tests of the editor screen.

use super::*;

/// no status segment paints the theme's black: it shows midnight blue.
/// every busy word (thinking, checking, saving...) must sit on the terminal.
#[test]
fn no_status_segment_paints_the_theme_black() {
    for busy in ["thinking", "checking", "reading", "writing"] {
        let mut ed = Editor::open(None).unwrap();
        ed.ai_busy = busy == "thinking";
        ed.status = busy.to_string();
        let backend = ratatui::backend::TestBackend::new(120, 10);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        term.draw(|f| draw(f, &mut ed)).unwrap();
        let buf = term.backend().buffer();
        let row = 9u16;
        for x in 0..120u16 {
            assert_ne!(
                buf[(x, row)].bg,
                Color::Indexed(0),
                "status bar cell {x} paints the theme black while {busy}"
            );
        }
    }
}

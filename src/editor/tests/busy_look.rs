//! busy look tests of the editor screen.

use super::*;

/// the spinner says "thinking" on the bar with no background of its own:
/// the theme's black is midnight blue, and it must never show behind it.
#[test]
fn the_busy_segment_has_no_midnight_background() {
    let mut ed = Editor::open(None).unwrap();
    ed.ai_busy = true;
    let backend = ratatui::backend::TestBackend::new(120, 10);
    let mut term = ratatui::Terminal::new(backend).unwrap();
    term.draw(|f| draw(f, &mut ed)).unwrap();
    let buf = term.backend().buffer();
    let word = "thinking";
    let (w, h) = (120u16, 10u16);
    let mut found = None;
    for y in 0..h {
        let text: String = (0..w).map(|x| buf[(x, y)].symbol()).collect();
        if let Some(at) = text.find(word) {
            found = Some((at as u16, y));
        }
    }
    let (at, row) = found.expect("the busy word is on the bar");
    for x in at..at + word.len() as u16 {
        assert_ne!(
            buf[(x, row)].bg,
            Color::Indexed(0),
            "the busy segment paints the theme's black behind '{word}'"
        );
    }
}

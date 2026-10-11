//! rounded bars tests of the editor screen.

use super::*;

/// the brand and the status badge are pills again: a half-circle on each
/// end, so the two corners of the screen stay round. The other segments
/// stay flat, the rule of the last cleanup.
#[test]
fn the_brand_and_the_status_badge_are_rounded_pills() {
    let mut ed = Editor::open(None).unwrap();
    let backend = ratatui::backend::TestBackend::new(100, 20);
    let mut term = ratatui::Terminal::new(backend).unwrap();
    term.draw(|f| draw(f, &mut ed)).unwrap();
    let buf = term.backend().buffer();
    let top: String = (0..100).map(|x| buf[(x, 0)].symbol()).collect();
    let bottom: String = (0..100).map(|x| buf[(x, 19)].symbol()).collect();
    assert!(
        top.contains('\u{E0B6}') && top.contains('\u{E0B4}'),
        "brand not rounded: {top:?}"
    );
    assert!(
        bottom.contains('\u{E0B6}') && bottom.contains('\u{E0B4}'),
        "badge not rounded: {bottom:?}"
    );
}

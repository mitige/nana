//! sober badge tests of the editor screen.

use super::*;

/// the search is a window of its own, so it draws no status bar and no
/// badge: the focus accent lives on its border, never a magenta second colour.
#[test]
fn search_window_has_no_magenta_badge() {
    let mut ed = Editor::open(None).unwrap();
    ed.focus = Focus::Search;
    ed.search = Some(FileSearch::new(std::path::PathBuf::from(".")));
    let backend = ratatui::backend::TestBackend::new(110, 30);
    let mut term = ratatui::Terminal::new(backend).unwrap();
    term.draw(|f| draw(f, &mut ed)).unwrap();
    let buf = term.backend().buffer();
    let bottom: String = (0..110u16)
        .map(|x| buf[(x, 29)].symbol().to_string())
        .collect();
    assert!(
        !bottom.contains("search"),
        "a status bar is drawn under the search: {bottom}"
    );
    let magenta = (0..30u16)
        .flat_map(|y| (0..110u16).map(move |x| (x, y)))
        .any(|(x, y)| buf[(x, y)].fg == Color::Indexed(5));
    assert!(!magenta, "the search window has a magenta cell");
}
